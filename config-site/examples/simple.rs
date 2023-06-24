#![feature(async_fn_in_trait)]
#![allow(incomplete_features)]

use bad_server::{
    connector::std_compat::StdTcpSocket,
    handler::{RequestHandler, StaticHandler},
    BadServer,
};
use config_site::{
    data::{SharedWebContext, WebContext},
    handlers::{list_known_networks::ListKnownNetworks, HEADER_FONT, INDEX_HANDLER},
};
use log::LevelFilter;

fn main() {
    simple_logger::SimpleLogger::new()
        .with_level(LevelFilter::Debug)
        .env()
        .init()
        .unwrap();

    smol::block_on(run());
}

pub async fn run() {
    let mut socket = StdTcpSocket::new();

    let context = SharedWebContext::new(WebContext {
        known_networks: heapless::Vec::new(),
    });

    BadServer::new()
        .with_request_buffer_size::<2048>()
        .with_header_count::<48>()
        .with_handler(RequestHandler::get("/", INDEX_HANDLER))
        .with_handler(RequestHandler::get("/font", HEADER_FONT))
        .with_handler(RequestHandler::get(
            "/demo",
            StaticHandler::new(&[], b"Hello, World!"),
        ))
        .with_handler(RequestHandler::get(
            "/si",
            StaticHandler::new(&[], b"0.1.0-b66903b"),
        ))
        .with_handler(RequestHandler::get(
            "/kn",
            ListKnownNetworks { context: &context },
        ))
        .listen(&mut socket, 8080)
        .await;
}
