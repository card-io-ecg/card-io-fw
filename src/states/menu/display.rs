use crate::{
    board::{
        config::types::{DisplayBrightness, FilterStrength},
        initialized::Board,
    },
    states::menu::{AppMenu, MenuScreen},
    AppState,
};
use embedded_menu::items::{NavigationItem, Select};
use gui::{screens::create_menu, widgets::battery_small::BatteryStyle};

pub async fn display_menu(board: &mut Board) -> AppState {
    let result = DisplayMenu
        .display(board)
        .await
        .unwrap_or(AppState::Shutdown);

    board.inner.save_config().await;

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

    async fn menu(&mut self, board: &mut Board) -> impl super::AppMenuBuilder<Self::Event> {
        create_menu("Display")
            .add_item(
                Select::new("Brightness", board.inner.config.display_brightness)
                    .with_value_converter(DisplayMenuEvents::ChangeBrigtness),
            )
            .add_item(
                Select::new("Battery", board.inner.config.battery_display_style)
                    .with_value_converter(DisplayMenuEvents::ChangeBatteryStyle),
            )
            .add_item(
                Select::new("EKG Filter", board.inner.config.filter_strength)
                    .with_value_converter(DisplayMenuEvents::ChangeFilterStrength),
            )
            .add_item(NavigationItem::new("Back", DisplayMenuEvents::Back))
    }

    async fn handle_event(
        &mut self,
        event: Self::Event,
        board: &mut Board,
    ) -> Option<Self::Result> {
        match event {
            DisplayMenuEvents::ChangeBrigtness(brightness) => {
                board
                    .inner
                    .update_config(|config| config.display_brightness = brightness);
                board.apply_hw_config_changes().await;
            }
            DisplayMenuEvents::ChangeBatteryStyle(style) => {
                board
                    .inner
                    .update_config(|config| config.battery_display_style = style);
            }
            DisplayMenuEvents::ChangeFilterStrength(strength) => {
                board
                    .inner
                    .update_config(|config| config.filter_strength = strength);
            }
            DisplayMenuEvents::Back => return Some(AppState::Menu(AppMenu::Main)),
        }

        None
    }
}
