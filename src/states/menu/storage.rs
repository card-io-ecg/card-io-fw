use crate::{
    board::{initialized::Board, storage::FileSystem},
    states::{display_message, AppMenu, TouchInputShaper, MENU_IDLE_DURATION, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState,
};
use embassy_time::Ticker;
use embedded_graphics::prelude::*;
use embedded_menu::{
    items::{NavigationItem, Select},
    Menu,
};
use gui::screens::{menu_style, screen::Screen};

#[derive(Clone, Copy)]
pub enum StorageMenuEvents {
    StoreMeasurement(bool),
    Format,
    Upload,
    Back,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StorageMenu {
    pub store_measurement: bool,
}

pub async fn storage_menu(board: &mut Board) -> AppState {
    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);

    let mut items = heapless::Vec::<_, 2>::new();
    unwrap!(items
        .push(NavigationItem::new(
            "Format storage",
            StorageMenuEvents::Format,
        ))
        .ok());

    if board.can_enable_wifi()
        && !board.config.known_networks.is_empty()
        && !board.config.backend_url.is_empty()
    {
        unwrap!(items
            .push(NavigationItem::new(
                "Upload data",
                StorageMenuEvents::Upload
            ))
            .ok());
    }

    let mut menu_screen = Screen {
        content: Menu::with_style("Storage", menu_style())
            .add_item(
                Select::new("Store EKG", board.config.store_measurement)
                    .with_value_converter(StorageMenuEvents::StoreMeasurement),
            )
            .add_items(&mut items[..])
            .add_item(NavigationItem::new("Back", StorageMenuEvents::Back))
            .build(),

        status_bar: board.status_bar(),
    };

    let mut ticker = Ticker::every(MIN_FRAME_TIME);
    let mut input = TouchInputShaper::new();

    while !exit_timer.is_elapsed() {
        input.update(&mut board.frontend);
        let is_touched = input.is_touched();
        if is_touched {
            exit_timer.reset();
        }

        if let Some(event) = menu_screen.content.interact(is_touched) {
            match event {
                StorageMenuEvents::Format => {
                    info!("Format requested");
                    display_message(board, "Formatting storage...").await;
                    core::mem::drop(board.storage.take());
                    FileSystem::format().await;
                    board.storage = FileSystem::mount().await;

                    return AppState::Menu(AppMenu::Main);
                }
                StorageMenuEvents::Upload => return AppState::UploadStored(AppMenu::Storage),
                StorageMenuEvents::Back => {
                    board.save_config().await;
                    return AppState::Menu(AppMenu::Main);
                }
                StorageMenuEvents::StoreMeasurement(store) => {
                    debug!("Settings changed");

                    board.config_changed = true;
                    board.config.store_measurement = store;
                }
            };
        }

        #[cfg(feature = "battery_max17055")]
        if board.battery_monitor.is_low() {
            return AppState::Shutdown;
        }

        menu_screen.status_bar = board.status_bar();

        board
            .display
            .frame(|display| {
                menu_screen.content.update(display);
                menu_screen.draw(display)
            })
            .await;

        ticker.next().await;
    }

    info!("Menu timeout");
    board.save_config().await;
    AppState::Shutdown
}
