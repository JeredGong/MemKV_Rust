#![deny(missing_docs)]
//! A simple key/value store.

pub use kv::KvStore;
pub use client::KvsClient;
pub use error::{KvsError,Result};
pub use engines::engine;
pub use engines::{KvsEngine, SledKvsEngine};
pub use server::KvsServer;
/// Record模块
pub mod record;
/// Index模块
pub mod index;
/// Config模块
pub mod options;
/// engines模块
pub mod engines;


mod kv;
mod error;
mod common;
mod server;
mod client;