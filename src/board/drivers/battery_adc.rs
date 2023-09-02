use crate::{
    board::{
        drivers::battery_monitor::SharedBatteryState,
        hal::{
            adc::{
                AdcCalEfuse, AdcCalLine, AdcConfig, AdcHasLineCal, AdcPin, Attenuation,
                CalibrationAccess, ADC,
            },
            peripheral::Peripheral,
            prelude::*,
        },
    },
    task_control::TaskControlToken,
    Shared,
};
use embassy_futures::yield_now;
use embassy_time::{Duration, Ticker};
use embedded_hal_old::adc::{Channel, OneShot};

#[derive(Clone, Copy, Debug, defmt::Format)]
pub struct BatteryAdcData {
    pub voltage: u16,
    pub charge_current: u16,
}

pub struct BatteryAdc<V, A, EN, ADCI: 'static> {
    pub voltage_in: AdcPin<V, ADCI, AdcCalLine<ADCI>>,
    pub current_in: AdcPin<A, ADCI, AdcCalLine<ADCI>>,
    pub enable: EN,
    pub adc: ADC<'static, ADCI>,
}

fn raw_to_mv(raw: u16) -> u16 {
    (raw as u32 * Attenuation::Attenuation11dB.ref_mv() as u32 / 4096) as u16
}

impl<V, A, EN, ADCI> BatteryAdc<V, A, EN, ADCI>
where
    ADCI: CalibrationAccess + AdcCalEfuse + AdcHasLineCal + 'static + Peripheral<P = ADCI>,
    ADC<'static, ADCI>: OneShot<ADCI, u16, AdcPin<V, ADCI>>,
    ADC<'static, ADCI>: OneShot<ADCI, u16, AdcPin<A, ADCI>>,
    V: Channel<ADCI, ID = u8>,
    A: Channel<ADCI, ID = u8>,
{
    pub fn new(
        adc: ADCI,
        voltage_in: impl Into<V>,
        current_in: impl Into<A>,
        enable: impl Into<EN>,
    ) -> Self {
        let mut adc_config = AdcConfig::new();

        Self {
            voltage_in: adc_config
                .enable_pin_with_cal(voltage_in.into(), Attenuation::Attenuation11dB),
            current_in: adc_config
                .enable_pin_with_cal(current_in.into(), Attenuation::Attenuation11dB),
            enable: enable.into(),
            adc: unwrap!(ADC::adc(adc, adc_config)),
        }
    }

    pub async fn read_battery_voltage(&mut self) -> Result<u16, ()> {
        loop {
            match self.adc.read(&mut self.voltage_in) {
                Ok(out) => {
                    return Ok(2 * raw_to_mv(out)); // 2x voltage divider
                }
                Err(nb::Error::Other(_e)) => return Err(()),
                Err(nb::Error::WouldBlock) => yield_now().await,
            }
        }
    }

    pub async fn read_charge_current(&mut self) -> Result<u16, ()> {
        loop {
            match self.adc.read(&mut self.current_in) {
                Ok(out) => return Ok(raw_to_mv(out)),
                Err(nb::Error::Other(_e)) => return Err(()),
                Err(nb::Error::WouldBlock) => yield_now().await,
            }
        }
    }

    pub async fn read_data(&mut self) -> Result<BatteryAdcData, ()> {
        Ok(BatteryAdcData {
            voltage: self.read_battery_voltage().await?,
            charge_current: self.read_charge_current().await?,
        })
    }
}

#[embassy_executor::task]
pub async fn monitor_task_adc(
    battery: Shared<crate::board::BatteryAdc>,
    battery_state: SharedBatteryState,
    mut task_control: TaskControlToken<()>,
) {
    task_control
        .run_cancellable(async {
            let mut timer = Ticker::every(Duration::from_millis(10));
            info!("ADC monitor started");

            unwrap!(battery.lock().await.enable.set_high().ok());

            let mut voltage_accumulator = 0;
            let mut current_accumulator = 0;

            let mut sample_count = 0;

            const AVG_SAMPLE_COUNT: u32 = 128;

            loop {
                let data = unwrap!(battery.lock().await.read_data().await);

                voltage_accumulator += data.voltage as u32;
                current_accumulator += data.charge_current as u32;

                if sample_count == AVG_SAMPLE_COUNT {
                    let mut state = battery_state.lock().await;

                    let average = BatteryAdcData {
                        voltage: (voltage_accumulator / AVG_SAMPLE_COUNT) as u16,
                        charge_current: (current_accumulator / AVG_SAMPLE_COUNT) as u16,
                    };
                    state.data = Some(average);

                    debug!("Battery data: {:?}", average);

                    sample_count = 0;

                    voltage_accumulator = 0;
                    current_accumulator = 0;
                } else {
                    sample_count += 1;
                }

                timer.next().await;
            }
        })
        .await;

    unwrap!(battery.lock().await.enable.set_low().ok());

    info!("Monitor exited");
}
