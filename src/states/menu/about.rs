use crate::{
    board::{hal::efuse::Efuse, initialized::Board, wifi::sta::Sta},
    states::{menu::AppMenu, MENU_IDLE_DURATION, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState,
};
use alloc::format;
use embassy_net::Config;
use embassy_time::Ticker;
use embedded_graphics::Drawable;
use gui::{
    screens::about_menu::{AboutMenuData, AboutMenuEvents, AboutMenuScreen},
    widgets::{battery_small::Battery, status_bar::StatusBar, wifi::WifiStateView},
};

pub async fn about_menu(board: &mut Board) -> AppState {
    let sta = if !board.config.known_networks.is_empty() {
        // Enable wifi STA. This enabled wifi for the whole menu and re-enables when the user exits the
        // wifi AP config menu.
        board.wifi.initialize(&board.clocks);

        let sta = board
            .wifi
            .configure_sta(Config::dhcpv4(Default::default()))
            .await;

        sta.update_known_networks(&board.config.known_networks)
            .await;

        Some(sta)
    } else {
        board.wifi.stop_if().await;

        None
    };
    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);

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
        status_bar: StatusBar {
            battery: Battery::with_style(
                board.battery_monitor.battery_data(),
                board.config.battery_style(),
            ),
            wifi: WifiStateView::new(sta.as_ref().map(Sta::connection_state)),
        },
    };

    let mut ticker = Ticker::every(MIN_FRAME_TIME);

    while !exit_timer.is_elapsed() {
        let is_touched = board.frontend.is_touched();
        if is_touched {
            exit_timer.reset();
        }

        if let Some(event) = menu_screen.menu.interact(is_touched) {
            match event {
                AboutMenuEvents::None => {}
                AboutMenuEvents::Back => return AppState::Menu(AppMenu::Main),
            };
        }

        let battery_data = board.battery_monitor.battery_data();

        if let Some(battery) = battery_data {
            if battery.is_low {
                return AppState::Shutdown;
            }
        }

        menu_screen.status_bar.update_battery_data(battery_data);
        if let Some(ref sta) = sta {
            menu_screen.status_bar.wifi.update(sta.connection_state());
        };

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
