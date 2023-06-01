use core::{fmt::Write as _, marker::PhantomData};

use httparse::Header;

use crate::{connector::Connection, HandleError};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ResponseStatus {
    Ok = 200,
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
            .write_all(b"HTTP/1.0 ")
            .await
            .map_err(HandleError::Write)?;

        let mut status_code = heapless::Vec::<u8, 4>::new();
        log::debug!("Response status: {}", status as u16);
        write!(&mut status_code, "{}", status as u16).unwrap();
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
        self.socket
            .write_all(header.name.as_bytes())
            .await
            .map_err(HandleError::Write)?;
        self.socket
            .write_all(b": ")
            .await
            .map_err(HandleError::Write)?;
        self.socket
            .write_all(header.value)
            .await
            .map_err(HandleError::Write)?;
        self.socket
            .write_all(b"\r\n")
            .await
            .map_err(HandleError::Write)?;

        Ok(())
    }

    pub async fn start_body(self) -> Result<Response<'s, C, Body>, HandleError<C>> {
        self.socket
            .write_all(b"\r\n")
            .await
            .map_err(HandleError::Write)?;
        Ok(Response {
            socket: self.socket,
            _state: PhantomData,
        })
    }

    pub async fn start_chunked_body(
        mut self,
    ) -> Result<Response<'s, C, BodyChunked>, HandleError<C>> {
        self.send_header(Header {
            name: "transfer-encoding",
            value: b"chunked",
        })
        .await?;
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

impl<'s, C: Connection> Response<'s, C, Body> {
    pub async fn write_string(&mut self, data: &str) -> Result<(), HandleError<C>> {
        self.write_raw(data.as_bytes()).await
    }

    pub async fn write_raw(&mut self, data: &[u8]) -> Result<(), HandleError<C>> {
        self.socket
            .write_all(data)
            .await
            .map_err(HandleError::Write)
    }
}

impl<'s, C: Connection> Response<'s, C, BodyChunked> {
    pub async fn write_string(&mut self, data: &str) -> Result<(), HandleError<C>> {
        self.write_raw(data.as_bytes()).await
    }

    pub async fn write_raw(&mut self, data: &[u8]) -> Result<(), HandleError<C>> {
        let mut chunk_header = heapless::Vec::<u8, 12>::new();
        write!(&mut chunk_header, "{:X}\r\n", data.len()).unwrap();
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
}
