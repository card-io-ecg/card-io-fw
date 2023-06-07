#![cfg_attr(not(test), no_std)]
#![feature(async_fn_in_trait)]
#![allow(incomplete_features)]

pub mod ll;
pub mod medium;

pub struct Storage<P> {
    partition: P,
}

enum ObjectKind {
    Header { first_data: u32, next_header: u32 },
    Data { next: u32 },
}

struct Object {
    state: u8,
    kind: ObjectKind,
}

pub struct Reader<'a, P> {
    storage: &'a mut Storage<P>,
    object: ObjectId,
    cursor: u32,
}

struct ObjectId {
    offset: u32,
}

impl<P> Storage<P> {
    pub fn new(partition: P) -> Self {
        Self { partition }
    }

    pub fn delete(&mut self, path: &str) -> Result<(), ()> {
        let object = self.lookup(path)?;
        self.delete_object(object)
    }

    pub fn store(&mut self, path: &str, data: &[u8]) -> Result<(), ()> {
        let object = self.lookup(path);

        let new_object = self.allocate_object(path)?;
        self.write_object(&new_object, data)?;
        self.finalize(new_object)?;

        if let Ok(object) = object {
            self.delete_object(object)?;
        }

        Ok(())
    }

    pub fn read(&mut self, path: &str) -> Result<Reader<'_, P>, ()> {
        let object = self.lookup(path)?;
        todo!()
    }

    fn lookup(&mut self, path: &str) -> Result<ObjectId, ()> {
        todo!()
    }

    fn delete_object(&mut self, object: ObjectId) -> Result<(), ()> {
        todo!()
    }

    fn allocate_object(&mut self, path: &str) -> Result<ObjectId, ()> {
        todo!()
    }

    fn write_object(&mut self, object: &ObjectId, data: &[u8]) -> Result<(), ()> {
        todo!()
    }

    fn finalize(&mut self, object: ObjectId) -> Result<(), ()> {
        todo!()
    }
}
