use crate::{
    board::{
        config::{types::MeasurementAction, Config},
        initialized::Context,
        storage::FileSystem,
    },
    human_readable::BinarySize,
    states::menu::{AppMenu, MenuScreen},
    uformat, AppState,
};
use embedded_menu::items::{NavigationItem, Select};
use gui::screens::create_menu;

use super::AppMenuBuilder;

#[derive(Clone, Copy)]
pub enum StorageMenuEvents {
    ChangeMeasurementAction(MeasurementAction),
    Format,
    Upload,
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

struct StorageMenu;
type StorageMenuBuilder = impl AppMenuBuilder<StorageMenuEvents>;

async fn storage_menu_builder(context: &mut Context) -> StorageMenuBuilder {
    // needs to be separate because the item is of a different type
    let mut used_item = heapless::Vec::<_, 1>::new();
    if let Some(storage) = context.storage.as_mut() {
        if let Ok(used) = storage.used_bytes().await {
            let used_str = uformat!(
                32,
                "{}/{}",
                BinarySize(used),
                BinarySize(storage.capacity())
            );

            unwrap!(used_item
                .push(NavigationItem::new("Used", StorageMenuEvents::Nothing).with_marker(used_str))
                .ok());
        }
    }

    let mut items = heapless::Vec::<_, 2>::new();
    unwrap!(items
        .push(NavigationItem::new(
            "Format storage",
            StorageMenuEvents::Format,
        ))
        .ok());

    if context.can_enable_wifi()
        && !context.config.known_networks.is_empty()
        && !context.config.backend_url.is_empty()
        && context.sta_has_work().await
    {
        unwrap!(items
            .push(NavigationItem::new(
                "Upload data",
                StorageMenuEvents::Upload
            ))
            .ok());
    }

    create_menu("Storage")
        .add_item(
            Select::new("New EKG", context.config.measurement_action)
                .with_value_converter(StorageMenuEvents::ChangeMeasurementAction)
                .with_detail_text("What to do with new measurements"),
        )
        .add_items(used_item)
        .add_items(items)
        .add_item(NavigationItem::new("Back", StorageMenuEvents::Back))
}

impl MenuScreen for StorageMenu {
    type Event = StorageMenuEvents;
    type Result = AppState;
    type MenuBuilder = StorageMenuBuilder;

    async fn menu(&mut self, context: &mut Context) -> Self::MenuBuilder {
        storage_menu_builder(context).await
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
            StorageMenuEvents::Upload => return Some(AppState::UploadStored(AppMenu::Storage)),
            StorageMenuEvents::Back => return Some(AppState::Menu(AppMenu::Main)),
            StorageMenuEvents::Nothing => {}
        }

        None
    }
}
