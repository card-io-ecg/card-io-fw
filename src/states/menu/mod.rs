pub mod about;
#[cfg(feature = "battery_max17055")]
pub mod battery_info;
pub mod display;
pub mod main;
pub mod wifi_ap;
pub mod wifi_sta;

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum AppMenu {
    Main,
    Display,
    Storage,
    DeviceInfo,
    #[cfg(feature = "battery_max17055")]
    BatteryInfo,
    WifiAP,
    WifiListVisible,
}
