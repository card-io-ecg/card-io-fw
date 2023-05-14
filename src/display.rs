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
    command::AddrMode, mode::BufferedGraphicsMode, prelude::Brightness, rotation::DisplayRotation,
    size::DisplaySize128x64, Ssd1306,
};

pub struct Display<DI, RESET> {
    display: Ssd1306<DI, DisplaySize128x64, BufferedGraphicsMode<DisplaySize128x64>>,
    reset: RESET,
}

impl<DI, RESET> Display<DI, RESET>
where
    RESET: OutputPin,
{
    pub fn new(spi: DI, reset: RESET) -> Self {
        Self {
            display: Ssd1306::new(spi, DisplaySize128x64, DisplayRotation::Rotate0)
                .into_buffered_graphics_mode(),
            reset,
        }
    }

    pub async fn enable(mut self) -> Result<PoweredDisplay<DI, RESET>, DisplayError>
    where
        DI: AsyncWriteOnlyDataCommand,
    {
        self.display
            .reset_async::<_, Delay>(&mut self.reset, &mut Delay)
            .await
            .unwrap();

        self.display
            .init_with_addr_mode_async(AddrMode::Horizontal)
            .await?;
        self.display.clear();
        self.display.flush_async().await?;

        Ok(PoweredDisplay { display: self })
    }
}

pub struct PoweredDisplay<DI, RESET> {
    display: Display<DI, RESET>,
}

impl<DI, RESET> Dimensions for PoweredDisplay<DI, RESET> {
    fn bounding_box(&self) -> Rectangle {
        self.display.display.bounding_box()
    }
}

impl<DI, RESET> DrawTarget for PoweredDisplay<DI, RESET> {
    type Color = BinaryColor;
    type Error = DisplayError;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.display.display.draw_iter(pixels)
    }
}

impl<DI, RESET> PoweredDisplay<DI, RESET>
where
    DI: AsyncWriteOnlyDataCommand,
{
    pub async fn frame(
        &mut self,
        render: impl FnOnce(&mut Self) -> Result<(), DisplayError>,
    ) -> Result<(), DisplayError> {
        self.clear(BinaryColor::Off)?;

        render(self)?;

        self.flush().await
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

impl<DI, RESET> PoweredDisplay<DI, RESET>
where
    RESET: OutputPin,
{
    pub fn shut_down(mut self) -> Display<DI, RESET> {
        self.display.reset.set_low().unwrap();
        self.display
    }
}
