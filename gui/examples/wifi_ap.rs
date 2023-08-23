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
    screens::{
        menu_style,
        wifi_ap::{ApMenu, WifiApScreen, WifiApScreenState},
        BatteryInfo,
    },
    widgets::{
        battery_small::Battery,
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

    let mut window = Window::new("Wifi AP screen", &output_settings);

    'running: loop {
        display.clear(BinaryColor::Off).unwrap();

        WifiApScreen {
            menu: ApMenu {}.create_menu_with_style(menu_style()),
            state: WifiApScreenState::Idle,
            status_bar: StatusBar {
                battery: Battery::percentage(Some(BatteryInfo {
                    voltage: 4200,
                    percentage: 100,
                    is_charging: true,
                    is_low: false,
                })),
                wifi: WifiStateView::disabled(WifiState::Connected),
            },
        }
        .draw(&mut display)
        .unwrap();

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
