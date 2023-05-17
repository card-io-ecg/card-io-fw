use core::fmt::Write;

use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, DrawTargetExt, Point, Size},
    primitives::Rectangle,
    text::{renderer::TextRenderer, Baseline, Text},
    Drawable,
};
use embedded_layout::prelude::*;

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
                    .measure_string("0.000V", Point::zero(), Baseline::Top)
                    .bounding_box
                    .size
            }
        }
    }

    fn draw<D: DrawTarget<Color = BinaryColor>>(
        &self,
        target: &mut D,
        voltage: u16,
    ) -> Result<(), D::Error> {
        match self {
            BatteryStyle::MilliVolts => {
                let mut string = heapless::String::<8>::new();

                let volts = voltage / 1000;
                let millis = voltage - volts * 1000;
                write!(&mut string, "{volts}.{millis:03}V").ok();

                Text::with_baseline(&string, Point::zero(), Self::text_style(), Baseline::Top)
                    .draw(target)?;

                Ok(())
            }
        }
    }
}

pub struct Battery {
    pub voltage: u16,
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
        self.style.draw(&mut cropped, self.voltage)
    }
}
