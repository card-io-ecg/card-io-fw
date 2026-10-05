#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct WifiNetwork {
    pub ssid: heapless::String<32>,
    pub pass: heapless::String<64>,
}
