use alloc::string::String;
use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point},
    Drawable,
};
use embedded_layout::{
    chain,
    prelude::{horizontal, vertical, Align, Chain, Link},
};
use embedded_menu::{
    interaction::single_touch::SingleTouch,
    items::{MenuLine, NavigationItem},
    selection_indicator::{style::animated_triangle::AnimatedTriangle, AnimatedPosition},
    Menu,
};

use crate::{
    screens::{BatteryInfo, MENU_STYLE},
    widgets::battery_small::{Battery, BatteryStyle},
};

#[derive(Clone, Copy)]
pub enum AboutMenuEvents {
    None,
    Back,
}

type AboutMenu<'a> = Menu<
    SingleTouch,
    chain! {
        MenuLine<NavigationItem<'a, AboutMenuEvents>>,
        MenuLine<NavigationItem<'a, AboutMenuEvents>>,
        MenuLine<NavigationItem<'a, AboutMenuEvents>>,
        MenuLine<NavigationItem<'a, AboutMenuEvents>>
    },
    AboutMenuEvents,
    BinaryColor,
    AnimatedPosition,
    AnimatedTriangle,
>;

pub struct AboutMenuData {
    pub version: String,
    pub serial: String,
    pub adc: String,
}

impl AboutMenuData {
    pub fn create<'a>(&'a self) -> AboutMenu<'a> {
        Menu::with_style("Device info", MENU_STYLE)
            .add_item(NavigationItem::new(&self.version, AboutMenuEvents::None))
            .add_item(NavigationItem::new(&self.serial, AboutMenuEvents::None))
            .add_item(NavigationItem::new(&self.adc, AboutMenuEvents::None))
            .add_item(NavigationItem::new("Back", AboutMenuEvents::Back))
            .build()
    }
}

pub struct AboutMenuScreen<'a> {
    pub menu: AboutMenu<'a>,
    pub battery_data: Option<BatteryInfo>,
    pub battery_style: BatteryStyle,
}

impl Drawable for AboutMenuScreen<'_> {
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
