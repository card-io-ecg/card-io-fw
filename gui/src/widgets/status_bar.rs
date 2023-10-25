use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point},
    Drawable,
};
use embedded_layout::{
    layout::linear::LinearLayout, prelude::*, view_group::EmptyViewGroup, ViewGroup,
};

use crate::{
    screens::BatteryInfo,
    widgets::{
        battery_small::{Battery, BatteryStyle},
        wifi_access_point::WifiAccessPointStateView,
        wifi_client::WifiClientStateView,
    },
};

#[derive(ViewGroup, Clone, Copy)]
pub struct StatusBar {
    pub battery: Battery,
    pub wifi_sta: WifiClientStateView,
    pub wifi_ap: WifiAccessPointStateView,
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
        // Roundabout way because we can't call draw on the LinearLayout as it results in an
        // indirect infinite recursion.
        let mut views = *self;

        LinearLayout::horizontal(EmptyViewGroup)
            .with_alignment(vertical::Top)
            .arrange_view_group(&mut views);

        views.align_to_mut(&display.bounding_box(), horizontal::Right, vertical::Top);

        views.battery.draw(display)?;
        views.wifi_sta.draw(display)?;
        views.wifi_ap.draw(display)?;

        Ok(())
    }
}
