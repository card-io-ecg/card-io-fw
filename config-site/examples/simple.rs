#![feature(async_fn_in_trait)]
#![allow(incomplete_features)]

use bad_server::{
    connector::std_compat::StdTcpSocket,
    handler::{RequestHandler, StaticHandler},
    BadServer,
};
use config_site::INDEX_HANDLER;
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
        .with_handler(RequestHandler::get(
            "/demo",
            StaticHandler(&[], b"Hello, World!"),
        ))
        .listen(&mut socket, 8080)
        .await;
}
