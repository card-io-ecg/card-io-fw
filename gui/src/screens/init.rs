use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point},
    Drawable,
};
use embedded_layout::prelude::{horizontal, vertical, Align};

use crate::{
    screens::BatteryInfo,
    widgets::{
        battery_small::{Battery, BatteryStyle},
        progress_bar::ProgressBar,
    },
};

pub struct StartupScreen<'a> {
    pub label: &'a str,
    pub progress: u32,
    pub max_progress: u32,
    pub battery_data: Option<BatteryInfo>,
    pub battery_style: BatteryStyle,
}

impl Drawable for StartupScreen<'_> {
    type Color = BinaryColor;
    type Output = ();

    fn draw<DT: DrawTarget<Color = BinaryColor>>(&self, display: &mut DT) -> Result<(), DT::Error> {
        ProgressBar {
            label: self.label,
            progress: self.progress,
            max_progress: self.max_progress,
        }
        .draw(display)?;

        if let Some(data) = self.battery_data {
            Battery {
                data,
                style: self.battery_style,
                top_left: Point::zero(),
            }
            .align_to_mut(&display.bounding_box(), horizontal::Right, vertical::Top)
            .draw(display)?;
        }

        Ok(())
    }
}
