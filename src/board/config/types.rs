use embedded_io::asynch::{Read, Write};
use embedded_menu::SelectValue;
use norfs::storable::{LoadError, Loadable, Storable};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, SelectValue)]
pub enum DisplayBrightness {
    Dimmest,
    Dim,
    Normal,
    Bright,
    Brightest,
}

impl Loadable for DisplayBrightness {
    async fn load<R: Read>(reader: &mut R) -> Result<Self, LoadError<R::Error>> {
        let data = match u8::load(reader).await? {
            0 => Self::Dimmest,
            1 => Self::Dim,
            2 => Self::Normal,
            3 => Self::Bright,
            4 => Self::Brightest,
            _ => return Err(LoadError::InvalidValue),
        };

        Ok(data)
    }
}

impl Storable for DisplayBrightness {
    async fn store<W: Write>(&self, writer: &mut W) -> Result<(), W::Error> {
        (*self as u8).store(writer).await
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, SelectValue)]
pub enum FilterStrength {
    None = 0,
    Weak = 1,
    Strong = 2,
}

impl Loadable for FilterStrength {
    async fn load<R: Read>(reader: &mut R) -> Result<Self, LoadError<R::Error>> {
        let data = match u8::load(reader).await? {
            0 => Self::None,
            1 => Self::Weak,
            2 => Self::Strong,
            _ => return Err(LoadError::InvalidValue),
        };

        Ok(data)
    }
}

impl Storable for FilterStrength {
    async fn store<W: Write>(&self, writer: &mut W) -> Result<(), W::Error> {
        writer.write_all(&[*self as u8]).await
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, SelectValue)]
pub enum MeasurementAction {
    Ask = 0,
    Auto = 1,
    Store = 2,
    Upload = 3,
    Discard = 4,
}

impl Loadable for MeasurementAction {
    async fn load<R: Read>(reader: &mut R) -> Result<Self, LoadError<R::Error>> {
        let data = match u8::load(reader).await? {
            0 => Self::Ask,
            1 => Self::Auto,
            2 => Self::Store,
            3 => Self::Upload,
            4 => Self::Discard,
            _ => return Err(LoadError::InvalidValue),
        };

        Ok(data)
    }
}

impl Storable for MeasurementAction {
    async fn store<W: Write>(&self, writer: &mut W) -> Result<(), W::Error> {
        writer.write_all(&[*self as u8]).await
    }
}
