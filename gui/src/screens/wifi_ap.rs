use embedded_graphics::{pixelcolor::BinaryColor, prelude::DrawTarget, Drawable};
use embedded_menu::{
    interaction::single_touch::SingleTouch,
    selection_indicator::{style::animated_triangle::AnimatedTriangle, AnimatedPosition},
    Menu,
};
use embedded_text::TextBox;
use ufmt::uwrite;

use crate::{
    screens::{menu_style, BOTTOM_CENTERED_TEXTBOX, NORMAL_TEXT},
    widgets::wifi_access_point::WifiAccessPointState,
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
    pub state: WifiAccessPointState,
    pub timeout: Option<u8>,
}

impl WifiApScreen {
    pub fn new() -> Self {
        Self {
            menu: ApMenu {}.create_menu_with_style(menu_style()),
            state: WifiAccessPointState::NotConnected,
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
        if self.state == WifiAccessPointState::Connected {
            unwrap!(text.push_str("Connected. Open site at 192.168.2.1"));
        } else {
            unwrap!(text.push_str("No client connected. Look for a network called "));
            unwrap!(text.push_str(network_name));
            if let Some(timeout) = self.timeout {
                unwrap!(uwrite!(&mut text, "\nExiting in {}", timeout).map_err(|_| ()));
            }
        }

        TextBox::with_textbox_style(
            text.as_str(),
            display.bounding_box(),
            NORMAL_TEXT,
            BOTTOM_CENTERED_TEXTBOX,
        )
        .draw(display)?;

        Ok(())
    }
}
