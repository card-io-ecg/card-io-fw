use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point},
    text::{Baseline, Text},
    Drawable,
};

use crate::screens::NORMAL_TEXT;

/// The local time as `HH:MM`, at the top left of the screen.
pub struct Clock {
    pub hour: u8,
    pub minute: u8,
}

impl Drawable for Clock {
    type Color = BinaryColor;
    type Output = ();

    fn draw<DT: DrawTarget<Color = BinaryColor>>(&self, display: &mut DT) -> Result<(), DT::Error> {
        let digits = [
            b'0' + self.hour / 10,
            b'0' + self.hour % 10,
            b':',
            b'0' + self.minute / 10,
            b'0' + self.minute % 10,
        ];
        let text = unwrap!(core::str::from_utf8(&digits).ok());

        Text::with_baseline(text, Point::zero(), NORMAL_TEXT, Baseline::Top).draw(display)?;

        Ok(())
    }
}
