use alloc::string::String;
use embedded_graphics::{pixelcolor::BinaryColor, prelude::DrawTarget, Drawable};
use embedded_layout::{
    chain,
    prelude::{horizontal, vertical, Align, Chain, Link},
};
use embedded_menu::{
    collection::MenuItems,
    interaction::single_touch::SingleTouch,
    items::NavigationItem,
    selection_indicator::{style::animated_triangle::AnimatedTriangle, AnimatedPosition},
    Menu,
};

use crate::{screens::menu_style, widgets::status_bar::StatusBar};

#[derive(Clone, Copy)]
pub enum AboutMenuEvents {
    None,
    Back,
}

type NavMenuItem = NavigationItem<String, &'static str, &'static str, AboutMenuEvents>;
type AboutMenu = Menu<
    &'static str,
    SingleTouch,
    chain! {
        MenuItems<
            [NavMenuItem; 4],
            NavMenuItem,
            AboutMenuEvents
        >,
        NavigationItem<&'static str, &'static str, &'static str, AboutMenuEvents>
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
    pub fn create(self) -> AboutMenu {
        Menu::with_style("Device info", menu_style())
            .add_items([
                NavigationItem::new(self.serial, AboutMenuEvents::None),
                NavigationItem::new(self.hw_version, AboutMenuEvents::None),
                NavigationItem::new(self.fw_version, AboutMenuEvents::None),
                NavigationItem::new(self.adc, AboutMenuEvents::None),
            ])
            .add_item(NavigationItem::new("Back", AboutMenuEvents::Back))
            .build()
    }
}

pub struct AboutMenuScreen {
    pub menu: AboutMenu,
    pub status_bar: StatusBar,
}

impl Drawable for AboutMenuScreen {
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
