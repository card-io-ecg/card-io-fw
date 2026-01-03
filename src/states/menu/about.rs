use crate::{
    board::initialized::Context,
    states::menu::{AppMenu, MenuBuilder, MenuScreen},
    uformat, AppState, SerialNumber,
};
use ads129x::ll;

use embedded_menu::items::menu_item::{MenuItem, SelectValue};
use gui::{
    embedded_layout::{
        chain,
        object_chain::{Chain, Link},
    },
    screens::create_menu,
};
use ufmt::uDisplay;

#[derive(Clone, Copy)]
pub enum AboutMenuEvents {
    None,
    ToBatteryInfo,
    ToSerial,
    Back,
}

pub async fn about_menu(context: &mut Context) -> AppState {
    AboutAppMenu
        .display(context)
        .await
        .unwrap_or(AppState::Shutdown)
}

#[derive(Clone, PartialEq)]
struct MenuString<const N: usize>(heapless::String<N>);

impl<D: uDisplay, const N: usize> From<D> for MenuString<N> {
    fn from(value: D) -> Self {
        Self(uformat!(N, "{}", value))
    }
}

impl<const N: usize> SelectValue for MenuString<N> {
    fn marker(&self) -> &str {
        self.0.as_str()
    }
}

type AboutMenuItem<T> = MenuItem<&'static str, AboutMenuEvents, T, true>;

struct AboutAppMenu;
type AboutMenuBuilder = MenuBuilder<
    chain!(
        AboutMenuItem<&'static str>,
        AboutMenuItem<&'static str>,
        AboutMenuItem<MenuString<12>>,
        AboutMenuItem<&'static str>,
        AboutMenuItem<&'static str>,
        AboutMenuItem<&'static str>
    ),
    AboutMenuEvents,
>;

fn about_menu_builder(context: &mut Context) -> AboutMenuBuilder {
    let adc_model = match context.frontend.device_id() {
        Some(id) => match id {
            ll::DeviceId::Ads1191 => "ADS1191",
            ll::DeviceId::Ads1192 => "ADS1192",
            ll::DeviceId::Ads1291 => "ADS1291",
            ll::DeviceId::Ads1292 => "ADS1292",
            ll::DeviceId::Ads1292r => "ADS1292R",
        },
        None => "Unknown",
    };

    create_menu("Device info")
        .add_item("FW", env!("FW_VERSION"), |_| AboutMenuEvents::None)
        .add_item("HW", env!("COMPLETE_HW_VERSION"), |_| AboutMenuEvents::None)
        .add_item("Serial", MenuString::from(SerialNumber), |_| {
            AboutMenuEvents::ToSerial
        })
        .add_item("ADC", adc_model, |_| AboutMenuEvents::None)
        .add_item("Fuel gauge", "MAX17055", |_| AboutMenuEvents::ToBatteryInfo)
        .add_item("Back", "<-", |_| AboutMenuEvents::Back)
}

impl MenuScreen for AboutAppMenu {
    type Event = AboutMenuEvents;
    type Result = AppState;
    type MenuBuilder = AboutMenuBuilder;

    async fn menu(&mut self, context: &mut Context) -> Self::MenuBuilder {
        about_menu_builder(context)
    }

    async fn handle_event(
        &mut self,
        event: Self::Event,
        _board: &mut Context,
    ) -> Option<Self::Result> {
        match event {
            AboutMenuEvents::None => None,
            AboutMenuEvents::ToBatteryInfo => Some(AppState::Menu(AppMenu::BatteryInfo)),
            AboutMenuEvents::ToSerial => Some(AppState::DisplaySerial),
            AboutMenuEvents::Back => Some(AppState::Menu(AppMenu::Main)),
        }
    }
}
