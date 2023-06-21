use gui::{
    screens::display_menu::{BatteryDisplayStyle, DisplayBrightness},
    widgets::battery_small::BatteryStyle,
};
use norfs::{
    medium::StorageMedium,
    reader::BoundReader,
    storable::{LoadError, Storable},
    writer::BoundWriter,
    StorageError,
};
use ssd1306::prelude::Brightness;

use crate::board::BATTERY_MODEL;

#[derive(Clone, Copy)]
pub struct Config {
    pub battery_display_style: BatteryDisplayStyle,
    pub display_brightness: DisplayBrightness,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            battery_display_style: BatteryDisplayStyle::Indicator,
            display_brightness: DisplayBrightness::Normal,
        }
    }
}

impl Config {
    pub fn battery_style(&self) -> BatteryStyle {
        BatteryStyle::new(self.battery_display_style, BATTERY_MODEL)
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

impl Storable for Config {
    async fn load<M>(reader: &mut BoundReader<'_, M>) -> Result<Self, LoadError>
    where
        M: StorageMedium,
        [(); M::BLOCK_COUNT]: Sized,
    {
        let data = Self {
            battery_display_style: BatteryDisplayStyle::load(reader).await?,
            display_brightness: DisplayBrightness::load(reader).await?,
        };

        Ok(data)
    }

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
