use gui::{
    screens::display_menu::{BatteryDisplayStyle, DisplayBrightness},
    widgets::battery_small::BatteryStyle,
};
use serde::{Deserialize, Serialize};
use ssd1306::prelude::Brightness;

use crate::board::BATTERY_MODEL;

#[derive(Clone, Copy, Serialize, Deserialize)]
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
    pub const MAX_CONFIG_SIZE: usize = 2;

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
