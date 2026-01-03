use display_interface::{AsyncWriteOnlyDataCommand, DisplayError};
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

type Driver<IFACE> =
    Ssd1306Async<IFACE, DisplaySize128x64, BufferedGraphicsModeAsync<DisplaySize128x64>>;

pub struct Display<IFACE, RESET> {
    display: Driver<IFACE>,
    reset: RESET,
}

impl<IFACE, RESET> Display<IFACE, RESET>
where
    IFACE: AsyncWriteOnlyDataCommand,
    RESET: OutputPin,
{
    pub fn new(interface: IFACE, reset: RESET) -> Self {
        Self {
            display: Ssd1306Async::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
                .into_buffered_graphics_mode(),
            reset,
        }
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

    pub async fn frame(&mut self, render: impl FnOnce(&mut Self) -> Result<(), DisplayError>) {
        unwrap!(self.clear(BinaryColor::Off), "Failed to clear display");
        unwrap!(render(self), "Failed to render frame");
        unwrap!(self.display.flush().await);
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

impl<IFACE, RESET> Dimensions for Display<IFACE, RESET>
where
    IFACE: AsyncWriteOnlyDataCommand,
{
    fn bounding_box(&self) -> Rectangle {
        self.display.bounding_box()
    }
}

impl<IFACE, RESET> DrawTarget for Display<IFACE, RESET>
where
    IFACE: AsyncWriteOnlyDataCommand,
{
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
