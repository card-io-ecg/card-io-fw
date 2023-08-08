use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point},
    Drawable,
};
use embedded_layout::{layout::linear::LinearLayout, prelude::*, ViewGroup};

use crate::{
    screens::BatteryInfo,
    widgets::{
        battery_small::{Battery, BatteryStyle},
        wifi::WifiStateView,
    },
};

#[derive(ViewGroup, Clone, Copy)]
pub struct StatusBar {
    pub battery: Battery,
    pub wifi: WifiStateView,
}

impl StatusBar {
    #[inline]
    pub fn update_battery_style(&mut self, style: BatteryStyle) {
        self.battery.style = style;
    }

    #[inline]
    pub fn update_battery_data(&mut self, battery_data: Option<BatteryInfo>) {
        self.battery.data = battery_data;
    }
}

impl Drawable for StatusBar {
    type Color = BinaryColor;
    type Output = ();

    #[inline]
    fn draw<DT: DrawTarget<Color = BinaryColor>>(&self, display: &mut DT) -> Result<(), DT::Error> {
        LinearLayout::horizontal(*self).arrange().draw(display)
    }
}
