use embedded_graphics::{
    image::Image,
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point},
    Drawable,
};
use tinybmp::Bmp;

use crate::widgets::progress_bar::ProgressBar;

pub struct StartupScreen<'a> {
    pub label: &'a str,
    pub progress: u32,
}

impl Drawable for StartupScreen<'_> {
    type Color = BinaryColor;
    type Output = ();

    #[inline]
    fn draw<DT: DrawTarget<Color = BinaryColor>>(&self, display: &mut DT) -> Result<(), DT::Error> {
        ProgressBar {
            label: self.label,
            progress: self.progress,
            max_progress: 255,
        }
        .draw(display)?;

        let logo = include_bytes!("../static/logo.bmp");
        let bmp = unwrap!(Bmp::from_slice(logo).ok());

        Image::new(&bmp, Point::new(1, 12)).draw(display)?;

        Ok(())
    }
}
