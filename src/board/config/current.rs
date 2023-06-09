use config_site::data::network::WifiNetwork;
use embedded_io::asynch::{Read, Write};
use gui::{screens::display_menu::DisplayBrightness, widgets::battery_small::BatteryStyle};
use norfs::storable::{LoadError, Loadable, Storable};
use ssd1306::prelude::Brightness;

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
            ..Default::default()
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            battery_display_style: BatteryStyle::LowIndicator,
            display_brightness: DisplayBrightness::Normal,
            known_networks: heapless::Vec::new(),
        }
    }
}

impl Config {
    pub fn battery_style(&self) -> BatteryStyle {
        self.battery_display_style
    }

    pub fn display_brightness(&self) -> Brightness {
        match self.display_brightness {
            DisplayBrightness::Dimmest => Brightness::DIMMEST,
            DisplayBrightness::Dim => Brightness::DIM,
            DisplayBrightness::Normal => Brightness::NORMAL,
            DisplayBrightness::Bright => Brightness::BRIGHT,
            DisplayBrightness::Brightest => Brightness::BRIGHTEST,
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

impl Storable for Config {
    async fn store<W: Write>(&self, writer: &mut W) -> Result<(), W::Error> {
        self.battery_display_style.store(writer).await?;
        self.display_brightness.store(writer).await?;
        self.known_networks.store(writer).await?;

        Ok(())
    }
}
