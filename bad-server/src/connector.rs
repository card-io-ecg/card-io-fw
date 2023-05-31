use core::fmt::Debug;

use embedded_io::asynch::{Read, Write};

pub trait Connection: Read + Write {
    type AcceptError: Debug;
    type Reader<'a>: Read<Error = Self::Error>
    where
        Self: 'a;
    type Writer<'a>: Write<Error = Self::Error>
    where
        Self: 'a;

    // TODO: separate listener and socket
    async fn listen(&mut self, port: u16) -> Result<(), Self::AcceptError>;
    fn split(&mut self) -> (Self::Reader<'_>, Self::Writer<'_>);

    fn close(&mut self);
}

#[cfg(feature = "embassy")]
pub mod embassy_net_compat {
    use super::*;
    use embassy_net::{
        tcp::{AcceptError, TcpReader, TcpSocket, TcpWriter},
        IpListenEndpoint,
    };

    impl<'a> Connection for TcpSocket<'a> {
        type AcceptError = AcceptError;
        type Reader<'s> = TcpReader<'s> where Self: 's;
        type Writer<'s> = TcpWriter<'s> where Self: 's;

        async fn listen(&mut self, port: u16) -> Result<(), Self::AcceptError> {
            self.accept(IpListenEndpoint { addr: None, port }).await
        }

        fn split(&mut self) -> (Self::Reader<'_>, Self::Writer<'_>) {
            TcpSocket::split(self)
        }

        fn close(&mut self) {
            TcpSocket::close(self);
            TcpSocket::abort(self);
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
        socket: Option<Async<TcpStream>>,
    }

    impl Clone for StdTcpSocket {
        fn clone(&self) -> Self {
            Self {
                socket: match self.socket {
                    Some(ref socket) => {
                        Some(Async::new(socket.get_ref().try_clone().unwrap()).unwrap())
                    }
                    None => None,
                },
            }
        }
    }

    impl StdTcpSocket {
        pub fn new() -> Self {
            Self { socket: None }
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

    impl<'a> Connection for StdTcpSocket {
        type AcceptError = StdError;
        type Reader<'s> = StdTcpSocket where Self: 's;
        type Writer<'s> = StdTcpSocket where Self: 's;

        async fn listen(&mut self, port: u16) -> Result<(), Self::AcceptError> {
            let listener = Async::<TcpListener>::bind(SocketAddr::from(([127, 0, 0, 1], port)))?;
            let (socket, _) = listener.accept().await?;

            self.socket = Some(socket);

            Ok(())
        }

        fn split(&mut self) -> (Self::Reader<'_>, Self::Writer<'_>) {
            (self.clone(), self.clone())
        }

        fn close(&mut self) {
            self.socket.take();
        }
    }
}
