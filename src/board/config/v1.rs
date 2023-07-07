use embedded_io::asynch::{Read, Write};
use gui::{screens::display_menu::DisplayBrightness, widgets::battery_small::BatteryStyle};
use norfs::storable::{LoadError, Loadable, Storable};

#[derive(Clone)]
pub struct Config {
    pub battery_display_style: BatteryStyle,
    pub display_brightness: DisplayBrightness,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            battery_display_style: BatteryStyle::LowIndicator,
            display_brightness: DisplayBrightness::Normal,
        }
    }
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

impl Storable for Config {
    async fn store<W: Write>(&self, writer: &mut W) -> Result<(), W::Error> {
        self.battery_display_style.store(writer).await?;
        self.display_brightness.store(writer).await?;

        Ok(())
    }
}
