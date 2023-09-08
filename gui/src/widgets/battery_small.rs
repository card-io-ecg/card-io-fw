use embedded_graphics::{
    geometry::AnchorPoint,
    image::{Image, ImageRaw},
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, DrawTargetExt, Point, Size},
    primitives::{Line, Primitive, PrimitiveStyle, Rectangle},
    text::{renderer::TextRenderer, Alignment, Baseline, Text, TextStyleBuilder},
    Drawable,
};
use embedded_layout::prelude::*;
use ufmt::uwrite;

use crate::screens::{BatteryInfo, NORMAL_TEXT};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BatteryStyle {
    MilliVolts,
    Percentage,
    Icon,
    LowIndicator,
}

impl BatteryStyle {
    fn size(&self) -> Size {
        match self {
            BatteryStyle::MilliVolts => {
                NORMAL_TEXT
                    .measure_string("C0.000V", Point::zero(), Baseline::Top)
                    .bounding_box
                    .size
            }
            BatteryStyle::Percentage => {
                NORMAL_TEXT
                    .measure_string("C000%", Point::zero(), Baseline::Top)
                    .bounding_box
                    .size
            }
            BatteryStyle::Icon | BatteryStyle::LowIndicator => Size::new(13, 10) + Size::new(6, 10),
        }
    }

    fn draw_battery_outline<D: DrawTarget<Color = BinaryColor>>(
        &self,
        target: &mut D,
        top_right: Point,
    ) -> Result<Point, D::Error> {
        #[rustfmt::skip]
        #[allow(clippy::unusual_byte_groupings)]
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
            string,
            Point::new(target.bounding_box().size.width as i32 - 1, 0),
            NORMAL_TEXT,
            TextStyleBuilder::new()
                .baseline(Baseline::Top)
                .alignment(Alignment::Right)
                .build(),
        )
        .draw(target)?;

        Ok(NORMAL_TEXT
            .measure_string(string, Point::zero(), Baseline::Top)
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
        #[allow(clippy::unusual_byte_groupings)]
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
                unwrap!(uwrite!(&mut string, "{}mV", data.voltage));

                self.draw_text(target, &string)?
            }
            BatteryStyle::Percentage => {
                let mut string = heapless::String::<4>::new();

                unwrap!(uwrite!(&mut string, "{}%", data.percentage));

                self.draw_text(target, &string)?
            }
            BatteryStyle::LowIndicator if !data.is_charging => {
                if data.percentage < 25 {
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
            BatteryStyle::Icon | BatteryStyle::LowIndicator => {
                let bars = (data.percentage.saturating_sub(1)) / 25;

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

        if data.is_charging {
            self.draw_charging_indicator(target, battery_data_width)?;
        }

        Ok(())
    }
}

#[derive(Clone, Copy)]
pub struct Battery {
    pub data: Option<BatteryInfo>,
    pub style: BatteryStyle,
    top_left: Point,
}

impl Battery {
    #[inline]
    pub fn with_style(data: Option<BatteryInfo>, style: BatteryStyle) -> Self {
        Self {
            data,
            style,
            top_left: Point::zero(),
        }
    }

    #[inline]
    pub fn icon(data: Option<BatteryInfo>) -> Self {
        Self::with_style(data, BatteryStyle::Icon)
    }

    #[inline]
    pub fn percentage(data: Option<BatteryInfo>) -> Self {
        Self::with_style(data, BatteryStyle::Percentage)
    }
}

impl View for Battery {
    #[inline]
    fn translate_impl(&mut self, by: Point) {
        self.top_left += by;
    }

    #[inline]
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
        if let Some(data) = self.data {
            let mut cropped = target.cropped(&self.bounds());
            self.style.draw(&mut cropped, data)?;
        }

        Ok(())
    }
}
