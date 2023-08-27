#![no_std]

use core::{convert::TryFrom, marker::PhantomData};

pub trait RegisterWidthType: Copy {
    const WIDTH: u8;

    fn from_32(data: u32) -> Self;
    fn to_32(self) -> u32;
}

impl RegisterWidthType for u8 {
    const WIDTH: u8 = 8;

    fn from_32(data: u32) -> Self {
        debug_assert!(data <= u8::MAX as u32);
        data as u8
    }

    fn to_32(self) -> u32 {
        self as u32
    }
}
impl RegisterWidthType for u16 {
    const WIDTH: u8 = 16;

    fn from_32(data: u32) -> Self {
        debug_assert!(data <= u16::MAX as u32);
        data as u16
    }

    fn to_32(self) -> u32 {
        self as u32
    }
}

pub trait ReaderProxy {
    type RegisterWidth: RegisterWidthType;

    fn bits(&self) -> Self::RegisterWidth;
    fn from_bits(bits: Self::RegisterWidth) -> Self;
}

pub trait ReadOnlyRegister: Copy + ReaderProxy {
    const ADDRESS: u8;
    const NAME: &'static str;

    fn value(&self) -> Self::RegisterWidth;
}

pub trait Register: ReadOnlyRegister {
    type Writer: WriterProxy<RegisterWidth = Self::RegisterWidth>;

    const DEFAULT_VALUE: Self::RegisterWidth;

    #[inline(always)]
    fn new(f: impl Fn(Self::Writer) -> Self::Writer) -> Self {
        Self::from_bits(f(Self::Writer::from_bits(Self::DEFAULT_VALUE)).bits())
    }

    #[inline(always)]
    fn modify(self, f: impl Fn(Self::Writer) -> Self::Writer) -> Self {
        Self::from_bits(f(Self::Writer::from_bits(self.value())).bits())
    }
}

pub trait WriterProxy: ReaderProxy {
    fn write_bits(self, bits: Self::RegisterWidth) -> Self;
    fn reset(self) -> Self;
}

pub struct Field<const POS: u8, const WIDTH: u8, DataType, Writer> {
    _marker: PhantomData<DataType>,
    reg: Writer,
}

impl<const POS: u8, const WIDTH: u8, DataType, P> Field<POS, WIDTH, DataType, P>
where
    P: ReaderProxy,
    DataType: TryFrom<P::RegisterWidth> + Into<P::RegisterWidth>,
{
    const _CONST_CHECK: () = assert!(POS + WIDTH <= <P::RegisterWidth as RegisterWidthType>::WIDTH);

    pub const fn new(reg: P) -> Self {
        Field {
            _marker: PhantomData,
            reg,
        }
    }

    #[inline(always)]
    pub fn read_field_bits(&self) -> P::RegisterWidth {
        <P::RegisterWidth as RegisterWidthType>::from_32(
            (self.reg.bits().to_32() >> POS as u32) & ((1 << WIDTH) - 1),
        )
    }

    #[inline(always)]
    pub fn read(&self) -> Option<DataType> {
        DataType::try_from(self.read_field_bits()).ok()
    }
}

impl<const POS: u8, const WIDTH: u8, DataType, P> Field<POS, WIDTH, DataType, P>
where
    P: WriterProxy,
    DataType: TryFrom<P::RegisterWidth> + Into<P::RegisterWidth>,
{
    #[inline(always)]
    fn write_field(data: P::RegisterWidth, value: P::RegisterWidth) -> P::RegisterWidth {
        // make sure value fits into field
        debug_assert!(value.to_32() <= ((1 << WIDTH) - 1));

        let shifted_mask = ((1 << WIDTH) - 1) << POS;
        let masked_field = data.to_32() & !shifted_mask;

        <P::RegisterWidth as RegisterWidthType>::from_32(
            masked_field | (value.to_32() << POS as u32),
        )
    }

    #[inline(always)]
    pub fn write(self, value: DataType) -> P {
        let bits = self.reg.bits();

        self.reg.write_bits(Self::write_field(bits, value.into()))
    }
}

#[macro_export]
macro_rules! impl_fields {
    () => {};

    ($($(#[$field_meta:meta])* $field:ident(pos = $pos:literal, width = $width:literal): $type:ty),*) => {
        $(
            $(#[$field_meta])*
            #[inline(always)]
            #[allow(non_snake_case)]
            pub fn $field(self) -> Field<$pos, $width, $type, Self> {
                Field::new(self)
            }
        )*
    };
}

#[macro_export]
macro_rules! define_register_type {
    ($rwt:ty, $type:ident {
        $(
            $( #[$variant_attr:meta] )*
            $name:ident = $value:expr
        ),+
    }) => {
        #[derive(Debug, PartialEq, Copy, Clone)]
        pub enum $type {
            $(
                $(#[$variant_attr])*
                $name = $value
            ),+
        }

        impl core::convert::TryFrom<$rwt> for $type {
            type Error = $rwt;

            fn try_from(data: $rwt) -> Result<Self, Self::Error> {
                match data {
                    $($value => Ok($type::$name)),+,
                    _ => Err(data)
                }
            }
        }

        impl From<$type> for $rwt {
            fn from(data: $type) -> $rwt {
                data as $rwt
            }
        }
    }
}

/// Specifying a default value for the register makes the register writeable.
#[macro_export]
macro_rules! register {
    ($(#[$meta:meta])* $reg:ident ($rwt:tt @ $addr:literal) {
        $($(#[$field_meta:meta])* $field:ident($($field_args:tt)*): $type:ty ),*
    } ) => {
        $(#[$meta])*
        #[derive(Debug, Copy, Clone)]
        #[must_use]
        #[allow(non_camel_case_types)]
        pub struct $reg {
            bits: $rwt
        }

        impl ReadOnlyRegister for $reg {
            const ADDRESS: u8 = $addr;
            const NAME: &'static str = stringify!($reg);

            #[inline(always)]
            fn value(&self) -> $rwt {
                self.bits
            }
        }

        impl ReaderProxy for $reg {
            type RegisterWidth = $rwt;

            #[inline(always)]
            fn from_bits(bits: $rwt) -> Self {
                Self { bits }
            }

            #[inline(always)]
            fn bits(&self) -> $rwt {
                self.bits
            }
        }

        impl $reg {
            $crate::impl_fields! { $($(#[$field_meta])* $field($($field_args)*): $type),* }
        }
    };

    ($(#[$meta:meta])* $reg:ident ($rwt:tt @ $addr:literal, default = $default:literal) {
        $($(#[$field_meta:meta])* $field:ident($($field_args:tt)*): $type:ty ),*
    } ) => {
        $crate::register!($(#[$meta])* $reg($rwt @ $addr) { $( $field($($field_args)*): $type ),* });

        impl Default for $reg {
            #[inline(always)]
            fn default() -> Self {
                Self::from_bits(Self::DEFAULT_VALUE)
            }
        }

        impl Register for $reg {
            type Writer = writer_proxies::$reg;

            const DEFAULT_VALUE: $rwt = $default;
        }

        impl writer_proxies::$reg {
            $crate::impl_fields! { $($(#[$field_meta])* $field($($field_args)*): $type),* }
        }
    };

    ($(#[$meta:meta])* $reg:ident ($rwt:tt @ $addr:literal $(,$($reg_args:tt)*)?) {
        $($(#[$field_meta:meta])* $field:ident($($field_args:tt)*): $type:ident $({
            $($field_type_tokens:tt)*
        })? ),*
    } ) => {
        $( // for each field
            $( // if field has embedded type definition
                $crate::define_register_type!(
                    $rwt,
                    $type {
                        $($field_type_tokens)*
                    }
                );
            )?
        )*

        $crate::register!($(#[$meta])* $reg ($rwt @ $addr $(,$($reg_args)*)?) { $( $field($($field_args)*): $type ),*} );
    };
}

/// This macro will only generate a writeable register if a default value is specified.
#[macro_export]
macro_rules! writer_proxy {
    ($(#[$meta:meta])* $reg:ident ($rwt:tt @ $_addr:literal)) => {};

    ($(#[$meta:meta])* $reg:ident ($rwt:tt @ $_addr:literal, default = $default:literal)) => {
        $(#[$meta])*
        #[allow(non_camel_case_types)]
        pub struct $reg {
            bits: $rwt,
        }

        impl ReaderProxy for $reg {
            type RegisterWidth = $rwt;

            #[inline(always)]
            fn from_bits(bits: $rwt) -> Self {
                Self { bits }
            }

            #[inline(always)]
            fn bits(&self) -> $rwt {
                self.bits
            }
        }

        impl WriterProxy for $reg {
            #[inline(always)]
            fn write_bits(self, bits: $rwt) -> Self {
                Self::from_bits(bits)
            }

            #[inline(always)]
            fn reset(self) -> Self {
                self.write_bits($default)
            }
        }
    };
}

#[macro_export]
macro_rules! device {
    (
        $(
            $(#[$meta:meta])*
            $reg:ident($($proto:tt)*) {
            $($fields:tt)*
        } )+
    ) => {

        mod writer_proxies {
            use device_descriptor::*;

            $(
                $crate::writer_proxy!($(#[$meta])* $reg($($proto)*) );
            )+
        }

        $(
            $crate::register!( $(#[$meta])* $reg($($proto)*) { $($fields)* } );
        )+
    }
}
