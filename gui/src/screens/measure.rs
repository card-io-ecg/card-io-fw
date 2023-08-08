use core::{cell::RefCell, fmt::Write, num::NonZeroU8};

use crate::widgets::status_bar::StatusBar;
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

struct CameraConfig {
    shrink_frames: usize,
    shrink_delay: usize,
}

enum LimitKind {
    Min,
    Max,
}

struct Limit {
    current: f32,
    kind: LimitKind,
    age: usize,
}

impl Limit {
    fn new(kind: LimitKind) -> Limit {
        Self {
            current: match kind {
                LimitKind::Min => f32::MAX,
                LimitKind::Max => f32::MIN,
            },
            kind,
            age: 0,
        }
    }

    pub fn update(&mut self, value: f32, config: &CameraConfig) -> f32 {
        let reset = match self.kind {
            LimitKind::Min => value < self.current,
            LimitKind::Max => value > self.current,
        };

        if reset {
            self.current = value;
            self.age = 0;
        } else {
            self.age += 1;
            if self.age > config.shrink_delay {
                let remaining_shrink_frames =
                    config.shrink_frames - (self.age - config.shrink_delay);

                if remaining_shrink_frames == 0 {
                    self.age = 0;
                    self.current = value;
                } else {
                    let delta = (value - self.current) / remaining_shrink_frames as f32;
                    self.current += delta;
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
    discard: usize,
    pub heart_rate: Option<NonZeroU8>,
    camera: RefCell<Camera>,
    pub status_bar: StatusBar,
}

impl EcgScreen {
    pub fn new(discarded_samples: usize, status_bar: StatusBar) -> Self {
        Self {
            buffer: SlidingWindow::new(),
            discard: discarded_samples,
            heart_rate: None,
            camera: RefCell::new(Camera {
                min_limit: Limit::new(LimitKind::Min),
                max_limit: Limit::new(LimitKind::Max),
                config: CameraConfig {
                    shrink_frames: 60,
                    shrink_delay: 60,
                },
            }),
            status_bar,
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

    #[inline]
    fn draw<DT: DrawTarget<Color = BinaryColor>>(&self, display: &mut DT) -> Result<(), DT::Error> {
        self.status_bar
            .align_to(&display.bounding_box(), horizontal::Right, vertical::Top)
            .draw(display)?;

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

        let scaler = self.camera.borrow_mut().update(min, max, display);

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
