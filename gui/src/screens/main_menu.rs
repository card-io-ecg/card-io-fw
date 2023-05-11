use embedded_menu::Menu;

#[derive(Clone, Copy)]
pub enum MainMenuEvents {
    WifiSetup,
    Shutdown,
}

#[derive(Clone, Copy, Menu)]
#[menu(
    title = "Main menu",
    navigation(events = MainMenuEvents),
    items = [
        navigation(label = "Wifi setup", event = MainMenuEvents::WifiSetup),
        navigation(label = "Shutdown", event = MainMenuEvents::Shutdown)
    ]
)]
pub struct MainMenu {}
