use core::num::NonZeroU32;

use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point},
    Drawable,
};
use embedded_layout::prelude::{horizontal, vertical, Align};

use crate::{
    screens::BatteryInfo,
    widgets::{battery::Battery, progress_bar::ProgressBar},
};

pub struct ChargingScreen {
    pub battery_data: Option<BatteryInfo>,
    pub is_charging: bool,
    pub frames: u32,
    pub fps: u32,
    pub progress: u32,
}

impl ChargingScreen {
    fn max_progress(&self) -> NonZeroU32 {
        NonZeroU32::new(self.fps * 2).unwrap_or(NonZeroU32::MIN)
    }

    pub fn update_touched(&mut self, touched: bool) -> bool {
        if touched {
            self.progress += 1;
            self.progress >= self.max_progress().get()
        } else {
            self.progress = 0;
            false
        }
    }
}

impl Drawable for ChargingScreen {
    type Color = BinaryColor;
    type Output = ();

    #[inline]
    fn draw<DT: DrawTarget<Color = BinaryColor>>(&self, display: &mut DT) -> Result<(), DT::Error> {
        if let Some(data) = self.battery_data {
            let n_bars = (data.percentage.saturating_sub(1)) / 20; // 0-4 solid bars

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

        if self.progress > 0 {
            ProgressBar {
                label: "Enter menu",
                progress: self.progress,
                max_progress: self.max_progress(),
            }
            .draw(display)?;
        }

        Ok(())
    }
}
