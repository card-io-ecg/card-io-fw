use std::convert::Infallible;

use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Drawable, Size},
};
use embedded_graphics_simulator::{
    sdl2::Keycode, BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent,
    Window,
};
use gui::screens::{
    display_menu::{BatteryDisplayStyle, DisplayBrightness, DisplayMenu, DisplayMenuEvents},
    MENU_STYLE,
};

fn main() -> Result<(), Infallible> {
    let mut display = SimulatorDisplay::<BinaryColor>::new(Size::new(128, 64));

    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::OledBlue)
        .max_fps(100)
        .build();

    let mut window = Window::new("Display menu screen", &output_settings);

    let mut menu = DisplayMenu {
        brightness: DisplayBrightness::Normal,
        battery_display: BatteryDisplayStyle::MilliVolts,
    }
    .create_menu_with_style(MENU_STYLE);
    let mut pressed = false;

    'running: loop {
        display.clear(BinaryColor::Off).unwrap();

        menu.update(&display);
        menu.draw(&mut display).unwrap();

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

        if let Some(event) = menu.interact(pressed) {
            match event {
                DisplayMenuEvents::Back => break 'running,
            }
        }
    }

    Ok(())
}
