use config_site::data::{network::WifiNetwork, TimeZoneRule, UTC_RULE};
use gui::widgets::battery_small::BatteryStyle;
use ssd1306::prelude::Brightness;

use super::types::{
    DisplayBrightness, FilterStrength, Gain, LeadOffCurrent, LeadOffFrequency, LeadOffThreshold,
    MeasurementAction, MeasurementWifi, MenuWifi, MinBattery,
};

pub use config_site::data::DEFAULT_NTP_SERVER;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub battery_display_style: BatteryStyle,
    pub display_brightness: DisplayBrightness,
    pub known_networks: heapless::Vec<WifiNetwork, 8>,
    pub filter_strength: FilterStrength,
    pub backend_url: heapless::String<64>,
    pub measurement_action: MeasurementAction,
    // ADC frontend config
    pub use_external_clock: bool,
    pub lead_off_current: LeadOffCurrent,
    pub lead_off_threshold: LeadOffThreshold,
    pub lead_off_frequency: LeadOffFrequency,
    pub gain: Gain,
    pub time_zone: TimeZoneRule,
    pub ntp_server: heapless::String<64>,
    pub wifi_in_menu: MenuWifi,
    pub wifi_in_measurement: MeasurementWifi,
    pub wifi_min_battery: MinBattery,
}

impl Default for Config {
    #[inline(never)]
    fn default() -> Self {
        Self {
            battery_display_style: BatteryStyle::LowIndicator,
            display_brightness: DisplayBrightness::Normal,
            known_networks: heapless::Vec::new(),
            filter_strength: FilterStrength::Weak,
            backend_url: heapless::String::new(),
            measurement_action: MeasurementAction::Auto,
            use_external_clock: true,
            lead_off_current: LeadOffCurrent::Normal,
            lead_off_threshold: LeadOffThreshold::_95,
            lead_off_frequency: LeadOffFrequency::Dc,
            gain: Gain::X1,
            time_zone: unwrap!(TimeZoneRule::try_from(UTC_RULE).ok()),
            ntp_server: unwrap!(heapless::String::try_from(DEFAULT_NTP_SERVER).ok()),
            wifi_in_menu: MenuWifi::OnDemand,
            wifi_in_measurement: MeasurementWifi::Keep,
            wifi_min_battery: MinBattery::_50,
        }
    }
}

impl Config {
    pub fn battery_style(&self) -> BatteryStyle {
        self.battery_display_style
    }

    pub fn display_brightness(&self) -> Brightness {
        match self.display_brightness {
            DisplayBrightness::Dimmest => Brightness::DIMMEST,
            DisplayBrightness::Dim => Brightness::DIM,
            DisplayBrightness::Normal => Brightness::NORMAL,
            DisplayBrightness::Bright => Brightness::BRIGHT,
            DisplayBrightness::Brightest => Brightness::BRIGHTEST,
        }
    }

    pub fn filter_strength(&self) -> FilterStrength {
        self.filter_strength
    }
}
