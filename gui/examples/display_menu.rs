use std::convert::Infallible;

use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Drawable, Size},
};
use embedded_graphics_simulator::{
    sdl2::Keycode, BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent,
    Window,
};
use gui::{
    screens::{
        display_menu::{DisplayBrightness, DisplayMenu, DisplayMenuEvents, DisplayMenuScreen},
        menu_style, BatteryInfo,
    },
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

    let mut window = Window::new("Display menu screen", &output_settings);

    let mut menu_screen = DisplayMenuScreen {
        menu: DisplayMenu {
            brightness: DisplayBrightness::Normal,
            battery_display: BatteryStyle::MilliVolts,
        }
        .create_menu_with_style(menu_style()),

        status_bar: StatusBar {
            battery: Battery::percentage(Some(BatteryInfo {
                voltage: 3650,
                percentage: 50,
                is_charging: true,
                is_low: false,
            })),
            wifi: WifiStateView::enabled(WifiState::Connecting),
        },
    };
    let mut pressed = false;

    'running: loop {
        display.clear(BinaryColor::Off).unwrap();

        menu_screen
            .status_bar
            .update_battery_style(menu_screen.menu.data().battery_display);

        menu_screen.menu.update(&display);
        menu_screen.draw(&mut display).unwrap();

        window.update(&display);

        for event in window.events() {
            match event {
                SimulatorEvent::Quit => break 'running,
                SimulatorEvent::KeyDown {
                    keycode: Keycode::Space,
                    ..
                } => pressed = true,
                SimulatorEvent::KeyUp {
                    keycode: Keycode::Space,
                    ..
                } => pressed = false,
                _ => {}
            }
        }

        if let Some(event) = menu_screen.menu.interact(pressed) {
            match event {
                DisplayMenuEvents::Back => break 'running,
            }
        }
    }

    Ok(())
}
