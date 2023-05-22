use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point},
    Drawable,
};
use embedded_layout::prelude::{horizontal, vertical, Align};

use signal_processing::battery::BatteryModel;

use crate::{screens::BatteryInfo, widgets::battery::Battery};

pub struct ChargingScreen {
    pub battery_data: Option<BatteryInfo>,
    pub model: BatteryModel,
    pub is_charging: bool,
    pub frames: u32,
    pub fps: u32,
}

impl Drawable for ChargingScreen {
    type Color = BinaryColor;
    type Output = ();

    fn draw<DT: DrawTarget<Color = BinaryColor>>(&self, display: &mut DT) -> Result<(), DT::Error> {
        if let Some(data) = self.battery_data {
            let percentage = self.model.estimate(data.voltage, data.charge_current);
            let n_bars = (percentage.saturating_sub(1)) / 20; // 0-4 solid bars

            let period = self.fps * 3 / 2;
            let blinking_on = self.is_charging && self.frames % period < period / 2;

            Battery::new(
                if self.is_charging {
                    n_bars.min(3) + blinking_on as u8
                } else {
                    n_bars
                } as u32,
                Point::zero(),
            )
            .align_to(
                &display.bounding_box(),
                horizontal::Center,
                vertical::Center,
            )
            .draw(display)?;
        }
        Ok(())
    }
}
