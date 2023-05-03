#![no_std]
#![feature(async_fn_in_trait)]
#![allow(incomplete_features)]

use device_descriptor::{ReadOnlyRegister, Register};

pub trait RegisterReader: Sized {
    fn read<E>(iface: &mut impl RegisterAccess<Error = E>) -> Result<Self, E>;
}

pub trait AsyncRegisterReader: RegisterReader {
    async fn read_async<E>(iface: &mut impl AsyncRegisterAccess<Error = E>) -> Result<Self, E>;
}

pub trait RegisterWriter {
    fn write<E>(self, iface: &mut impl RegisterAccess<Error = E>) -> Result<(), E>;
}

pub trait AsyncRegisterWriter: RegisterWriter {
    async fn write_async<E>(self, iface: &mut impl AsyncRegisterAccess<Error = E>)
        -> Result<(), E>;
}

impl<T: ReadOnlyRegister<u8>> RegisterReader for T {
    fn read<E>(iface: &mut impl RegisterAccess<Error = E>) -> Result<Self, E> {
        iface.read_register()
    }
}

impl<T: ReadOnlyRegister<u8>> AsyncRegisterReader for T {
    async fn read_async<E>(iface: &mut impl AsyncRegisterAccess<Error = E>) -> Result<Self, E> {
        iface.read_register_async().await
    }
}

impl<T: Register<u8>> RegisterWriter for T {
    fn write<E>(self, iface: &mut impl RegisterAccess<Error = E>) -> Result<(), E> {
        iface.write_register(self)
    }
}

impl<T: Register<u8>> AsyncRegisterWriter for T {
    async fn write_async<E>(
        self,
        iface: &mut impl AsyncRegisterAccess<Error = E>,
    ) -> Result<(), E> {
        iface.write_register_async(self).await
    }
}

pub trait RegisterAccess {
    type Error;

    fn read_register<R: ReadOnlyRegister<u8>>(&mut self) -> Result<R, Self::Error>;
    fn write_register<R: Register<u8>>(&mut self, reg: R) -> Result<(), Self::Error>;

    fn read_sequential<R: ReadOnlyRegister<u8>>(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<(), Self::Error>;
    fn write_sequential<R: Register<u8>>(&mut self, bytes: &mut [u8]) -> Result<(), Self::Error>;
}

pub trait AsyncRegisterAccess {
    type Error;

    async fn read_register_async<R: ReadOnlyRegister<u8>>(&mut self) -> Result<R, Self::Error>;
    async fn write_register_async<R: Register<u8>>(&mut self, reg: R) -> Result<(), Self::Error>;

    async fn read_sequential_async<R: ReadOnlyRegister<u8>>(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<(), Self::Error>;
    async fn write_sequential_async<R: Register<u8>>(
        &mut self,
        bytes: &mut [u8],
    ) -> Result<(), Self::Error>;
}
