use embedded_graphics::{pixelcolor::BinaryColor, prelude::DrawTarget, Drawable};
use embedded_text::TextBox;

use crate::screens::{CENTERED_TEXTBOX, NORMAL_TEXT};

pub struct MessageScreen<'a> {
    pub message: &'a str,
}

impl Drawable for MessageScreen<'_> {
    type Color = BinaryColor;
    type Output = ();

    #[inline]
    fn draw<D>(&self, display: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        TextBox::with_textbox_style(
            self.message,
            display.bounding_box(),
            NORMAL_TEXT,
            CENTERED_TEXTBOX,
        )
        .draw(display)?;

        Ok(())
    }
}
