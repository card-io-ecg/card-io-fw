use alloc::string::String;
use embedded_graphics::{pixelcolor::BinaryColor, prelude::DrawTarget, Drawable};
use embedded_layout::{
    chain,
    prelude::{Chain, Link},
};
use embedded_menu::{
    collection::MenuItems,
    interaction::single_touch::SingleTouch,
    items::NavigationItem,
    selection_indicator::{style::animated_triangle::AnimatedTriangle, AnimatedPosition},
    Menu, MenuState,
};

use crate::{screens::MENU_STYLE, widgets::status_bar::StatusBar};

#[derive(Clone, Copy)]
pub enum WifiStaMenuEvents {
    None,
    Back,
}

type NavMenuItem = NavigationItem<String, &'static str, &'static str, WifiStaMenuEvents>;

type WifiStaMenu<'a> = Menu<
    &'static str,
    SingleTouch,
    chain! {
        MenuItems<
            &'a mut [NavMenuItem],
            NavMenuItem,
            WifiStaMenuEvents
        >,
        NavigationItem<&'static str, &'static str, &'static str, WifiStaMenuEvents>
    },
    WifiStaMenuEvents,
    BinaryColor,
    AnimatedPosition,
    AnimatedTriangle,
>;

pub struct WifiStaMenuData<'a> {
    pub networks: &'a mut [NavMenuItem],
}

impl<'a> WifiStaMenuData<'a> {
    pub fn create(
        &'a mut self,
        state: MenuState<SingleTouch, AnimatedPosition, AnimatedTriangle>,
    ) -> WifiStaMenu<'a> {
        Menu::with_style("Access points", MENU_STYLE)
            .add_items(&mut *self.networks)
            .add_item(NavigationItem::new("Back", WifiStaMenuEvents::Back))
            .build_with_state(state)
    }
}

pub struct WifiStaMenuScreen<'a> {
    pub menu: WifiStaMenu<'a>,
    pub status_bar: StatusBar,
}

impl Drawable for WifiStaMenuScreen<'_> {
    type Color = BinaryColor;
    type Output = ();

    #[inline]
    fn draw<D>(&self, display: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        self.menu.draw(display)?;
        self.status_bar.draw(display)?;

        Ok(())
    }
}
