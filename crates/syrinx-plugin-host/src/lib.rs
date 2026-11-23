//! Syrinx's plugin host: loads, links and runs WebAssembly plugins; the only crate that links wasmtime.

pub mod bindings;
mod cache;
mod checks;
pub mod deadline;
mod engine;
mod error;
pub mod grants;
mod identity;
mod instance;
pub mod limits;
pub mod manifest;
#[cfg(feature = "store")]
pub mod owner;
mod registry;
pub mod service;
pub mod session;
pub mod test_host;
mod ticker;
mod validate;
pub mod view_bridge;
pub mod views;
mod wasi;
pub mod worker;

pub use cache::{CacheError, Refusal};
pub use checks::{CONTRACT_TRACK, CONTRACT_VERSION};
pub use engine::{CompileCounts, Runtime};
pub use error::{Failure, HostError};
pub use identity::{IdentityMismatch, SignedIdentity, SignedManifest, check_signed};
pub use instance::{LoadedPlugin, PluginHost, PreparedPlugin, StoreData};
pub use registry::{PluginRegistry, RegisteredPlugin, RegistrationError};
pub use wasi::{OUTPUT_LIMIT_BYTES, OutputBuffer, PROVIDED_INTERFACES, WasiState};
