use crate::{
    board::initialized::Context,
    states::menu::{AppMenu, MenuBuilder, MenuItems, MenuScreen},
    AppState,
};
use config_types::types::{MeasurementWifi, MenuWifi, MinBattery};
use embedded_menu::items::menu_item::{MenuItem, SelectValue};
use gui::{
    embedded_layout::{
        chain,
        object_chain::{Chain, Link},
    },
    screens::create_menu,
};

#[derive(Clone, Copy, PartialEq)]
pub enum WifiMenuEvents {
    Setup,
    Networks,
    SpeedTest,
    ChangeInMenu(MenuWifi),
    ChangeInMeasurement(MeasurementWifi),
    ChangeMinBattery(MinBattery),
    Back,
}

impl SelectValue for WifiMenuEvents {
    fn marker(&self) -> &'static str {
        match self {
            Self::Back => "<-",
            _ => "->",
        }
    }
}

pub async fn wifi_menu(context: &mut Context) -> AppState {
    let result = WifiMenu
        .display(context)
        .await
        .unwrap_or(AppState::Shutdown);

    context.save_config().await;

    result
}

type WifiMenuItem<T> = MenuItem<&'static str, WifiMenuEvents, T, true>;

struct WifiMenu;
type WifiMenuBuilder = MenuBuilder<
    chain!(
        MenuItems<WifiMenuItem<WifiMenuEvents>, WifiMenuEvents, 3>,
        WifiMenuItem<MenuWifi>,
        WifiMenuItem<MeasurementWifi>,
        WifiMenuItem<MinBattery>,
        WifiMenuItem<WifiMenuEvents>
    ),
    WifiMenuEvents,
>;

fn wifi_menu_builder(context: &mut Context) -> WifiMenuBuilder {
    let mut radio_items = heapless::Vec::<_, 3>::new();

    if context.can_enable_wifi() {
        let mut radio_item = |label, event| {
            unwrap!(radio_items
                .push(MenuItem::new(label, event).with_value_converter(|evt| evt))
                .ok())
        };

        radio_item("Wifi setup", WifiMenuEvents::Setup);
        radio_item("Wifi networks", WifiMenuEvents::Networks);
        if context.backend_ready().is_ok() {
            radio_item("Speed test", WifiMenuEvents::SpeedTest);
        }
    }

    create_menu("Wi-Fi")
        .add_menu_items(radio_items)
        .add_item(
            "In menu",
            context.config.wifi_in_menu,
            WifiMenuEvents::ChangeInMenu,
        )
        .add_item(
            "In EKG",
            context.config.wifi_in_measurement,
            WifiMenuEvents::ChangeInMeasurement,
        )
        .add_item(
            "Min battery",
            context.config.wifi_min_battery,
            WifiMenuEvents::ChangeMinBattery,
        )
        .add_item("Back", WifiMenuEvents::Back, |evt| evt)
}

impl MenuScreen for WifiMenu {
    type Event = WifiMenuEvents;
    type Result = AppState;
    type MenuBuilder = WifiMenuBuilder;

    async fn menu(&mut self, context: &mut Context) -> Self::MenuBuilder {
        wifi_menu_builder(context)
    }

    async fn handle_event(
        &mut self,
        event: Self::Event,
        context: &mut Context,
    ) -> Option<Self::Result> {
        match event {
            WifiMenuEvents::Setup => return Some(AppState::Menu(AppMenu::WifiAP)),
            WifiMenuEvents::Networks => return Some(AppState::Menu(AppMenu::WifiListVisible)),
            WifiMenuEvents::SpeedTest => return Some(AppState::Throughput),
            WifiMenuEvents::ChangeInMenu(value) => {
                context.update_config(|config| config.wifi_in_menu = value);
            }
            WifiMenuEvents::ChangeInMeasurement(value) => {
                context.update_config(|config| config.wifi_in_measurement = value);
            }
            WifiMenuEvents::ChangeMinBattery(value) => {
                context.update_config(|config| config.wifi_min_battery = value);
            }
            WifiMenuEvents::Back => return Some(AppState::Menu(AppMenu::Main)),
        }

        None
    }
}
