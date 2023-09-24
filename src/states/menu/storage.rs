use crate::{
    board::{initialized::Board, storage::FileSystem},
    states::{
        display_message, menu::AppMenu, TouchInputShaper, MENU_IDLE_DURATION, MIN_FRAME_TIME,
    },
    timeout::Timeout,
    AppState,
};
use embassy_time::Ticker;
use embedded_graphics::prelude::*;
use embedded_io::asynch::{Read, Write};
use embedded_menu::{
    items::{NavigationItem, Select},
    Menu, SelectValue,
};
use gui::screens::{menu_style, screen::Screen};
use norfs::storable::{LoadError, Loadable, Storable};
use ufmt::{uDisplay, uwrite};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, SelectValue)]
pub enum MeasurementAction {
    Ask = 0,
    Auto = 1,
    Store = 2,
    Upload = 3,
    Discard = 4,
}

impl MeasurementAction {
    pub fn should_upload(self) -> bool {
        matches!(self, Self::Upload | Self::Auto)
    }

    pub fn should_store(self) -> bool {
        matches!(self, Self::Store | Self::Auto)
    }
}

impl Loadable for MeasurementAction {
    async fn load<R: Read>(reader: &mut R) -> Result<Self, LoadError<R::Error>> {
        let data = match u8::load(reader).await? {
            0 => Self::Ask,
            1 => Self::Auto,
            2 => Self::Store,
            3 => Self::Upload,
            4 => Self::Discard,
            _ => return Err(LoadError::InvalidValue),
        };

        Ok(data)
    }
}

impl Storable for MeasurementAction {
    async fn store<W: Write>(&self, writer: &mut W) -> Result<(), W::Error> {
        writer.write_all(&[*self as u8]).await
    }
}

#[derive(Clone, Copy)]
pub enum StorageMenuEvents {
    MeasurementAction(MeasurementAction),
    Format,
    Upload,
    Nothing,
    Back,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StorageMenu {
    pub store_measurement: bool,
}

struct BinarySize(usize);

impl uDisplay for BinarySize {
    fn fmt<W>(&self, f: &mut ufmt::Formatter<'_, W>) -> Result<(), W::Error>
    where
        W: ufmt::uWrite + ?Sized,
    {
        const SUFFIXES: &[&str] = &["kB", "MB", "GB"];
        const SIZES: &[usize] = &[1024, 1024 * 1024, 1024 * 1024 * 1024];

        for (size, suffix) in SIZES.iter().cloned().zip(SUFFIXES.iter().cloned()).rev() {
            if self.0 >= size {
                let int = self.0 / size;
                let frac = (self.0 % size) / (size / 10);
                uwrite!(f, "{}.{}{}", int, frac, suffix)?;
                return Ok(());
            }
        }

        uwrite!(f, "{}B", self.0)
    }
}

pub async fn storage_menu(board: &mut Board) -> AppState {
    let mut exit_timer = Timeout::new(MENU_IDLE_DURATION);

    let mut used_str = heapless::String::<32>::new();

    let mut items = heapless::Vec::<_, 3>::new();

    if let Some(storage) = board.storage.as_mut() {
        if let Ok(used) = storage.used_bytes().await {
            unwrap!(uwrite!(
                &mut used_str,
                "{}/{}",
                BinarySize(used),
                BinarySize(storage.capacity())
            )
            .ok());
            unwrap!(items
                .push(
                    NavigationItem::new("Used", StorageMenuEvents::Nothing)
                        .with_marker(used_str.as_str())
                )
                .ok());
        }
    }

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
                Select::new("New EKG", board.config.measurement_action)
                    .with_value_converter(StorageMenuEvents::MeasurementAction)
                    .with_detail_text("What to do with new measurements"),
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
                StorageMenuEvents::Nothing => {}
                StorageMenuEvents::MeasurementAction(action) => {
                    debug!("Settings changed");

                    board.config_changed = true;
                    board.config.measurement_action = action;
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
