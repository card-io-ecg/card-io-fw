use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::DrawTarget,
    Drawable,
};
use embedded_menu::{
    interaction::single_touch::SingleTouch,
    selection_indicator::{style::animated_triangle::AnimatedTriangle, AnimatedPosition},
    Menu,
};
use embedded_text::{
    alignment::{HorizontalAlignment, VerticalAlignment},
    style::{HeightMode, TextBoxStyleBuilder, VerticalOverdraw},
    TextBox,
};

use crate::{
    screens::menu_style,
    widgets::{status_bar::StatusBar, wifi::WifiState},
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
    pub menu: ApMenuMenuWrapper<SingleTouch, AnimatedPosition, AnimatedTriangle>,
    pub state: WifiState,
    pub status_bar: StatusBar,
}

impl WifiApScreen {
    pub fn new(status_bar: StatusBar) -> Self {
        Self {
            menu: ApMenu {}.create_menu_with_style(menu_style()),
            state: WifiState::NotConnected,
            status_bar,
        }
    }
}

impl Drawable for WifiApScreen {
    type Color = BinaryColor;
    type Output = ();

    #[inline]
    fn draw<DT: DrawTarget<Color = BinaryColor>>(&self, display: &mut DT) -> Result<(), DT::Error> {
        self.menu.draw(display)?;
        self.status_bar.draw(display)?;

        // TODO: use actual network name
        let text = if self.state == WifiState::Connected {
            "Connected. Open site at 192.168.2.1"
        } else {
            "No client connected. Look for a network called Card/IO"
        };

        let textbox_style = TextBoxStyleBuilder::new()
            .height_mode(HeightMode::Exact(VerticalOverdraw::FullRowsOnly))
            .alignment(HorizontalAlignment::Center)
            .vertical_alignment(VerticalAlignment::Bottom)
            .build();
        let character_style = MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(BinaryColor::On) // On on normally-Off background
            .build();
        TextBox::with_textbox_style(text, display.bounding_box(), character_style, textbox_style)
            .draw(display)?;

        Ok(())
    }
}
