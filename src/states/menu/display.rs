use crate::{
    board::{
        config::types::{DisplayBrightness, FilterStrength},
        initialized::Context,
    },
    states::menu::{AppMenu, AppMenuBuilder, MenuScreen},
    AppState,
};
use embedded_menu::items::{NavigationItem, Select};
use gui::{screens::create_menu, widgets::battery_small::BatteryStyle};

pub async fn display_menu(context: &mut Context) -> AppState {
    let result = DisplayMenu
        .display(context)
        .await
        .unwrap_or(AppState::Shutdown);

    context.save_config().await;

    result
}

#[derive(Clone, Copy)]
pub enum DisplayMenuEvents {
    ChangeBrigtness(DisplayBrightness),
    ChangeBatteryStyle(BatteryStyle),
    ChangeFilterStrength(FilterStrength),
    Back,
}

struct DisplayMenu;
type DisplayMenuBuilder = impl AppMenuBuilder<DisplayMenuEvents>;

fn display_menu_builder(context: &mut Context) -> DisplayMenuBuilder {
    create_menu("Display")
        .add_item(
            Select::new("Brightness", context.config.display_brightness)
                .with_value_converter(DisplayMenuEvents::ChangeBrigtness),
        )
        .add_item(
            Select::new("Battery", context.config.battery_display_style)
                .with_value_converter(DisplayMenuEvents::ChangeBatteryStyle),
        )
        .add_item(
            Select::new("EKG Filter", context.config.filter_strength)
                .with_value_converter(DisplayMenuEvents::ChangeFilterStrength),
        )
        .add_item(NavigationItem::new("Back", DisplayMenuEvents::Back))
}

impl MenuScreen for DisplayMenu {
    type Event = DisplayMenuEvents;
    type Result = AppState;
    type MenuBuilder = DisplayMenuBuilder;

    async fn menu(&mut self, context: &mut Context) -> Self::MenuBuilder {
        display_menu_builder(context)
    }

    async fn handle_event(
        &mut self,
        event: Self::Event,
        context: &mut Context,
    ) -> Option<Self::Result> {
        match event {
            DisplayMenuEvents::ChangeBrigtness(brightness) => {
                context.update_config(|config| config.display_brightness = brightness);
                context.apply_hw_config_changes().await;
            }
            DisplayMenuEvents::ChangeBatteryStyle(style) => {
                context.update_config(|config| config.battery_display_style = style);
            }
            DisplayMenuEvents::ChangeFilterStrength(strength) => {
                context.update_config(|config| config.filter_strength = strength);
            }
            DisplayMenuEvents::Back => return Some(AppState::Menu(AppMenu::Main)),
        }

        None
    }
}
