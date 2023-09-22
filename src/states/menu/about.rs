use crate::{
    board::initialized::Board,
    states::{menu::AppMenu, TouchInputShaper, MENU_IDLE_DURATION, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState, SerialNumber,
};
use alloc::{format, string::String};
use embassy_time::Ticker;
use embedded_graphics::Drawable;
use embedded_menu::{items::NavigationItem, Menu};
use gui::screens::{menu_style, screen::Screen};
use ufmt::uwrite;

#[derive(Clone, Copy)]
pub enum AboutMenuEvents {
    None,
    #[cfg(feature = "battery_max17055")]
    ToBatteryInfo,
    ToSerial,
    Back,
}

pub async fn about_menu(board: &mut Board) -> AppState {
    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);

    let list_item = |label| NavigationItem::new(label, AboutMenuEvents::None);

    let mut serial = heapless::String::<12>::new();
    unwrap!(uwrite!(&mut serial, "{}", SerialNumber::new()));

    let mut hw_version = heapless::String::<16>::new();
    unwrap!(uwrite!(&mut hw_version, "ESP32-S3/{}", env!("HW_VERSION")));

    let mut items = heapless::Vec::<_, 5>::new();
    items.extend([
        list_item(format!("FW {:>17}", env!("FW_VERSION"))),
        list_item(format!("HW {:>17}", hw_version)),
        NavigationItem::new(format!("Serial  {}", serial), AboutMenuEvents::ToSerial),
        list_item(match board.frontend.device_id() {
            Some(id) => format!("ADC {:>16}", format!("{id:?}")),
            None => format!("ADC          Unknown"),
        }),
    ]);

    #[cfg(feature = "battery_max17055")]
    {
        unwrap!(items
            .push(
                NavigationItem::new(String::from("Fuel gauge"), AboutMenuEvents::ToBatteryInfo)
                    .with_marker("MAX17055")
            )
            .ok());
    }

    let mut menu_screen = Screen {
        content: Menu::with_style("Device info", menu_style())
            .add_items(&mut items[..])
            .add_item(NavigationItem::new("Back", AboutMenuEvents::Back))
            .build(),
        status_bar: board.status_bar(),
    };

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let mut input = TouchInputShaper::new();

    while !exit_timer.is_elapsed() {
        input.update(&mut board.frontend);
        let is_touched = input.is_touched();
        if is_touched {
            exit_timer.reset();
        }

        if let Some(event) = menu_screen.content.interact(is_touched) {
            match event {
                AboutMenuEvents::None => {}
                #[cfg(feature = "battery_max17055")]
                AboutMenuEvents::ToBatteryInfo => return AppState::Menu(AppMenu::BatteryInfo),
                AboutMenuEvents::ToSerial => return AppState::DisplaySerial,
                AboutMenuEvents::Back => return AppState::Menu(AppMenu::Main),
            };
        }

        if board.battery_monitor.is_low() {
            return AppState::Shutdown;
        }

        menu_screen.status_bar = board.status_bar();

        board
            .display
            .frame(|display| {
                menu_screen.content.update(display);
                menu_screen.draw(display)
            })
            .await;

        ticker.next().await;
    }

    AppState::Menu(AppMenu::Main)
}
