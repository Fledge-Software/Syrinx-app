//! The seam between compiling a plugin and Syrinx's compiled-code cache (#137). The cache
//! itself, kept beside Syrinx's store, is `owner::cache`; without one every activation compiles.

use std::fmt;
use std::io;

use wasmtime::Engine;
use wasmtime::component::Component;

/// Native code kept between sessions so a plugin activates without compiling again. Each
/// method reports its own problems as `PluginEvent::Cache`; none stops a plugin from loading.
pub(crate) trait CompiledCache: Send + Sync {
    /// The component compiled earlier from exactly these `plugin.wasm` bytes, on an engine
    /// like `engine` and this Syrinx; `None` on a miss. An entry it refuses is deleted.
    fn fetch(&self, engine: &Engine, plugin_id: &str, wasm: &[u8]) -> Option<Component>;

    /// Delete an entry `fetch` returned that failed a later check, and report why.
    fn refuse(&self, plugin_id: &str, refusal: Refusal);

    /// Keep a component just compiled from `wasm`, which passed every check.
    fn keep(&self, plugin_id: &str, wasm: &[u8], component: &Component);

    /// Whether an entry for these bytes is recorded under the current key. Its file is not
    /// read: `fetch` checks that when the plugin activates. Unknown counts as not current.
    #[cfg(feature = "store")]
    fn is_current(&self, plugin_id: &str, wasm: &[u8]) -> bool;
}

/// Why the cache could not be used or kept. The plugin is compiled instead; it never stops a
/// plugin from loading. No message carries a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheError {
    /// An entry was refused, for the reason given, and deleted with its record.
    Refused(Refusal),
    /// A file in the cache folder could not be read, written or removed.
    File(io::ErrorKind),
    /// Something other than a plain file is in the cache folder; it was left alone.
    NotAFile,
    /// The plugin id cannot name a cache file.
    Unnamed,
    /// The store could not read or keep a record.
    Store(String),
    /// SHA-256 could not be computed.
    Hash(String),
    /// wasmtime could not serialise the compiled code.
    Serialize(String),
}

/// Why a cached entry was refused and deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The record names a file that is not there.
    Missing,
    /// The file's length differs from the record's.
    Length,
    /// The file's SHA-256 differs from the record's.
    Hash,
    /// wasmtime refused the checked bytes, for example for another CPU's features.
    Deserialize(String),
    /// The deserialised component failed the identity or contract checks a compile passes.
    Checks(String),
    /// The entry was compiled from other bytes than the plugin's install record names; for
    /// example, once after an update recorded before the next start.
    OtherBytes,
}

/// How many components the runtime compiled from `plugin.wasm` and loaded from the cache; a
/// diagnostic, not part of the application's contract.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompileCounts {
    pub compiled: u64,
    pub loaded: u64,
}

impl fmt::Display for CacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(refusal) => write!(formatter, "a cached entry was refused: {refusal}"),
            Self::File(kind) => write!(formatter, "a cache file failed: {kind}"),
            Self::NotAFile => write!(formatter, "the cache folder holds something not a file"),
            Self::Unnamed => write!(formatter, "the plugin id cannot name a cache file"),
            Self::Store(error) => write!(formatter, "the cache's record failed: {error}"),
            Self::Hash(error) => write!(formatter, "hashing for the cache failed: {error}"),
            Self::Serialize(error) => write!(formatter, "compiled code was not kept: {error}"),
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => write!(formatter, "its file is missing"),
            Self::Length => write!(formatter, "its file has the wrong length"),
            Self::Hash => write!(formatter, "its file does not match its SHA-256"),
            Self::Deserialize(error) => write!(formatter, "wasmtime refused it: {error}"),
            Self::Checks(error) => write!(formatter, "it failed the plugin's checks: {error}"),
            Self::OtherBytes => write!(
                formatter,
                "it was compiled from other bytes than those installed"
            ),
        }
    }
}

impl std::error::Error for CacheError {}

impl From<io::Error> for CacheError {
    fn from(error: io::Error) -> Self {
        Self::File(error.kind())
    }
}
