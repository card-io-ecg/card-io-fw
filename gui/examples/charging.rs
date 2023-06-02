use std::convert::Infallible;

use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Size},
    Drawable,
};
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};
use gui::screens::{charging::ChargingScreen, BatteryInfo};
use signal_processing::battery::BatteryModel;

fn main() -> Result<(), Infallible> {
    let mut display = SimulatorDisplay::<BinaryColor>::new(Size::new(128, 64));

    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::OledBlue)
        .max_fps(100)
        .build();

    let mut window = Window::new("Charging screen", &output_settings);

    let model = BatteryModel {
        voltage: (3300, 4200),
        charge_current: (0, 1000),
    };
    let mut frames = 0;
    'running: loop {
        display.clear(BinaryColor::Off).unwrap();

        ChargingScreen {
            battery_data: Some(BatteryInfo {
                voltage: 4200,
                charge_current: Some(100),
            }),
            model,
            is_charging: true,
            frames,
            fps: 100,
            progress: 1,
        }
        .draw(&mut display)
        .unwrap();

        frames = frames.wrapping_add(1);

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
