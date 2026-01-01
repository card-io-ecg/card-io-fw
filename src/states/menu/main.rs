use crate::{
    board::initialized::Context,
    states::menu::{AppMenu, MenuBuilder, MenuItems, MenuScreen},
    AppState,
};
use embedded_menu::items::menu_item::{MenuItem, SelectValue};
use gui::{
    embedded_layout::{
        chain,
        object_chain::{Chain, Link},
    },
    screens::create_menu,
};

#[derive(Clone, Copy, PartialEq)]
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

impl SelectValue for MainMenuEvents {
    fn marker(&self) -> &'static str {
        ""
    }
}

pub async fn main_menu(context: &mut Context) -> AppState {
    info!("Free heap: {} bytes", esp_alloc::HEAP.free());

    MainMenu
        .display(context)
        .await
        .unwrap_or(AppState::Shutdown)
}

type MainMenuItem<T> = MenuItem<&'static str, MainMenuEvents, T, true>;

struct MainMenu;
type MainMenuBuilder = MenuBuilder<
    chain!(
        MainMenuItem<MainMenuEvents>,
        MainMenuItem<MainMenuEvents>,
        MainMenuItem<MainMenuEvents>,
        MainMenuItem<MainMenuEvents>,
        MenuItems<MainMenuItem<MainMenuEvents>, MainMenuEvents, 4>,
        MainMenuItem<MainMenuEvents>
    ),
    MainMenuEvents,
>;

fn main_menu_builder(context: &mut Context) -> MainMenuBuilder {
    let mut optional_items = heapless::Vec::<_, 4>::new();

    if context.can_enable_wifi() {
        let mut optional_item = |label, event| {
            unwrap!(optional_items
                .push(MenuItem::new(label, event).with_value_converter(|evt| evt))
                .ok())
        };
        let network_configured =
            !context.config.backend_url.is_empty() && !context.config.known_networks.is_empty();

        optional_item("Wifi setup", MainMenuEvents::WifiSetup);
        optional_item("Wifi networks", MainMenuEvents::WifiListVisible);

        if network_configured {
            optional_item("Firmware update", MainMenuEvents::FirmwareUpdate);
            optional_item("Speed test", MainMenuEvents::Throughput);
        }
    }

    create_menu("Main menu")
        .add_item("Measure", MainMenuEvents::Measure, |evt| evt)
        .add_item("Display", MainMenuEvents::Display, |evt| evt)
        .add_item("Storage", MainMenuEvents::Storage, |evt| evt)
        .add_item("Device info", MainMenuEvents::About, |evt| evt)
        .add_menu_items(optional_items)
        .add_item("Shutdown", MainMenuEvents::Shutdown, |evt| evt)
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
