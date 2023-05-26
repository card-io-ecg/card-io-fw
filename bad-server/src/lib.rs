#![no_std]
#![feature(async_fn_in_trait)]
#![allow(incomplete_features)]

use embassy_net::{tcp::TcpSocket, IpListenEndpoint};
use embedded_io::asynch::Write;
use object_chain::{Chain, ChainElement, Link};

use crate::{
    handler::{Handler, Request},
    method::Method,
};

pub mod handler;
pub mod method;

pub struct BadServer<H: Handler, const REQUEST_BUFFER: usize> {
    handler: H,
}

impl<'s> BadServer<(), 1024> {
    pub fn new() -> Self {
        Self { handler: () }
    }

    pub fn add_handler<H: Handler>(self, handler: H) -> BadServer<Chain<H>, 1024> {
        BadServer {
            handler: Chain::new(handler),
        }
    }
}

impl<H, const REQUEST_BUFFER: usize> BadServer<Chain<H>, REQUEST_BUFFER>
where
    H: Handler,
{
    pub fn add_handler<H2: Handler>(
        self,
        handler: H2,
    ) -> BadServer<Link<H2, Chain<H>>, REQUEST_BUFFER> {
        BadServer {
            handler: self.handler.append(handler),
        }
    }
}

impl<H, P, const REQUEST_BUFFER: usize> BadServer<Link<H, P>, REQUEST_BUFFER>
where
    H: Handler,
    P: ChainElement + Handler,
{
    pub fn add_handler<H2: Handler>(
        self,
        handler: H2,
    ) -> BadServer<Link<H2, Link<H, P>>, REQUEST_BUFFER> {
        BadServer {
            handler: self.handler.append(handler),
        }
    }
}

impl<H, const REQUEST_BUFFER: usize> BadServer<H, REQUEST_BUFFER>
where
    H: Handler,
{
    pub fn with_buffer_size<const NEW_BUFFER_SIZE: usize>(self) -> BadServer<H, NEW_BUFFER_SIZE> {
        BadServer {
            handler: self.handler,
        }
    }

    pub async fn listen(&self, socket: &mut TcpSocket<'_>, port: u16) {
        loop {
            log::info!("Wait for connection");

            let r = socket.accept(IpListenEndpoint { addr: None, port }).await;

            log::info!("Connected");

            if let Err(e) = r {
                log::warn!("connect error: {:?}", e);
                continue;
            }

            self.handle(socket).await;
        }
    }

    async fn handle(&self, socket: &mut TcpSocket<'_>) {
        let mut buffer = [0u8; REQUEST_BUFFER];
        let mut pos = 0;

        loop {
            let len = match socket.read(&mut buffer).await {
                Ok(0) => {
                    // We're here because the previous read wasn't a complete request. Reading 0
                    // means the request will not ever be completed.
                    log::warn!("read EOF");
                    break;
                }
                Ok(len) => len,
                Err(e) => {
                    log::warn!("read error: {:?}", e);
                    break;
                }
            };

            pos += len;
            log::info!("Buffer size: {pos}");

            let mut headers = [httparse::EMPTY_HEADER; 20];
            let mut req = httparse::Request::new(&mut headers);

            let res = match req.parse(&buffer) {
                Ok(res) => res,
                Err(_) => {
                    log::warn!("Parsing request failed");
                    socket.close();
                    continue;
                }
            };

            if let (Some(method), Some(path)) = (req.method, req.path) {
                let Some(method) = Method::new(method) else {
                    log::warn!("Unknown method {method}");
                    // TODO: send a proper response
                    socket.close();
                    continue;
                };

                // we can send 404 early if none of our handlers match
                let request = Request {
                    method,
                    path,
                    body: b"",
                };
                if !self.handler.handles(&request) {
                    // TODO: response builder
                    let r = socket
                        .write_all(
                            b"HTTP/1.0 404 Not Found\r\n\r\n\
                                <html>\
                                    <body>\
                                        <h1>404 Not Found</h1>\
                                    </body>\
                                </html>\r\n\
                                ",
                        )
                        .await;

                    if let Err(e) = r {
                        log::warn!("write error: {:?}", e);
                    }

                    if let Err(e) = socket.flush().await {
                        log::warn!("flush error: {:?}", e);
                    }

                    pos = 0;
                    continue;
                }
            }

            if res.is_complete() {
                let r = socket
                    .write_all(
                        b"HTTP/1.0 200 OK\r\n\r\n\
                        <html>\
                            <body>\
                                <h1>Hello Rust! Hello esp-wifi!</h1>\
                            </body>\
                        </html>\r\n\
                        ",
                    )
                    .await;

                if let Err(e) = r {
                    log::warn!("write error: {:?}", e);
                }

                if let Err(e) = socket.flush().await {
                    log::warn!("flush error: {:?}", e);
                }

                pos = 0;
            }
        }
    }
}
