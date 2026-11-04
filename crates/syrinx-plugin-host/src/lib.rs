//! Syrinx's plugin host: loads, links and runs WebAssembly plugins; the only crate that links wasmtime.

pub mod bindings;
mod checks;
mod engine;
mod error;
mod instance;
pub mod limits;
pub mod test_host;
mod validate;
mod wasi;

pub use checks::{CONTRACT_TRACK, CONTRACT_VERSION};
pub use engine::Runtime;
pub use error::HostError;
pub use instance::{LoadedPlugin, PluginHost, StoreData};
pub use wasi::{OUTPUT_LIMIT_BYTES, OutputBuffer, PROVIDED_INTERFACES, WasiState};
