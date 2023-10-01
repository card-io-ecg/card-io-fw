use embedded_io::asynch::Read;
use gui::widgets::battery_small::BatteryStyle;
use norfs::storable::{LoadError, Loadable};

use super::types::DisplayBrightness;

#[derive(Clone)]
pub struct Config {
    pub battery_display_style: BatteryStyle,
    pub display_brightness: DisplayBrightness,
}

impl Loadable for Config {
    async fn load<R: Read>(reader: &mut R) -> Result<Self, LoadError<R::Error>> {
        let data = Self {
            battery_display_style: BatteryStyle::load(reader).await?,
            display_brightness: DisplayBrightness::load(reader).await?,
        };

        Ok(data)
    }
}
