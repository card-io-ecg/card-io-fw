use embedded_menu::Menu;

#[derive(Clone, Copy)]
pub enum StorageMenuEvents {
    Format,
    Back,
}

#[derive(Clone, Copy, PartialEq, Eq, Menu)]
#[menu(
    title = "Storage",
    navigation(events = StorageMenuEvents),
    items = [
        data(label = "Store EKG", field = store_measurement),
        navigation(label = "Format storage",  event = StorageMenuEvents::Format),
        navigation(label = "Back", event = StorageMenuEvents::Back)
    ]
)]
pub struct StorageMenu {
    pub store_measurement: bool,
}
