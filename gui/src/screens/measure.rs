use core::{cell::RefCell, num::NonZeroU8};

use embedded_graphics::{
    image::{Image, ImageRaw},
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, OriginDimensions, Point},
    primitives::{Line, Primitive, PrimitiveStyle},
    text::{Baseline, Text},
    Drawable,
};
use itertools::Itertools;
use signal_processing::{
    lerp::{Interval, Lerp},
    sliding::SlidingWindow,
};
use ufmt::uwrite;

use crate::screens::{message::MessageScreen, NORMAL_TEXT};

struct CameraConfig {
    shrink_end: usize,
    shrink_delay: usize,
}

enum LimitKind {
    Min,
    Max,
}

struct Limit {
    current: f32,
    target: f32,
    delta: f32,
    kind: LimitKind,
    age: usize,
}

impl Limit {
    fn new(kind: LimitKind) -> Limit {
        let current = match kind {
            LimitKind::Min => f32::MAX,
            LimitKind::Max => f32::MIN,
        };
        Self {
            current,
            target: current,
            delta: 0.0,
            kind,
            age: 0,
        }
    }

    pub fn update(&mut self, value: f32, config: &CameraConfig) -> f32 {
        let reset = match self.kind {
            LimitKind::Min => value <= self.current,
            LimitKind::Max => value >= self.current,
        };

        if reset {
            self.current = value;
            self.target = value;
            self.delta = 0.0;
            self.age = 0;
        } else if self.current != value {
            // Short circuit if the value hasn't changed

            if value != self.target {
                // target changed, reset age and compute new delta
                self.age = self.age.min(config.shrink_delay);
                self.target = value;
                self.delta =
                    (value - self.current) / (config.shrink_end - config.shrink_delay) as f32;
            } else {
                // target unchanged, increment age
                self.age += 1;
            }

            if self.age > config.shrink_delay {
                let remaining_shrink_frames = config.shrink_end - self.age;

                if remaining_shrink_frames == 0 {
                    self.age = 0;
                    self.current = value;
                    self.delta = 0.0;
                } else {
                    self.current += self.delta;
                }
            }
        }

        self.current
    }
}

struct Camera {
    config: CameraConfig,
    min_limit: Limit,
    max_limit: Limit,
}

impl Camera {
    fn update_range(&mut self, min: f32, max: f32) -> (f32, f32) {
        let min = self.min_limit.update(min, &self.config);
        let max = self.max_limit.update(max, &self.config);
        (min, max)
    }

    fn update(&mut self, min: f32, max: f32, display: &impl DrawTarget) -> Lerp {
        let (min, max) = self.update_range(min, max);

        Lerp {
            from: Interval::new(min, max),
            to: Interval::new(0.0, display.bounding_box().size.height as f32 - 1.0),
        }
    }
}

pub struct EcgScreen {
    buffer: SlidingWindow<128>,
    pub heart_rate: Option<NonZeroU8>,
    pub elapsed_secs: usize,
    camera: RefCell<Camera>,
}

impl EcgScreen {
    pub fn new() -> Self {
        Self {
            buffer: SlidingWindow::new(),
            heart_rate: None,
            elapsed_secs: 0,
            camera: RefCell::new(Camera {
                min_limit: Limit::new(LimitKind::Min),
                max_limit: Limit::new(LimitKind::Max),
                config: CameraConfig {
                    // We display samples at 125sps. A 50 sample delay means we don't shrink the viewport
                    // between pulses if the heart rate is above 1.4s or 42bpm.
                    shrink_end: 120,
                    shrink_delay: 50,
                },
            }),
        }
    }

    pub fn push(&mut self, sample: f32) {
        self.buffer.push(sample);
    }

    pub fn buffer_full(&self) -> bool {
        self.buffer.is_full()
    }

    fn limits(&self) -> (f32, f32) {
        let mut samples = self.buffer.iter_unordered();

        let Some(first) = samples.next() else {
            return (0.0, 0.0);
        };

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

    pub fn update_heart_rate(&mut self, hr: Option<NonZeroU8>) {
        self.heart_rate = hr;
    }
}

impl Drawable for EcgScreen {
    type Color = BinaryColor;
    type Output = ();

    #[inline]
    fn draw<DT: DrawTarget<Color = BinaryColor>>(&self, display: &mut DT) -> Result<(), DT::Error> {
        if !self.buffer.is_full() {
            MessageScreen {
                message: "Collecting data...",
            }
            .draw(display)?;

            return Ok(());
        }

        let mut status_loc = display.bounding_box().top_left;

        let mut str_buffer = heapless::String::<16>::new();
        unwrap!(uwrite!(&mut str_buffer, "{}s", self.elapsed_secs));
        status_loc = Text::with_baseline(&str_buffer, status_loc, NORMAL_TEXT, Baseline::Top)
            .draw(display)?;

        if let Some(hr) = self.heart_rate {
            const HEART: ImageRaw<'_, BinaryColor> = ImageRaw::new(
                &[
                    0b00000000, //
                    0b01101100, //
                    0b11111110, //
                    0b11111110, //
                    0b11111110, //
                    0b01111100, //
                    0b00111000, //
                    0b00010000, //
                ],
                8,
            );

            Image::new(&HEART, status_loc).draw(display)?;
            status_loc += Point::new(HEART.size().width as i32, 0);

            str_buffer.clear();
            unwrap!(uwrite!(&mut str_buffer, "{}", hr).ok());

            Text::with_baseline(&str_buffer, status_loc, NORMAL_TEXT, Baseline::Top)
                .draw(display)?;
        }

        let (min, max) = self.limits();

        let scaler = unwrap!(self.camera.try_borrow_mut()).update(min, max, display);

        const LINE_STYLE: PrimitiveStyle<BinaryColor> =
            PrimitiveStyle::with_stroke(BinaryColor::On, 1);

        let line_segments = self
            .buffer
            .iter()
            .enumerate()
            .map(|(x, y)| Point::new(x as i32, scaler.map(y) as i32))
            .tuple_windows()
            .map(|(from, to)| Line::new(from, to).into_styled(LINE_STYLE));

        for line in line_segments {
            line.draw(display)?;
        }

        Ok(())
    }
}
