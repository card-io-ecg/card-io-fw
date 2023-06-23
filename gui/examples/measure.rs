use std::{convert::Infallible, f32::consts::PI};

use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Size},
    Drawable,
};
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};
use gui::{
    screens::{measure::EcgScreen, BatteryInfo},
    widgets::battery_small::BatteryStyle,
};

fn main() -> Result<(), Infallible> {
    let mut display = SimulatorDisplay::<BinaryColor>::new(Size::new(128, 64));

    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::OledBlue)
        .max_fps(100)
        .build();

    let mut window = Window::new("Measurement screen", &output_settings);

    let mut screen = EcgScreen::new(96);

    screen.update_heart_rate(67);
    screen.battery_voltage = Some(3650);
    screen.battery_data = Some(BatteryInfo {
        voltage: 4200,
        percentage: 100,
        is_charging: true,
        is_low: false,
    });
    screen.battery_style = BatteryStyle::Percentage;

    let mut progress = 0;
    'running: loop {
        display.clear(BinaryColor::Off).unwrap();

        const PERIOD: u32 = 500;
        let t = progress as f32 / PERIOD as f32;
        progress = (progress + 1) % PERIOD;

        let f = 10.0;
        let f2 = 11.0;

        let wt = 2.0 * PI * f * t;
        let wt2 = 2.0 * PI * f2 * t;

        let sample1 = wt.sin();
        let sample2 = wt2.sin();

        screen.push(sample1 * sample2);

        screen.draw(&mut display).unwrap();

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
