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

pub struct Initial(());
pub struct Headers(());
pub struct Body(());

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

pub struct Response<S>
where
    S: ResponseState,
{
    _state: PhantomData<S>,
}

impl Response<Initial> {
    pub fn new() -> Self {
        Self {
            _state: PhantomData,
        }
    }

    pub async fn send_status<C: Connection>(
        self,
        status: ResponseStatus,
        socket: &mut C,
    ) -> Result<Response<Headers>, C::Error> {
        socket.write_all(b"HTTP/1.0 ").await?;

        let mut status_code = heapless::Vec::<u8, 4>::new();
        write!(&mut status_code, "{}", status as u16).unwrap();
        socket.write_all(&status_code).await?;

        socket.write_all(b" ").await?;
        socket.write_all(status.name().as_bytes()).await?;
        socket.write_all(b"\r\n").await?;

        Ok(Response {
            _state: PhantomData,
        })
    }
}

impl Response<Headers> {
    pub async fn send_header<C: Connection>(
        &mut self,
        header: Header<'_>,
        socket: &mut C,
    ) -> Result<&mut Self, C::Error> {
        self.send_raw_header(header, socket).await?;
        Ok(self)
    }

    pub async fn send_headers<C: Connection>(
        &mut self,
        headers: &[Header<'_>],
        socket: &mut C,
    ) -> Result<&mut Self, C::Error> {
        for &header in headers {
            self.send_raw_header(header, socket).await?;
        }
        Ok(self)
    }

    async fn send_raw_header<C: Connection>(
        &mut self,
        header: Header<'_>,
        socket: &mut C,
    ) -> Result<(), C::Error> {
        socket.write_all(header.name.as_bytes()).await?;
        socket.write_all(b": ").await?;
        socket.write_all(header.value).await?;
        socket.write_all(b"\r\n").await?;

        Ok(())
    }

    pub async fn end_headers<C: Connection>(
        self,
        socket: &mut C,
    ) -> Result<Response<Body>, C::Error> {
        socket.write_all(b"\r\n").await?;
        Ok(Response {
            _state: PhantomData,
        })
    }
}

impl Response<Body> {
    pub async fn write_string<C: Connection>(
        &mut self,
        data: &str,
        socket: &mut C,
    ) -> Result<(), C::Error> {
        self.write_raw(data.as_bytes(), socket).await
    }

    pub async fn write_raw<C: Connection>(
        &mut self,
        data: &[u8],
        socket: &mut C,
    ) -> Result<(), C::Error> {
        socket.write_all(data).await
    }
}
