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
        display_menu::{
            BatteryDisplayStyle, DisplayBrightness, DisplayMenu, DisplayMenuEvents,
            DisplayMenuScreen,
        },
        BatteryInfo, MENU_STYLE,
    },
    widgets::battery_small::BatteryStyle,
};
use signal_processing::battery::BatteryModel;

fn main() -> Result<(), Infallible> {
    let mut display = SimulatorDisplay::<BinaryColor>::new(Size::new(128, 64));

    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::OledBlue)
        .max_fps(100)
        .build();

    let mut window = Window::new("Display menu screen", &output_settings);

    let model = BatteryModel {
        voltage: (3300, 4200),
        charge_current: (0, 1000),
    };

    let mut menu_screen = DisplayMenuScreen {
        menu: DisplayMenu {
            brightness: DisplayBrightness::Normal,
            battery_display: BatteryDisplayStyle::MilliVolts,
        }
        .create_menu_with_style(MENU_STYLE),

        battery_data: Some(BatteryInfo {
            voltage: 3650,
            charge_current: None,
        }),
        battery_style: BatteryStyle::Percentage(model),
    };
    let mut pressed = false;

    'running: loop {
        display.clear(BinaryColor::Off).unwrap();

        menu_screen.battery_style =
            BatteryStyle::new(menu_screen.menu.data().battery_display, model);

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
