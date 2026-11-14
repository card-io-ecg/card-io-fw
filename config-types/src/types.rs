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
