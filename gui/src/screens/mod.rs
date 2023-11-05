use embedded_graphics::{
    mono_font::{
        ascii::{FONT_6X10, FONT_7X13_BOLD},
        MonoTextStyle,
    },
    pixelcolor::BinaryColor,
    text::{Alignment, Baseline, TextStyle, TextStyleBuilder},
};
use embedded_menu::{
    builder::MenuBuilder,
    interaction::single_touch::SingleTouch,
    selection_indicator::{style::animated_triangle::AnimatedTriangle, AnimatedPosition},
    Menu, MenuStyle, NoItems,
};
use embedded_text::{
    alignment::{HorizontalAlignment, VerticalAlignment},
    style::{HeightMode, TextBoxStyle, TextBoxStyleBuilder, VerticalOverdraw},
};

pub mod charging;
pub mod init;
pub mod measure;
pub mod message;
pub mod qr;
pub mod wifi_ap;

pub const fn menu_style<R>(
) -> MenuStyle<AnimatedTriangle, SingleTouch, AnimatedPosition, R, BinaryColor> {
    MenuStyle::new(BinaryColor::On)
        .with_animated_selection_indicator(10)
        .with_selection_indicator(AnimatedTriangle::new(200))
        .with_input_adapter(SingleTouch {
            debounce_time: 1,
            ignore_time: 10,
            max_time: 75,
        })
        .with_title_font(&FONT_7X13_BOLD)
}

pub fn create_menu<T: AsRef<str>, R>(
    title: T,
) -> MenuBuilder<T, SingleTouch, NoItems, R, AnimatedPosition, AnimatedTriangle, BinaryColor> {
    Menu::with_style(title, menu_style())
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

pub const CENTERED_TEXT: TextStyle = TextStyleBuilder::new()
    .alignment(Alignment::Center)
    .baseline(Baseline::Middle)
    .build();

#[derive(Clone, Copy, PartialEq)]
pub enum ChargingState {
    Discharging,
    Plugged,
    Charging,
}

#[derive(Clone, Copy, PartialEq)]
pub struct BatteryInfo {
    pub voltage: u16,
    pub percentage: u8,
    pub charging_state: ChargingState,
    pub is_low: bool,
}

impl BatteryInfo {
    pub fn is_charging(&self) -> bool {
        self.charging_state == ChargingState::Charging
    }

    pub fn is_discharging(&self) -> bool {
        self.charging_state == ChargingState::Discharging
    }

    pub fn is_plugged(&self) -> bool {
        self.charging_state != ChargingState::Discharging
    }
}
