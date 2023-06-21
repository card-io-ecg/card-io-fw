#![cfg_attr(not(feature = "std"), no_std)]
#![feature(async_fn_in_trait)]
#![feature(impl_trait_projections)]
#![allow(incomplete_features)]

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

pub struct BadServer<
    H: Handler,
    EH: ErrorHandler,
    const REQUEST_BUFFER: usize,
    const MAX_HEADERS: usize,
> {
    handler: H,
    error_handler: EH,
}

impl<C: Connection> Default for BadServer<NoHandler<C>, DefaultErrorHandler<C>, 1024, 32> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: Connection> BadServer<NoHandler<C>, DefaultErrorHandler<C>, 1024, 32> {
    pub const fn new() -> Self {
        Self {
            handler: NoHandler(PhantomData),
            error_handler: DefaultErrorHandler(PhantomData),
        }
    }
}

impl<C, EH, const REQUEST_BUFFER: usize, const MAX_HEADERS: usize>
    BadServer<NoHandler<C>, EH, REQUEST_BUFFER, MAX_HEADERS>
where
    C: Connection,
    EH: ErrorHandler,
{
    pub fn with_handler<H: Handler>(
        self,
        handler: H,
    ) -> BadServer<Chain<H>, EH, REQUEST_BUFFER, MAX_HEADERS> {
        BadServer {
            handler: Chain::new(handler),
            error_handler: self.error_handler,
        }
    }
}

impl<H, EH, const REQUEST_BUFFER: usize, const MAX_HEADERS: usize>
    BadServer<Chain<H>, EH, REQUEST_BUFFER, MAX_HEADERS>
where
    H: Handler,
    EH: ErrorHandler,
{
    pub fn with_handler<H2: Handler<Connection = H::Connection>>(
        self,
        handler: H2,
    ) -> BadServer<Link<H2, Chain<H>>, EH, REQUEST_BUFFER, MAX_HEADERS> {
        BadServer {
            handler: self.handler.append(handler),
            error_handler: self.error_handler,
        }
    }
}

impl<H, EH, P, const REQUEST_BUFFER: usize, const MAX_HEADERS: usize>
    BadServer<Link<H, P>, EH, REQUEST_BUFFER, MAX_HEADERS>
where
    H: Handler,
    P: ChainElement + Handler<Connection = H::Connection>,
    EH: ErrorHandler<Connection = H::Connection>,
{
    pub fn with_handler<H2: Handler<Connection = H::Connection>>(
        self,
        handler: H2,
    ) -> BadServer<Link<H2, Link<H, P>>, EH, REQUEST_BUFFER, MAX_HEADERS> {
        BadServer {
            handler: self.handler.append(handler),
            error_handler: self.error_handler,
        }
    }
}

pub enum HandleError<C: Io> {
    Read(ReadError<C>),
    Write(C::Error),
    TooManyHeaders,
    RequestParse(httparse::Error),
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
            HandleError::RequestParse(f0) => f.debug_tuple("RequestParse").field(&f0).finish(),
        }
    }
}

impl<H, EH, const REQUEST_BUFFER: usize, const MAX_HEADERS: usize>
    BadServer<H, EH, REQUEST_BUFFER, MAX_HEADERS>
where
    H: Handler,
    EH: ErrorHandler<Connection = H::Connection>,
{
    pub fn with_request_buffer_size<const NEW_BUFFER_SIZE: usize>(
        self,
    ) -> BadServer<H, EH, NEW_BUFFER_SIZE, MAX_HEADERS> {
        BadServer {
            handler: self.handler,
            error_handler: self.error_handler,
        }
    }

    pub fn with_header_count<const NEW_HEADER_COUNT: usize>(
        self,
    ) -> BadServer<H, EH, REQUEST_BUFFER, NEW_HEADER_COUNT> {
        BadServer {
            handler: self.handler,
            error_handler: self.error_handler,
        }
    }

    pub fn with_error_handler<EH2>(
        self,
        error_handler: EH2,
    ) -> BadServer<H, EH2, REQUEST_BUFFER, MAX_HEADERS>
    where
        EH2: ErrorHandler<Connection = H::Connection>,
    {
        BadServer {
            handler: self.handler,
            error_handler,
        }
    }

    pub async fn listen(&self, socket: &mut H::Connection, port: u16) {
        loop {
            log::info!("Wait for connection");

            if let Err(e) = socket.listen(port).await {
                log::warn!("Connect error: {:?}", e);
                socket.close();
                continue;
            }

            log::info!("Connected");
            let handle_result = self.handle(socket).await;

            if let Err(e) = socket.flush().await {
                log::warn!("Flush error: {:?}", e);
            }

            // Handle errors after flushing
            if let Err(e) = handle_result {
                log::warn!("Handle error: {:?}", e);
            }

            socket.close();
            log::info!("Done");
        }
    }

    async fn load_headers<'b>(
        &self,
        buffer: &'b mut [u8],
        socket: &mut H::Connection,
    ) -> Result<(&'b [u8], &'b [u8]), HandleError<H::Connection>> {
        let mut pos = 0;
        while pos < buffer.len() {
            match socket.read(&mut buffer[pos..]).await {
                Ok(0) => {
                    // We're here because the previous read wasn't a complete request. Reading 0
                    // means the request will not ever be completed.
                    log::warn!("read EOF");
                    return Err(HandleError::Read(ReadError::UnexpectedEof));
                }
                Ok(len) => pos += len,
                Err(e) => {
                    log::warn!("read error: {:?}", e);
                    return Err(HandleError::Read(ReadError::Io(e)));
                }
            }

            log::debug!("Buffer size: {pos}");

            let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
            let mut req = httparse::Request::new(&mut headers);

            match req.parse(&buffer[0..pos]) {
                Ok(Status::Complete(header_size)) => {
                    let (header, body) = buffer[..pos].split_at(header_size);
                    return Ok((header, body));
                }
                Ok(Status::Partial) => {
                    // We need to read more
                }
                Err(e) => {
                    log::warn!("Parsing request failed: {e}");
                    return Err(HandleError::RequestParse(e));
                }
            };
        }

        // Can't read more, but we don't have a complete request yet.
        Err(HandleError::TooManyHeaders)
    }

    async fn handle(&self, socket: &mut H::Connection) -> Result<(), HandleError<H::Connection>> {
        let mut buffer = [0u8; REQUEST_BUFFER];

        let status = match self.load_headers(&mut buffer, socket).await {
            Ok((header, body)) => {
                let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
                let mut req = httparse::Request::new(&mut headers);
                req.parse(header).unwrap();

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
            Err(HandleError::TooManyHeaders) => ResponseStatus::RequestEntityTooLarge,
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
