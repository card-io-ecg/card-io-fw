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
};

pub mod connector;
pub mod handler;
pub mod method;
pub mod request_body;

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
                        ErrorResponse { code: 501 }.send(socket).await;
                        return;
                    }
                    Err(RequestBodyError::BodyType(BodyTypeError::ConflictingHeaders)) => {
                        ErrorResponse { code: 400 }.send(socket).await;
                        return;
                    }
                };

                match Request::new(req, body, socket) {
                    Ok(request) => {
                        if self.handler.handles(&request) {
                            self.handler.handle(request).await;
                        } else {
                            ErrorResponse { code: 404 }.send(socket).await;
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
    code: u16,
}

impl ErrorResponse {
    async fn send(&self, socket: &mut impl Connection) {
        const KNOWN_CODES: [(u16, &str); 4] = [
            (400, "Bad Request"),
            (404, "Not Found"),
            (500, "Internal Server Error"),
            (501, "Not Implemented"),
        ];

        // if code is not known, send 500
        let (code, reason) = KNOWN_CODES
            .iter()
            .find(|(code, _)| *code == self.code)
            .cloned()
            .unwrap_or((500, "Internal Server Error"));

        let mut body = heapless::String::<128>::new();
        // build response
        let _ = write!(&mut body, "HTTP/1.0 501 Not Implemented\r\n");
        let _ = write!(&mut body, "\r\n");
        let _ = write!(
            &mut body,
            "<html><body><h1>{code} {reason}</h1></body></html>\r\n"
        );

        if let Err(e) = socket.write_all(body.as_bytes()).await {
            log::warn!("write error: {:?}", e);
        }

        if let Err(e) = socket.flush().await {
            log::warn!("flush error: {:?}", e);
        }
    }
}
