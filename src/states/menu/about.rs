use alloc::format;
use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::Drawable;
use gui::{
    screens::about_menu::{AboutMenuData, AboutMenuEvents, AboutMenuScreen},
    widgets::{battery_small::Battery, slot::Slot, status_bar::StatusBar},
};

use crate::{
    board::{hal::efuse::Efuse, initialized::Board},
    states::MIN_FRAME_TIME,
    AppState,
};

use super::AppMenu;

pub async fn about_menu(board: &mut Board) -> AppState {
    const MENU_IDLE_DURATION: Duration = Duration::from_secs(30);

    let mac_address = Efuse::get_mac_address();

    let menu_data = AboutMenuData {
        hw_version: format!("FW: {:>16}", env!("FW_VERSION")),
        fw_version: format!("HW: {:>16}", format!("ESP32-S3/{}", env!("HW_VERSION"))),
        serial: format!(
            "Serial: {:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            mac_address[0],
            mac_address[1],
            mac_address[2],
            mac_address[3],
            mac_address[4],
            mac_address[5]
        ),
        adc: match board.frontend.device_id() {
            Some(id) => format!("ADC: {:>15}", format!("{id:?}")),
            None => format!("ADC:         Unknown"),
        },
    };

    let battery_style = board.config.battery_style();

    let mut menu_screen = AboutMenuScreen {
        menu: menu_data.create(),
        status_bar: StatusBar {
            battery: board
                .battery_monitor
                .battery_data()
                .await
                .map(|data| Slot::visible(Battery::with_style(data, battery_style)))
                .unwrap_or_default(),
        },
    };

    let mut last_interaction = Instant::now();
    let mut ticker = Ticker::every(MIN_FRAME_TIME);

    while last_interaction.elapsed() < MENU_IDLE_DURATION {
        let is_touched = board.frontend.is_touched();
        if is_touched {
            last_interaction = Instant::now();
        }
        if let Some(event) = menu_screen.menu.interact(is_touched) {
            match event {
                AboutMenuEvents::None => {}
                AboutMenuEvents::Back => return AppState::Menu(AppMenu::Main),
            };
        }

        let battery_data = board.battery_monitor.battery_data().await;

        if let Some(battery) = battery_data {
            if battery.is_low {
                return AppState::Shutdown;
            }
        }

        menu_screen
            .status_bar
            .update_battery_data(battery_data, battery_style);

        board
            .display
            .frame(|display| {
                menu_screen.menu.update(display);
                menu_screen.draw(display)
            })
            .await
            .unwrap();

        ticker.next().await;
    }

    AppState::Menu(AppMenu::Main)
}
