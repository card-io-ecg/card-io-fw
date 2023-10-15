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
    screens::{init::StartupScreen, screen::Screen, BatteryInfo, ChargingState},
    widgets::{
        battery_small::{Battery, BatteryStyle},
        status_bar::StatusBar,
        wifi::{WifiState, WifiStateView},
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

        Screen {
            content: StartupScreen {
                label: "Release to shutdown",
                progress: if progress > 255 {
                    510 - progress
                } else {
                    progress
                },
            },
            status_bar: StatusBar {
                battery: Battery::with_style(
                    Some(BatteryInfo {
                        voltage: 4100,
                        percentage: 90,
                        charging_state: ChargingState::Charging,
                        is_low: false,
                    }),
                    BatteryStyle::Percentage,
                ),
                wifi: WifiStateView::enabled(WifiState::Connected),
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
