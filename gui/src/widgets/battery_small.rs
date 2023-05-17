use core::fmt::Write;

use embedded_graphics::{
    image::{Image, ImageRaw},
    mono_font::{ascii::FONT_6X10, MonoTextStyle, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, DrawTargetExt, Point, Size},
    primitives::Rectangle,
    text::{renderer::TextRenderer, Alignment, Baseline, Text, TextStyleBuilder},
    Drawable,
};
use embedded_layout::prelude::*;

use crate::screens::BatteryInfo;

pub enum BatteryStyle {
    MilliVolts,
}

impl BatteryStyle {
    fn text_style() -> MonoTextStyle<'static, BinaryColor> {
        MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(BinaryColor::On)
            .build()
    }

    fn size(&self) -> Size {
        match self {
            BatteryStyle::MilliVolts => {
                Self::text_style()
                    .measure_string("C0.000V", Point::zero(), Baseline::Top)
                    .bounding_box
                    .size
            }
        }
    }

    fn draw<D: DrawTarget<Color = BinaryColor>>(
        &self,
        target: &mut D,
        data: BatteryInfo,
    ) -> Result<(), D::Error> {
        match self {
            BatteryStyle::MilliVolts => {
                let mut string = heapless::String::<8>::new();

                let volts = data.voltage / 1000;
                let millis = data.voltage - volts * 1000;
                write!(&mut string, "{volts}.{millis:03}V").ok();

                Text::with_text_style(
                    &string,
                    Point::new(target.bounding_box().size.width as i32 - 1, 0),
                    Self::text_style(),
                    TextStyleBuilder::new()
                        .baseline(Baseline::Top)
                        .alignment(Alignment::Right)
                        .build(),
                )
                .draw(target)?;

                if data.charge_current.is_some() {
                    #[rustfmt::skip]
                    const DATA: &[u8] = &[
                        0b00000000,
                        0b01010000,
                        0b01010000,
                        0b11111000,
                        0b01110000,
                        0b01110000,
                        0b00100000,
                        0b00100000,
                        0b01000000,
                    ];
                    let raw_image = ImageRaw::<BinaryColor>::new(DATA, 6);
                    let image = Image::new(&raw_image, Point::zero());
                    image.draw(target)?;
                }

                Ok(())
            }
        }
    }
}

pub struct Battery {
    pub data: BatteryInfo,
    pub style: BatteryStyle,
    pub top_left: Point,
}

impl View for Battery {
    fn translate_impl(&mut self, by: Point) {
        self.top_left += by;
    }

    fn bounds(&self) -> Rectangle {
        Rectangle::new(self.top_left, self.style.size())
    }
}

impl Drawable for Battery {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        let mut cropped = target.cropped(&self.bounds());
        self.style.draw(&mut cropped, self.data)
    }
}
