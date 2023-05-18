use embedded_menu::{Menu, SelectValue};

#[derive(Clone, Copy)]
pub enum DisplayMenuEvents {
    Back,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, SelectValue)]
pub enum DisplayBrightness {
    Dimmest,
    Dim,
    Normal,
    Bright,
    Brightest,
}

#[derive(Clone, Copy, PartialEq, Eq, Menu)]
#[menu(
    title = "Display",
    navigation(events = DisplayMenuEvents),
    items = [
        data(label = "Brightness", field = brightness),
        navigation(label = "Back", event = DisplayMenuEvents::Back)
    ]
)]
pub struct DisplayMenu {
    pub brightness: DisplayBrightness,
}
