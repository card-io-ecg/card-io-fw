use core::fmt::Write;
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle, MonoTextStyleBuilder},
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
    style::{HeightMode, TextBoxStyle, TextBoxStyleBuilder, VerticalOverdraw},
    TextBox,
};

use crate::{screens::menu_style, widgets::wifi::WifiState};

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
    pub timeout: Option<u8>,
}

impl WifiApScreen {
    pub fn new() -> Self {
        Self {
            menu: ApMenu {}.create_menu_with_style(menu_style()),
            state: WifiState::NotConnected,
            timeout: None,
        }
    }
}

impl Drawable for WifiApScreen {
    type Color = BinaryColor;
    type Output = ();

    #[inline]
    fn draw<DT: DrawTarget<Color = BinaryColor>>(&self, display: &mut DT) -> Result<(), DT::Error> {
        self.menu.draw(display)?;

        // TODO: use actual network name
        let network_name = "Card/IO";

        let mut text = heapless::String::<128>::new();
        if self.state == WifiState::Connected {
            unwrap!(text.push_str("Connected. Open site at 192.168.2.1"));
        } else {
            unwrap!(text.push_str("No client connected. Look for a network called "));
            unwrap!(text.push_str(network_name));
            if let Some(timeout) = self.timeout {
                unwrap!(write!(&mut text, "\nExiting in {}", timeout).map_err(|_| ()));
            }
        }

        const TEXTBOX_STYLE: TextBoxStyle = TextBoxStyleBuilder::new()
            .height_mode(HeightMode::Exact(VerticalOverdraw::FullRowsOnly))
            .alignment(HorizontalAlignment::Center)
            .vertical_alignment(VerticalAlignment::Bottom)
            .build();
        const CHARACTER_STYLE: MonoTextStyle<'_, BinaryColor> = MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(BinaryColor::On) // On on normally-Off background
            .build();

        TextBox::with_textbox_style(
            text.as_str(),
            display.bounding_box(),
            CHARACTER_STYLE,
            TEXTBOX_STYLE,
        )
        .draw(display)?;

        Ok(())
    }
}
