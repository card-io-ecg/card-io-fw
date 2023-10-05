use crate::{
    board::initialized::Board,
    states::menu::{AppMenu, AppMenuBuilder, MenuScreen},
    AppState, SerialNumber,
};

use alloc::{borrow::Cow, format};
use embedded_menu::items::NavigationItem;
use gui::screens::create_menu;

#[derive(Clone, Copy)]
pub enum AboutMenuEvents {
    None,
    #[cfg(feature = "battery_max17055")]
    ToBatteryInfo,
    ToSerial,
    Back,
}

pub async fn about_menu(board: &mut Board) -> AppState {
    AboutAppMenu
        .display(board)
        .await
        .unwrap_or(AppState::Shutdown)
}

struct AboutAppMenu;

impl MenuScreen for AboutAppMenu {
    type Event = AboutMenuEvents;
    type Result = AppState;

    async fn menu(&mut self, board: &mut Board) -> impl AppMenuBuilder<Self::Event> {
        let list_item = |label| NavigationItem::new(label, AboutMenuEvents::None);

        let mut items = heapless::Vec::<_, 5>::new();
        items.extend([
            list_item(Cow::from(env!("FW_VERSION_MENU_ITEM"))),
            list_item(Cow::from(env!("HW_VERSION_MENU_ITEM"))),
            NavigationItem::new(
                Cow::from(format!("Serial  {}", SerialNumber)),
                AboutMenuEvents::ToSerial,
            ),
            list_item(Cow::from(match board.frontend.device_id() {
                Some(id) => format!("ADC {:>16}", format!("{id:?}")),
                None => format!("ADC          Unknown"),
            })),
        ]);

        #[cfg(feature = "battery_max17055")]
        {
            unwrap!(items
                .push(
                    NavigationItem::new(Cow::from("Fuel gauge"), AboutMenuEvents::ToBatteryInfo)
                        .with_marker("MAX17055")
                )
                .ok());
        }

        create_menu("Device info")
            .add_items(items)
            .add_item(NavigationItem::new("Back", AboutMenuEvents::Back))
    }

    async fn handle_event(
        &mut self,
        event: Self::Event,
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
