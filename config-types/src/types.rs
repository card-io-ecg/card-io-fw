use embedded_io_async::{Read, Write};
use embedded_menu::SelectValue;
use norfs::storable::{LoadError, Loadable, Storable};

macro_rules! implement_enum {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $enum_name:ident {
            $( $(#[$meta:meta])* $variant_name:ident = $value:literal, )*
        }
    ) => {
        $(#[$enum_meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, SelectValue)]
        $vis enum $enum_name {
            $( $(#[$meta])* $variant_name = $value ),*
        }

        impl Loadable for $enum_name {
            async fn load<R: Read>(reader: &mut R) -> Result<Self, LoadError<R::Error>> {
                let data = match u8::load(reader).await? {
                    $( $value => Self::$variant_name, )*
                    _ => return Err(LoadError::InvalidValue),
                };

                Ok(data)
            }
        }

        impl Storable for $enum_name {
            async fn store<W: Write>(&self, writer: &mut W) -> Result<(), W::Error> {
                writer.write_all(&[*self as u8]).await
            }
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
