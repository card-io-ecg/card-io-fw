use crate::{v1::Config1, Config};

pub const VERSION: u32 = 2;
pub const VERSION_LEN: usize = 5;
pub const CONFIG_LEN: usize = 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ConfigError {
    MissingRecord,
    UnknownVersion(u32),
    Malformed,
}

pub fn encode_version(buf: &mut [u8; VERSION_LEN]) -> Result<&[u8], postcard::Error> {
    postcard::to_slice(&VERSION, buf).map(|encoded| &*encoded)
}

pub fn encode_config<'a>(
    config: &Config,
    buf: &'a mut [u8; CONFIG_LEN],
) -> Result<&'a [u8], postcard::Error> {
    postcard::to_slice(config, buf).map(|encoded| &*encoded)
}

pub fn decode_config(version: Option<&[u8]>, config: Option<&[u8]>) -> Result<Config, ConfigError> {
    let (Some(version), Some(config)) = (version, config) else {
        return Err(ConfigError::MissingRecord);
    };

    match postcard::from_bytes::<u32>(version).map_err(|_| ConfigError::Malformed)? {
        VERSION => postcard::from_bytes(config).map_err(|_| ConfigError::Malformed),
        1 => postcard::from_bytes::<Config1>(config)
            .map(Config::from)
            .map_err(|_| ConfigError::Malformed),
        other => Err(ConfigError::UnknownVersion(other)),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::types::{
        DisplayBrightness, FilterStrength, Gain, LeadOffCurrent, LeadOffFrequency,
        LeadOffThreshold, MeasurementAction, MeasurementWifi, MenuWifi, MinBattery,
    };
    use config_site::data::network::WifiNetwork;
    use gui::widgets::battery_small::BatteryStyle;

    fn worst_case_config() -> Config {
        let network = WifiNetwork {
            ssid: core::iter::repeat_n('s', 32).collect(),
            pass: core::iter::repeat_n('p', 64).collect(),
        };

        Config {
            battery_display_style: BatteryStyle::MilliVolts,
            display_brightness: DisplayBrightness::Brightest,
            known_networks: core::iter::repeat_n(network, 8).collect(),
            filter_strength: FilterStrength::Strong,
            backend_url: core::iter::repeat_n('u', 64).collect(),
            measurement_action: MeasurementAction::Discard,
            use_external_clock: false,
            lead_off_current: LeadOffCurrent::Strongest,
            lead_off_threshold: LeadOffThreshold::_70,
            lead_off_frequency: LeadOffFrequency::Ac,
            gain: Gain::X12,
            time_zone: core::iter::repeat_n('z', 48).collect(),
            ntp_server: core::iter::repeat_n('n', 64).collect(),
            wifi_in_menu: MenuWifi::Always,
            wifi_in_measurement: MeasurementWifi::Stop,
            wifi_min_battery: MinBattery::_90,
        }
    }

    fn version_1_config() -> Config1 {
        let config = worst_case_config();

        Config1 {
            battery_display_style: config.battery_display_style,
            display_brightness: config.display_brightness,
            known_networks: config.known_networks,
            filter_strength: config.filter_strength,
            backend_url: config.backend_url,
            measurement_action: config.measurement_action,
            use_external_clock: config.use_external_clock,
            lead_off_current: config.lead_off_current,
            lead_off_threshold: config.lead_off_threshold,
            lead_off_frequency: config.lead_off_frequency,
            gain: config.gain,
        }
    }

    fn encoded_version() -> ([u8; VERSION_LEN], usize) {
        let mut buf = [0; VERSION_LEN];
        let len = encode_version(&mut buf).unwrap().len();
        (buf, len)
    }

    fn encoded_config(config: &Config) -> ([u8; CONFIG_LEN], usize) {
        let mut buf = [0; CONFIG_LEN];
        let len = encode_config(config, &mut buf).unwrap().len();
        (buf, len)
    }

    fn decode(version: &[u8], config: &[u8]) -> Result<Config, ConfigError> {
        decode_config(Some(version), Some(config))
    }

    #[test]
    fn default_config_round_trips() {
        let config = Config::default();
        let (version, version_len) = encoded_version();
        let (encoded, len) = encoded_config(&config);

        assert_eq!(Ok(config), decode(&version[..version_len], &encoded[..len]));
    }

    #[test]
    fn config_with_every_field_changed_round_trips() {
        let config = worst_case_config();
        let (version, version_len) = encoded_version();
        let (encoded, len) = encoded_config(&config);

        assert_eq!(Ok(config), decode(&version[..version_len], &encoded[..len]));
    }

    #[test]
    fn version_1_config_gets_the_defaults_of_the_new_fields() {
        let old = version_1_config();
        let mut buf = [0; CONFIG_LEN];
        let encoded = postcard::to_slice(&old, &mut buf).unwrap();

        let defaults = Config::default();
        let expected = Config {
            time_zone: defaults.time_zone,
            ntp_server: defaults.ntp_server,
            wifi_in_menu: defaults.wifi_in_menu,
            wifi_in_measurement: defaults.wifi_in_measurement,
            wifi_min_battery: defaults.wifi_min_battery,
            ..worst_case_config()
        };

        assert_eq!(Ok(expected), decode(&[1], encoded));
    }

    #[test]
    fn worst_case_config_fits_the_config_record() {
        let mut buf = [0; CONFIG_LEN];

        let len = encode_config(&worst_case_config(), &mut buf).unwrap().len();

        assert!(len <= CONFIG_LEN, "{len} bytes");
    }

    #[test]
    fn version_fits_the_version_record() {
        let mut buf = [0; VERSION_LEN];

        assert!(encode_version(&mut buf).is_ok());
    }

    #[test]
    fn missing_record_is_reported() {
        let (version, version_len) = encoded_version();
        let (config, config_len) = encoded_config(&Config::default());
        let version = &version[..version_len];
        let config = &config[..config_len];

        assert_eq!(
            Err(ConfigError::MissingRecord),
            decode_config(None, Some(config))
        );
        assert_eq!(
            Err(ConfigError::MissingRecord),
            decode_config(Some(version), None)
        );
        assert_eq!(Err(ConfigError::MissingRecord), decode_config(None, None));
    }

    #[test]
    fn unknown_version_is_reported() {
        let (config, config_len) = encoded_config(&Config::default());
        let config = &config[..config_len];

        assert_eq!(Err(ConfigError::UnknownVersion(3)), decode(&[3], config));
        assert_eq!(Err(ConfigError::UnknownVersion(0)), decode(&[0], config));
    }

    #[test]
    fn unreadable_version_is_malformed() {
        let (config, config_len) = encoded_config(&Config::default());

        assert_eq!(
            Err(ConfigError::Malformed),
            decode(&[], &config[..config_len])
        );
    }

    #[test]
    fn truncated_config_is_malformed() {
        let (version, version_len) = encoded_version();
        let (config, config_len) = encoded_config(&worst_case_config());

        assert_eq!(
            Err(ConfigError::Malformed),
            decode(&version[..version_len], &config[..config_len - 1])
        );
    }

    #[test]
    fn enum_outside_its_variants_is_malformed() {
        let (version, version_len) = encoded_version();
        let (mut config, config_len) = encoded_config(&Config::default());
        // The first field is `BatteryStyle`, encoded as its variant index.
        config[0] = 0x7f;

        assert_eq!(
            Err(ConfigError::Malformed),
            decode(&version[..version_len], &config[..config_len])
        );
    }
}
