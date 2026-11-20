//! Syrinx's compiled-code cache (#137): each plugin's native code in a file in the cache
//! folder beside the database, and its key and SHA-256 in the store, checked before every load.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use syrinx_store::{MAX_PLUGIN_COMPILED_BYTES, PluginCompiled, Store, StoreError};
use wasmtime::Engine;
use wasmtime::component::Component;

use crate::cache::{CacheError, CompiledCache, Refusal};

/// SHA-256 as Syrinx computes it. The application passes `syrinx_update::sha256`, which this
/// crate may not depend on (`cargo xtask check-deps`).
pub type Sha256 = fn(&[u8]) -> Result<[u8; 32], String>;

/// What the application gives the runtime for its compiled-code cache.
#[derive(Clone)]
pub struct CacheSetup {
    /// The cache folder, which the application holds under its storage lease for as long as
    /// the runtime runs: local, never synced or roamed, and outside every backup.
    pub folder: PathBuf,
    pub sha256: Sha256,
}

/// The file extension of a cache entry; an entry being written carries `PARTIAL` after it.
const ENTRY: &str = "cwasm";
const PARTIAL: &str = "partial";

/// The compiled-code cache, keyed by the engine it was made for and this Syrinx's version.
pub(crate) struct StoreCache {
    store: Arc<Store>,
    folder: PathBuf,
    sha256: Sha256,
    engine_key: [u8; 32],
    syrinx_version: String,
}

impl StoreCache {
    /// A cache for code compiled on `engine` by this `syrinx_version`.
    pub(crate) fn new(
        store: Arc<Store>,
        setup: CacheSetup,
        engine: &Engine,
        syrinx_version: String,
    ) -> Result<Self, CacheError> {
        let engine_key = engine_key(engine, setup.sha256)?;
        Ok(Self {
            store,
            folder: setup.folder,
            sha256: setup.sha256,
            engine_key,
            syrinx_version,
        })
    }

    /// Delete every record made under another engine key or Syrinx version, with its file,
    /// then every file in the folder that no current record names, including partly written
    /// ones. A subfolder or link is reported and left alone.
    pub(crate) fn clean(&self) -> Vec<CacheError> {
        let mut problems = Vec::new();
        let records = match self.store.plugin_compiled_entries() {
            Ok(records) => records,
            Err(error) => return vec![store_error(error)],
        };
        let mut current = BTreeSet::new();
        for record in records {
            if self.is_current_key(&record) {
                current.insert(entry_name(&record.plugin_id));
                continue;
            }
            if let Err(error) = self.discard(&record.plugin_id) {
                problems.push(error);
            }
        }
        if let Err(error) = self.remove_unnamed_files(&current, &mut problems) {
            problems.push(error);
        }
        problems
    }

    /// Delete the plugin's record and file. Both are tried; the first failure is returned.
    pub(crate) fn discard(&self, plugin_id: &str) -> Result<(), CacheError> {
        let record = self
            .store
            .forget_plugin_compiled(plugin_id.to_owned())
            .map(drop)
            .map_err(store_error);
        let file = self
            .entry_path(plugin_id)
            .and_then(|path| remove_if_present(&path));
        record.and(file)
    }

    /// Remove each plain file whose name is not in `current`.
    fn remove_unnamed_files(
        &self,
        current: &BTreeSet<String>,
        problems: &mut Vec<CacheError>,
    ) -> Result<(), CacheError> {
        for entry in fs::read_dir(&self.folder)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if !kind.is_file() {
                problems.push(CacheError::NotAFile);
                continue;
            }
            let kept = entry
                .file_name()
                .to_str()
                .is_some_and(|name| current.contains(name));
            if !kept {
                remove_if_present(&entry.path())?;
            }
        }
        Ok(())
    }

    fn is_current_key(&self, record: &PluginCompiled) -> bool {
        record.engine_key == self.engine_key && record.syrinx_version == self.syrinx_version
    }

    /// The record for these exact bytes under the current key, if there is one.
    fn current_record(
        &self,
        plugin_id: &str,
        wasm: &[u8],
    ) -> Result<Option<PluginCompiled>, CacheError> {
        let Some(record) = self.store.plugin_compiled(plugin_id).map_err(store_error)? else {
            return Ok(None);
        };
        let module_sha256 = (self.sha256)(wasm).map_err(CacheError::Hash)?;
        let current = self.is_current_key(&record) && record.module_sha256 == module_sha256;
        Ok(current.then_some(record))
    }

    /// The current entry, checked against its record and deserialised; `None` on a miss.
    fn load(
        &self,
        engine: &Engine,
        plugin_id: &str,
        wasm: &[u8],
    ) -> Result<Option<Component>, CacheError> {
        let path = self.entry_path(plugin_id)?;
        let Some(record) = self.current_record(plugin_id, wasm)? else {
            return Ok(None);
        };
        let bytes = self.read_checked(&path, &record)?;
        // SAFETY: `bytes` is the in-memory copy whose length and SHA-256 were just checked
        // against the store's record of what this Syrinx serialised from a component it
        // compiled itself; no file mapping is involved, so a later change on disk cannot
        // reach it. wasmtime also refuses another engine's or CPU's code.
        let component = unsafe { Component::deserialize(engine, &bytes) }
            .map_err(|error| CacheError::Refused(Refusal::Deserialize(format!("{error:#}"))))?;
        Ok(Some(component))
    }

    /// The entry's bytes, read with the record's length as the bound, once they match it.
    fn read_checked(&self, path: &Path, record: &PluginCompiled) -> Result<Vec<u8>, CacheError> {
        let expected = record.artifact_bytes.min(MAX_PLUGIN_COMPILED_BYTES);
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(CacheError::Refused(Refusal::Missing));
            }
            Err(error) => return Err(error.into()),
        };
        // One byte past the record shows a file that is too long.
        let mut bytes = Vec::new();
        file.take(expected + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != record.artifact_bytes {
            return Err(CacheError::Refused(Refusal::Length));
        }
        let artifact_sha256 = (self.sha256)(&bytes).map_err(CacheError::Hash)?;
        if artifact_sha256 != record.artifact_sha256 {
            return Err(CacheError::Refused(Refusal::Hash));
        }
        Ok(bytes)
    }

    /// Record the artefact's key and SHA-256, then write its file.
    fn save(&self, plugin_id: &str, wasm: &[u8], component: &Component) -> Result<(), CacheError> {
        let path = self.entry_path(plugin_id)?;
        let bytes = component
            .serialize()
            .map_err(|error| CacheError::Serialize(format!("{error:#}")))?;
        let record = PluginCompiled {
            plugin_id: plugin_id.to_owned(),
            module_sha256: (self.sha256)(wasm).map_err(CacheError::Hash)?,
            engine_key: self.engine_key,
            syrinx_version: self.syrinx_version.clone(),
            artifact_sha256: (self.sha256)(&bytes).map_err(CacheError::Hash)?,
            artifact_bytes: bytes.len() as u64,
        };
        self.store
            .save_plugin_compiled(record)
            .map_err(store_error)?;
        write_replacing(&path, &bytes)
    }

    /// The entry's file, named from a plugin id that is one plain segment.
    fn entry_path(&self, plugin_id: &str) -> Result<PathBuf, CacheError> {
        if !is_plain_id(plugin_id) {
            return Err(CacheError::Unnamed);
        }
        Ok(self.folder.join(entry_name(plugin_id)))
    }
}

impl CompiledCache for StoreCache {
    fn fetch(
        &self,
        engine: &Engine,
        plugin_id: &str,
        wasm: &[u8],
        problems: &mut Vec<CacheError>,
    ) -> Option<Component> {
        let error = match self.load(engine, plugin_id, wasm) {
            Ok(found) => return found,
            Err(error) => error,
        };
        // Whatever failed, the entry goes, so the next activation compiles it afresh.
        problems.push(error);
        if let Err(error) = self.discard(plugin_id) {
            problems.push(error);
        }
        None
    }

    fn keep(&self, plugin_id: &str, wasm: &[u8], component: &Component) -> Result<(), CacheError> {
        self.save(plugin_id, wasm, component)
    }

    fn is_current(&self, plugin_id: &str, wasm: &[u8]) -> Result<bool, CacheError> {
        Ok(self.current_record(plugin_id, wasm)?.is_some())
    }
}

/// The SHA-256 of every byte the engine's compatibility hash writes. That hash covers the
/// target, Cranelift's flags and the CPU features it found, the tunables, the WebAssembly
/// features and wasmtime's version, so a change to any of them is a miss.
fn engine_key(engine: &Engine, sha256: Sha256) -> Result<[u8; 32], CacheError> {
    let mut written = Written(Vec::new());
    engine.precompile_compatibility_hash().hash(&mut written);
    sha256(&written.0).map_err(CacheError::Hash)
}

/// A `Hasher` that keeps what is written to it; its 64-bit `finish` is never the key.
struct Written(Vec<u8>);

impl Hasher for Written {
    fn write(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    fn finish(&self) -> u64 {
        self.0.len() as u64
    }
}

/// A plugin id usable as a file name: lowercase letters, digits, hyphens and underscores.
fn is_plain_id(plugin_id: &str) -> bool {
    let allowed =
        |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte);
    !plugin_id.is_empty() && plugin_id.len() <= 64 && plugin_id.bytes().all(allowed)
}

fn entry_name(plugin_id: &str) -> String {
    format!("{plugin_id}.{ENTRY}")
}

/// Write `bytes` to a partial file beside `path`, then rename it over `path`, so a reader
/// never sees half an entry. A partial file left by a failure is removed at the next start.
fn write_replacing(path: &Path, bytes: &[u8]) -> Result<(), CacheError> {
    let partial = path.with_extension(format!("{ENTRY}.{PARTIAL}"));
    let mut file = File::create(&partial)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&partial, path)?;
    Ok(())
}

/// Remove a file; one already gone is not an error.
fn remove_if_present(path: &Path) -> Result<(), CacheError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn store_error(error: StoreError) -> CacheError {
    CacheError::Store(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_plain_plugin_id_names_a_cache_file() {
        for id in ["cache-test", "a_b", "x1"] {
            assert!(is_plain_id(id), "{id}");
        }
        let long = "a".repeat(65);
        for id in ["", "..", "a.b", "a/b", "a\\b", "Cache", long.as_str()] {
            assert!(!is_plain_id(id), "{id}");
        }
    }

    #[test]
    fn the_engine_key_is_stable_for_one_configuration_and_follows_the_bytes_written() {
        let hash: Sha256 = |bytes| {
            let mut key = [0; 32];
            for (index, byte) in bytes.iter().enumerate() {
                key[index % 32] ^= byte.rotate_left(index as u32 % 8);
            }
            Ok(key)
        };
        let engine = crate::engine::configured_engine().expect("an engine");
        let again = crate::engine::configured_engine().expect("another engine");
        let first = engine_key(&engine, hash).expect("a key");
        assert_eq!(first, engine_key(&again, hash).expect("a key"));
        let mut other = wasmtime::Config::new();
        other.epoch_interruption(false);
        let different = Engine::new(&other).expect("a differently configured engine");
        assert_ne!(first, engine_key(&different, hash).expect("a key"));
    }
}
