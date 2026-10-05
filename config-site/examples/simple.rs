use core::fmt::{Debug, Display};
use std::net::SocketAddr;

use config_site::{
    data::{network::WifiNetwork, SharedWebContext, WebContext},
    ConfigSite,
};
use edge_http::{
    io::{
        server::{Connection, DefaultServer, Handler},
        Error,
    },
    Method,
};
use edge_nal::TcpBind;
use embedded_io_async::{Read, Write};

fn main() {
    smol::block_on(run());
}

pub async fn run() {
    let mut known_networks = heapless::Vec::<_, 8>::new();

    known_networks
        .push(WifiNetwork {
            ssid: "Demo network 1".try_into().unwrap(),
            pass: heapless::String::new(),
        })
        .unwrap();
    known_networks
        .push(WifiNetwork {
            ssid: "Demo network 2".try_into().unwrap(),
            pass: heapless::String::new(),
        })
        .unwrap();

    let context = SharedWebContext::new(WebContext {
        known_networks,
        backend_url: "http://localhost:8080".try_into().unwrap(),
    });

    let acceptor = edge_nal_std::Stack::new()
        .bind(SocketAddr::from(([127, 0, 0, 1], 8080)))
        .await
        .unwrap();

    let handler = Site {
        config: ConfigSite::new(&context, "Example"),
    };

    DefaultServer::new()
        .run(None, acceptor, handler)
        .await
        .unwrap();
}

struct Site<'a> {
    config: ConfigSite<'a>,
}

impl Handler for Site<'_> {
    type Error<E>
        = Error<E>
    where
        E: Debug;

    async fn handle<T, const N: usize>(
        &self,
        task_id: impl Display + Copy,
        conn: &mut Connection<'_, T, N>,
    ) -> Result<(), Self::Error<T::Error>>
    where
        T: Read + Write + edge_nal::TcpSplit,
    {
        let headers = conn.headers()?;
        if !(matches!(headers.method, Method::Get) && headers.path == "/vn") {
            return self.config.handle(task_id, conn).await;
        }

        conn.initiate_response(200, None, &[("Content-Type", "text/plain; charset=utf-8")])
            .await?;
        conn.write_all(b"Demo network 1\nDemo network 3\n").await
    }
}
