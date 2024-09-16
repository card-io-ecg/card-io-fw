use display_interface::DisplayError;
use embassy_time::Delay;
use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{Dimensions, DrawTarget},
    primitives::Rectangle,
    Pixel,
};
use embedded_hal::digital::OutputPin;
use ssd1306::{
    mode::BufferedGraphicsModeAsync, prelude::*, rotation::DisplayRotation,
    size::DisplaySize128x64, Ssd1306Async,
};
use static_cell::make_static;

use crate::board::DisplayInterface;

pub struct Display<RESET> {
    display: &'static mut Ssd1306Async<
        DisplayInterface<'static>,
        DisplaySize128x64,
        BufferedGraphicsModeAsync<DisplaySize128x64>,
    >,
    reset: RESET,
}

impl<RESET> Display<RESET>
where
    RESET: OutputPin,
{
    pub fn new(spi: DisplayInterface<'static>, reset: RESET) -> Self {
        let display = make_static! {
            Ssd1306Async::new(spi, DisplaySize128x64, DisplayRotation::Rotate0)
                .into_buffered_graphics_mode()
        };

        Self { display, reset }
    }

    pub async fn enable(&mut self) -> Result<(), DisplayError> {
        unwrap!(self
            .display
            .reset::<_, Delay>(&mut self.reset, &mut Delay)
            .await
            .ok());

        self.display.init().await?;
        self.display.flush().await?;

        Ok(())
    }

    async fn frame_impl(
        &mut self,
        render: impl FnOnce(&mut Self) -> Result<(), DisplayError>,
    ) -> Result<(), DisplayError> {
        self.clear(BinaryColor::Off)?;

        render(self)?;

        self.flush().await
    }

    pub async fn frame(&mut self, render: impl FnOnce(&mut Self) -> Result<(), DisplayError>) {
        unwrap!(self.frame_impl(render).await.ok());
    }

    pub async fn flush(&mut self) -> Result<(), DisplayError> {
        self.display.flush().await
    }

    pub async fn update_brightness_async(
        &mut self,
        brightness: Brightness,
    ) -> Result<(), DisplayError> {
        self.display.set_brightness(brightness).await
    }

    pub fn shut_down(&mut self) {
        unwrap!(self.reset.set_low().ok());
    }
}

impl<RESET> Dimensions for Display<RESET> {
    fn bounding_box(&self) -> Rectangle {
        self.display.bounding_box()
    }
}

impl<RESET> DrawTarget for Display<RESET> {
    type Color = BinaryColor;
    type Error = DisplayError;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.display.draw_iter(pixels)
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.display.clear(color)
    }
}
