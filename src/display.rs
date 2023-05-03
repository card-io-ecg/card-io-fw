use display_interface::AsyncWriteOnlyDataCommand;
use embassy_time::Delay;
use embedded_hal::digital::OutputPin;
use embedded_hal_async::spi::SpiDevice as AsyncSpiDevice;
use ssd1306::{
    mode::BufferedGraphicsMode, rotation::DisplayRotation, size::DisplaySize128x64, Ssd1306,
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

    pub async fn enable(&mut self) -> PoweredDisplay<'_, DI, RESET>
    where
        DI: AsyncWriteOnlyDataCommand,
    {
        self.display
            .reset_async::<_, Delay>(&mut self.reset, &mut Delay)
            .await
            .unwrap();

        // TODO configure

        PoweredDisplay { display: self }
    }
}

pub struct PoweredDisplay<'a, S, RESET>
where
    RESET: OutputPin,
{
    display: &'a mut Display<S, RESET>,
}

impl<'a, S, RESET> PoweredDisplay<'a, S, RESET>
where
    RESET: OutputPin,
    S: AsyncSpiDevice,
{
    pub fn shut_down(self) {
        // Implemented in Drop
    }
}

impl<'a, S, RESET> Drop for PoweredDisplay<'a, S, RESET>
where
    RESET: OutputPin,
{
    fn drop(&mut self) {
        self.display.reset.set_low().unwrap();
    }
}
