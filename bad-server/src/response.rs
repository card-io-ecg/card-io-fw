use core::marker::PhantomData;

use embedded_io_async::ErrorType;
use httparse::Header;
use ufmt::uwrite;

use crate::{connector::Connection, HandleError};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ResponseStatus {
    Ok = 200,
    NotModified = 304,
    BadRequest = 400,
    NotFound = 404,
    RequestEntityTooLarge = 413,
    InternalServerError = 500,
    NotImplemented = 501,
}

impl ResponseStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::NotModified => "Not Modified",
            Self::BadRequest => "Bad Request",
            Self::NotFound => "Not Found",
            Self::RequestEntityTooLarge => "Request Entity Too Large",
            Self::InternalServerError => "Internal Server Error",
            Self::NotImplemented => "Not Implemented",
        }
    }
}

pub struct Initial(());
pub struct Headers(());
pub struct Body(());
pub struct BodyChunked(());

mod sealed {
    pub trait Sealed {}

    impl Sealed for super::Initial {}
    impl Sealed for super::Headers {}
    impl Sealed for super::Body {}
    impl Sealed for super::BodyChunked {}
}

pub trait ResponseState: sealed::Sealed {}

impl ResponseState for Initial {}
impl ResponseState for Headers {}
impl ResponseState for Body {}
impl ResponseState for BodyChunked {}

pub struct Response<'s, C, S = Initial>
where
    C: Connection + 's,
    S: ResponseState,
{
    socket: &'s mut C,
    _state: PhantomData<S>,
}

impl<'s, C: Connection> Response<'s, C, Initial> {
    pub fn new(socket: &'s mut C) -> Self {
        Self {
            socket,
            _state: PhantomData,
        }
    }

    pub async fn send_status(
        self,
        status: ResponseStatus,
    ) -> Result<Response<'s, C, Headers>, HandleError<C>> {
        self.socket
            .write_all(b"HTTP/1.1 ")
            .await
            .map_err(HandleError::Write)?;

        debug!("Response status: {}", status as u16);

        let mut status_code = heapless::Vec::<u8, 4>::new();
        if uwrite!(&mut status_code, "{}", status as u16).is_err() {
            return Err(HandleError::InternalError);
        }

        self.socket
            .write_all(&status_code)
            .await
            .map_err(HandleError::Write)?;

        self.socket
            .write_all(b" ")
            .await
            .map_err(HandleError::Write)?;
        self.socket
            .write_all(status.name().as_bytes())
            .await
            .map_err(HandleError::Write)?;
        self.socket
            .write_all(b"\r\n")
            .await
            .map_err(HandleError::Write)?;

        Ok(Response {
            socket: self.socket,
            _state: PhantomData,
        })
    }
}

impl<'s, C: Connection> Response<'s, C, Headers> {
    pub async fn send_header(&mut self, header: Header<'_>) -> Result<&mut Self, HandleError<C>> {
        self.send_raw_header(header).await?;
        Ok(self)
    }

    pub async fn send_headers(
        &mut self,
        headers: &[Header<'_>],
    ) -> Result<&mut Self, HandleError<C>> {
        for &header in headers {
            self.send_raw_header(header).await?;
        }
        Ok(self)
    }

    async fn send_raw_header(&mut self, header: Header<'_>) -> Result<(), HandleError<C>> {
        async fn send<C: Connection>(
            socket: &mut C,
            header: Header<'_>,
        ) -> Result<(), <C as ErrorType>::Error> {
            socket.write_all(header.name.as_bytes()).await?;
            socket.write_all(b": ").await?;
            socket.write_all(header.value).await?;
            socket.write_all(b"\r\n").await
        }

        send(self.socket, header).await.map_err(HandleError::Write)
    }

    async fn end_headers<B: ResponseState>(self) -> Result<Response<'s, C, B>, HandleError<C>> {
        self.socket
            .write_all(b"\r\n")
            .await
            .map_err(HandleError::Write)?;
        Ok(Response {
            socket: self.socket,
            _state: PhantomData,
        })
    }

    pub async fn start_body(self) -> Result<Response<'s, C, Body>, HandleError<C>> {
        self.end_headers().await
    }

    pub async fn start_chunked_body(
        mut self,
    ) -> Result<Response<'s, C, BodyChunked>, HandleError<C>> {
        self.send_header(Header {
            name: "Transfer-Encoding",
            value: b"chunked",
        })
        .await?;

        self.end_headers().await
    }

    pub async fn send_body(mut self, data: impl AsRef<[u8]>) -> Result<(), HandleError<C>> {
        let data = data.as_ref();
        let mut buffer = heapless::String::<12>::new();
        if uwrite!(&mut buffer, "{}", data.len()).is_err() {
            return Err(HandleError::InternalError);
        }

        self.send_header(Header {
            name: "Content-Length",
            value: buffer.as_bytes(),
        })
        .await?;

        let mut response = self.start_body().await?;
        response.write(data).await
    }
}

impl<'s, C: Connection> Response<'s, C, Body> {
    pub async fn write(&mut self, data: impl AsRef<[u8]>) -> Result<(), HandleError<C>> {
        self.socket
            .write_all(data.as_ref())
            .await
            .map_err(HandleError::Write)
    }
}

impl<'s, C: Connection> Response<'s, C, BodyChunked> {
    pub async fn write(&mut self, data: impl AsRef<[u8]>) -> Result<(), HandleError<C>> {
        let data = data.as_ref();
        let mut chunk_header = heapless::Vec::<u8, 12>::new();
        if uwrite!(&mut chunk_header, "{:X}\r\n", data.len()).is_err() {
            return Err(HandleError::InternalError);
        }

        self.socket
            .write_all(&chunk_header)
            .await
            .map_err(HandleError::Write)?;
        self.socket
            .write_all(data)
            .await
            .map_err(HandleError::Write)?;
        self.socket
            .write_all(b"\r\n")
            .await
            .map_err(HandleError::Write)
    }

    pub async fn end_chunked_response(mut self) -> Result<(), HandleError<C>> {
        self.write("").await
    }
}
