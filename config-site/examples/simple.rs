#![feature(async_fn_in_trait)]

use bad_server::connector::std_compat::StdTcpSocket;
use config_site::data::{network::WifiNetwork, SharedWebContext, WebContext};
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

    let mut known_networks = heapless::Vec::<_, 8>::new();

    known_networks
        .push(WifiNetwork {
            ssid: heapless::String::from("Demo network 1"),
            pass: heapless::String::new(),
        })
        .unwrap();
    known_networks
        .push(WifiNetwork {
            ssid: heapless::String::from("Demo network 2"),
            pass: heapless::String::new(),
        })
        .unwrap();

    let context = SharedWebContext::new(WebContext {
        known_networks,
        backend_url: heapless::String::from("http://localhost:8080"),
    });

    config_site::create(&context, "Example")
        .with_handler(RequestHandler::get("/vn", VisibleNetworks))
        .with_request_buffer_size::<2048>()
        .with_header_count::<48>()
        .listen(&mut socket, 8080)
        .await;
}

struct VisibleNetworks;
impl<C: Connection> RequestHandler<C> for VisibleNetworks {
    async fn handle(&self, request: Request<'_, '_, C>) -> Result<(), HandleError<C>> {
        let response = request.start_response(ResponseStatus::Ok).await?;
        let mut response = response.start_chunked_body().await?;

        response.write("Demo network 1").await?;
        response.write("\n").await?;

        response.write("Demo network 3").await?;
        response.write("\n").await?;

        response.end_chunked_response().await
    }
}
