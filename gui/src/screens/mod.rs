use embedded_graphics::{
    mono_font::{
        ascii::{FONT_6X10, FONT_7X13_BOLD},
        MonoTextStyle,
    },
    pixelcolor::BinaryColor,
};
use embedded_menu::{
    interaction::single_touch::SingleTouch,
    selection_indicator::{style::animated_triangle::AnimatedTriangle, AnimatedPosition},
    MenuStyle,
};
use embedded_text::{
    alignment::{HorizontalAlignment, VerticalAlignment},
    style::{HeightMode, TextBoxStyle, TextBoxStyleBuilder, VerticalOverdraw},
};

pub mod charging;
pub mod display_menu;
pub mod init;
pub mod measure;
pub mod message;
pub mod screen;
pub mod storage_menu;
pub mod wifi_ap;

pub const fn menu_style<R>(
) -> MenuStyle<BinaryColor, AnimatedTriangle, SingleTouch, AnimatedPosition, R> {
    MenuStyle::new(BinaryColor::On)
        .with_animated_selection_indicator(10)
        .with_details_delay(300)
        .with_selection_indicator(AnimatedTriangle::new(200))
        .with_input_adapter(SingleTouch {
            debounce_time: 1,
            ignore_time: 10,
            max_time: 75,
        })
        .with_title_font(&FONT_7X13_BOLD)
}

pub const CENTERED_TEXTBOX: TextBoxStyle = TextBoxStyleBuilder::new()
    .alignment(HorizontalAlignment::Center)
    .vertical_alignment(VerticalAlignment::Middle)
    .height_mode(HeightMode::Exact(VerticalOverdraw::Visible))
    .build();

pub const BOTTOM_CENTERED_TEXTBOX: TextBoxStyle = TextBoxStyleBuilder::new()
    .alignment(HorizontalAlignment::Center)
    .vertical_alignment(VerticalAlignment::Bottom)
    .height_mode(HeightMode::Exact(VerticalOverdraw::FullRowsOnly))
    .build();

pub const NORMAL_TEXT: MonoTextStyle<'static, BinaryColor> =
    MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

#[derive(Clone, Copy, PartialEq)]
pub struct BatteryInfo {
    pub voltage: u16,
    pub percentage: u8,
    pub is_charging: bool,
    pub is_low: bool,
}
