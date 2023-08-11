use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{Dimensions, DrawTarget},
    Drawable,
};
use embedded_layout::{
    chain,
    prelude::{Chain, Link},
};
use embedded_menu::{
    interaction::single_touch::SingleTouch,
    items::NavigationItem,
    selection_indicator::{style::animated_triangle::AnimatedTriangle, AnimatedPosition},
    Menu,
};

use crate::{screens::MENU_STYLE, widgets::status_bar::StatusBar};

#[derive(Clone, Copy)]
pub enum MainMenuEvents {
    Display,
    About,
    WifiSetup,
    WifiListVisible,
    Shutdown,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MainMenuData {}

pub struct MainMenu {
    menu: Menu<
        &'static str,
        SingleTouch,
        chain! {
            NavigationItem<&'static str, &'static str, &'static str, MainMenuEvents>,
            NavigationItem<&'static str, &'static str, &'static str, MainMenuEvents>,
            NavigationItem<&'static str, &'static str, &'static str, MainMenuEvents>,
            NavigationItem<&'static str, &'static str, &'static str, MainMenuEvents>,
            NavigationItem<&'static str, &'static str, &'static str, MainMenuEvents>
        },
        MainMenuEvents,
        BinaryColor,
        AnimatedPosition,
        AnimatedTriangle,
    >,
    data: MainMenuData,
}
impl MainMenu {
    pub fn data(&self) -> &MainMenuData {
        &self.data
    }
    pub fn interact(&mut self, event: bool) -> Option<MainMenuEvents> {
        self.menu.interact(event)
    }
    pub fn update(&mut self, display: &impl Dimensions) {
        self.menu.update(display)
    }
}
impl MainMenuData {
    pub fn create_menu(self) -> MainMenu {
        let builder = Menu::with_style("Main menu", MENU_STYLE);

        MainMenu {
            data: self,
            menu: builder
                .add_item(NavigationItem::new(
                    "Display settings",
                    MainMenuEvents::Display,
                ))
                .add_item(NavigationItem::new("Device info", MainMenuEvents::About))
                .add_item(NavigationItem::new("Wifi setup", MainMenuEvents::WifiSetup))
                .add_item(NavigationItem::new(
                    "Wifi networks",
                    MainMenuEvents::WifiListVisible,
                ))
                .add_item(NavigationItem::new("Shutdown", MainMenuEvents::Shutdown))
                .build(),
        }
    }
}

pub struct MainMenuScreen {
    pub menu: MainMenu,
    pub status_bar: StatusBar,
}

impl Drawable for MainMenuScreen {
    type Color = BinaryColor;
    type Output = ();

    #[inline]
    fn draw<D>(&self, display: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        self.menu.menu.draw(display)?;
        self.status_bar.draw(display)?;

        Ok(())
    }
}
