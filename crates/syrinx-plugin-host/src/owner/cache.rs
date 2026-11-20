//! Syrinx's compiled-code cache (#137): each plugin's native code in a file in the cache
//! folder beside the database, and its key and SHA-256 in the store, checked before every load.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Sender;

use syrinx_store::{MAX_PLUGIN_COMPILED_BYTES, PluginCompiled, Store, StoreError};
use wasmtime::Engine;
use wasmtime::component::Component;

use crate::cache::{CacheError, CompiledCache, Refusal};
use crate::worker::{PluginEvent, send_event};

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

/// A cache entry's file name is `PREFIX`, the plugin id and `.ENTRY`; one being written carries
/// `.PARTIAL` after that. The prefix keeps every name clear of Windows' reserved device names,
/// such as `nul` or `com1`, which a bare plugin id could otherwise be.
const PREFIX: &str = "plugin-";
const ENTRY: &str = "cwasm";
const PARTIAL: &str = "partial";

/// FIPS 180-2's first test vector: the SHA-256 of `abc`.
const ABC_SHA256: [u8; 32] = [
    0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23,
    0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad,
];

/// The compiled-code cache, keyed by the engine it was made for and this Syrinx's version.
/// Every problem it meets is sent as `PluginEvent::Cache` on `events`.
pub(crate) struct StoreCache {
    store: Arc<Store>,
    folder: PathBuf,
    sha256: Sha256,
    engine_key: [u8; 32],
    syrinx_version: String,
    events: Sender<PluginEvent>,
}

impl StoreCache {
    /// A cache for code compiled on `engine` by this `syrinx_version`. A `sha256` that does
    /// not compute SHA-256 is refused: tamper resistance rests on it.
    pub(crate) fn new(
        store: Arc<Store>,
        setup: CacheSetup,
        engine: &Engine,
        syrinx_version: String,
        events: Sender<PluginEvent>,
    ) -> Result<Self, CacheError> {
        if (setup.sha256)(b"abc").map_err(CacheError::Hash)? != ABC_SHA256 {
            return Err(CacheError::Hash("the function given is not SHA-256".into()));
        }
        let engine_key = engine_key(engine, setup.sha256)?;
        Ok(Self {
            store,
            folder: setup.folder,
            sha256: setup.sha256,
            engine_key,
            syrinx_version,
            events,
        })
    }
}

impl CompiledCache for StoreCache {
    fn fetch(&self, engine: &Engine, plugin_id: &str, wasm: &[u8]) -> Option<Component> {
        let error = match self.load(engine, plugin_id, wasm) {
            Ok(found) => return found,
            Err(error) => error,
        };
        // Only a refused entry goes; a busy store or a held file is a miss that keeps it.
        if let CacheError::Refused(refusal) = error {
            self.refuse(plugin_id, refusal);
        } else {
            self.report(Some(plugin_id), error);
        }
        None
    }

    fn refuse(&self, plugin_id: &str, refusal: Refusal) {
        self.report(Some(plugin_id), CacheError::Refused(refusal));
        self.delete_entry_or_report(plugin_id);
    }

    /// `save`, reporting a failure as `PluginEvent::Cache`.
    fn keep(&self, plugin_id: &str, wasm: &[u8], component: &Component) {
        if let Err(error) = self.save(plugin_id, wasm, component) {
            self.report(Some(plugin_id), error);
        }
    }

    fn is_current(&self, plugin_id: &str, wasm: &[u8]) -> bool {
        match self.current_record(plugin_id, wasm) {
            Ok(record) => record.is_some(),
            Err(error) => {
                // Unknown counts as not current: compiling costs only what a hit would save.
                self.report(Some(plugin_id), error);
                false
            }
        }
    }
}

/// What start-up clean-up does with one record.
enum Fate {
    /// Current: compiled under this key from the bytes the install record names.
    Keep,
    /// Under this key but from other bytes than the install record names: reported, deleted.
    OtherBytes,
    /// Under another key or for a plugin no longer installed: an expected miss, deleted.
    Stale,
}

impl StoreCache {
    /// Delete every record that is not current, with its file: one made under another engine
    /// key or Syrinx version, or for other bytes than the plugin's install records. Then
    /// delete every file in the folder no current record names, partly written ones included.
    /// A subfolder or link is reported and left alone.
    pub(crate) fn clean(&self) {
        let current = match self.clean_records() {
            Ok(current) => current,
            Err(error) => return self.report(None, error),
        };
        if let Err(error) = self.remove_unrecorded_files(&current) {
            self.report(None, error);
        }
    }

    /// Delete the plugin's record and file, reporting a failure; for an uninstall too.
    pub(crate) fn delete_entry_or_report(&self, plugin_id: &str) {
        if let Err(error) = self.delete_entry(plugin_id) {
            self.report(Some(plugin_id), error);
        }
    }

    // Clean-up.

    /// Delete the records that are not current; the file names of those that are.
    fn clean_records(&self) -> Result<BTreeSet<String>, CacheError> {
        let records = self.store.plugin_compiled_entries().map_err(store_error)?;
        let installed: BTreeMap<String, [u8; 32]> = self
            .store
            .plugin_installs()
            .map_err(store_error)?
            .into_iter()
            .map(|install| (install.plugin_id, install.module_sha256))
            .collect();
        let mut current = BTreeSet::new();
        for record in records {
            let plugin_id = record.plugin_id.as_str();
            match self.fate(&record, installed.get(plugin_id)) {
                Fate::Keep => {
                    current.insert(entry_name(plugin_id));
                }
                Fate::OtherBytes => {
                    let refused = CacheError::Refused(Refusal::OtherBytes);
                    self.report(Some(plugin_id), refused);
                    self.delete_entry_or_report(plugin_id);
                }
                Fate::Stale => self.delete_entry_or_report(plugin_id),
            }
        }
        Ok(current)
    }

    /// Whether clean-up keeps `record`, given the module hash its install record names.
    fn fate(&self, record: &PluginCompiled, installed: Option<&[u8; 32]>) -> Fate {
        let Some(installed) = installed else {
            return Fate::Stale;
        };
        if !self.is_current_key(record) {
            return Fate::Stale;
        }
        if *installed != record.module_sha256 {
            return Fate::OtherBytes;
        }
        Fate::Keep
    }

    /// Remove each plain file whose name is not in `current`; each failure is reported and
    /// the sweep goes on.
    fn remove_unrecorded_files(&self, current: &BTreeSet<String>) -> Result<(), CacheError> {
        for entry in fs::read_dir(&self.folder)? {
            let removed = entry.map_err(CacheError::from).and_then(|entry| {
                let kind = entry.file_type()?;
                if !kind.is_file() {
                    return Err(CacheError::NotAFile);
                }
                let kept = entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| current.contains(name));
                if kept {
                    return Ok(());
                }
                remove_if_present(&entry.path())
            });
            if let Err(error) = removed {
                self.report(None, error);
            }
        }
        Ok(())
    }

    // Loading.

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
        // The store already bounds the length; the `min` is a second guard on the read.
        let bound = record.artifact_bytes.min(MAX_PLUGIN_COMPILED_BYTES);
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Err(CacheError::Refused(Refusal::Missing));
            }
            Err(error) => return Err(error.into()),
        };
        // One byte past the record shows a file that is too long.
        let mut bytes = Vec::new();
        file.take(bound + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != record.artifact_bytes {
            return Err(CacheError::Refused(Refusal::Length));
        }
        let artifact_sha256 = (self.sha256)(&bytes).map_err(CacheError::Hash)?;
        if artifact_sha256 != record.artifact_sha256 {
            return Err(CacheError::Refused(Refusal::Hash));
        }
        Ok(bytes)
    }

    // Saving.

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

    // Shared.

    /// Delete the plugin's record and file. Both are tried; the first failure is returned.
    fn delete_entry(&self, plugin_id: &str) -> Result<(), CacheError> {
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

    fn report(&self, plugin_id: Option<&str>, error: CacheError) {
        let plugin = plugin_id.map(str::to_owned);
        send_event(&self.events, PluginEvent::Cache { plugin, error });
    }

    fn is_current_key(&self, record: &PluginCompiled) -> bool {
        record.engine_key == self.engine_key && record.syrinx_version == self.syrinx_version
    }

    /// The entry's file, named from a plugin id that is one plain segment.
    fn entry_path(&self, plugin_id: &str) -> Result<PathBuf, CacheError> {
        if !is_plain_id(plugin_id) {
            return Err(CacheError::Unnamed);
        }
        Ok(self.folder.join(entry_name(plugin_id)))
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
    format!("{PREFIX}{plugin_id}.{ENTRY}")
}

/// Write `bytes` to a fresh partial file beside `path`, then rename it over `path`, so a
/// reader never sees half an entry. Whatever stood at the partial name, even a link, is
/// removed first, and the new file is created exclusively, so nothing outside the folder is
/// written through it. A partial file left by a failure is removed at the next start.
fn write_replacing(path: &Path, bytes: &[u8]) -> Result<(), CacheError> {
    let partial = path.with_extension(format!("{ENTRY}.{PARTIAL}"));
    remove_if_present(&partial)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&partial, path)?;
    Ok(())
}

/// Remove a file, or a link without following it; one already gone is not an error.
fn remove_if_present(path: &Path) -> Result<(), CacheError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn store_error(error: StoreError) -> CacheError {
    CacheError::Store(error.to_string())
}

// Shared with the integration tests: the host may not depend on syrinx-update, even for tests.
#[cfg(test)]
#[path = "../../tests/support/sha256.rs"]
mod test_sha256;

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
    fn no_entry_name_is_a_windows_device_name() {
        for (id, name) in [
            ("nul", "plugin-nul.cwasm"),
            ("com1", "plugin-com1.cwasm"),
            ("lpt9", "plugin-lpt9.cwasm"),
        ] {
            assert_eq!(entry_name(id), name);
        }
    }

    #[test]
    fn the_engine_key_is_stable_for_one_configuration_and_follows_the_bytes_written() {
        let hash: Sha256 = test_sha256::sha256;
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
