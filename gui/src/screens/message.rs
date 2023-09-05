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
        const TEXT_STYLE: embedded_text::style::TextBoxStyle = TextBoxStyleBuilder::new()
            .alignment(HorizontalAlignment::Center)
            .vertical_alignment(VerticalAlignment::Middle)
            .height_mode(HeightMode::Exact(VerticalOverdraw::Visible))
            .build();
        const CHARACTER_STYLE: MonoTextStyle<'static, BinaryColor> =
            MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

        TextBox::with_textbox_style(
            self.message,
            display.bounding_box(),
            CHARACTER_STYLE,
            TEXT_STYLE,
        )
        .draw(display)?;

        Ok(())
    }
}
