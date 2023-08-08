use embedded_graphics::{
    image::Image,
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point},
    Drawable,
};
use tinybmp::Bmp;

use crate::widgets::{progress_bar::ProgressBar, status_bar::StatusBar};

pub struct StartupScreen<'a> {
    pub label: &'a str,
    pub progress: u32,
    pub max_progress: u32,
    pub status_bar: StatusBar,
}

impl Drawable for StartupScreen<'_> {
    type Color = BinaryColor;
    type Output = ();

    #[inline]
    fn draw<DT: DrawTarget<Color = BinaryColor>>(&self, display: &mut DT) -> Result<(), DT::Error> {
        ProgressBar {
            label: self.label,
            progress: self.progress,
            max_progress: self.max_progress,
        }
        .draw(display)?;

        let logo = include_bytes!("../static/logo.bmp");
        let bmp = Bmp::from_slice(logo).unwrap();

        Image::new(&bmp, Point::new(1, 12)).draw(display)?;

        self.status_bar.draw(display)?;

        Ok(())
    }
}
