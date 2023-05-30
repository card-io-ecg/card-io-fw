#![no_std]
#![feature(async_fn_in_trait)]
#![feature(impl_trait_projections)]
#![allow(incomplete_features)]

use core::{fmt::Debug, marker::PhantomData};

use embedded_io::asynch::{Read, Write as _};
use httparse::Status;
use object_chain::{Chain, ChainElement, Link};

use crate::{
    connector::Connection,
    error_handler::{DefaultErrorHandler, ErrorHandler, ResponseBuilder},
    handler::{Handler, NoHandler},
    request::Request,
    request_body::{BodyTypeError, ReadError, RequestBody, RequestBodyError},
    request_context::RequestContext,
    response::ResponseStatus,
};

pub mod connector;
pub mod error_handler;
pub mod handler;
pub mod method;
pub mod request;
pub mod request_body;
pub mod request_context;
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

impl<C: Connection> BadServer<NoHandler<C>, DefaultErrorHandler<C>, 1024, 32> {
    pub fn new() -> Self {
        Self {
            handler: NoHandler(PhantomData),
            error_handler: DefaultErrorHandler(PhantomData),
        }
    }

    pub fn add_handler<H: Handler>(
        self,
        handler: H,
    ) -> BadServer<Chain<H>, DefaultErrorHandler<C>, 1024, 32> {
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
    pub fn add_handler<H2: Handler<Connection = H::Connection>>(
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
    pub fn add_handler<H2: Handler<Connection = H::Connection>>(
        self,
        handler: H2,
    ) -> BadServer<Link<H2, Link<H, P>>, EH, REQUEST_BUFFER, MAX_HEADERS> {
        BadServer {
            handler: self.handler.append(handler),
            error_handler: self.error_handler,
        }
    }
}

pub enum HandleError<C: Connection> {
    Read(ReadError<C>),
    Write(C::Error),
    TooManyHeaders,
    RequestParse(httparse::Error),
    Request,
    Other, //Tech debt, replace with a real error
}

impl<C> Debug for HandleError<C>
where
    C: Connection,
{
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            HandleError::Read(f0) => f.debug_tuple("Read").field(&f0).finish(),
            HandleError::Write(f0) => f.debug_tuple("Write").field(&f0).finish(),
            HandleError::TooManyHeaders => f.write_str("TooManyHeaders"),
            HandleError::RequestParse(f0) => f.debug_tuple("RequestParse").field(&f0).finish(),
            HandleError::Request => f.write_str("Request"),
            HandleError::Other => f.write_str("Other"),
        }
    }
}

impl<H, EH, const REQUEST_BUFFER: usize, const MAX_HEADERS: usize>
    BadServer<H, EH, REQUEST_BUFFER, MAX_HEADERS>
where
    H: Handler,
    EH: ErrorHandler<Connection = H::Connection>,
{
    pub fn with_buffer_size<const NEW_BUFFER_SIZE: usize>(
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

    pub fn with_error_handler<EH2: ErrorHandler<Connection = H::Connection>>(
        self,
        error_handler: EH2,
    ) -> BadServer<H, EH2, REQUEST_BUFFER, MAX_HEADERS> {
        BadServer {
            handler: self.handler,
            error_handler,
        }
    }

    pub async fn listen(&self, socket: &mut H::Connection, port: u16) {
        loop {
            log::info!("Wait for connection");

            let r = socket.listen(port).await;

            log::info!("Connected");

            if let Err(e) = r {
                log::warn!("connect error: {:?}", e);
                socket.close();
                continue;
            }

            let handle_result = self.handle(socket).await;

            if let Err(e) = socket.flush().await {
                log::warn!("flush error: {:?}", e);
            }

            // Handle errors after flushing
            if let Err(e) = handle_result {
                log::warn!("handle error: {:?}", e);
                socket.close();
            }
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
                    log::warn!("Parsing request failed");
                    return Err(HandleError::RequestParse(e));
                }
            };
        }

        // Can't read more, but we don't have a complete request yet.
        Err(HandleError::TooManyHeaders)
    }

    async fn handle(&self, socket: &mut H::Connection) -> Result<(), HandleError<H::Connection>> {
        let mut buffer = [0u8; REQUEST_BUFFER];

        match self.load_headers(&mut buffer, socket).await {
            Ok((header, body)) => {
                let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
                let mut req = httparse::Request::new(&mut headers);
                req.parse(header).unwrap();

                let body = match RequestBody::new(req.headers, body) {
                    Ok(body) => body,
                    Err(RequestBodyError::BodyType(BodyTypeError::IncorrectEncoding)) => {
                        // A server that receives a request message with a transfer coding it does
                        // not understand SHOULD respond with 501 (Not Implemented).

                        // Note: this is a bit of a stretch, because this error is for incorrectly
                        // encoded strings, but I think technically we are correct.
                        return self
                            .error_handler
                            .handle(ResponseStatus::NotImplemented, ResponseBuilder::new(socket))
                            .await;
                    }
                    Err(RequestBodyError::BodyType(
                        BodyTypeError::ConflictingHeaders
                        | BodyTypeError::IncorrectTransferEncoding, // must return 400
                    )) => {
                        return self
                            .error_handler
                            .handle(ResponseStatus::BadRequest, ResponseBuilder::new(socket))
                            .await;
                    }
                };

                match Request::new(req, body) {
                    Ok(request) => {
                        let request = RequestContext::new(socket, request);
                        if self.handler.handles(&request) {
                            self.handler.handle(request).await
                        } else {
                            self.error_handler
                                .handle(ResponseStatus::NotFound, ResponseBuilder::new(socket))
                                .await
                        }
                    }
                    Err(_) => {
                        // TODO: send a proper response
                        socket.close();
                        Err(HandleError::Request)
                    }
                }
            }
            Err(e) => Err(e),
        }
    }
}
