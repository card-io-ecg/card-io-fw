pub mod about;
pub mod display;
pub mod main;
pub mod wifi_ap;
pub mod wifi_sta;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AppMenu {
    Main,
    Display,
    About,
    WifiAP,
    WifiListVisible,
}
