#![no_std]
#![feature(async_fn_in_trait)]
#![feature(impl_trait_projections)]
#![allow(incomplete_features)]

use core::{
    fmt::{Debug, Write},
    marker::PhantomData,
};

use embedded_io::asynch::{Read, Write as _};
use httparse::Status;
use object_chain::{Chain, ChainElement, Link};

use crate::{
    connector::Connection,
    handler::{Handler, NoHandler, Request},
    request_body::{BodyTypeError, ReadError, RequestBody, RequestBodyError},
    response::{Response, ResponseStatus},
};

pub mod connector;
pub mod handler;
pub mod method;
pub mod request_body;
pub mod response;

pub struct BadServer<H: Handler, const REQUEST_BUFFER: usize, const MAX_HEADERS: usize> {
    handler: H,
}

impl<C: Connection> BadServer<NoHandler<C>, 1024, 32> {
    pub fn new() -> Self {
        Self {
            handler: NoHandler(PhantomData),
        }
    }

    pub fn add_handler<H: Handler>(self, handler: H) -> BadServer<Chain<H>, 1024, 32> {
        BadServer {
            handler: Chain::new(handler),
        }
    }
}

impl<H, const REQUEST_BUFFER: usize, const MAX_HEADERS: usize>
    BadServer<Chain<H>, REQUEST_BUFFER, MAX_HEADERS>
where
    H: Handler,
{
    pub fn add_handler<H2: Handler<Connection = H::Connection>>(
        self,
        handler: H2,
    ) -> BadServer<Link<H2, Chain<H>>, REQUEST_BUFFER, MAX_HEADERS> {
        BadServer {
            handler: self.handler.append(handler),
        }
    }
}

impl<H, P, const REQUEST_BUFFER: usize, const MAX_HEADERS: usize>
    BadServer<Link<H, P>, REQUEST_BUFFER, MAX_HEADERS>
where
    H: Handler,
    P: ChainElement + Handler<Connection = H::Connection>,
{
    pub fn add_handler<H2: Handler<Connection = H::Connection>>(
        self,
        handler: H2,
    ) -> BadServer<Link<H2, Link<H, P>>, REQUEST_BUFFER, MAX_HEADERS> {
        BadServer {
            handler: self.handler.append(handler),
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

impl<H, const REQUEST_BUFFER: usize, const MAX_HEADERS: usize>
    BadServer<H, REQUEST_BUFFER, MAX_HEADERS>
where
    H: Handler,
{
    pub fn with_buffer_size<const NEW_BUFFER_SIZE: usize>(
        self,
    ) -> BadServer<H, NEW_BUFFER_SIZE, MAX_HEADERS> {
        BadServer {
            handler: self.handler,
        }
    }

    pub fn with_header_count<const NEW_HEADER_COUNT: usize>(
        self,
    ) -> BadServer<H, REQUEST_BUFFER, NEW_HEADER_COUNT> {
        BadServer {
            handler: self.handler,
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

                let body = match RequestBody::new(req.headers, body, socket) {
                    Ok(body) => body,
                    Err(RequestBodyError::BodyType(BodyTypeError::IncorrectEncoding)) => {
                        // A server that receives a request message with a transfer coding it does
                        // not understand SHOULD respond with 501 (Not Implemented).

                        // Note: this is a bit of a stretch, because this error is for incorrectly
                        // encoded strings, but I think technically we are correct.
                        return ErrorResponse(ResponseStatus::NotImplemented)
                            .send(socket)
                            .await
                            .map_err(HandleError::Write);
                    }
                    Err(RequestBodyError::BodyType(BodyTypeError::ConflictingHeaders)) => {
                        return ErrorResponse(ResponseStatus::BadRequest)
                            .send(socket)
                            .await
                            .map_err(HandleError::Write);
                    }
                };

                match Request::new(req, body) {
                    Ok(request) => {
                        if self.handler.handles(&request) {
                            self.handler.handle(request).await;
                            // TODO
                            Ok(())
                        } else {
                            return ErrorResponse(ResponseStatus::NotFound)
                                .send(socket)
                                .await
                                .map_err(HandleError::Write);
                        }
                    }
                    Err(_) => {
                        // TODO: send a proper response
                        socket.close();
                        return Err(HandleError::Request);
                    }
                }
            }
            Err(e) => return Err(e),
        }
    }
}

struct ErrorResponse(ResponseStatus);

impl ErrorResponse {
    async fn send<C: Connection>(&self, socket: &mut C) -> Result<(), C::Error> {
        let mut response = Response::new(socket)
            .send_status(self.0)
            .await?
            .end_headers()
            .await?;

        let mut body = heapless::String::<128>::new();
        let _ = write!(
            &mut body,
            "<html><body><h1>{code} {reason}</h1></body></html>\r\n",
            code = self.0 as u16,
            reason = self.0.name(),
        );
        response.write_string(&body).await?;

        Ok(())
    }
}
