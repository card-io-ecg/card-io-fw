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
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_menu::{
    builder::MenuBuilder,
    collection::MenuItems,
    interaction::single_touch::SingleTouch,
    items::menu_item::{MenuItem, SelectValue},
    selection_indicator::{style::AnimatedTriangle, AnimatedPosition},
};
use gui::{embedded_layout::object_chain, screens::create_menu};

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

#[derive(Clone, PartialEq)]
struct UsedStorage(heapless::String<32>);

impl SelectValue for UsedStorage {
    fn marker(&self) -> &str {
        self.0.as_str()
    }
}

struct StorageMenu;
type StorageMenuBuilder = MenuBuilder<
    &'static str,
    SingleTouch,
    object_chain::Link<
        MenuItem<&'static str, StorageMenuEvents, &'static str, true>,
        object_chain::Link<
            MenuItem<&'static str, StorageMenuEvents, &'static str, true>,
            object_chain::Link<
                MenuItems<
                    heapless::Vec<MenuItem<&'static str, StorageMenuEvents, &'static str, true>, 2>,
                    MenuItem<&'static str, StorageMenuEvents, &'static str, true>,
                    StorageMenuEvents,
                >,
                object_chain::Link<
                    MenuItems<
                        heapless::Vec<
                            MenuItem<&'static str, StorageMenuEvents, UsedStorage, true>,
                            2,
                        >,
                        MenuItem<&'static str, StorageMenuEvents, UsedStorage, true>,
                        StorageMenuEvents,
                    >,
                    object_chain::Chain<
                        MenuItem<&'static str, StorageMenuEvents, MeasurementAction, true>,
                    >,
                >,
            >,
        >,
    >,
    StorageMenuEvents,
    AnimatedPosition,
    AnimatedTriangle,
    BinaryColor,
>;

async fn storage_menu_builder(context: &mut Context) -> StorageMenuBuilder {
    let mut used_item = heapless::Vec::<_, 2>::new();
    let mut items = heapless::Vec::<_, 2>::new();

    if let Some(storage) = context.storage.as_mut() {
        if let Ok(used) = storage.used_bytes().await {
            let used_str = UsedStorage(uformat!(
                32,
                "{}/{}",
                BinarySize(used),
                BinarySize(storage.capacity())
            ));

            unwrap!(used_item
                .push(
                    MenuItem::new("Used", used_str)
                        .with_value_converter(|_| StorageMenuEvents::Nothing)
                )
                .ok());
        }
    }

    if context.can_enable_wifi()
        && !context.config.known_networks.is_empty()
        && !context.config.backend_url.is_empty()
        && context.sta_has_work().await
    {
        unwrap!(items
            .push(
                MenuItem::new("Upload data", "->")
                    .with_value_converter(|_| StorageMenuEvents::Upload)
            )
            .ok());
    }

    create_menu("Storage")
        .add_item(
            "New EKG",
            context.config.measurement_action,
            StorageMenuEvents::ChangeMeasurementAction,
        )
        .add_menu_items(used_item)
        .add_menu_items(items)
        .add_item("Format storage", "->", |_| StorageMenuEvents::Format)
        .add_item("Back", "<-", |_| StorageMenuEvents::Back)
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
