use core::fmt::{Debug, Display};
use std::{
    cell::Cell,
    net::SocketAddr,
    time::{Duration, Instant},
};

use config_site::{
    data::{network::WifiNetwork, SharedWebContext, WebContext},
    ConfigSite, PairingControl,
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
use network_services::pairing::{format, parse_code, Name, Refusal, Status};

const REQUEST_DURATION: Duration = Duration::from_secs(3);

struct FakePairing {
    name: Name,
    status: Cell<Status>,
    started: Cell<Instant>,
}

impl FakePairing {
    fn new() -> Self {
        Self {
            name: Name::from_mac([0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6]),
            status: Cell::new(Status::Unpaired(None)),
            started: Cell::new(Instant::now()),
        }
    }

    fn status(&self) -> Status {
        let finished = self.started.get().elapsed() >= REQUEST_DURATION;
        let status = match self.status.get() {
            Status::Pairing if finished => Status::Paired(None),
            Status::Unpairing if finished => Status::Unpaired(None),
            status => status,
        };
        self.status.set(status);
        status
    }

    fn start(&self, status: Status) {
        self.status.set(status);
        self.started.set(Instant::now());
    }
}

impl PairingControl for FakePairing {
    async fn status_line(&self, out: &mut impl core::fmt::Write) {
        format(&self.name, &self.status(), out).unwrap();
    }

    async fn pair(&self, typed: &str) -> Result<(), Refusal> {
        parse_code(typed).ok_or(Refusal::Code)?;

        match self.status() {
            Status::Unpaired(_) => {
                self.start(Status::Pairing);
                Ok(())
            }
            Status::Paired(_) => Err(Refusal::Paired),
            Status::Pairing | Status::Unpairing => Err(Refusal::Busy),
        }
    }

    async fn unpair(&self) -> Result<(), Refusal> {
        match self.status() {
            Status::Paired(_) => {
                self.start(Status::Unpairing);
                Ok(())
            }
            Status::Unpaired(_) => Err(Refusal::Unpaired),
            Status::Pairing | Status::Unpairing => Err(Refusal::Busy),
        }
    }
}

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

    let pairing = FakePairing::new();
    let handler = Site {
        config: ConfigSite::new(&context, &pairing, "Example"),
    };

    DefaultServer::new()
        .run(None, acceptor, handler)
        .await
        .unwrap();
}

struct Site<'a> {
    config: ConfigSite<'a, FakePairing>,
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
