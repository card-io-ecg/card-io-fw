pub mod network;

use network::WifiNetwork;

#[cfg(feature = "embedded")]
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};

#[cfg(feature = "std")]
use smol::lock::Mutex;

/// A POSIX TZ rule, in the format that libc reads from `TZ`.
pub type TimeZoneRule = heapless::String<48>;

pub const UTC_RULE: &str = "UTC0";
pub const CENTRAL_EUROPE_RULE: &str = "CET-1CEST,M3.5.0,M10.5.0/3";
/// The rules that the page offers.
pub const TIME_ZONE_RULES: [&str; 2] = [UTC_RULE, CENTRAL_EUROPE_RULE];

pub const DEFAULT_NTP_SERVER: &str = "pool.ntp.org";

/// The settings that the page changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub backend_url: heapless::String<64>,
    pub ntp_server: heapless::String<64>,
    pub time_zone: TimeZoneRule,
}

pub struct WebContext {
    pub known_networks: heapless::Vec<WifiNetwork, 8>,
    pub settings: Settings,
    settings_when_opened: Settings,
}

impl WebContext {
    /// Creates the context when setup opens. Its `settings` are the ones that
    /// `POST /settings/discard` restores.
    pub fn new(known_networks: heapless::Vec<WifiNetwork, 8>, settings: Settings) -> Self {
        Self {
            known_networks,
            settings_when_opened: settings.clone(),
            settings,
        }
    }

    pub fn discard_settings(&mut self) {
        self.settings.clone_from(&self.settings_when_opened);
    }
}

/// An empty name gives the default server.
pub fn parse_ntp_server(name: &str) -> Result<heapless::String<64>, &'static str> {
    let name = if name.is_empty() {
        DEFAULT_NTP_SERVER
    } else {
        name
    };

    let valid = name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-');

    valid
        .then(|| heapless::String::try_from(name).ok())
        .flatten()
        .ok_or("ntp")
}

/// Takes only one of `TIME_ZONE_RULES`.
pub fn parse_time_zone(rule: &str) -> Result<TimeZoneRule, &'static str> {
    TIME_ZONE_RULES
        .contains(&rule)
        .then(|| TimeZoneRule::try_from(rule).ok())
        .flatten()
        .ok_or("zone")
}

#[cfg(feature = "embedded")]
pub type SharedWebContext = Mutex<NoopRawMutex, WebContext>;

#[cfg(feature = "std")]
pub type SharedWebContext = Mutex<WebContext>;

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn empty_server_gives_the_default() {
        assert_eq!(Ok(DEFAULT_NTP_SERVER), parse_ntp_server("").as_deref());
    }

    #[test]
    fn server_name_is_checked() {
        assert_eq!(
            Ok("time.example-1.org"),
            parse_ntp_server("time.example-1.org").as_deref()
        );
        assert_eq!(Err("ntp"), parse_ntp_server("time example.org"));
        assert_eq!(Err("ntp"), parse_ntp_server("http://example.org"));
        assert_eq!(
            Err("ntp"),
            parse_ntp_server(core::str::from_utf8(&[b'a'; 65]).unwrap())
        );
    }

    #[test]
    fn only_offered_time_zones_are_taken() {
        assert_eq!(Ok(UTC_RULE), parse_time_zone(UTC_RULE).as_deref());
        assert_eq!(
            Ok(CENTRAL_EUROPE_RULE),
            parse_time_zone(CENTRAL_EUROPE_RULE).as_deref()
        );
        assert_eq!(Err("zone"), parse_time_zone("EST5EDT,M3.2.0,M11.1.0"));
        assert_eq!(Err("zone"), parse_time_zone(""));
    }
}
