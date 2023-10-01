use config_site::data::network::WifiNetwork;
use embedded_io::asynch::Read;
use gui::widgets::battery_small::BatteryStyle;
use norfs::storable::{LoadError, Loadable};

use super::types::DisplayBrightness;

#[derive(Clone)]
pub struct Config {
    pub battery_display_style: BatteryStyle,
    pub display_brightness: DisplayBrightness,
    pub known_networks: heapless::Vec<WifiNetwork, 8>,
}

impl From<super::v1::Config> for Config {
    fn from(value: super::v1::Config) -> Self {
        Self {
            battery_display_style: value.battery_display_style,
            display_brightness: value.display_brightness,
            known_networks: heapless::Vec::new(),
        }
    }
}

impl Loadable for Config {
    async fn load<R: Read>(reader: &mut R) -> Result<Self, LoadError<R::Error>> {
        let data = Self {
            battery_display_style: BatteryStyle::load(reader).await?,
            display_brightness: DisplayBrightness::load(reader).await?,
            known_networks: heapless::Vec::load(reader).await?,
        };

        Ok(data)
    }
}
