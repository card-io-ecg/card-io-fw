use crate::{
    board::initialized::Board,
    states::{display_menu_screen, menu::AppMenu, MenuEventHandler, MENU_IDLE_DURATION},
    AppState, SerialNumber,
};
use alloc::{format, string::String};
use embedded_menu::items::NavigationItem;
use gui::screens::create_menu;
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

    let menu = create_menu("Device info")
        .add_items(&mut items[..])
        .add_item(NavigationItem::new("Back", AboutMenuEvents::Back))
        .build();

    display_menu_screen(menu, board, MENU_IDLE_DURATION, AboutMenuHandler)
        .await
        .unwrap_or(AppState::Menu(AppMenu::Main))
}

struct AboutMenuHandler;

impl MenuEventHandler for AboutMenuHandler {
    type Input = AboutMenuEvents;
    type Result = AppState;

    async fn handle_event(
        &mut self,
        event: Self::Input,
        _board: &mut Board,
    ) -> Option<Self::Result> {
        match event {
            AboutMenuEvents::None => None,
            #[cfg(feature = "battery_max17055")]
            AboutMenuEvents::ToBatteryInfo => Some(AppState::Menu(AppMenu::BatteryInfo)),
            AboutMenuEvents::ToSerial => Some(AppState::DisplaySerial),
            AboutMenuEvents::Back => Some(AppState::Menu(AppMenu::Main)),
        }
    }
}
