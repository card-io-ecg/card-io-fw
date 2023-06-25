pub mod network;

use network::WifiNetwork;

#[cfg(feature = "embedded")]
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};

#[cfg(feature = "std")]
use smol::lock::Mutex;

pub struct WebContext {
    pub known_networks: heapless::Vec<WifiNetwork, 8>,
}

#[cfg(feature = "embedded")]
pub type SharedWebContext = Mutex<NoopRawMutex, WebContext>;

#[cfg(feature = "std")]
pub type SharedWebContext = Mutex<WebContext>;
