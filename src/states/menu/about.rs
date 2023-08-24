use crate::{
    board::{
        hal::efuse::Efuse,
        initialized::{Board, StaMode},
        wifi::sta::Sta,
    },
    states::{menu::AppMenu, MENU_IDLE_DURATION, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState,
};
use alloc::format;
use embassy_time::Ticker;
use embedded_graphics::Drawable;
use embedded_menu::{items::NavigationItem, Menu};
use gui::{
    screens::{menu_style, screen::Screen},
    widgets::{battery_small::Battery, status_bar::StatusBar, wifi::WifiStateView},
};

#[derive(Clone, Copy)]
pub enum AboutMenuEvents {
    None,
    Back,
}

pub async fn about_menu(board: &mut Board) -> AppState {
    let sta = if !board.config.known_networks.is_empty() {
        board.enable_wifi_sta(StaMode::OnDemand).await
    } else {
        board.wifi.stop_if().await;
        None
    };
    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);

    let mac_address = Efuse::get_mac_address();

    let mut items = [
        NavigationItem::new(
            format!("FW: {:>16}", env!("FW_VERSION")),
            AboutMenuEvents::None,
        ),
        NavigationItem::new(
            format!("HW: {:>16}", format!("ESP32-S3/{}", env!("HW_VERSION"))),
            AboutMenuEvents::None,
        ),
        NavigationItem::new(
            format!(
                "Serial: {:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
                mac_address[0],
                mac_address[1],
                mac_address[2],
                mac_address[3],
                mac_address[4],
                mac_address[5]
            ),
            AboutMenuEvents::None,
        ),
        NavigationItem::new(
            match board.frontend.device_id() {
                Some(id) => format!("ADC: {:>15}", format!("{id:?}")),
                None => format!("ADC:         Unknown"),
            },
            AboutMenuEvents::None,
        ),
    ];

    let mut menu_screen = Screen {
        content: Menu::with_style("Device info", menu_style())
            .add_items(&mut items[..])
            .add_item(NavigationItem::new("Back", AboutMenuEvents::Back))
            .build(),
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

        if let Some(event) = menu_screen.content.interact(is_touched) {
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
                menu_screen.content.update(display);
                menu_screen.draw(display)
            })
            .await
            .unwrap();

        ticker.next().await;
    }

    AppState::Menu(AppMenu::Main)
}
