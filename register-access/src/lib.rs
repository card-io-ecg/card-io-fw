#![no_std]
#![feature(async_fn_in_trait)]
#![allow(incomplete_features)]

use device_descriptor::{ReadOnlyRegister, Register, RegisterWidthType};

pub trait RegisterReader<RWT>: Sized
where
    RWT: RegisterWidthType,
{
    fn read<E>(iface: &mut impl RegisterAccess<RWT, Error = E>) -> Result<Self, E>;
}

pub trait AsyncRegisterReader<RWT>: RegisterReader<RWT>
where
    RWT: RegisterWidthType,
{
    async fn read_async<E>(iface: &mut impl AsyncRegisterAccess<RWT, Error = E>)
        -> Result<Self, E>;
}

pub trait RegisterWriter<RWT>
where
    RWT: RegisterWidthType,
{
    fn write<E>(self, iface: &mut impl RegisterAccess<RWT, Error = E>) -> Result<(), E>;
}

pub trait AsyncRegisterWriter<RWT>: RegisterWriter<RWT>
where
    RWT: RegisterWidthType,
{
    async fn write_async<E>(
        self,
        iface: &mut impl AsyncRegisterAccess<RWT, Error = E>,
    ) -> Result<(), E>;
}

impl<T> RegisterReader<T::RegisterWidth> for T
where
    T: ReadOnlyRegister,
{
    fn read<E>(iface: &mut impl RegisterAccess<T::RegisterWidth, Error = E>) -> Result<Self, E> {
        iface.read_register()
    }
}

impl<T: ReadOnlyRegister> AsyncRegisterReader<T::RegisterWidth> for T {
    async fn read_async<E>(
        iface: &mut impl AsyncRegisterAccess<T::RegisterWidth, Error = E>,
    ) -> Result<Self, E> {
        iface.read_register_async().await
    }
}

impl<T: Register> RegisterWriter<T::RegisterWidth> for T {
    fn write<E>(
        self,
        iface: &mut impl RegisterAccess<T::RegisterWidth, Error = E>,
    ) -> Result<(), E> {
        iface.write_register(self)
    }
}

impl<T: Register> AsyncRegisterWriter<T::RegisterWidth> for T {
    async fn write_async<E>(
        self,
        iface: &mut impl AsyncRegisterAccess<T::RegisterWidth, Error = E>,
    ) -> Result<(), E> {
        iface.write_register_async(self).await
    }
}

pub trait RegisterAccess<RWT>
where
    RWT: RegisterWidthType,
{
    type Error;

    fn read_register<R>(&mut self) -> Result<R, Self::Error>
    where
        R: ReadOnlyRegister<RegisterWidth = RWT>;
    fn write_register<R>(&mut self, reg: R) -> Result<(), Self::Error>
    where
        R: Register<RegisterWidth = RWT>;

    fn read_sequential<R>(&mut self, buffer: &mut [u8]) -> Result<(), Self::Error>
    where
        R: ReadOnlyRegister<RegisterWidth = RWT>;
    fn write_sequential<R>(&mut self, bytes: &mut [u8]) -> Result<(), Self::Error>
    where
        R: Register<RegisterWidth = RWT>;
}

pub trait AsyncRegisterAccess<RWT>
where
    RWT: RegisterWidthType,
{
    type Error;

    async fn read_register_async<R>(&mut self) -> Result<R, Self::Error>
    where
        R: ReadOnlyRegister<RegisterWidth = RWT>;
    async fn write_register_async<R>(&mut self, reg: R) -> Result<(), Self::Error>
    where
        R: Register<RegisterWidth = RWT>;

    async fn read_sequential_async<R>(&mut self, buffer: &mut [u8]) -> Result<(), Self::Error>
    where
        R: ReadOnlyRegister<RegisterWidth = RWT>;
    async fn write_sequential_async<R>(&mut self, bytes: &mut [u8]) -> Result<(), Self::Error>
    where
        R: Register<RegisterWidth = RWT>;
}
