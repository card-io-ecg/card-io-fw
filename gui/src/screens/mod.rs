use embedded_graphics::{mono_font::ascii::FONT_8X13_BOLD, pixelcolor::BinaryColor};
use embedded_menu::{
    interaction::single_touch::SingleTouch,
    selection_indicator::{style::animated_triangle::AnimatedTriangle, AnimatedPosition},
    MenuStyle,
};

pub mod init;
pub mod main_menu;
pub mod measure;

pub const MENU_STYLE: MenuStyle<BinaryColor, AnimatedTriangle, SingleTouch, AnimatedPosition> =
    MenuStyle::new(BinaryColor::On)
        .with_animated_selection_indicator(10)
        .with_details_delay(300)
        .with_selection_indicator(AnimatedTriangle::new(120))
        .with_interaction_controller(SingleTouch::new(10, 50))
        .with_title_font(&FONT_8X13_BOLD);
