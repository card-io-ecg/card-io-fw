use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point},
    Drawable,
};
use embedded_layout::prelude::{horizontal, vertical, Align};
use embedded_menu::{
    interaction::single_touch::SingleTouch,
    selection_indicator::{style::animated_triangle::AnimatedTriangle, AnimatedPosition},
    Menu,
};

use crate::{
    screens::{BatteryInfo, MENU_STYLE},
    widgets::battery_small::{Battery, BatteryStyle},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ApMenuEvents {
    Exit,
}

#[derive(Clone, Copy, PartialEq, Eq, Menu)]
#[menu(
    title = "WiFi Config",
    navigation(events = ApMenuEvents),
    items = [
        navigation(label = "Exit", event = ApMenuEvents::Exit)
    ]
)]
pub struct ApMenu {}

pub struct WifiApScreen {
    pub battery_data: Option<BatteryInfo>,
    pub battery_style: BatteryStyle,
    pub menu: ApMenuMenuWrapper<SingleTouch, AnimatedPosition, AnimatedTriangle>,
}

impl WifiApScreen {
    pub fn new(battery_data: Option<BatteryInfo>, battery_style: BatteryStyle) -> Self {
        Self {
            battery_data,
            battery_style,
            menu: ApMenu {}.create_menu_with_style(MENU_STYLE),
        }
    }
}

impl Drawable for WifiApScreen {
    type Color = BinaryColor;
    type Output = ();

    fn draw<DT: DrawTarget<Color = BinaryColor>>(&self, display: &mut DT) -> Result<(), DT::Error> {
        self.menu.draw(display)?;

        if let Some(data) = self.battery_data {
            Battery {
                data,
                style: self.battery_style,
                top_left: Point::zero(),
            }
            .align_to_mut(&display.bounding_box(), horizontal::Right, vertical::Top)
            .draw(display)?;
        }

        Ok(())
    }
}
