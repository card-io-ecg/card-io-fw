use core::fmt::Debug;

use embedded_io_async::{Read, Write};

pub trait Connection: Read + Write {
    #[cfg(feature = "defmt")]
    type AcceptError: Debug + defmt::Format;
    #[cfg(not(feature = "defmt"))]
    type AcceptError: Debug;

    /// Waits for an incoming connection and sets up the connection for it.
    async fn accept(&mut self) -> Result<(), Self::AcceptError>;

    fn close(&mut self);
}

#[cfg(feature = "embassy")]
pub mod embassy_net_compat {

    use core::future::poll_fn;

    use embassy_net::{
        tcp::{AcceptError, AcceptToken, TcpListener, TcpSocket},
        Full, Stack,
    };
    use embassy_sync::{
        blocking_mutex::raw::NoopRawMutex,
        channel::{Channel, DynamicReceiver},
    };
    use embassy_time::Duration;

    use super::*;

    /// Hands over incoming connections from a single [`listen`] loop to the connection handlers.
    pub type AcceptQueue<const N: usize> = Channel<NoopRawMutex, AcceptToken, N>;

    /// Listens on `port` and passes each incoming connection attempt to `queue`.
    ///
    /// A port can only have a single listener, so run this in one task, and let multiple
    /// [`TcpConnection`]s take their work from the queue.
    pub async fn listen<const N: usize>(
        stack: Stack<'_>,
        port: u16,
        queue: &AcceptQueue<N>,
    ) -> Result<(), AcceptError> {
        let mut listener = unwrap!(TcpListener::new(stack));
        unwrap!(listener.listen(port));

        loop {
            // Dropping a token forgets the connection attempt, and the client's retransmitted SYN
            // queues it up again. Only take one once the queue has room for it.
            poll_fn(|cx| queue.poll_ready_to_send(cx)).await;
            let _ = queue.try_send(listener.accept().await?);
        }
    }

    pub struct TcpConnection<'a, 'd> {
        socket: TcpSocket<'a, 'd>,
        tokens: DynamicReceiver<'a, AcceptToken>,
    }

    impl<'a, 'd> TcpConnection<'a, 'd> {
        pub fn new(
            stack: Stack<'d>,
            rx_buffer: &'a mut [u8],
            tx_buffer: &'a mut [u8],
            tokens: DynamicReceiver<'a, AcceptToken>,
        ) -> Result<Self, Full> {
            Ok(Self {
                socket: TcpSocket::new(stack, rx_buffer, tx_buffer)?,
                tokens,
            })
        }

        pub fn set_timeout(&mut self, duration: Option<Duration>) {
            self.socket.set_timeout(duration);
        }
    }

    impl Connection for TcpConnection<'_, '_> {
        type AcceptError = AcceptError;

        async fn accept(&mut self) -> Result<(), Self::AcceptError> {
            let token = self.tokens.receive().await;
            self.socket.accept(token).await
        }

        fn close(&mut self) {
            self.socket.close();
            self.socket.abort();
            debug!("Socket closed");
        }
    }

    impl embedded_io_async::ErrorType for TcpConnection<'_, '_> {
        type Error = embassy_net::tcp::Error;
    }

    impl Read for TcpConnection<'_, '_> {
        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
            self.socket.read(buf).await
        }
    }

    impl Write for TcpConnection<'_, '_> {
        async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
            self.socket.write(buf).await
        }

        async fn flush(&mut self) -> Result<(), Self::Error> {
            self.socket.flush().await
        }
    }
}

#[cfg(feature = "std")]
pub mod std_compat {
    use std::net::{SocketAddr, TcpListener, TcpStream};

    use async_io::Async;
    use embedded_io::Io;
    use smol::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    pub struct StdTcpSocket {
        listener: Async<TcpListener>,
        socket: Option<Async<TcpStream>>,
    }

    impl StdTcpSocket {
        pub fn new(port: u16) -> std::io::Result<Self> {
            Ok(Self {
                listener: Async::<TcpListener>::bind(SocketAddr::from(([127, 0, 0, 1], port)))?,
                socket: None,
            })
        }
    }

    #[derive(Debug)]
    pub struct StdError(std::io::Error);
    impl From<std::io::Error> for StdError {
        fn from(value: std::io::Error) -> Self {
            Self(value)
        }
    }

    impl embedded_io::Error for StdError {
        fn kind(&self) -> embedded_io::ErrorKind {
            embedded_io::ErrorKind::Other
        }
    }

    impl Io for StdTcpSocket {
        type Error = StdError;
    }

    impl Write for StdTcpSocket {
        async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
            let count = self.socket.as_mut().unwrap().write(buf).await?;
            Ok(count)
        }
    }

    impl Read for StdTcpSocket {
        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
            let count = self.socket.as_mut().unwrap().read(buf).await?;
            Ok(count)
        }
    }

    impl Connection for StdTcpSocket {
        type AcceptError = StdError;

        async fn accept(&mut self) -> Result<(), Self::AcceptError> {
            let (socket, _) = self.listener.accept().await?;

            self.socket = Some(socket);

            Ok(())
        }

        fn close(&mut self) {
            let Some(socket) = self.socket.take() else {
                return;
            };
            let socket = socket.into_inner().unwrap();

            socket.shutdown(std::net::Shutdown::Both).unwrap();
            debug!("Socket closed");
        }
    }
}
