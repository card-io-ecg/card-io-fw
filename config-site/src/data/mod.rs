pub mod network;

use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};
use network::WifiNetwork;

pub struct WebContext {
    pub known_networks: heapless::Vec<WifiNetwork, 8>,
}
pub type SharedWebContext = Mutex<NoopRawMutex, WebContext>;
