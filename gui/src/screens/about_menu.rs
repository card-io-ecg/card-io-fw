use alloc::string::String;
use embedded_graphics::{pixelcolor::BinaryColor, prelude::DrawTarget, Drawable};
use embedded_layout::{
    chain,
    prelude::{horizontal, vertical, Align, Chain, Link},
};
use embedded_menu::{
    interaction::single_touch::SingleTouch,
    items::NavigationItem,
    selection_indicator::{style::animated_triangle::AnimatedTriangle, AnimatedPosition},
    Menu,
};

use crate::{screens::MENU_STYLE, widgets::status_bar::StatusBar};

#[derive(Clone, Copy)]
pub enum AboutMenuEvents {
    None,
    Back,
}

type AboutMenu<'a> = Menu<
    SingleTouch,
    chain! {
        NavigationItem<'a, AboutMenuEvents>,
        NavigationItem<'a, AboutMenuEvents>,
        NavigationItem<'a, AboutMenuEvents>,
        NavigationItem<'a, AboutMenuEvents>,
        NavigationItem<'a, AboutMenuEvents>
    },
    AboutMenuEvents,
    BinaryColor,
    AnimatedPosition,
    AnimatedTriangle,
>;

pub struct AboutMenuData {
    pub hw_version: String,
    pub fw_version: String,
    pub serial: String,
    pub adc: String,
}

impl AboutMenuData {
    pub fn create(&self) -> AboutMenu<'_> {
        Menu::with_style("Device info", MENU_STYLE)
            .add_item(NavigationItem::new(&self.serial, AboutMenuEvents::None))
            .add_item(NavigationItem::new(&self.hw_version, AboutMenuEvents::None))
            .add_item(NavigationItem::new(&self.fw_version, AboutMenuEvents::None))
            .add_item(NavigationItem::new(&self.adc, AboutMenuEvents::None))
            .add_item(NavigationItem::new("Back", AboutMenuEvents::Back))
            .build()
    }
}

pub struct AboutMenuScreen<'a> {
    pub menu: AboutMenu<'a>,
    pub status_bar: StatusBar,
}

impl Drawable for AboutMenuScreen<'_> {
    type Color = BinaryColor;
    type Output = ();

    #[inline]
    fn draw<D>(&self, display: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        self.menu.draw(display)?;

        self.status_bar
            .align_to(&display.bounding_box(), horizontal::Right, vertical::Top)
            .draw(display)?;

        Ok(())
    }
}
