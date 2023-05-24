use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point},
    Drawable,
};
use embedded_layout::prelude::{horizontal, vertical, Align};
use embedded_text::{
    alignment::{HorizontalAlignment, VerticalAlignment},
    style::{HeightMode, TextBoxStyleBuilder, VerticalOverdraw},
    TextBox,
};

use crate::{
    screens::BatteryInfo,
    widgets::battery_small::{Battery, BatteryStyle},
};

pub struct WifiApScreen {
    pub battery_data: Option<BatteryInfo>,
    pub battery_style: BatteryStyle,
}

impl Drawable for WifiApScreen {
    type Color = BinaryColor;
    type Output = ();

    fn draw<DT: DrawTarget<Color = BinaryColor>>(&self, display: &mut DT) -> Result<(), DT::Error> {
        let textbox_style = TextBoxStyleBuilder::new()
            .height_mode(HeightMode::Exact(VerticalOverdraw::FullRowsOnly))
            .alignment(HorizontalAlignment::Center)
            .vertical_alignment(VerticalAlignment::Middle)
            .build();

        // using embedded-text because I'm lazy to position the label vertically
        TextBox::with_textbox_style(
            "Wifi AP ready",
            display.bounding_box(),
            MonoTextStyleBuilder::new()
                .font(&FONT_6X10)
                .text_color(BinaryColor::On) // On on normally-Off background
                .build(),
            textbox_style,
        )
        .draw(display)?;

        if let Some(data) = self.battery_data {
            Battery {
                data,
                style: self.battery_style,
                top_left: Point::zero(),
            }
            .align_to_mut(&display.bounding_box(), horizontal::Right, vertical::Top)
            .draw(display)?;
        }

        Ok(())
    }
}
