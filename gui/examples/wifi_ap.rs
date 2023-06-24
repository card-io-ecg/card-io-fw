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
        wifi_ap::{ApMenu, WifiApScreen},
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

    let mut window = Window::new("Wifi AP screen", &output_settings);

    'running: loop {
        display.clear(BinaryColor::Off).unwrap();

        WifiApScreen {
            battery_data: Some(BatteryInfo {
                voltage: 4200,
                percentage: 100,
                is_charging: true,
                is_low: false,
            }),
            battery_style: BatteryStyle::Icon,
            menu: ApMenu {}.create_menu_with_style(MENU_STYLE),
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
