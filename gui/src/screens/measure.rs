use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point},
    primitives::{Line, Primitive, PrimitiveStyle},
    text::Text,
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
}

impl EcgScreen {
    pub fn new(discarded_samples: usize) -> Self {
        Self {
            buffer: SlidingWindow::new(),
            discard: discarded_samples,
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

    pub async fn draw_async<DT: DrawTarget<Color = BinaryColor>>(
        &self,
        display: &mut DT,
    ) -> Result<(), DT::Error> {
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

        let line_segments = points.clone().zip(points.skip(1)).map(|(from, to)| {
            Line::new(from, to).into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        });

        const YIELD_EVERY: usize = 16;
        let mut yield_after = YIELD_EVERY;
        for line in line_segments {
            line.draw(display)?;

            yield_after -= 1;
            if yield_after == 0 {
                yield_after = YIELD_EVERY;
                embassy_futures::yield_now().await;
            }
        }

        Ok(())
    }
}
