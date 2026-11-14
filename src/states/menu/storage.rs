use crate::{
    board::{initialized::Context, logging, storage::FileSystem},
    states::menu::{AppMenu, MenuBuilder, MenuItems, MenuScreen},
    uformat, AppState,
};
use config_types::{types::MeasurementAction, Config};
use embedded_menu::items::menu_item::{MenuItem, SelectValue};
use gui::{
    embedded_layout::{
        chain,
        object_chain::{Chain, Link},
    },
    screens::create_menu,
};

#[derive(Clone, Copy)]
pub enum StorageMenuEvents {
    ChangeMeasurementAction(MeasurementAction),
    SaveLogs,
    Format,
    Nothing,
    Back,
}

pub async fn storage_menu(context: &mut Context) -> AppState {
    let result = StorageMenu
        .display(context)
        .await
        .unwrap_or(AppState::Shutdown);

    context.save_config().await;

    result
}

#[derive(Clone, PartialEq)]
struct SavedCount(heapless::String<32>);

impl SelectValue for SavedCount {
    fn marker(&self) -> &str {
        self.0.as_str()
    }
}

type StorageMenuItem<T> = MenuItem<&'static str, StorageMenuEvents, T, true>;

struct StorageMenu;
type StorageMenuBuilder = MenuBuilder<
    chain!(
        StorageMenuItem<MeasurementAction>,
        MenuItems<StorageMenuItem<SavedCount>, StorageMenuEvents, 2>,
        StorageMenuItem<&'static str>,
        StorageMenuItem<&'static str>,
        StorageMenuItem<&'static str>
    ),
    StorageMenuEvents,
>;

async fn storage_menu_builder(context: &mut Context) -> StorageMenuBuilder {
    let mut used_item = heapless::Vec::<_, 2>::new();

    if let Some(storage) = context.storage.as_mut() {
        if let Ok(count) = storage.measurement_count().await {
            let saved = SavedCount(uformat!(32, "{}", count));

            unwrap!(used_item
                .push(
                    MenuItem::new("Saved", saved)
                        .with_value_converter(|_| StorageMenuEvents::Nothing)
                )
                .ok());
        }
        if let Ok(waiting) = storage.waiting_log_records().await {
            let logs = SavedCount(uformat!(32, "{}", waiting.len()));

            unwrap!(used_item
                .push(
                    MenuItem::new("Logs", logs)
                        .with_value_converter(|_| StorageMenuEvents::Nothing)
                )
                .ok());
        }
    }

    create_menu("Storage")
        .add_item(
            "New EKG",
            context.config.measurement_action,
            StorageMenuEvents::ChangeMeasurementAction,
        )
        .add_menu_items(used_item)
        .add_item("Save logs", "->", |_| StorageMenuEvents::SaveLogs)
        .add_item("Format storage", "->", |_| StorageMenuEvents::Format)
        .add_item("Back", "<-", |_| StorageMenuEvents::Back)
}

impl MenuScreen for StorageMenu {
    type Event = StorageMenuEvents;
    type Result = AppState;
    type MenuBuilder = StorageMenuBuilder;

    fn menu(
        &mut self,
        context: &mut Context,
    ) -> impl core::future::Future<Output = Self::MenuBuilder> {
        storage_menu_builder(context)
    }

    async fn handle_event(
        &mut self,
        event: Self::Event,
        context: &mut Context,
    ) -> Option<Self::Result> {
        match event {
            StorageMenuEvents::ChangeMeasurementAction(action) => {
                debug!("Settings changed");

                context.update_config(|config| config.measurement_action = action);
            }
            StorageMenuEvents::SaveLogs => {
                let message = match context.storage.as_mut() {
                    Some(storage) => match logging::save_now(storage).await {
                        Ok(()) => "Logs saved",
                        Err(e) => {
                            warn!("Failed to save the logs: {:?}", e);
                            "Could not save the logs"
                        }
                    },
                    None => "Storage not available",
                };
                context.display_message(message).await;

                // The menu shows the new count of saved logs.
                return Some(AppState::Menu(AppMenu::Storage));
            }
            StorageMenuEvents::Format => {
                info!("Format requested");
                context.display_message("Formatting storage...").await;
                core::mem::drop(context.storage.take());
                FileSystem::format().await;
                context.storage = FileSystem::mount().await;

                context.update_config(|config| *config = Config::default());
                context.apply_hw_config_changes().await;
                // Prevent saving config changes
                context.config_changed = false;

                return Some(AppState::Menu(AppMenu::Main));
            }
            StorageMenuEvents::Back => return Some(AppState::Menu(AppMenu::Main)),
            StorageMenuEvents::Nothing => {}
        }

        None
    }
}
