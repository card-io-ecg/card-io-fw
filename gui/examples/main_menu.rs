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
        main_menu::{MainMenu, MainMenuEvents, MainMenuScreen},
        BatteryInfo, MENU_STYLE,
    },
    widgets::battery_small::BatteryStyle,
};

fn main() -> Result<(), Infallible> {
    let mut display = SimulatorDisplay::<BinaryColor>::new(Size::new(128, 64));

    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::OledBlue)
        .max_fps(100)
        .build();

    let mut window = Window::new("Main menu screen", &output_settings);

    let mut menu_screen = MainMenuScreen {
        menu: MainMenu {}.create_menu_with_style(MENU_STYLE),

        status_bar: StatusBar {
            battery: Slot::visible(Battery::percentage(BatteryInfo {
                voltage: 4200,
                percentage: 100,
                is_charging: false,
                is_low: false,
            })),
        },
    };
    let mut pressed = false;

    'running: loop {
        display.clear(BinaryColor::Off).unwrap();

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
            if let MainMenuEvents::Shutdown = event {
                break 'running;
            }
        }
    }

    Ok(())
}
