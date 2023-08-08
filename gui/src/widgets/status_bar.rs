use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point},
    Drawable,
};
use embedded_layout::{prelude::*, ViewGroup};

use crate::{
    screens::BatteryInfo,
    widgets::battery_small::{Battery, BatteryStyle},
};

#[derive(ViewGroup, Clone, Copy)]
pub struct StatusBar {
    pub battery: Battery,
}

impl StatusBar {
    pub fn update_battery_style(&mut self, style: BatteryStyle) {
        self.battery.style = style;
    }

    pub fn update_battery_data(&mut self, battery_data: Option<BatteryInfo>) {
        self.battery.data = battery_data;
    }
}

impl Drawable for StatusBar {
    type Color = BinaryColor;
    type Output = ();

    fn draw<DT: DrawTarget<Color = BinaryColor>>(&self, display: &mut DT) -> Result<(), DT::Error> {
        self.battery.draw(display)?;

        Ok(())
    }
}
