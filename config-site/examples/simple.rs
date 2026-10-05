use std::{
    cell::{Cell, RefCell},
    net::SocketAddr,
    time::{Duration, Instant},
};

use config_site::{
    data::{
        network::{StationStatus, VisibleNetwork, WifiNetwork, MAX_VISIBLE_NETWORKS},
        SharedWebContext, WebContext,
    },
    ConfigSite, PairingControl, Station,
};
use edge_http::io::server::DefaultServer;
use edge_nal::TcpBind;
use network_services::pairing::{format, parse_code, Name, Refusal, Status};

const REQUEST_DURATION: Duration = Duration::from_secs(3);
const JOIN_DURATION: Duration = Duration::from_secs(3);
const UNREACHABLE_NETWORK: &str = "Failing network";

const VISIBLE_NETWORKS: [(&str, i8, bool); 5] = [
    ("Demo network 1", -52, true),
    ("Open network", -45, false),
    ("Home network", -66, true),
    (UNREACHABLE_NETWORK, -74, true),
    ("Far network", -84, true),
];

struct FakeStation<'a> {
    context: &'a SharedWebContext,
    seen: RefCell<Vec<heapless::String<32>>>,
    attempt: RefCell<Option<(heapless::String<32>, Instant)>>,
}

impl<'a> FakeStation<'a> {
    fn new(context: &'a SharedWebContext) -> Self {
        Self {
            context,
            seen: RefCell::new(Vec::new()),
            attempt: RefCell::new(None),
        }
    }
}

impl Station for FakeStation<'_> {
    /// Joins the first saved network, or the one added last. It fails for `UNREACHABLE_NETWORK`.
    async fn status(&self) -> StationStatus {
        let saved: Vec<_> = self
            .context
            .lock()
            .await
            .known_networks
            .iter()
            .map(|network| network.ssid.clone())
            .collect();

        let mut seen = self.seen.borrow_mut();
        let mut attempt = self.attempt.borrow_mut();
        if let Some(added) = saved.iter().find(|ssid| !seen.contains(ssid)) {
            *attempt = Some((added.clone(), Instant::now()));
        }
        *seen = saved;
        if attempt.is_none() {
            *attempt = seen.first().map(|ssid| (ssid.clone(), Instant::now()));
        }

        let Some((ssid, since)) = attempt.as_ref() else {
            return StationStatus::Disconnected;
        };
        let ssid = ssid.clone();
        match (
            since.elapsed() < JOIN_DURATION,
            ssid.as_str() == UNREACHABLE_NETWORK,
        ) {
            (true, _) => StationStatus::Joining(ssid),
            (false, false) => StationStatus::Joined(ssid),
            (false, true) => StationStatus::Failed(ssid),
        }
    }

    async fn visible_networks(
        &self,
        out: &mut heapless::Vec<VisibleNetwork, MAX_VISIBLE_NETWORKS>,
    ) {
        for (ssid, rssi, locked) in VISIBLE_NETWORKS {
            let network = VisibleNetwork {
                ssid: ssid.try_into().unwrap(),
                rssi,
                locked,
            };
            out.push(network).unwrap();
        }
    }

    async fn scan(&self) {}

    async fn drop_link(&self, _ssid: &str) {
        *self.attempt.borrow_mut() = None;
    }
}

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

    let context = SharedWebContext::new(WebContext::new(
        known_networks,
        "http://localhost:8080".try_into().unwrap(),
    ));

    let acceptor = edge_nal_std::Stack::new()
        .bind(SocketAddr::from(([127, 0, 0, 1], 8080)))
        .await
        .unwrap();

    let station = FakeStation::new(&context);
    let pairing = FakePairing::new();
    let handler = ConfigSite::new(&context, &station, &pairing, "Example");

    DefaultServer::new()
        .run(None, acceptor, handler)
        .await
        .unwrap();
}
