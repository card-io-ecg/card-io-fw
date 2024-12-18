use crate::{
    board::{
        config::types::{DisplayBrightness, FilterStrength},
        initialized::Context,
    },
    states::menu::{AppMenu, AppMenuBuilder, MenuScreen},
    AppState,
};
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_menu::{
    builder::MenuBuilder,
    interaction::single_touch::SingleTouch,
    items::MenuItem,
    selection_indicator::{style::AnimatedTriangle, AnimatedPosition},
};
use gui::{
    embedded_layout::object_chain, screens::create_menu, widgets::battery_small::BatteryStyle,
};

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
type DisplayMenuBuilder = MenuBuilder<
    &'static str,
    SingleTouch,
    object_chain::Link<
        MenuItem<&'static str, DisplayMenuEvents, &'static str, true>,
        object_chain::Link<
            MenuItem<&'static str, DisplayMenuEvents, FilterStrength, true>,
            object_chain::Link<
                MenuItem<&'static str, DisplayMenuEvents, BatteryStyle, true>,
                object_chain::Chain<
                    MenuItem<&'static str, DisplayMenuEvents, DisplayBrightness, true>,
                >,
            >,
        >,
    >,
    DisplayMenuEvents,
    AnimatedPosition,
    AnimatedTriangle,
    BinaryColor,
>;

fn display_menu_builder(context: &mut Context) -> DisplayMenuBuilder {
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
