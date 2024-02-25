use crate::{
    board::initialized::Context,
    human_readable::LeftPadAny,
    states::menu::{AppMenu, AppMenuBuilder, MenuScreen},
    uformat, AppState, SerialNumber,
};

use embedded_menu::items::menu_item::MenuItem;
use gui::screens::create_menu;

#[derive(Clone, Copy)]
pub enum AboutMenuEvents {
    None,
    #[cfg(feature = "battery_max17055")]
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

struct AboutAppMenu;
type AboutMenuBuilder = impl AppMenuBuilder<AboutMenuEvents>;

fn about_menu_builder(context: &mut Context) -> AboutMenuBuilder {
    let list_item =
        |label| MenuItem::new(label, "").with_value_converter(|_| AboutMenuEvents::None);

    let mut items = heapless::Vec::<_, 5>::new();
    items.extend([
        list_item(uformat!(20, "{}", env!("FW_VERSION_MENU_ITEM"))),
        list_item(uformat!(20, "{}", env!("HW_VERSION_MENU_ITEM"))),
        list_item(uformat!(20, "Serial  {}", SerialNumber))
            .with_value_converter(|_| AboutMenuEvents::ToSerial),
        list_item(match context.frontend.device_id() {
            Some(id) => uformat!(20, "ADC {:?}", LeftPadAny(16, id)),
            None => uformat!(20, "ADC          Unknown"),
        }),
    ]);

    #[cfg(feature = "battery_max17055")]
    {
        unwrap!(items
            .push(
                MenuItem::new(uformat!(20, "Fuel gauge"), "MAX17055")
                    .with_value_converter(|_| AboutMenuEvents::ToBatteryInfo)
            )
            .ok());
    }

    create_menu("Device info")
        .add_menu_items(items)
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
            #[cfg(feature = "battery_max17055")]
            AboutMenuEvents::ToBatteryInfo => Some(AppState::Menu(AppMenu::BatteryInfo)),
            AboutMenuEvents::ToSerial => Some(AppState::DisplaySerial),
            AboutMenuEvents::Back => Some(AppState::Menu(AppMenu::Main)),
        }
    }
}
