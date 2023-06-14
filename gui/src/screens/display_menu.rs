use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point},
    Drawable,
};
use embedded_layout::prelude::{horizontal, vertical, Align};
use embedded_menu::{
    interaction::single_touch::SingleTouch,
    selection_indicator::{style::animated_triangle::AnimatedTriangle, AnimatedPosition},
    Menu, SelectValue,
};
use serde::{Deserialize, Serialize};

use crate::{
    screens::BatteryInfo,
    widgets::battery_small::{Battery, BatteryStyle},
};

#[derive(Clone, Copy)]
pub enum DisplayMenuEvents {
    Back,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, SelectValue, Serialize, Deserialize)]
pub enum DisplayBrightness {
    Dimmest,
    Dim,
    Normal,
    Bright,
    Brightest,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, SelectValue, Serialize, Deserialize)]
pub enum BatteryDisplayStyle {
    #[display_as("Voltage")]
    MilliVolts,
    Percentage,
    Icon,
    Indicator,
}

#[derive(Clone, Copy, PartialEq, Eq, Menu)]
#[menu(
    title = "Display",
    navigation(events = DisplayMenuEvents),
    items = [
        data(label = "Brightness", field = brightness),
        data(label = "Battery", field = battery_display),
        navigation(label = "Back", event = DisplayMenuEvents::Back)
    ]
)]
pub struct DisplayMenu {
    pub brightness: DisplayBrightness,
    pub battery_display: BatteryDisplayStyle,
}

pub struct DisplayMenuScreen {
    pub menu: DisplayMenuMenuWrapper<SingleTouch, AnimatedPosition, AnimatedTriangle>,
    pub battery_data: Option<BatteryInfo>,
    pub battery_style: BatteryStyle,
}

impl Drawable for DisplayMenuScreen {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, display: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        if let Some(data) = self.battery_data {
            Battery {
                data,
                style: self.battery_style,
                top_left: Point::zero(),
            }
            .align_to(&display.bounding_box(), horizontal::Right, vertical::Top)
            .draw(display)?;
        }

        self.menu.draw(display)
    }
}
