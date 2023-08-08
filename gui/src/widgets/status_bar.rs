use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point},
    Drawable,
};
use embedded_layout::{prelude::*, ViewGroup};

use crate::{
    screens::BatteryInfo,
    widgets::{
        battery_small::{Battery, BatteryStyle},
        slot::Slot,
    },
};

#[derive(ViewGroup, Clone, Copy)]
pub struct StatusBar {
    pub battery: Slot<Battery>,
}

impl StatusBar {
    pub fn update_battery_style(&mut self, style: BatteryStyle) {
        if let Some(battery) = self.battery.as_visible_mut() {
            battery.style = style;
        }
    }

    pub fn update_battery_data(
        &mut self,
        battery_data: Option<BatteryInfo>,
        battery_style: BatteryStyle,
    ) {
        if let Some(battery_data) = battery_data {
            if let Some(battery) = self.battery.as_visible_mut() {
                battery.data = battery_data;
            } else {
                self.battery
                    .set_visible(Battery::with_style(battery_data, battery_style));
            }
        } else {
            self.battery.set_hidden();
        }
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
