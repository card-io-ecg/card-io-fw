use core::fmt::Debug;

use embedded_io::asynch::{Read, Write};

pub trait Connection: Read + Write {
    type AcceptError: Debug;

    async fn listen(&mut self, port: u16) -> Result<(), Self::AcceptError>;

    fn close(&mut self);
}

pub mod embassy_net_compat {
    use super::*;
    use embassy_net::{
        tcp::{AcceptError, TcpSocket},
        IpListenEndpoint,
    };

    impl<'a> Connection for TcpSocket<'a> {
        type AcceptError = AcceptError;

        async fn listen(&mut self, port: u16) -> Result<(), Self::AcceptError> {
            self.accept(IpListenEndpoint { addr: None, port }).await?;

            Ok(())
        }

        fn close(&mut self) {
            TcpSocket::close(self);
        }
    }
}
