use std::convert::Infallible;

use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Point, Size},
    Drawable,
};
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};
use gui::{
    screens::{init::StartupScreen, BatteryInfo},
    widgets::{
        battery_small::{Battery, BatteryStyle},
        slot::Slot,
        status_bar::StatusBar,
    },
};

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
            status_bar: StatusBar {
                battery: Slot::visible(Battery::icon(BatteryInfo {
                    voltage: 4200,
                    percentage: 100,
                    is_charging: true,
                    is_low: false,
                })),
            },
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
