use gui::{screens::display_menu::DisplayBrightness, widgets::battery_small::BatteryStyle};
use norfs::{
    medium::StorageMedium,
    reader::BoundReader,
    storable::{LoadError, Loadable, Storable},
    writer::BoundWriter,
    StorageError,
};

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
    async fn load<M>(reader: &mut BoundReader<'_, M>) -> Result<Self, LoadError>
    where
        M: StorageMedium,
        [(); M::BLOCK_COUNT]: Sized,
    {
        let data = Self {
            battery_display_style: BatteryStyle::load(reader).await?,
            display_brightness: DisplayBrightness::load(reader).await?,
        };

        Ok(data)
    }
}

impl Storable for Config {
    async fn store<M>(&self, writer: &mut BoundWriter<'_, M>) -> Result<(), StorageError>
    where
        M: StorageMedium,
        [(); M::BLOCK_COUNT]: Sized,
    {
        self.battery_display_style.store(writer).await?;
        self.display_brightness.store(writer).await?;

        Ok(())
    }
}
