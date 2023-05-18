use core::{fmt::Write, num::NonZeroU8};

use embedded_graphics::{
    geometry::AnchorPoint,
    image::{Image, ImageRaw},
    mono_font::{ascii::FONT_6X10, MonoTextStyle, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::{Dimensions, DrawTarget, Point},
    primitives::{Line, Primitive, PrimitiveStyle},
    text::{Baseline, Text},
    Drawable,
};
use embedded_layout::prelude::{horizontal, vertical, Align};
use signal_processing::{
    lerp::{Interval, Lerp},
    sliding::SlidingWindow,
};

#[derive(Default)]
pub struct EcgScreen {
    buffer: SlidingWindow<128>,
    discard: usize,
    pub heart_rate: Option<NonZeroU8>,
}

impl EcgScreen {
    pub fn new(discarded_samples: usize) -> Self {
        Self {
            buffer: SlidingWindow::new(),
            discard: discarded_samples,
            heart_rate: None,
        }
    }

    pub fn push(&mut self, sample: f32) {
        if self.discard > 0 {
            self.discard -= 1;
            return;
        }
        self.buffer.push(sample);
    }

    fn limits(&self) -> (f32, f32) {
        let mut samples = self.buffer.iter();

        let Some(first) = samples.next() else { return (0.0, 0.0); };

        let mut min = first;
        let mut max = first;

        for sample in samples {
            if sample > max {
                max = sample;
            }
            if sample < min {
                min = sample;
            }
        }

        (min, max)
    }

    pub fn update_heart_rate(&mut self, hr: u8) {
        self.heart_rate = NonZeroU8::new(hr);
    }

    pub fn clear_heart_rate(&mut self) {
        self.heart_rate = None;
    }
}

impl Drawable for EcgScreen {
    type Color = BinaryColor;
    type Output = ();

    fn draw<DT: DrawTarget<Color = BinaryColor>>(&self, display: &mut DT) -> Result<(), DT::Error> {
        if !self.buffer.is_full() {
            let text_style = MonoTextStyleBuilder::new()
                .font(&FONT_6X10)
                .text_color(BinaryColor::On)
                .build();

            Text::new("Collecting data...", Point::zero(), text_style)
                .align_to(
                    &display.bounding_box(),
                    horizontal::Center,
                    vertical::Center,
                )
                .draw(display)?;

            return Ok(());
        }

        if let Some(hr) = self.heart_rate {
            #[rustfmt::skip]
            const HEART: &[u8] = &[
                0b00000000,
                0b01101100,
                0b11111110,
                0b11111110,
                0b11111110,
                0b01111100,
                0b00111000,
                0b00010000,
            ];
            const IMAGE_WIDTH: u32 = 8;

            let top_left = display.bounding_box().top_left;

            let raw_image = ImageRaw::<BinaryColor>::new(HEART, IMAGE_WIDTH);
            let image = Image::new(&raw_image, top_left);

            image.draw(display)?;

            let mut hr_string = heapless::String::<3>::new();
            write!(&mut hr_string, "{hr}").ok();

            Text::with_baseline(
                &hr_string,
                image.bounding_box().anchor_point(AnchorPoint::TopRight) + Point::new(1, 0),
                MonoTextStyle::new(&FONT_6X10, BinaryColor::On),
                Baseline::Top,
            )
            .draw(display)?;
        }

        let (min, max) = self.limits();

        let scaler = Lerp {
            from: Interval::new(min, max),
            to: Interval::new(0.0, display.bounding_box().size.height as f32 - 1.0),
        };

        let points = self
            .buffer
            .iter()
            .enumerate()
            .map(|(x, y)| Point::new(x as i32, scaler.map(y) as i32));

        let line_segments = points.clone().zip(points.skip(1)).map(|(from, to)| {
            Line::new(from, to).into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        });

        for line in line_segments {
            line.draw(display)?;
        }

        Ok(())
    }
}
