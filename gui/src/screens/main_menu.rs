use embedded_graphics::{pixelcolor::BinaryColor, prelude::DrawTarget, Drawable};
use embedded_layout::prelude::{horizontal, vertical, Align};
use embedded_menu::{
    interaction::single_touch::SingleTouch,
    selection_indicator::{style::animated_triangle::AnimatedTriangle, AnimatedPosition},
    Menu,
};

use crate::widgets::status_bar::StatusBar;

#[derive(Clone, Copy)]
pub enum MainMenuEvents {
    Display,
    About,
    WifiSetup,
    WifiListVisible,
    Shutdown,
}

#[derive(Clone, Copy, PartialEq, Eq, Menu)]
#[menu(
    title = "Main menu",
    navigation(events = MainMenuEvents),
    items = [
        navigation(label = "Display settings", event = MainMenuEvents::Display),
        navigation(label = "Device info", event = MainMenuEvents::About),
        navigation(label = "Wifi setup", event = MainMenuEvents::WifiSetup),
        navigation(label = "Wifi networks", event = MainMenuEvents::WifiListVisible),
        navigation(label = "Shutdown", event = MainMenuEvents::Shutdown)
    ]
)]
pub struct MainMenu {}

pub struct MainMenuScreen {
    pub menu: MainMenuMenuWrapper<SingleTouch, AnimatedPosition, AnimatedTriangle>,
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
        self.menu.draw(display)?;

        self.status_bar
            .align_to(&display.bounding_box(), horizontal::Right, vertical::Top)
            .draw(display)?;

        Ok(())
    }
}
