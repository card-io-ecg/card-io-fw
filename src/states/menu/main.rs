use crate::{
    board::initialized::Context,
    heap::ALLOCATOR,
    states::menu::{AppMenu, MenuScreen},
    AppState,
};
use embedded_menu::items::NavigationItem;
use gui::screens::create_menu;

use super::AppMenuBuilder;

#[derive(Clone, Copy)]
pub enum MainMenuEvents {
    Measure,
    Display,
    About,
    WifiSetup,
    WifiListVisible,
    FirmwareUpdate,
    Throughput,
    Storage,
    Shutdown,
}

pub async fn main_menu(context: &mut Context) -> AppState {
    info!("Free heap: {} bytes", ALLOCATOR.free());

    MainMenu
        .display(context)
        .await
        .unwrap_or(AppState::Shutdown)
}

struct MainMenu;
type MainMenuBuilder = impl AppMenuBuilder<MainMenuEvents>;

fn main_menu_builder(context: &mut Context) -> MainMenuBuilder {
    let mut optional_items = heapless::Vec::<_, 4>::new();

    let mut optional_item =
        |label, event| unwrap!(optional_items.push(NavigationItem::new(label, event)).ok());

    if context.can_enable_wifi() {
        optional_item("Wifi setup", MainMenuEvents::WifiSetup);
        optional_item("Wifi networks", MainMenuEvents::WifiListVisible);

        let network_configured =
            !context.config.backend_url.is_empty() && !context.config.known_networks.is_empty();

        if network_configured {
            optional_item("Firmware update", MainMenuEvents::FirmwareUpdate);
            optional_item("Speed test", MainMenuEvents::Throughput);
        }
    }

    create_menu("Main menu")
        .add_item(NavigationItem::new("Measure", MainMenuEvents::Measure))
        .add_item(NavigationItem::new("Display", MainMenuEvents::Display))
        .add_item(NavigationItem::new("Storage", MainMenuEvents::Storage))
        .add_item(NavigationItem::new("Device info", MainMenuEvents::About))
        .add_items(optional_items)
        .add_item(NavigationItem::new("Shutdown", MainMenuEvents::Shutdown))
}

impl MenuScreen for MainMenu {
    type Event = MainMenuEvents;
    type Result = AppState;
    type MenuBuilder = MainMenuBuilder;

    async fn menu(&mut self, context: &mut Context) -> Self::MenuBuilder {
        main_menu_builder(context)
    }

    async fn handle_event(
        &mut self,
        event: Self::Event,
        _board: &mut Context,
    ) -> Option<Self::Result> {
        let event = match event {
            MainMenuEvents::Measure => AppState::Initialize,
            MainMenuEvents::Display => AppState::Menu(AppMenu::Display),
            MainMenuEvents::About => AppState::Menu(AppMenu::DeviceInfo),
            MainMenuEvents::WifiSetup => AppState::Menu(AppMenu::WifiAP),
            MainMenuEvents::WifiListVisible => AppState::Menu(AppMenu::WifiListVisible),
            MainMenuEvents::Storage => AppState::Menu(AppMenu::Storage),
            MainMenuEvents::FirmwareUpdate => AppState::FirmwareUpdate,
            MainMenuEvents::Throughput => AppState::Throughput,
            MainMenuEvents::Shutdown => AppState::Shutdown,
        };

        Some(event)
    }
}
