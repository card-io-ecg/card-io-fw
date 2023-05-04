use std::convert::Infallible;

use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Size},
};
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};

fn main() -> Result<(), Infallible> {
    let mut display = SimulatorDisplay::<BinaryColor>::new(Size::new(128, 64));

    // Uncomment one of the `theme` lines to use a different theme.
    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::OledBlue)
        .build();

    let mut window = Window::new("Init screen", &output_settings);

    let mut progress = 0;
    'running: loop {
        display.clear(BinaryColor::Off).unwrap();

        gui::draw_startup_progress_bar(
            "Release to shutdown",
            &mut display,
            if progress > 255 {
                510 - progress
            } else {
                progress
            },
            255,
        )
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
