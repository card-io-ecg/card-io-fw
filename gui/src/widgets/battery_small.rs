use embedded_graphics::{
    geometry::AnchorPoint,
    image::{Image, ImageRaw},
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, OriginDimensions, Point, Size},
    primitives::{Primitive, PrimitiveStyle, Rectangle},
    text::{renderer::TextRenderer, Alignment, Baseline, Text, TextStyleBuilder},
    Drawable,
};
use embedded_io_async::{Read, Write};
use embedded_layout::prelude::*;
use embedded_menu::items::menu_item::SelectValue;
use norfs::storable::{LoadError, Loadable, Storable};
use ufmt::uwrite;

use crate::screens::{BatteryInfo, ChargingState, NORMAL_TEXT};

fn draw_image_right_aligned<D>(
    target: &mut D,
    raw_image: &ImageRaw<'_, BinaryColor>,
    top_right: Point,
) -> Result<Point, <D as DrawTarget>::Error>
where
    D: DrawTarget<Color = BinaryColor>,
{
    let top_left = top_right - Point::new(raw_image.size().width as i32, 0);
    Image::new(raw_image, top_left).draw(target)?;

    Ok(top_left)
}

struct ChargingIndicator {
    state: ChargingState,
    top_right: Point,
}

impl Drawable for ChargingIndicator {
    type Color = BinaryColor;
    type Output = Point;

    fn draw<D>(&self, target: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        #[allow(clippy::unusual_byte_groupings)]
        const PLUGGED_IMAGE: ImageRaw<'_, BinaryColor> = ImageRaw::new(
            &[
                0b010100_00,
                0b010100_00,
                0b111110_00,
                0b011100_00,
                0b011100_00,
                0b001000_00,
                0b001000_00,
                0b010000_00,
            ],
            6,
        );
        #[allow(clippy::unusual_byte_groupings)]
        const CHARGING_IMAGE: ImageRaw<'_, BinaryColor> = ImageRaw::new(
            &[
                0b00000_000,
                0b00010_000,
                0b00100_000,
                0b01100_000,
                0b00110_000,
                0b00011_000,
                0b00010_000,
                0b00100_000,
            ],
            5,
        );

        let raw_image = match self.state {
            ChargingState::Discharging => return Ok(self.top_right),
            ChargingState::Plugged => &PLUGGED_IMAGE,
            ChargingState::Charging => &CHARGING_IMAGE,
        };

        draw_image_right_aligned(target, raw_image, self.top_right)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BatteryStyle {
    MilliVolts,
    Percentage,
    Icon,
    LowIndicator,
}

impl BatteryStyle {
    #[rustfmt::skip]
    #[allow(clippy::unusual_byte_groupings)]
    const BATTERY_OUTLINE: ImageRaw<'static, BinaryColor> = ImageRaw::new(
        &[
            0b00000000, 0b00000_000,
            0b11111111, 0b11110_000,
            0b10000000, 0b00010_000,
            0b10000000, 0b00011_000,
            0b10000000, 0b00011_000,
            0b10000000, 0b00011_000,
            0b10000000, 0b00010_000,
            0b11111111, 0b11110_000,
            0b00000000, 0b00000_000,
        ],
        13
    );

    #[rustfmt::skip]
    #[allow(clippy::unusual_byte_groupings)]
    const LOW_BATTERY: ImageRaw<'static, BinaryColor> = ImageRaw::new(
        &[
            0b00000000, 0b00000_000,
            0b11111111, 0b11110_000,
            0b10000000, 0b00010_000,
            0b10100000, 0b00011_000,
            0b10100000, 0b00011_000,
            0b10100000, 0b00011_000,
            0b10000000, 0b00010_000,
            0b11111111, 0b11110_000,
            0b00000000, 0b00000_000,
        ],
        13
    );

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
        draw_image_right_aligned(target, &Self::BATTERY_OUTLINE, top_right)
    }

    fn draw_low_battery<D: DrawTarget<Color = BinaryColor>>(
        &self,
        target: &mut D,
        top_right: Point,
    ) -> Result<Point, D::Error> {
        draw_image_right_aligned(target, &Self::LOW_BATTERY, top_right)
    }

    fn draw_text<D: DrawTarget<Color = BinaryColor>>(
        &self,
        target: &mut D,
        string: &str,
        bounds: &Rectangle,
    ) -> Result<u32, D::Error> {
        let top_right = bounds.anchor_point(AnchorPoint::TopRight);
        Text::with_text_style(
            string,
            top_right,
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

    fn draw<D: DrawTarget<Color = BinaryColor>>(
        &self,
        target: &mut D,
        data: BatteryInfo,
        bounds: &Rectangle,
    ) -> Result<(), D::Error> {
        let battery_data_width = match self {
            BatteryStyle::MilliVolts | BatteryStyle::Percentage => {
                let mut string = heapless::String::<8>::new();

                if matches!(self, BatteryStyle::MilliVolts) {
                    _ = uwrite!(&mut string, "{}mV", data.voltage);
                } else {
                    _ = uwrite!(&mut string, "{}%", data.percentage);
                }

                self.draw_text(target, &string, bounds)?
            }
            BatteryStyle::LowIndicator if !data.is_charging() => {
                if data.percentage < 25 {
                    let top_right = bounds.anchor_point(AnchorPoint::TopRight);
                    let box_top_left = self.draw_low_battery(target, top_right)?;

                    (top_right.x - box_top_left.x + 1) as u32
                } else {
                    0
                }
            }
            BatteryStyle::Icon | BatteryStyle::LowIndicator => {
                let bars = (data.percentage.saturating_sub(1)) / 25;

                let top_right = bounds.anchor_point(AnchorPoint::TopRight);
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

        ChargingIndicator {
            state: data.charging_state,
            top_right: bounds.anchor_point(AnchorPoint::TopRight)
                - Point::new(battery_data_width as i32, 0),
        }
        .draw(target)?;

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
            self.style.draw(target, data, &self.bounds())?;
        }

        Ok(())
    }
}

impl SelectValue for BatteryStyle {
    fn next(&mut self) {
        *self = match self {
            Self::MilliVolts => Self::Percentage,
            Self::Percentage => Self::Icon,
            Self::Icon => Self::LowIndicator,
            Self::LowIndicator => Self::MilliVolts,
        };
    }

    fn marker(&self) -> &str {
        match self {
            Self::MilliVolts => "MilliVolts",
            Self::Percentage => "Percentage",
            Self::Icon => "Icon",
            Self::LowIndicator => "Indicator",
        }
    }
}

impl Loadable for BatteryStyle {
    async fn load<R: Read>(reader: &mut R) -> Result<Self, LoadError<R::Error>> {
        let data = match u8::load(reader).await? {
            0 => Self::MilliVolts,
            1 => Self::Percentage,
            2 => Self::Icon,
            3 => Self::LowIndicator,
            _ => return Err(LoadError::InvalidValue),
        };

        Ok(data)
    }
}

impl Storable for BatteryStyle {
    async fn store<W: Write>(&self, writer: &mut W) -> Result<(), W::Error> {
        (*self as u8).store(writer).await
    }
}
