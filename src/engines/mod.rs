use bytes::Bytes;
use std::io::{self, Error, ErrorKind, Write};


/// 暂时：暴露接口，后续将改造成为一个engine 泛型，可容纳多个Engine
pub mod engine;

/// Bench Sled
pub mod sled;


/// Trait for a key value storage engine.
pub trait KvsEngine {
    /// Sets the value of a string key to a string.
    ///
    /// If the key already exists, the previous value will be overwritten.
    fn set(&mut self, key: Bytes, value: Bytes) -> io::Result<()>;

    /// Gets the string value of a given string key.
    ///
    /// Returns `None` if the given key does not exist.
    fn get(&mut self, key: Bytes) -> io::Result<Option<Bytes>>;

    /// Removes a given key.
    ///
    /// # Errors
    ///
    /// It returns `KvsError::KeyNotFound` if the given key is not found.
    fn remove(&mut self, key: Bytes) -> io::Result<()>;
}

pub use self::sled::SledKvsEngine;
