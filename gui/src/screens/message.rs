use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::DrawTarget,
    Drawable,
};
use embedded_text::{
    alignment::{HorizontalAlignment, VerticalAlignment},
    style::{HeightMode, TextBoxStyleBuilder, VerticalOverdraw},
    TextBox,
};

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
        let character_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
        let text_style = TextBoxStyleBuilder::new()
            .alignment(HorizontalAlignment::Center)
            .vertical_alignment(VerticalAlignment::Middle)
            .height_mode(HeightMode::Exact(VerticalOverdraw::Visible))
            .build();

        TextBox::with_textbox_style(
            self.message,
            display.bounding_box(),
            character_style,
            text_style,
        )
        .draw(display)?;

        Ok(())
    }
}
