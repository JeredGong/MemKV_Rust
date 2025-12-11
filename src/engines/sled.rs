use super::KvsEngine;
use bytes::Bytes;
use sled::{Db, Tree};
use std::io;
use std::io::ErrorKind;

/// Wrapper of `sled::Db`
#[derive(Clone)]
pub struct SledKvsEngine(Db);

impl SledKvsEngine {
    /// Creates a `SledKvsEngine` from `sled::Db`.
    pub fn new(db: Db) -> Self {
        SledKvsEngine(db)
    }
}

impl KvsEngine for SledKvsEngine {
    fn set(&mut self, key: Bytes, value: Bytes) -> io::Result<()> {
        let tree: &Tree = &self.0;
        tree.insert(key.to_vec(), value.to_vec()).map(|_| ())?;
        tree.flush()?;
        Ok(())
    }

    fn get(&mut self, key: Bytes) -> io::Result<Option<Bytes>> {
        let tree: &Tree = &self.0;
        Ok(tree
            .get(key.to_vec())?
            .map(|ivec| Bytes::copy_from_slice(ivec.as_ref())))
    }

    fn remove(&mut self, key: Bytes) -> io::Result<()> {
        let tree: &Tree = &self.0;
        tree
            .remove(key.to_vec())?
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "Key not found"))?;
        tree.flush()?;
        Ok(())
    }
}
