pub mod network;

use network::WifiNetwork;

#[cfg(feature = "embedded")]
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};

#[cfg(feature = "std")]
use smol::lock::Mutex;

pub struct WebContext {
    pub known_networks: heapless::Vec<WifiNetwork, 8>,
    pub backend_url: heapless::String<64>,
    backend_url_when_opened: heapless::String<64>,
}

impl WebContext {
    /// Creates the context when setup opens. Its `backend_url` is the one that
    /// `POST /settings/discard` restores.
    pub fn new(
        known_networks: heapless::Vec<WifiNetwork, 8>,
        backend_url: heapless::String<64>,
    ) -> Self {
        Self {
            known_networks,
            backend_url_when_opened: backend_url.clone(),
            backend_url,
        }
    }

    pub fn discard_settings(&mut self) {
        self.backend_url.clone_from(&self.backend_url_when_opened);
    }
}

#[cfg(feature = "embedded")]
pub type SharedWebContext = Mutex<NoopRawMutex, WebContext>;

#[cfg(feature = "std")]
pub type SharedWebContext = Mutex<WebContext>;
