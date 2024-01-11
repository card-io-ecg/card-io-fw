use config_site::data::network::WifiNetwork;
use embedded_io_async::Read;
use gui::widgets::battery_small::BatteryStyle;
use norfs::storable::{LoadError, Loadable};

use crate::board::DEFAULT_BACKEND_URL;

use super::types::{DisplayBrightness, FilterStrength};

#[derive(Clone)]
pub struct Config {
    pub battery_display_style: BatteryStyle,
    pub display_brightness: DisplayBrightness,
    pub known_networks: heapless::Vec<WifiNetwork, 8>,
    pub filter_strength: FilterStrength,
    pub backend_url: heapless::String<64>,
}

impl From<super::v2::Config> for Config {
    fn from(value: super::v2::Config) -> Self {
        Self {
            battery_display_style: value.battery_display_style,
            display_brightness: value.display_brightness,
            known_networks: value.known_networks,
            filter_strength: FilterStrength::Weak,
            backend_url: heapless::String::try_from(DEFAULT_BACKEND_URL).unwrap(),
        }
    }
}

impl Loadable for Config {
    async fn load<R: Read>(reader: &mut R) -> Result<Self, LoadError<R::Error>> {
        let data = Self {
            battery_display_style: BatteryStyle::load(reader).await?,
            display_brightness: DisplayBrightness::load(reader).await?,
            known_networks: heapless::Vec::load(reader).await?,
            filter_strength: FilterStrength::load(reader).await?,
            backend_url: heapless::String::load(reader).await?,
        };

        Ok(data)
    }
}
