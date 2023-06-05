#![feature(async_fn_in_trait)]
#![allow(incomplete_features)]

use bad_server::{
    connector::std_compat::StdTcpSocket,
    handler::{RequestHandler, StaticHandler},
    BadServer,
};
use config_site::{HEADER_FONT, INDEX_HANDLER};
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

    BadServer::new()
        .with_request_buffer_size::<2048>()
        .with_header_count::<48>()
        .with_handler(RequestHandler::get("/", INDEX_HANDLER))
        .with_handler(RequestHandler::get("/font", HEADER_FONT))
        .with_handler(RequestHandler::get(
            "/demo",
            StaticHandler(&[], b"Hello, World!"),
        ))
        .with_handler(RequestHandler::get(
            "/si",
            StaticHandler(&[], b"0.1.0-b66903b"),
        ))
        .with_handler(RequestHandler::get(
            "/kn",
            StaticHandler(&[], b"Network1\nNetwork2\nNetwork3"),
        ))
        .listen(&mut socket, 8080)
        .await;
}
