#![feature(async_fn_in_trait)]
#![allow(incomplete_features)]

use bad_server::{
    connector::{std_compat::StdTcpSocket, Connection},
    handler::RequestHandler,
    request::Request,
    response::ResponseStatus,
    BadServer, HandleError,
};
use log::LevelFilter;

fn main() {
    simple_logger::SimpleLogger::new()
        .with_level(LevelFilter::Info)
        .env()
        .init()
        .unwrap();

    smol::block_on(run());
}

struct RootHandler;
impl<C: Connection> RequestHandler<C> for RootHandler {
    async fn handle(&self, request: Request<'_, '_, C>) -> Result<(), HandleError<C>> {
        let response = request.send_response(ResponseStatus::Ok).await?;
        let mut response = response.start_body().await?;
        response.write_string("Hello, world!").await?;
        Ok(())
    }
}

pub async fn run() {
    let mut socket = StdTcpSocket::new();

    BadServer::new()
        .with_request_buffer_size::<2048>()
        .with_header_count::<48>()
        .with_handler(RequestHandler::get("/", RootHandler))
        .listen(&mut socket, 8080)
        .await;
}
