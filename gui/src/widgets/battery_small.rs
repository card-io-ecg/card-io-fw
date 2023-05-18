use core::fmt::Write;

use embedded_graphics::{
    geometry::AnchorPoint,
    image::{Image, ImageRaw},
    mono_font::{ascii::FONT_6X10, MonoTextStyle, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, DrawTargetExt, Point, Size},
    primitives::{Line, Primitive, PrimitiveStyle, Rectangle},
    text::{renderer::TextRenderer, Alignment, Baseline, Text, TextStyleBuilder},
    Drawable,
};
use embedded_layout::prelude::*;
use signal_processing::battery::BatteryModel;

use crate::screens::BatteryInfo;

#[derive(Clone, Copy)]
pub enum BatteryStyle {
    MilliVolts,
    Percentage(BatteryModel),
    Icon(BatteryModel),
    LowIndicator(BatteryModel),
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
            BatteryStyle::Percentage(_) => {
                Self::text_style()
                    .measure_string("C000%", Point::zero(), Baseline::Top)
                    .bounding_box
                    .size
            }
            BatteryStyle::Icon(_) | BatteryStyle::LowIndicator(_) => {
                Size::new(13, 10) + Size::new(6, 10)
            }
        }
    }

    fn draw_battery_outline<D: DrawTarget<Color = BinaryColor>>(
        &self,
        target: &mut D,
        top_right: Point,
    ) -> Result<Point, D::Error> {
        #[rustfmt::skip]
        const DATA: &[u8] = &[
            0b00000000, 0b00000_000,
            0b11111111, 0b11110_000,
            0b10000000, 0b00010_000,
            0b10000000, 0b00011_000,
            0b10000000, 0b00011_000,
            0b10000000, 0b00011_000,
            0b10000000, 0b00010_000,
            0b11111111, 0b11110_000,
            0b00000000, 0b00000_000,
        ];
        const IMAGE_WIDTH: u32 = 13;

        let top_left = top_right - Point::new(IMAGE_WIDTH as i32 - 1, 0);

        let raw_image = ImageRaw::<BinaryColor>::new(DATA, IMAGE_WIDTH);
        Image::new(&raw_image, top_left).draw(target)?;

        Ok(top_left)
    }

    fn draw_text<D: DrawTarget<Color = BinaryColor>>(
        &self,
        target: &mut D,
        string: &str,
    ) -> Result<u32, D::Error> {
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

        Ok(Self::text_style()
            .measure_string(&string, Point::zero(), Baseline::Top)
            .bounding_box
            .size
            .width)
    }

    fn draw_charging_indicator<D: DrawTarget<Color = BinaryColor>>(
        &self,
        target: &mut D,
        battery_data_width: u32,
    ) -> Result<(), D::Error> {
        #[rustfmt::skip]
        const DATA: &[u8] = &[
            0b010100_00,
            0b010100_00,
            0b111110_00,
            0b011100_00,
            0b011100_00,
            0b001000_00,
            0b001000_00,
            0b010000_00,
        ];
        const IMAGE_WIDTH: u32 = 6;
        let raw_image = ImageRaw::<BinaryColor>::new(DATA, IMAGE_WIDTH);
        let image = Image::new(
            &raw_image,
            Point::new(
                (target.bounding_box().size.width - battery_data_width - IMAGE_WIDTH) as i32,
                0,
            ),
        );
        image.draw(target)
    }

    fn draw<D: DrawTarget<Color = BinaryColor>>(
        &self,
        target: &mut D,
        data: BatteryInfo,
    ) -> Result<(), D::Error> {
        let battery_data_width = match self {
            BatteryStyle::MilliVolts => {
                let mut string = heapless::String::<8>::new();

                let volts = data.voltage / 1000;
                let millis = data.voltage - volts * 1000;
                write!(&mut string, "{volts}.{millis:03}V").ok();

                self.draw_text(target, &string)?
            }
            BatteryStyle::Percentage(model) => {
                let mut string = heapless::String::<4>::new();

                let percentage = model.estimate(data.voltage, data.charge_current);
                write!(&mut string, "{percentage}%").ok();

                self.draw_text(target, &string)?
            }
            BatteryStyle::LowIndicator(model) if data.charge_current.is_none() => {
                let percentage = model.estimate(data.voltage, data.charge_current);
                if percentage < 25 {
                    let top_right = target.bounding_box().anchor_point(AnchorPoint::TopRight);
                    let box_top_left = self.draw_battery_outline(target, top_right)?;

                    Line::new(
                        box_top_left + Point::new(2, 3),
                        box_top_left + Point::new(2, 5),
                    )
                    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
                    .draw(target)?;

                    (top_right.x - box_top_left.x + 1) as u32
                } else {
                    0
                }
            }
            BatteryStyle::Icon(model) | BatteryStyle::LowIndicator(model) => {
                let percentage = model.estimate(data.voltage, data.charge_current);
                let bars = (percentage.saturating_sub(1)) / 25;

                let top_right = target.bounding_box().anchor_point(AnchorPoint::TopRight);
                let box_top_left = self.draw_battery_outline(target, top_right)?;

                let mut top_left = box_top_left + Point::new(1, 3);
                for _ in 0..bars {
                    Rectangle::new(top_left + Point::new(1, 0), Size::new(2, 3))
                        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
                        .draw(target)?;

                    top_left += Point::new(3, 0);
                }

                (top_right.x - box_top_left.x + 1) as u32
            }
        };

        if data.charge_current.is_some() {
            self.draw_charging_indicator(target, battery_data_width)?;
        }

        Ok(())
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
