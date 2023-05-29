use core::{fmt::Write, marker::PhantomData};

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

pub struct Initial;
pub struct Headers;
pub struct Body;

mod sealed {
    pub trait Sealed {}

    impl Sealed for super::Initial {}
    impl Sealed for super::Headers {}
    impl Sealed for super::Body {}
}

pub trait ResponseState: sealed::Sealed {}

impl ResponseState for Initial {}
impl ResponseState for Headers {}
impl ResponseState for Body {}

pub struct Response<'a, C, S>
where
    C: Connection,
    S: ResponseState,
{
    socket: &'a mut C,
    _state: PhantomData<S>,
}

impl<'a, C> Response<'a, C, Initial>
where
    C: Connection,
{
    pub fn new(socket: &'a mut C) -> Self {
        Self {
            socket,
            _state: PhantomData,
        }
    }

    pub async fn send_status(
        self,
        status: ResponseStatus,
    ) -> Result<Response<'a, C, Headers>, C::Error> {
        self.socket.write_all(b"HTTP/1.0 ").await?;

        let mut status_code = heapless::Vec::<u8, 4>::new();
        write!(&mut status_code, "{}", status as u16).unwrap();
        self.socket.write_all(&status_code).await?;

        self.socket.write_all(b" ").await?;
        self.socket.write_all(status.name().as_bytes()).await?;
        self.socket.write_all(b"\r\n").await?;

        Ok(Response {
            socket: self.socket,
            _state: PhantomData,
        })
    }
}

impl<'a, C> Response<'a, C, Headers>
where
    C: Connection,
{
    pub async fn send_header(
        mut self,
        header: Header<'_>,
    ) -> Result<Response<'a, C, Headers>, C::Error> {
        self.send_raw_header(header).await?;
        Ok(self)
    }

    pub async fn send_headers(
        mut self,
        headers: &[Header<'_>],
    ) -> Result<Response<'a, C, Headers>, C::Error> {
        for &header in headers {
            self.send_raw_header(header).await?;
        }
        Ok(self)
    }

    async fn send_raw_header(&mut self, header: Header<'_>) -> Result<(), C::Error> {
        self.socket.write_all(header.name.as_bytes()).await?;
        self.socket.write_all(b": ").await?;
        self.socket.write_all(header.value).await?;
        self.socket.write_all(b"\r\n").await?;

        Ok(())
    }

    pub async fn end_headers(self) -> Result<Response<'a, C, Body>, C::Error> {
        self.socket.write_all(b"\r\n").await?;
        Ok(Response {
            socket: self.socket,
            _state: PhantomData,
        })
    }
}

impl<'a, C> Response<'a, C, Body>
where
    C: Connection,
{
    pub async fn write_string(&mut self, data: &str) -> Result<(), C::Error> {
        self.write_raw(data.as_bytes()).await
    }

    pub async fn write_raw(&mut self, data: &[u8]) -> Result<(), C::Error> {
        self.socket.write_all(data).await
    }
}
