#![feature(async_fn_in_trait)]
#![allow(incomplete_features)]

use bad_server::{
    connector::{std_compat::StdTcpSocket, Connection},
    handler::RequestHandler,
    request::Request,
    response::ResponseStatus,
    BadServer, HandleError, Header,
};
use config_site::INDEX_HANDLER;
use log::LevelFilter;

fn main() {
    simple_logger::SimpleLogger::new()
        .with_level(LevelFilter::Info)
        .env()
        .init()
        .unwrap();

    smol::block_on(run());
}

struct DemoHandler;
impl<C: Connection> RequestHandler<C> for DemoHandler {
    async fn handle(&self, request: Request<'_, '_, C>) -> Result<(), HandleError<C>> {
        let mut response = request.send_response(ResponseStatus::Ok).await?;
        response
            .send_header(Header {
                name: "Content-Length",
                value: b"13",
            })
            .await?;
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
        .with_handler(RequestHandler::get("/", INDEX_HANDLER))
        .with_handler(RequestHandler::get("/demo", DemoHandler))
        .listen(&mut socket, 8080)
        .await;
}
