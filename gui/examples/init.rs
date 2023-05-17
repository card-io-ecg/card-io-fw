use std::convert::Infallible;

use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Size},
    Drawable,
};
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};
use gui::{
    screens::{init::StartupScreen, BatteryInfo},
    widgets::battery_small::BatteryStyle,
};
use signal_processing::battery::BatteryModel;

fn main() -> Result<(), Infallible> {
    let mut display = SimulatorDisplay::<BinaryColor>::new(Size::new(128, 64));

    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::OledBlue)
        .max_fps(100)
        .build();

    let mut window = Window::new("Init screen", &output_settings);

    let mut progress = 0;
    'running: loop {
        display.clear(BinaryColor::Off).unwrap();

        StartupScreen {
            label: "Release to shutdown",
            progress: if progress > 255 {
                510 - progress
            } else {
                progress
            },
            max_progress: 255,
            battery_data: Some(BatteryInfo {
                voltage: 4200,
                charge_current: Some(100),
            }),
            battery_style: BatteryStyle::Percentage(BatteryModel {
                voltage: (3300, 4200),
                charge_current: (0, 1000),
            }),
        }
        .draw(&mut display)
        .unwrap();

        progress = (progress + 1) % 510;

        window.update(&display);

        for event in window.events() {
            match event {
                SimulatorEvent::Quit => break 'running,
                _ => {}
            }
        }
    }

    Ok(())
}
