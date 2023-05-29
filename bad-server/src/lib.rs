#![no_std]
#![feature(async_fn_in_trait)]
#![feature(impl_trait_projections)]
#![allow(incomplete_features)]

use core::{fmt::Write as _, marker::PhantomData};

use embedded_io::asynch::Read;
use httparse::Status;
use object_chain::{Chain, ChainElement, Link};

use crate::{
    connector::Connection,
    handler::{Handler, NoHandler, Request},
    request_body::{BodyTypeError, RequestBody, RequestBodyError},
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

            match r {
                Ok(_) => self.handle(socket).await,
                Err(e) => {
                    log::warn!("connect error: {:?}", e);
                }
            }
        }
    }

    async fn load_headers(
        &self,
        buffer: &mut [u8],
        socket: &mut H::Connection,
    ) -> Result<(usize, usize), ()> {
        let mut pos = 0;
        while pos < buffer.len() {
            match socket.read(&mut buffer[pos..]).await {
                Ok(0) => {
                    // We're here because the previous read wasn't a complete request. Reading 0
                    // means the request will not ever be completed.
                    log::warn!("read EOF");
                    return Err(());
                }
                Ok(len) => pos += len,
                Err(e) => {
                    log::warn!("read error: {:?}", e);
                    return Err(());
                }
            }

            log::debug!("Buffer size: {pos}");

            let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
            let mut req = httparse::Request::new(&mut headers);

            match req.parse(&buffer[0..pos]) {
                Ok(Status::Complete(header_size)) => return Ok((header_size, pos)),
                Ok(Status::Partial) => {
                    // We need to read more
                }
                Err(_) => {
                    log::warn!("Parsing request failed");
                    return Err(());
                }
            };
        }

        // Can't read more, but we don't have a complete request yet.
        Err(())
    }

    async fn handle(&self, mut socket: &mut H::Connection) {
        let mut buffer = [0u8; REQUEST_BUFFER];

        match self.load_headers(&mut buffer, socket).await {
            Ok((header_size, total_read)) => {
                let (header_buf, body_buf) = buffer.split_at_mut(header_size);

                let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
                let mut req = httparse::Request::new(&mut headers);
                req.parse(header_buf).unwrap();

                let read_body = total_read - header_size;
                let body = RequestBody::from_preloaded(req.headers, body_buf, read_body);

                let body = match body {
                    Ok(body) => body,
                    Err(RequestBodyError::BodyType(BodyTypeError::IncorrectEncoding)) => {
                        // A server that receives a request message with a transfer coding it does
                        // not understand SHOULD respond with 501 (Not Implemented).

                        // Note: this is a bit of a stretch, because this error is for incorrectly
                        // encoded strings, but I think technically we are correct.
                        let _ = ErrorResponse {
                            status: ResponseStatus::NotImplemented,
                        }
                        .send(socket)
                        .await;
                        return;
                    }
                    Err(RequestBodyError::BodyType(BodyTypeError::ConflictingHeaders)) => {
                        let _ = ErrorResponse {
                            status: ResponseStatus::BadRequest,
                        }
                        .send(socket)
                        .await;
                        return;
                    }
                };

                match Request::new(req, body, socket) {
                    Ok(request) => {
                        if self.handler.handles(&request) {
                            self.handler.handle(request).await;
                        } else {
                            let _ = ErrorResponse {
                                status: ResponseStatus::NotFound,
                            }
                            .send(socket)
                            .await;
                        }
                    }
                    Err(_) => {
                        // TODO: send a proper response
                        socket.close();
                    }
                }
            }
            Err(_) => todo!(),
        }
    }
}

struct ErrorResponse {
    status: ResponseStatus,
}

impl ErrorResponse {
    async fn send<C: Connection>(&self, socket: &mut C) -> Result<(), C::Error> {
        let mut response = Response::send_headers(socket, self.status, &[]).await?;

        let mut body = heapless::String::<128>::new();
        let code = self.status as u16;
        let reason = self.status.name();
        // build response body
        let _ = write!(
            &mut body,
            "<html><body><h1>{code} {reason}</h1></body></html>\r\n"
        );
        response.write(&body).await?;

        if let Err(e) = socket.flush().await {
            log::warn!("flush error: {:?}", e);
        }

        Ok(())
    }
}
