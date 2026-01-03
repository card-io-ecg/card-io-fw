use embassy_time::{Delay, Duration, Ticker, Timer};
use embedded_hal::digital::OutputPin;
use embedded_hal_async::i2c::I2c;
use max17055::Max17055;

use crate::{task_control::TaskControlToken, Shared};

#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct BatteryFgData {
    pub voltage: u16,
    pub percentage: u8,
}

pub struct BatteryFg<I2C, EN> {
    pub fg: Max17055<I2C>,
    pub enable: EN,
}

impl<I2C, EN> BatteryFg<I2C, EN>
where
    EN: OutputPin,
    I2C: I2c,
{
    pub fn new(fg: Max17055<I2C>, enable: EN) -> Self {
        Self { fg, enable }
    }

    pub async fn enable(&mut self) -> Result<(), ()> {
        self.enable.set_high().map_err(|_| ())?;
        Timer::after(Duration::from_millis(10)).await;
        self.fg
            .load_initial_config_async(&mut Delay)
            .await
            .map_err(|_| ())?;
        Ok(())
    }

    pub async fn read_data(&mut self) -> Result<BatteryFgData, ()> {
        let voltage_uv = self.fg.read_vcell().await.map_err(|_| ())?;
        let percentage = self.fg.read_reported_soc().await.map_err(|_| ())?;

        Ok(BatteryFgData {
            voltage: (voltage_uv / 1000) as u16, // mV
            percentage,
        })
    }
}

#[embassy_executor::task]
pub async fn monitor_task_fg(
    fuel_gauge: Shared<super::BatterySensor>,
    mut task_control: TaskControlToken<()>,
) {
    task_control
        .run_cancellable(|_| async {
            if fuel_gauge.lock().await.enable().await.is_err() {
                error!("Failed to enable fuel gauge");
                return;
            }

            info!("Fuel gauge monitor started");

            let mut timer = Ticker::every(Duration::from_secs(1));
            loop {
                {
                    let mut sensor = fuel_gauge.lock().await;
                    if let Ok(data) = sensor.read_data().await {
                        sensor.update_data(data);
                        trace!("Battery data: {:?}", data);
                    } else {
                        error!("Failed to read battery data");
                    }
                }

                timer.next().await;
            }
        })
        .await;

    info!("Monitor exited");
}
