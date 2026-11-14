use config_site::data::network::WifiNetwork;
use gui::widgets::battery_small::BatteryStyle;

use crate::{
    types::{
        DisplayBrightness, FilterStrength, Gain, LeadOffCurrent, LeadOffFrequency,
        LeadOffThreshold, MeasurementAction,
    },
    Config,
};

/// The config of record version 1.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Config1 {
    pub battery_display_style: BatteryStyle,
    pub display_brightness: DisplayBrightness,
    pub known_networks: heapless::Vec<WifiNetwork, 8>,
    pub filter_strength: FilterStrength,
    pub backend_url: heapless::String<64>,
    pub measurement_action: MeasurementAction,
    pub use_external_clock: bool,
    pub lead_off_current: LeadOffCurrent,
    pub lead_off_threshold: LeadOffThreshold,
    pub lead_off_frequency: LeadOffFrequency,
    pub gain: Gain,
}

impl From<Config1> for Config {
    fn from(old: Config1) -> Self {
        Self {
            battery_display_style: old.battery_display_style,
            display_brightness: old.display_brightness,
            known_networks: old.known_networks,
            filter_strength: old.filter_strength,
            backend_url: old.backend_url,
            measurement_action: old.measurement_action,
            use_external_clock: old.use_external_clock,
            lead_off_current: old.lead_off_current,
            lead_off_threshold: old.lead_off_threshold,
            lead_off_frequency: old.lead_off_frequency,
            gain: old.gain,
            ..Self::default()
        }
    }
}
