use core::fmt;

#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct WifiNetwork {
    pub ssid: heapless::String<32>,
    pub pass: heapless::String<64>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum StationStatus {
    Disconnected,
    Joining(heapless::String<32>),
    Joined(heapless::String<32>),
    Failed(heapless::String<32>),
}

impl fmt::Display for StationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disconnected => f.write_str("disconnected"),
            Self::Joining(ssid) => write!(f, "joining {ssid}"),
            Self::Joined(ssid) => write!(f, "joined {ssid}"),
            Self::Failed(ssid) => write!(f, "failed {ssid}"),
        }
    }
}

pub const MAX_VISIBLE_NETWORKS: usize = 20;

#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct VisibleNetwork {
    pub ssid: heapless::String<32>,
    pub rssi: i8,
    pub locked: bool,
}

impl fmt::Display for VisibleNetwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.rssi, u8::from(self.locked), self.ssid)
    }
}
