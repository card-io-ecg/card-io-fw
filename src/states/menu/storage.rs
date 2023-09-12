use crate::{
    board::{initialized::Board, storage::FileSystem},
    states::{display_message, AppMenu, TouchInputShaper, MENU_IDLE_DURATION, MIN_FRAME_TIME},
    timeout::Timeout,
    AppState,
};
use embassy_time::Ticker;
use embedded_graphics::prelude::*;
use gui::screens::{
    menu_style,
    screen::Screen,
    storage_menu::{StorageMenu, StorageMenuEvents},
};

pub async fn storage_menu(board: &mut Board) -> AppState {
    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);

    let mut menu_values = StorageMenu {
        store_measurement: board.config.store_measurement,
    };

    let mut menu_screen = Screen {
        content: menu_values.create_menu_with_style(menu_style()),

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
            };
        }

        #[cfg(feature = "battery_max17055")]
        if board.battery_monitor.is_low() {
            return AppState::Shutdown;
        }

        menu_screen.status_bar = board.status_bar();

        if &menu_values != menu_screen.content.data() {
            debug!("Settings changed");
            let new = *menu_screen.content.data();
            if menu_values.store_measurement != new.store_measurement {
                board.config_changed = true;
                board.config.store_measurement = new.store_measurement;
            }

            menu_values = new;
        }

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
