use embedded_graphics::{pixelcolor::BinaryColor, prelude::DrawTarget, Drawable};
use embedded_layout::{chain, object_chain::Chain};
use embedded_menu::{
    interaction::single_touch::SingleTouch,
    items::MenuItem,
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

pub struct WifiApScreen {
    pub menu: Menu<
        &'static str,
        SingleTouch,
        chain! { MenuItem<&'static str, ApMenuEvents, (), true> },
        ApMenuEvents,
        AnimatedPosition,
        AnimatedTriangle,
        BinaryColor,
    >,
    pub state: WifiAccessPointState,
    pub timeout: Option<u8>,
}

impl WifiApScreen {
    pub fn new() -> Self {
        Self {
            menu: Menu::with_style("WiFi Config", menu_style())
                .add_item("Exit", (), |_| ApMenuEvents::Exit)
                .build(),
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
