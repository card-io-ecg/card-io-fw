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
    Frontend,
    About,
    #[cfg(feature = "wifi")]
    Wifi,
    #[cfg(feature = "wifi")]
    FirmwareUpdate,
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
        MenuItems<MainMenuItem<MainMenuEvents>, MainMenuEvents, 1>,
        MainMenuItem<MainMenuEvents>,
        MainMenuItem<MainMenuEvents>,
        MainMenuItem<MainMenuEvents>,
        MenuItems<MainMenuItem<MainMenuEvents>, MainMenuEvents, 1>,
        MainMenuItem<MainMenuEvents>,
        MainMenuItem<MainMenuEvents>
    ),
    MainMenuEvents,
>;

#[cfg(feature = "wifi")]
fn optional_item(label: &'static str, event: MainMenuEvents) -> MainMenuItem<MainMenuEvents> {
    MenuItem::new(label, event).with_value_converter(|evt| evt)
}

fn main_menu_builder(_ctx: &mut Context) -> MainMenuBuilder {
    #[allow(unused_mut)]
    let mut network_items = heapless::Vec::<_, 1>::new();
    #[allow(unused_mut)]
    let mut wifi_items = heapless::Vec::<_, 1>::new();

    #[cfg(feature = "wifi")]
    {
        if _ctx.can_enable_wifi() && _ctx.backend_ready().is_ok() {
            unwrap!(network_items
                .push(optional_item(
                    "Firmware update",
                    MainMenuEvents::FirmwareUpdate
                ))
                .ok());
        }
        unwrap!(wifi_items
            .push(optional_item("Wi-Fi", MainMenuEvents::Wifi))
            .ok());
    }

    create_menu("Main menu")
        .add_item("Measure", MainMenuEvents::Measure, |evt| evt)
        .add_menu_items(network_items)
        .add_item("Display", MainMenuEvents::Display, |evt| evt)
        .add_item("EKG", MainMenuEvents::Frontend, |evt| evt)
        .add_item("Storage", MainMenuEvents::Storage, |evt| evt)
        .add_menu_items(wifi_items)
        .add_item("Device info", MainMenuEvents::About, |evt| evt)
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
            MainMenuEvents::Frontend => AppState::Menu(AppMenu::Frontend),
            MainMenuEvents::About => AppState::Menu(AppMenu::DeviceInfo),
            #[cfg(feature = "wifi")]
            MainMenuEvents::Wifi => AppState::Menu(AppMenu::Wifi),
            MainMenuEvents::Storage => AppState::Menu(AppMenu::Storage),
            #[cfg(feature = "wifi")]
            MainMenuEvents::FirmwareUpdate => AppState::FirmwareUpdate,
            MainMenuEvents::Shutdown => AppState::Shutdown,
        };

        Some(event)
    }
}
