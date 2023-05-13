use embedded_menu::{Menu, SelectValue};

#[derive(Clone, Copy)]
pub enum MainMenuEvents {
    WifiSetup,
    Shutdown,
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
    title = "Main menu",
    navigation(events = MainMenuEvents),
    items = [
        data(label = "Display brightness", field = brightness),
        navigation(label = "Wifi setup", event = MainMenuEvents::WifiSetup),
        navigation(label = "Shutdown", event = MainMenuEvents::Shutdown)
    ]
)]
pub struct MainMenu {
    pub brightness: DisplayBrightness,
}
