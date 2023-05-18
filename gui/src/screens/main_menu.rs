use embedded_menu::Menu;

#[derive(Clone, Copy)]
pub enum MainMenuEvents {
    Display,
    WifiSetup,
    Shutdown,
}

#[derive(Clone, Copy, PartialEq, Eq, Menu)]
#[menu(
    title = "Main menu",
    navigation(events = MainMenuEvents),
    items = [
        navigation(label = "Display settings", event = MainMenuEvents::Display),
        navigation(label = "Wifi setup", event = MainMenuEvents::WifiSetup),
        navigation(label = "Shutdown", event = MainMenuEvents::Shutdown)
    ]
)]
pub struct MainMenu {}
