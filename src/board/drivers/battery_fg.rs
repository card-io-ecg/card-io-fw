use embassy_time::{Delay, Duration, Ticker};
use embedded_hal::digital::OutputPin;
use embedded_hal_async::{delay::DelayUs, i2c::I2c};
use max17055::Max17055;

use crate::{board::drivers::battery_monitor::SharedBatteryState, task_control::TaskControlToken};

#[derive(Clone, Copy, Debug)]
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

    pub async fn enable<D: DelayUs>(&mut self, delay: &mut D) {
        self.enable.set_high().unwrap();
        delay.delay_ms(10).await;
        self.fg.load_initial_config_async(delay).await.unwrap();
    }

    pub async fn read_data(&mut self) -> Result<BatteryFgData, ()> {
        let voltage_uv = self.fg.read_vcell().await.unwrap();
        let percentage = self.fg.read_reported_soc().await.unwrap();

        Ok(BatteryFgData {
            voltage: (voltage_uv / 1000) as u16, // mV
            percentage,
        })
    }

    pub fn disable(&mut self) {
        self.enable.set_low().unwrap();
    }
}

#[embassy_executor::task]
pub async fn monitor_task_fg(
    mut fuel_gauge: crate::board::BatteryFg,
    battery_state: SharedBatteryState,
    mut task_control: TaskControlToken<()>,
) {
    task_control
        .run_cancellable(async {
            let mut timer = Ticker::every(Duration::from_secs(1));
            log::info!("Fuel gauge monitor started");

            fuel_gauge.enable(&mut Delay).await;

            loop {
                let data = fuel_gauge.read_data().await.unwrap();

                {
                    let mut state = battery_state.lock().await;
                    state.fg_data = Some(data);
                }
                log::debug!("Battery data: {data:?}");

                timer.next().await;
            }
        })
        .await;

    fuel_gauge.disable();
    log::info!("Monitor exited");
}
