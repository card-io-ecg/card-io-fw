#![cfg_attr(not(feature = "std"), no_std)]
#![allow(stable_features)]
#![feature(async_fn_in_trait)]
#![feature(impl_trait_projections)]
#![allow(unknown_lints, async_fn_in_trait)]

#[macro_use]
extern crate logger;

use core::{fmt::Debug, marker::PhantomData};

use embedded_io::{
    asynch::{Read, Write as _},
    Io,
};
use httparse::Status;
use object_chain::{Chain, ChainElement, Link};

use crate::{
    connector::Connection,
    error_handler::{DefaultErrorHandler, ErrorHandler},
    handler::{Handler, NoHandler},
    request::Request,
    request_body::{ReadError, RequestBody},
    response::{Response, ResponseStatus},
};

pub use httparse::Header;

pub mod connector;
pub mod error_handler;
pub mod handler;
pub mod method;
pub mod request;
pub mod request_body;
pub mod response;

pub trait RequestBuffer {
    fn buffer(&mut self) -> &mut [u8];
}

impl<const N: usize> RequestBuffer for [u8; N] {
    fn buffer(&mut self) -> &mut [u8] {
        self
    }
}

impl RequestBuffer for &mut [u8] {
    fn buffer(&mut self) -> &mut [u8] {
        self
    }
}

pub struct BadServer<H: Handler, EH: ErrorHandler, RB: RequestBuffer, const MAX_HEADERS: usize> {
    handler: H,
    error_handler: EH,
    buffer: RB,
}

impl<C: Connection> Default for BadServer<NoHandler<C>, DefaultErrorHandler<C>, [u8; 1024], 32> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: Connection> BadServer<NoHandler<C>, DefaultErrorHandler<C>, [u8; 1024], 32> {
    pub const fn new() -> Self {
        Self {
            handler: NoHandler(PhantomData),
            error_handler: DefaultErrorHandler(PhantomData),
            buffer: [0; 1024],
        }
    }
}

impl<C, EH, RB: RequestBuffer, const MAX_HEADERS: usize>
    BadServer<NoHandler<C>, EH, RB, MAX_HEADERS>
where
    C: Connection,
    EH: ErrorHandler,
{
    pub fn with_handler<H: Handler>(self, handler: H) -> BadServer<Chain<H>, EH, RB, MAX_HEADERS> {
        BadServer {
            handler: Chain::new(handler),
            error_handler: self.error_handler,
            buffer: self.buffer,
        }
    }
}

impl<H, EH, RB: RequestBuffer, const MAX_HEADERS: usize> BadServer<Chain<H>, EH, RB, MAX_HEADERS>
where
    H: Handler,
    EH: ErrorHandler,
{
    pub fn with_handler<H2: Handler<Connection = H::Connection>>(
        self,
        handler: H2,
    ) -> BadServer<Link<H2, Chain<H>>, EH, RB, MAX_HEADERS> {
        BadServer {
            handler: self.handler.append(handler),
            error_handler: self.error_handler,
            buffer: self.buffer,
        }
    }
}

impl<H, EH, P, RB: RequestBuffer, const MAX_HEADERS: usize>
    BadServer<Link<H, P>, EH, RB, MAX_HEADERS>
where
    H: Handler,
    P: ChainElement + Handler<Connection = H::Connection>,
    EH: ErrorHandler<Connection = H::Connection>,
{
    pub fn with_handler<H2: Handler<Connection = H::Connection>>(
        self,
        handler: H2,
    ) -> BadServer<Link<H2, Link<H, P>>, EH, RB, MAX_HEADERS> {
        BadServer {
            handler: self.handler.append(handler),
            error_handler: self.error_handler,
            buffer: self.buffer,
        }
    }
}

pub enum HandleError<C: Io> {
    Read(ReadError<C>),
    Write(C::Error),
    TooManyHeaders,
    InternalError,
    RequestParse(httparse::Error),
}

impl<C: Io> From<ReadError<C>> for HandleError<C> {
    fn from(value: ReadError<C>) -> Self {
        HandleError::Read(value)
    }
}

impl<C> Debug for HandleError<C>
where
    C: Io,
{
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            HandleError::Read(f0) => f.debug_tuple("Read").field(&f0).finish(),
            HandleError::Write(f0) => f.debug_tuple("Write").field(&f0).finish(),
            HandleError::TooManyHeaders => f.write_str("TooManyHeaders"),
            HandleError::InternalError => f.write_str("InternalError"),
            HandleError::RequestParse(f0) => f.debug_tuple("RequestParse").field(&f0).finish(),
        }
    }
}

#[cfg(feature = "defmt")]
impl<C> defmt::Format for HandleError<C>
where
    C: Io,
    C::Error: defmt::Format,
{
    fn format(&self, f: defmt::Formatter) {
        match self {
            HandleError::Read(f0) => defmt::write!(f, "Read({})", f0),
            HandleError::Write(f0) => defmt::write!(f, "Write({})", f0),
            HandleError::TooManyHeaders => defmt::write!(f, "TooManyHeaders"),
            HandleError::InternalError => defmt::write!(f, "InternalError"),
            HandleError::RequestParse(f0) => {
                defmt::write!(
                    f,
                    "RequestParse({})",
                    match f0 {
                        httparse::Error::HeaderName => defmt::intern!("HeaderName"),
                        httparse::Error::HeaderValue => defmt::intern!("HeaderValue"),
                        httparse::Error::NewLine => defmt::intern!("NewLine"),
                        httparse::Error::Status => defmt::intern!("Status"),
                        httparse::Error::Token => defmt::intern!("Token"),
                        httparse::Error::TooManyHeaders => defmt::intern!("TooManyHeaders"),
                        httparse::Error::Version => defmt::intern!("Version"),
                    }
                )
            }
        }
    }
}

impl<H, EH, RB: RequestBuffer, const MAX_HEADERS: usize> BadServer<H, EH, RB, MAX_HEADERS>
where
    H: Handler,
    EH: ErrorHandler<Connection = H::Connection>,
{
    pub fn with_request_buffer_size<const NEW_BUFFER_SIZE: usize>(
        self,
    ) -> BadServer<H, EH, [u8; NEW_BUFFER_SIZE], MAX_HEADERS> {
        BadServer {
            handler: self.handler,
            error_handler: self.error_handler,
            buffer: [0; NEW_BUFFER_SIZE],
        }
    }
    pub fn with_request_buffer<RB2: RequestBuffer>(
        self,
        buffer: RB2,
    ) -> BadServer<H, EH, RB2, MAX_HEADERS> {
        BadServer {
            handler: self.handler,
            error_handler: self.error_handler,
            buffer,
        }
    }

    pub fn with_header_count<const NEW_HEADER_COUNT: usize>(
        self,
    ) -> BadServer<H, EH, RB, NEW_HEADER_COUNT> {
        BadServer {
            handler: self.handler,
            error_handler: self.error_handler,
            buffer: self.buffer,
        }
    }

    pub fn with_error_handler<EH2>(self, error_handler: EH2) -> BadServer<H, EH2, RB, MAX_HEADERS>
    where
        EH2: ErrorHandler<Connection = H::Connection>,
    {
        BadServer {
            handler: self.handler,
            error_handler,
            buffer: self.buffer,
        }
    }

    pub async fn listen(&mut self, socket: &mut H::Connection, port: u16) {
        loop {
            info!("Wait for connection");

            if let Err(e) = socket.listen(port).await {
                warn!("Connect error: {:?}", e);
                socket.close();
                continue;
            }

            info!("Connected");
            let handle_result = self.handle(socket).await;

            if let Err(_e) = socket.flush().await {
                warn!("Flush error");
                //warn!("Flush error: {:?}", e);
            }

            // Handle errors after flushing
            if let Err(_e) = handle_result {
                warn!("Handle error");
                //warn!("Handle error: {:?}", e);
            }

            socket.close();
            info!("Done");
        }
    }

    async fn load_headers<'b>(
        buffer: &'b mut [u8],
        socket: &mut H::Connection,
    ) -> Result<(&'b [u8], &'b [u8]), HandleError<H::Connection>> {
        let mut pos = 0;
        while pos < buffer.len() {
            match socket.read(&mut buffer[pos..]).await {
                Ok(0) => {
                    // We're here because the previous read wasn't a complete request. Reading 0
                    // means the request will not ever be completed.
                    warn!("read EOF");
                    return Err(HandleError::Read(ReadError::UnexpectedEof));
                }
                Ok(len) => pos += len,
                Err(e) => {
                    warn!("read error");
                    //warn!("read error: {:?}", e);
                    return Err(HandleError::Read(ReadError::Io(e)));
                }
            }

            debug!("Buffer size: {}", pos);

            let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
            let mut req = httparse::Request::new(&mut headers);

            match req.parse(&buffer[..pos]) {
                Ok(Status::Complete(header_size)) => {
                    let (header, body) = buffer[..pos].split_at(header_size);
                    return Ok((header, body));
                }
                Ok(Status::Partial) => {
                    // We need to read more
                }
                Err(e) => {
                    warn!("Parsing request failed");
                    //warn!("Parsing request failed: {}", e);
                    return Err(HandleError::RequestParse(e));
                }
            };
        }

        // Can't read more, but we don't have a complete request yet.
        Err(HandleError::TooManyHeaders)
    }

    async fn handle(
        &mut self,
        socket: &mut H::Connection,
    ) -> Result<(), HandleError<H::Connection>> {
        let status = match Self::load_headers(self.buffer.buffer(), socket).await {
            Ok((header, body)) => {
                let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
                let mut req = httparse::Request::new(&mut headers);
                if req.parse(header).is_err() {
                    ResponseStatus::InternalServerError
                } else {
                    match RequestBody::new(req.headers, body, socket) {
                        Ok(body) => match Request::new(req, body) {
                            Ok(request) if self.handler.handles(&request) => {
                                return self.handler.handle(request).await;
                            }
                            Ok(_request) => ResponseStatus::NotFound,
                            Err(status) => status,
                        },
                        Err(err) => err.into(),
                    }
                }
            }
            Err(HandleError::TooManyHeaders) => ResponseStatus::RequestEntityTooLarge,
            Err(HandleError::InternalError) => ResponseStatus::InternalServerError,
            Err(HandleError::RequestParse(_)) => ResponseStatus::BadRequest,
            Err(HandleError::Read(ReadError::Io(_))) => ResponseStatus::InternalServerError,
            Err(HandleError::Read(ReadError::Encoding)) => ResponseStatus::BadRequest,
            Err(HandleError::Read(ReadError::UnexpectedEof)) => ResponseStatus::BadRequest,
            Err(e @ HandleError::Write(_)) => return Err(e),
        };

        self.error_handler
            .handle(status, Response::new(socket))
            .await
    }
}
