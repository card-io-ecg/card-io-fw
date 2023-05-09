use std::convert::Infallible;

use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{Dimensions, DrawTarget, Point, Size},
    Drawable,
};
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};
use embedded_layout::prelude::{horizontal, vertical, Align};
use gui::widgets::battery::Battery;

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

        Battery::new((progress / 50) % 5, Point::zero())
            .align_to(
                &display.bounding_box(),
                horizontal::Center,
                vertical::Center,
            )
            .draw(&mut display)
            .unwrap();

        progress = (progress + 1) % 250;

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
