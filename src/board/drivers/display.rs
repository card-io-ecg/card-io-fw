use display_interface::DisplayError;
use embassy_executor::_export::StaticCell;
use embassy_time::Delay;
use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{Dimensions, DrawTarget},
    primitives::Rectangle,
    Pixel,
};
use embedded_hal::digital::OutputPin;
use ssd1306::{
    command::AddrMode, mode::BufferedGraphicsMode, prelude::Brightness, rotation::DisplayRotation,
    size::DisplaySize128x64, Ssd1306,
};

use crate::board::DisplayInterface;

static DISPLAY: StaticCell<
    Ssd1306<DisplayInterface, DisplaySize128x64, BufferedGraphicsMode<DisplaySize128x64>>,
> = StaticCell::new();

pub struct Display<RESET> {
    display: &'static mut Ssd1306<
        DisplayInterface<'static>,
        DisplaySize128x64,
        BufferedGraphicsMode<DisplaySize128x64>,
    >,
    reset: RESET,
}

impl<RESET> Display<RESET>
where
    RESET: OutputPin,
{
    pub fn new(spi: DisplayInterface<'static>, reset: RESET) -> Self {
        let display = DISPLAY.init_with(|| {
            Ssd1306::new(spi, DisplaySize128x64, DisplayRotation::Rotate0)
                .into_buffered_graphics_mode()
        });

        Self { display, reset }
    }

    pub async fn enable(mut self) -> Result<PoweredDisplay<RESET>, DisplayError> {
        unwrap!(self
            .display
            .reset_async::<_, Delay>(&mut self.reset, &mut Delay)
            .await
            .ok());

        self.display
            .init_with_addr_mode_async(AddrMode::Horizontal)
            .await?;
        self.display.clear(BinaryColor::Off)?;
        self.display.flush_async().await?;

        Ok(PoweredDisplay { display: self })
    }
}

pub struct PoweredDisplay<RESET> {
    display: Display<RESET>,
}

impl<RESET> Dimensions for PoweredDisplay<RESET> {
    fn bounding_box(&self) -> Rectangle {
        self.display.display.bounding_box()
    }
}

impl<RESET> DrawTarget for PoweredDisplay<RESET> {
    type Color = BinaryColor;
    type Error = DisplayError;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.display.display.draw_iter(pixels)
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.display.display.clear(color)
    }
}

impl<RESET> PoweredDisplay<RESET> {
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
        self.display.display.flush_async().await
    }

    pub async fn update_brightness_async(
        &mut self,
        brightness: Brightness,
    ) -> Result<(), DisplayError> {
        self.display.display.set_brightness_async(brightness).await
    }
}

impl<RESET> PoweredDisplay<RESET>
where
    RESET: OutputPin,
{
    pub fn shut_down(mut self) -> Display<RESET> {
        unwrap!(self.display.reset.set_low().ok());
        self.display
    }
}
