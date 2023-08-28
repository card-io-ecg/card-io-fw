use embedded_graphics::{mono_font::ascii::FONT_7X13_BOLD, pixelcolor::BinaryColor};
use embedded_menu::{
    interaction::single_touch::SingleTouch,
    selection_indicator::{style::animated_triangle::AnimatedTriangle, AnimatedPosition},
    MenuStyle,
};

pub mod charging;
pub mod display_menu;
pub mod init;
pub mod measure;
pub mod message;
pub mod screen;
pub mod wifi_ap;

pub const fn menu_style<R>(
) -> MenuStyle<BinaryColor, AnimatedTriangle, SingleTouch, AnimatedPosition, R> {
    MenuStyle::new(BinaryColor::On)
        .with_animated_selection_indicator(10)
        .with_details_delay(300)
        .with_selection_indicator(AnimatedTriangle::new(200))
        .with_input_adapter(SingleTouch {
            debounce_time: 1,
            ignore_time: 15,
            max_time: 100,
        })
        .with_title_font(&FONT_7X13_BOLD)
}

#[derive(Clone, Copy, PartialEq)]
pub struct BatteryInfo {
    pub voltage: u16,
    pub percentage: u8,
    pub is_charging: bool,
    pub is_low: bool,
}
