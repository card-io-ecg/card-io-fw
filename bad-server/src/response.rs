use core::fmt::Write;

use httparse::Header;

use crate::connector::Connection;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ResponseStatus {
    Ok = 200,
    BadRequest = 400,
    NotFound = 404,
    InternalServerError = 500,
    NotImplemented = 501,
}

impl ResponseStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::BadRequest => "Bad Request",
            Self::NotFound => "Not Found",
            Self::InternalServerError => "Internal Server Error",
            Self::NotImplemented => "Not Implemented",
        }
    }
}

pub struct Response<'a, C: Connection> {
    socket: &'a mut C,
}

impl<'a, C> Response<'a, C>
where
    C: Connection,
{
    pub async fn send_headers<'h>(
        socket: &'a mut C,
        status: ResponseStatus,
        headers: &'h [Header<'h>],
    ) -> Result<Self, C::Error> {
        let mut this = Self { socket };

        this.send_status(status).await?;
        for &header in headers {
            this.send_raw_header(header).await?;
        }
        this.socket.write_all(b"\r\n").await?;

        Ok(this)
    }

    async fn send_status(&mut self, status: ResponseStatus) -> Result<(), C::Error> {
        let code = status as u16;
        let reason = status.name();

        let mut body = heapless::String::<64>::new();
        let _ = write!(&mut body, "HTTP/1.0 {code} {reason}\r\n");

        self.socket.write_all(body.as_bytes()).await
    }

    async fn send_raw_header(&mut self, header: Header<'_>) -> Result<(), C::Error> {
        self.socket.write_all(header.name.as_bytes()).await?;
        self.socket.write_all(b": ").await?;
        self.socket.write_all(header.value).await?;
        self.socket.write_all(b"\r\n").await?;
        Ok(())
    }

    pub async fn write(&mut self, data: &str) -> Result<(), C::Error> {
        self.write_raw(data.as_bytes()).await
    }

    pub async fn write_raw(&mut self, data: &[u8]) -> Result<(), C::Error> {
        self.socket.write_all(data).await
    }
}
