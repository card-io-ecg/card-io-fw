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

pub trait ReadOnlyRegister<RWT: RegisterWidthType>: Proxy<RWT> + Copy {
    const ADDRESS: u8;
    const NAME: &'static str;
}

pub trait Register<RWT: RegisterWidthType>: ReadOnlyRegister<RWT> {
    type Writer: WriterProxy<RWT>;

    const DEFAULT_VALUE: RWT;

    fn new(f: impl Fn(Self::Writer) -> Self::Writer) -> Self;
    fn modify(self, f: impl Fn(Self::Writer) -> Self::Writer) -> Self;
}

pub trait Proxy<RWT: RegisterWidthType> {
    fn bits(&self) -> RWT;
    fn from_bits(bits: RWT) -> Self;
}

pub trait WriterProxy<RWT: RegisterWidthType>: Proxy<RWT> {
    fn write_bits(self, bits: RWT) -> Self;
    fn reset(self) -> Self;
}

pub struct Field<const POS: u8, const WIDTH: u8, DataType, Writer, RWT> {
    _marker: PhantomData<(DataType, RWT)>,
    reg: Writer,
}

impl<const POS: u8, const WIDTH: u8, DataType, P, RWT> Field<POS, WIDTH, DataType, P, RWT>
where
    DataType: TryFrom<RWT> + Into<RWT>,
    P: Proxy<RWT>,
    RWT: RegisterWidthType,
{
    const _CONST_CHECK: () = assert!(POS + WIDTH <= RWT::WIDTH);

    pub const fn new(reg: P) -> Self {
        Field {
            _marker: PhantomData,
            reg,
        }
    }

    #[inline(always)]
    pub fn read_field_bits(&self) -> RWT {
        RWT::from_32((self.reg.bits().to_32() >> POS as u32) & ((1 << WIDTH) - 1))
    }

    #[inline(always)]
    pub fn read(&self) -> Option<DataType> {
        DataType::try_from(self.read_field_bits()).ok()
    }
}

impl<const POS: u8, const WIDTH: u8, DataType, P, RWT> Field<POS, WIDTH, DataType, P, RWT>
where
    DataType: TryFrom<RWT> + Into<RWT>,
    P: WriterProxy<RWT>,
    RWT: RegisterWidthType,
{
    #[inline(always)]
    fn write_field(data: RWT, value: RWT) -> RWT {
        // make sure value fits into field
        debug_assert!(value.to_32() <= ((1 << WIDTH) - 1));

        let shifted_mask = ((1 << WIDTH) - 1) << POS;
        let masked_field = data.to_32() & !shifted_mask;

        RWT::from_32(masked_field | (value.to_32() << POS as u32))
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

    ($($field:ident($rwt:ty, pos = $pos:literal, width = $width:literal): $type:ty),*) => {
        $(
            #[inline(always)]
            #[allow(non_snake_case)]
            pub fn $field(self) -> Field<$pos, $width, $type, Self, $rwt> {
                Field::new(self)
            }
        )*
    };
}

#[macro_export]
macro_rules! register {
    ($reg:ident ($rwt:ty, addr = $addr:literal) {
        $( $field:ident(pos = $pos:literal, width = $width:literal): $type:ty ),*
    } ) => {
        impl ReadOnlyRegister<$rwt> for $reg {
            const ADDRESS: u8 = $addr;
            const NAME: &'static str = stringify!($reg);
        }

        impl Proxy<$rwt> for $reg {
            #[inline(always)]
            fn from_bits(bits: $rwt) -> Self {
                Self { value: bits }
            }

            #[inline(always)]
            fn bits(&self) -> $rwt {
                self.value
            }
        }

        #[derive(Debug, Copy, Clone)]
        #[must_use]
        #[allow(non_camel_case_types)]
        pub struct $reg {
            value: $rwt
        }

        impl $reg {
            $crate::impl_fields! { $($field($rwt, pos = $pos, width = $width): $type),* }
        }
    };

    ($reg:ident ($rwt:ty, addr = $addr:literal, default = $default:literal) {
        $( $field:ident(pos = $pos:literal, width = $width:literal): $type:ty ),*
    } ) => {

        $crate::register!($reg($rwt, addr=$addr) { $( $field(pos = $pos, width = $width): $type ),* });

        impl Default for $reg {
            #[inline(always)]
            fn default() -> Self {
                Self::from_bits(Self::DEFAULT_VALUE)
            }
        }

        impl Register<$rwt> for $reg {
            type Writer = writer_proxies::$reg;

            const DEFAULT_VALUE: $rwt = $default;

            #[inline(always)]
            fn new(f: impl Fn(Self::Writer) -> Self::Writer) -> Self {
                Self::from_bits(
                    f(Self::Writer::from_bits(Self::DEFAULT_VALUE)).bits()
                )
            }

            #[inline(always)]
            fn modify(self, f: impl Fn(Self::Writer) -> Self::Writer) -> Self {
                Self::from_bits(
                    f(Self::Writer::from_bits(self.value)).bits()
                )
            }
        }

        impl writer_proxies::$reg {
            $crate::impl_fields! { $($field($rwt, pos = $pos, width = $width): $type),* }
        }
    };

    ($reg:ident $proto:tt {
        $( $field:ident(pos = $pos:literal, width = $width:literal): $type:ident $({
            $( $name:ident = $value:expr),+
        })? ),*
    } ) => {
        $( $(
            #[derive(Debug, PartialEq, Copy, Clone)]
            pub enum $type {
                $($name = $value),+
            }

            impl core::convert::TryFrom<u8> for $type {
                type Error = u8;

                fn try_from(data: u8) -> Result<Self, Self::Error> {
                    match data {
                        $($value => Ok($type::$name)),+,
                        _ => Err(data)
                    }
                }
            }

            impl From<$type> for u8 {
                fn from(data: $type) -> u8 {
                    data as u8
                }
            }
        )? )*
        $crate::register!($reg $proto { $( $field(pos = $pos, width = $width): $type ),*} );
    };
}

#[macro_export]
macro_rules! writer_proxy {
    ($reg:ident ($rwt:ty, addr = $addr:literal) {
        $( $field:ident(pos = $pos:literal, width = $width:literal): $type:ty ),*
    } ) => {};

    ($reg:ident ($rwt:ty, addr = $addr:literal, default = $default:literal) {
        $( $field:ident(pos = $pos:literal, width = $width:literal): $type:ty ),*
    } ) => {
        #[allow(non_camel_case_types)]
        pub struct $reg {
            bits: $rwt
        }

        impl Proxy<$rwt> for $reg {
            #[inline(always)]
            fn from_bits(bits: $rwt) -> Self {
                Self {
                    bits
                }
            }

            #[inline(always)]
            fn bits(&self) -> $rwt {
                self.bits
            }
        }

        impl WriterProxy<$rwt> for $reg {
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

    ($reg:ident $proto:tt {
        $( $field:ident(pos = $pos:literal, width = $width:literal): $type:ident $({
            $( $name:ident = $value:expr),+
        })? ),*
    } ) => {
        $crate::writer_proxy!($reg $proto { $( $field(pos = $pos, width = $width): $type ),*} );
    };
}

#[macro_export]
macro_rules! device {
    (
        $( $reg:ident($($proto:tt)*) {
            $($fields:tt)*
        } )+
    ) => {

        mod writer_proxies {
            use device_descriptor::*;

            $(
                $crate::writer_proxy!($reg($($proto)*) { $($fields)* } );
            )+
        }

        $(
            $crate::register!($reg($($proto)*) { $($fields)* } );
        )+
    }
}
