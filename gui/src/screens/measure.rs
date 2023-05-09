use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point},
    primitives::{Line, Primitive, PrimitiveStyle},
    text::Text,
    Drawable,
};
use embedded_layout::prelude::{horizontal, vertical, Align};
use signal_processing::sliding::SlidingWindow;

#[derive(Default)]
pub struct EcgScreen {
    buffer: SlidingWindow<128>,
}

impl EcgScreen {
    pub fn new() -> Self {
        Self {
            buffer: SlidingWindow::new(),
        }
    }

    pub fn push(&mut self, sample: f32) {
        self.buffer.push(sample)
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
}

pub struct Interval {
    min: f32,
    width: f32,
}

impl Interval {
    pub fn new(min: f32, max: f32) -> Self {
        Self {
            min,
            width: max - min,
        }
    }
}

pub struct Lerp {
    pub from: Interval,
    pub to: Interval,
}

impl Lerp {
    pub fn map(&self, value: f32) -> f32 {
        if self.from.width == 0.0 {
            self.to.min
        } else {
            (value - self.from.min) * (self.to.width / self.from.width) + self.to.min
        }
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

        for (from, to) in points.clone().zip(points.skip(1)) {
            Line::new(from, to)
                .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
                .draw(display)?;
        }

        Ok(())
    }
}
