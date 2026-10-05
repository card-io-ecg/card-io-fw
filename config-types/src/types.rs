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
