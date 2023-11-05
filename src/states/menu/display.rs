use crate::{
    board::{
        config::types::{DisplayBrightness, FilterStrength},
        initialized::Context,
    },
    states::menu::{AppMenu, MenuScreen},
    AppState,
};
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

impl MenuScreen for DisplayMenu {
    type Event = DisplayMenuEvents;
    type Result = AppState;

    async fn menu(&mut self, context: &mut Context) -> impl super::AppMenuBuilder<Self::Event> {
        create_menu("Display")
            .add_item(
                "Brightness",
                context.config.display_brightness,
                DisplayMenuEvents::ChangeBrigtness,
            )
            .add_item(
                "Battery",
                context.config.battery_display_style,
                DisplayMenuEvents::ChangeBatteryStyle,
            )
            .add_item(
                "EKG Filter",
                context.config.filter_strength,
                DisplayMenuEvents::ChangeFilterStrength,
            )
            .add_item("Back", "<-", |_| DisplayMenuEvents::Back)
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
