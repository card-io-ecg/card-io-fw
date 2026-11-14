use config_site::data::{CENTRAL_EUROPE_RULE, UTC_RULE};
use embedded_menu::SelectValue;

macro_rules! implement_enum {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $enum_name:ident {
            $( $(#[$meta:meta])* $variant_name:ident = $value:literal, )*
        }
    ) => {
        $(#[$enum_meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, SelectValue, serde::Serialize, serde::Deserialize)]
        $vis enum $enum_name {
            $( $(#[$meta])* $variant_name = $value ),*
        }
    }
}

implement_enum! {
    pub enum DisplayBrightness {
        Dimmest = 0,
        Dim = 1,
        Normal = 2,
        Bright = 3,
        Brightest = 4,
    }
}
implement_enum! {
    pub enum FilterStrength {
        None = 0,
        Weak = 1,
        Strong = 2,
    }
}

implement_enum! {
    pub enum MeasurementAction {
        Ask = 0,
        Auto = 1,
        Store = 2,
        Upload = 3,
        Discard = 4,
    }
}

implement_enum! {
    pub enum LeadOffCurrent {
        Weak = 0,
        Normal = 1,
        Strong = 2,
        Strongest = 3,
    }
}

implement_enum! {
    pub enum LeadOffThreshold {
        _95 = 0,
        _92_5 = 1,
        _90 = 2,
        _87_5 = 3,
        _85 = 4,
        _80 = 5,
        _75 = 6,
        _70 = 7,
    }
}

implement_enum! {
    pub enum LeadOffFrequency {
        Dc = 0,
        Ac = 1,
    }
}

implement_enum! {
    pub enum Gain {
        X1 = 0,
        X2 = 1,
        X3 = 2,
        X4 = 3,
        X6 = 4,
        X8 = 5,
        X12 = 6,
    }
}

implement_enum! {
    /// When the menus start the station.
    pub enum MenuWifi {
        #[display_as("Off")]
        OnDemand = 0,
        #[display_as("On")]
        Always = 1,
    }
}

implement_enum! {
    /// What happens to a running station when a measurement starts.
    pub enum MeasurementWifi {
        #[display_as("On")]
        Keep = 0,
        #[display_as("Off")]
        Stop = 1,
    }
}

implement_enum! {
    /// Wi-Fi runs on battery only above this charge, in percent.
    pub enum MinBattery {
        #[display_as("10%")]
        _10 = 10,
        #[display_as("20%")]
        _20 = 20,
        #[display_as("30%")]
        _30 = 30,
        #[display_as("40%")]
        _40 = 40,
        #[display_as("50%")]
        _50 = 50,
        #[display_as("60%")]
        _60 = 60,
        #[display_as("70%")]
        _70 = 70,
        #[display_as("80%")]
        _80 = 80,
        #[display_as("90%")]
        _90 = 90,
    }
}

impl MinBattery {
    pub fn percentage(self) -> u8 {
        self as u8
    }
}

/// The hour and the minute of the local time at `unix_s`, or `None` for a time zone rule that the
/// firmware does not know.
pub fn local_hm(rule: &str, unix_s: u64) -> Option<(u8, u8)> {
    let unix_s = unix_s as i64;
    let offset_s = match rule {
        UTC_RULE => 0,
        CENTRAL_EUROPE_RULE if eu_summer_time(unix_s) => 2 * 3600,
        CENTRAL_EUROPE_RULE => 3600,
        _ => return None,
    };

    let of_day = (unix_s + offset_s).rem_euclid(SECONDS_PER_DAY);
    Some(((of_day / 3600) as u8, (of_day % 3600 / 60) as u8))
}

const SECONDS_PER_DAY: i64 = 86_400;

/// EU summer time runs from 01:00 UTC on the last Sunday of March to 01:00 UTC on the last
/// Sunday of October.
fn eu_summer_time(unix_s: i64) -> bool {
    let year = year_of_day(unix_s.div_euclid(SECONDS_PER_DAY));
    let switch = |month| last_sunday(year, month) * SECONDS_PER_DAY + 3600;

    (switch(3)..switch(10)).contains(&unix_s)
}

/// The day of the last Sunday of a month with 31 days, counted from 1970-01-01.
fn last_sunday(year: i64, month: i64) -> i64 {
    let last = days_from_civil(year, month, 31);
    // 1970-01-01 is a Thursday. Sunday is 0.
    last - (last + 4).rem_euclid(7)
}

// The two conversions are the `days_from_civil` and `civil_from_days` algorithms of Howard
// Hinnant, for the proleptic Gregorian calendar. Their years start on March 1.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

    era * 146_097 + day_of_era - 719_468
}

fn year_of_day(days: i64) -> i64 {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let march_based_month = (5 * day_of_year + 2) / 153;

    let year = era * 400 + year_of_era;
    // January and February belong to the next calendar year.
    if march_based_month >= 10 {
        year + 1
    } else {
        year
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // 2026-01-01T00:00:00Z
    const NEW_YEAR_2026: u64 = 1_767_225_600;
    // 2026-03-29T01:00:00Z and 2026-10-25T01:00:00Z
    const SUMMER_STARTS: u64 = 1_774_746_000;
    const SUMMER_ENDS: u64 = 1_792_890_000;

    fn central_europe(unix_s: u64) -> Option<(u8, u8)> {
        local_hm(CENTRAL_EUROPE_RULE, unix_s)
    }

    #[test]
    fn summer_time_starts_on_the_last_sunday_of_march() {
        assert_eq!(Some((1, 59)), central_europe(SUMMER_STARTS - 1));
        assert_eq!(Some((3, 0)), central_europe(SUMMER_STARTS));
    }

    #[test]
    fn summer_time_ends_on_the_last_sunday_of_october() {
        assert_eq!(Some((2, 59)), central_europe(SUMMER_ENDS - 1));
        assert_eq!(Some((2, 0)), central_europe(SUMMER_ENDS));
    }

    #[test]
    fn utc_has_no_summer_time() {
        assert_eq!(Some((1, 0)), local_hm(UTC_RULE, SUMMER_STARTS));
        assert_eq!(Some((1, 0)), local_hm(UTC_RULE, SUMMER_ENDS));
    }

    #[test]
    fn central_europe_goes_over_midnight() {
        assert_eq!(Some((0, 30)), central_europe(NEW_YEAR_2026 - 30 * 60));
    }

    #[test]
    fn an_unknown_rule_has_no_local_time() {
        assert_eq!(None, local_hm("EST5EDT,M3.2.0,M11.1.0", NEW_YEAR_2026));
    }

    #[test]
    fn year_of_day_changes_on_january_1() {
        let day = NEW_YEAR_2026 as i64 / SECONDS_PER_DAY;

        assert_eq!(2025, year_of_day(day - 1));
        assert_eq!(2026, year_of_day(day));
        assert_eq!(day, days_from_civil(2026, 1, 1));
    }
}
