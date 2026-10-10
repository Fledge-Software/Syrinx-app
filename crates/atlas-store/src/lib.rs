//! Persists Atlas state through a checked storage location and a serialized writer.

mod engine;
mod error;
mod location;
mod records;
mod schema;

pub use engine::{Store, StoredSnapshot};
pub use error::{EncodingField, StoreError, StoreOperation};
pub use location::{DirectoryLease, LocationError, LocationOperation};
