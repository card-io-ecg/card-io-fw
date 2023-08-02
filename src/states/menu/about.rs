use alloc::format;
use embassy_time::{Duration, Instant, Ticker};
use embedded_graphics::Drawable;
use gui::screens::about_menu::{AboutMenuData, AboutMenuEvents, AboutMenuScreen};

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

    let mut menu_screen = AboutMenuScreen {
        menu: menu_data.create(),
        battery_data: board.battery_monitor.battery_data().await,
        battery_style: board.config.battery_style(),
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

        menu_screen.battery_data = board.battery_monitor.battery_data().await;

        if let Some(battery) = menu_screen.battery_data {
            if battery.is_low {
                return AppState::Shutdown;
            }
        }

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
