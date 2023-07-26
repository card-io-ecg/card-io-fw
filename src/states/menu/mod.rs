pub mod about;
pub mod display;
pub mod main;
pub mod wifi_ap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AppMenu {
    Main,
    Display,
    About,
    WifiAP,
}
