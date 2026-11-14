use core::num::NonZeroU32;

use embedded_graphics::{
    image::Image,
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point},
    text::{Baseline, Text},
    Drawable,
};
use tinybmp::Bmp;

use crate::{screens::NORMAL_TEXT, widgets::progress_bar::ProgressBar};

pub struct StartupScreen<'a> {
    pub label: &'a str,
    pub progress: u32,
    /// The local hour and minute.
    pub time: Option<(u8, u8)>,
}

impl Drawable for StartupScreen<'_> {
    type Color = BinaryColor;
    type Output = ();

    #[inline]
    fn draw<DT: DrawTarget<Color = BinaryColor>>(&self, display: &mut DT) -> Result<(), DT::Error> {
        ProgressBar {
            label: self.label,
            progress: self.progress,
            max_progress: unwrap!(NonZeroU32::new(255)),
        }
        .draw(display)?;

        let logo = include_bytes!("../static/logo.bmp");
        let bmp = unwrap!(Bmp::from_slice(logo).ok());

        Image::new(&bmp, Point::new(1, 12)).draw(display)?;

        if let Some((hour, minute)) = self.time {
            let digits = [
                b'0' + hour / 10,
                b'0' + hour % 10,
                b':',
                b'0' + minute / 10,
                b'0' + minute % 10,
            ];
            let text = unwrap!(core::str::from_utf8(&digits).ok());

            Text::with_baseline(text, Point::zero(), NORMAL_TEXT, Baseline::Top).draw(display)?;
        }

        Ok(())
    }
}
