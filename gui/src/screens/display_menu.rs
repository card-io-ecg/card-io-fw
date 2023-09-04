use embedded_graphics::{prelude::DrawTarget, Drawable};
use embedded_io::asynch::{Read, Write};
use embedded_menu::{items::select::SelectValue, Menu, SelectValue};
use norfs::storable::{LoadError, Loadable, Storable};

use crate::widgets::battery_small::BatteryStyle;

#[derive(Clone, Copy)]
pub enum DisplayMenuEvents {
    Back,
}

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

impl SelectValue for BatteryStyle {
    fn next(&self) -> Self {
        match self {
            Self::MilliVolts => Self::Percentage,
            Self::Percentage => Self::Icon,
            Self::Icon => Self::LowIndicator,
            Self::LowIndicator => Self::MilliVolts,
        }
    }
    fn name(&self) -> &'static str {
        match self {
            Self::MilliVolts => "MilliVolts",
            Self::Percentage => "Percentage",
            Self::Icon => "Icon",
            Self::LowIndicator => "Indicator",
        }
    }
}

impl Loadable for BatteryStyle {
    async fn load<R: Read>(reader: &mut R) -> Result<Self, LoadError<R::Error>> {
        let data = match u8::load(reader).await? {
            0 => Self::MilliVolts,
            1 => Self::Percentage,
            2 => Self::Icon,
            3 => Self::LowIndicator,
            _ => return Err(LoadError::InvalidValue),
        };

        Ok(data)
    }
}

impl Storable for BatteryStyle {
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

#[derive(Clone, Copy, PartialEq, Eq, Menu)]
#[menu(
    title = "Display",
    navigation(events = DisplayMenuEvents),
    items = [
        data(label = "Brightness", field = brightness),
        data(label = "Battery", field = battery_display),
        data(label = "ECG Filter", field = filter_strength),
        navigation(label = "Back", event = DisplayMenuEvents::Back)
    ]
)]
pub struct DisplayMenu {
    pub brightness: DisplayBrightness,
    pub battery_display: BatteryStyle,
    pub filter_strength: FilterStrength,
}
