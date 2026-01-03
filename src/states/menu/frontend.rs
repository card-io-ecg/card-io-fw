use crate::{
    board::initialized::Context,
    states::menu::{AppMenu, MenuBuilder, MenuScreen},
    AppState,
};
use config_types::types::{Gain, LeadOffCurrent, LeadOffFrequency, LeadOffThreshold};
use embedded_menu::items::MenuItem;
use gui::{
    embedded_layout::{
        chain,
        object_chain::{Chain, Link},
    },
    screens::create_menu,
};

pub async fn frontend_menu(context: &mut Context) -> AppState {
    let result = FrontendMenu
        .display(context)
        .await
        .unwrap_or(AppState::Shutdown);

    context.save_config().await;

    result
}

#[derive(Clone, Copy)]
pub enum FrontendMenuEvents {
    ChangeClockSource(bool),
    ChangeLeadOffCurrent(LeadOffCurrent),
    ChangeLeadOffThreshold(LeadOffThreshold),
    ChangeLeadOffFrequency(LeadOffFrequency),
    ChangeGain(Gain),
    Back,
}

type FrontendMenuItem<T> = MenuItem<&'static str, FrontendMenuEvents, T, true>;

struct FrontendMenu;
type FrontendMenuBuilder = MenuBuilder<
    chain!(
        FrontendMenuItem<bool>,
        FrontendMenuItem<LeadOffCurrent>,
        FrontendMenuItem<LeadOffThreshold>,
        FrontendMenuItem<LeadOffFrequency>,
        FrontendMenuItem<Gain>,
        FrontendMenuItem<&'static str>
    ),
    FrontendMenuEvents,
>;

fn frontend_menu_builder(context: &mut Context) -> FrontendMenuBuilder {
    create_menu("EKG")
        .add_item(
            "External CLK",
            context.config.use_external_clock,
            FrontendMenuEvents::ChangeClockSource,
        )
        .add_item(
            "LOFF current",
            context.config.lead_off_current,
            FrontendMenuEvents::ChangeLeadOffCurrent,
        )
        .add_item(
            "LOFF threshold",
            context.config.lead_off_threshold,
            FrontendMenuEvents::ChangeLeadOffThreshold,
        )
        .add_item(
            "LOFF frequency",
            context.config.lead_off_frequency,
            FrontendMenuEvents::ChangeLeadOffFrequency,
        )
        .add_item("Gain", context.config.gain, FrontendMenuEvents::ChangeGain)
        .add_item("Back", "<-", |_| FrontendMenuEvents::Back)
}

impl MenuScreen for FrontendMenu {
    type Event = FrontendMenuEvents;
    type Result = AppState;
    type MenuBuilder = FrontendMenuBuilder;

    async fn menu(&mut self, context: &mut Context) -> Self::MenuBuilder {
        frontend_menu_builder(context)
    }

    async fn handle_event(
        &mut self,
        event: Self::Event,
        context: &mut Context,
    ) -> Option<Self::Result> {
        match event {
            FrontendMenuEvents::ChangeClockSource(use_external) => {
                context.update_config(|config| config.use_external_clock = use_external);
            }
            FrontendMenuEvents::ChangeLeadOffCurrent(current) => {
                context.update_config(|config| config.lead_off_current = current);
            }
            FrontendMenuEvents::ChangeLeadOffThreshold(threshold) => {
                context.update_config(|config| config.lead_off_threshold = threshold);
            }
            FrontendMenuEvents::ChangeLeadOffFrequency(frequency) => {
                context.update_config(|config| config.lead_off_frequency = frequency);
            }
            FrontendMenuEvents::ChangeGain(gain) => {
                context.update_config(|config| config.gain = gain);
            }
            FrontendMenuEvents::Back => return Some(AppState::Menu(AppMenu::Main)),
        }

        None
    }
}
