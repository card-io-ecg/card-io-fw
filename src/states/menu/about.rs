use crate::{
    board::{
        hal::efuse::Efuse,
        initialized::{Board, StaMode},
        wifi::sta::Sta,
    },
    states::{menu::AppMenu, TouchInputShaper, MENU_IDLE_DURATION, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState,
};
use alloc::format;
#[cfg(feature = "battery_max17055")]
use alloc::string::String;
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
    #[cfg(feature = "battery_max17055")]
    ToBatteryInfo,
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

    let list_item = |label| NavigationItem::new(label, AboutMenuEvents::None);

    let mut items = heapless::Vec::<_, 5>::new();

    items.extend([
        list_item(format!("FW: {:>16}", env!("FW_VERSION"))),
        list_item(format!(
            "HW: {:>16}",
            format!("ESP32-S3/{}", env!("HW_VERSION"))
        )),
        list_item(format!(
            "Serial: {:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            mac_address[0],
            mac_address[1],
            mac_address[2],
            mac_address[3],
            mac_address[4],
            mac_address[5]
        )),
        list_item(match board.frontend.device_id() {
            Some(id) => format!("ADC: {:>15}", format!("{id:?}")),
            None => format!("ADC:         Unknown"),
        }),
    ]);

    #[cfg(feature = "battery_max17055")]
    {
        unwrap!(items
            .push(NavigationItem::new(
                String::from("Fuel gauge: MAX17055"),
                AboutMenuEvents::ToBatteryInfo,
            ))
            .ok());
    }

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
    let mut input = TouchInputShaper::new(&mut board.frontend);

    while !exit_timer.is_elapsed() {
        let is_touched = input.is_touched();
        if is_touched {
            exit_timer.reset();
        }

        if let Some(event) = menu_screen.content.interact(is_touched) {
            match event {
                AboutMenuEvents::None => {}
                #[cfg(feature = "battery_max17055")]
                AboutMenuEvents::ToBatteryInfo => return AppState::Menu(AppMenu::BatteryInfo),
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

        unwrap!(board
            .display
            .frame(|display| {
                menu_screen.content.update(display);
                menu_screen.draw(display)
            })
            .await
            .ok());

        ticker.next().await;
    }

    AppState::Menu(AppMenu::Main)
}
