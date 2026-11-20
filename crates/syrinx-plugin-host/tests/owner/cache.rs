//! The compiled-code cache under the runtime the application owns (#137): a warm entry is
//! loaded instead of compiled, every damaged or stale entry is refused and compiled afresh,
//! and install and removal keep the folder and the store's records level.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use syrinx_plugin_host::grants::DenyList;
use syrinx_plugin_host::owner::{CacheSetup, OwnerRuntime, Sha256};
use syrinx_plugin_host::worker::PluginEvent;
use syrinx_plugin_host::{CacheError, CompileCounts, Refusal};
use syrinx_store::{PluginCompiled, Store};

use super::{STORAGE, TempStore, call, close, install, registry};
use crate::support::{self, FULL};

const PLUGIN: &str = "cache-test";
const VERSION: &str = "0.1.0-cache";

/// Stands in for SHA-256, which the application passes from `syrinx-update` (a crate this one
/// may not depend on, even for tests): SipHash under four keys, so one flipped byte changes it.
fn digest(bytes: &[u8]) -> Result<[u8; 32], String> {
    let mut key = [0; 32];
    for (lane, chunk) in key.chunks_mut(8).enumerate() {
        let mut hasher = DefaultHasher::new();
        lane.hash(&mut hasher);
        bytes.hash(&mut hasher);
        chunk.copy_from_slice(&hasher.finish().to_le_bytes());
    }
    Ok(key)
}

/// The store and the cache folder beside its database, as the application lays them out.
struct Cached {
    temp: TempStore,
    folder: PathBuf,
}

impl Cached {
    fn new() -> Self {
        let temp = TempStore::new();
        let folder = temp.folder.join("PluginCache");
        fs::create_dir(&folder).expect("a synthetic cache folder");
        Self { temp, folder }
    }

    fn start(&self, store: &Arc<Store>, version: &str, sha256: Sha256) -> OwnerRuntime {
        let setup = CacheSetup {
            folder: self.folder.clone(),
            sha256,
        };
        OwnerRuntime::start(Arc::clone(store), version, DenyList::new(), Some(setup))
            .expect("starts")
    }

    fn entry(&self) -> PathBuf {
        self.folder.join(format!("{PLUGIN}.cwasm"))
    }

    /// The folder's file names, sorted.
    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.folder)
            .expect("the cache folder")
            .map(|entry| {
                let name = entry.expect("an entry").file_name();
                name.to_str().expect("a synthetic name").to_owned()
            })
            .collect();
        names.sort();
        names
    }
}

/// Load the installed plugin again, as at a start-up.
fn load(runtime: &mut OwnerRuntime) {
    let registry = registry(PLUGIN, STORAGE);
    let registered = registry.get(PLUGIN).expect("registered");
    runtime
        .load(registered, Arc::from(support::bytes(FULL)))
        .expect("loaded");
}

/// Activate the plugin with an empty call; it answers once activation is done.
fn activate(runtime: &mut OwnerRuntime) {
    assert_eq!(call(runtime, PLUGIN, |_session| Ok(())), Ok(()));
}

/// Install and activate the plugin on a fresh runtime, leaving a warm entry, then close.
fn warm(cached: &Cached) {
    let store = cached.temp.open();
    let mut runtime = cached.start(&store, VERSION, digest);
    install(&mut runtime, PLUGIN, STORAGE);
    activate(&mut runtime);
    assert_eq!(
        runtime.compile_counts(),
        CompileCounts {
            compiled: 1,
            loaded: 0
        },
        "the background compile and the activation compiled once between them"
    );
    assert!(runtime.take_events().is_empty());
    close(runtime, store);
}

/// The cache problems the runtime reported for the plugin.
fn cache_problems(runtime: &mut OwnerRuntime) -> Vec<CacheError> {
    runtime
        .take_events()
        .into_iter()
        .filter_map(|event| match event {
            PluginEvent::Cache {
                plugin: Some(plugin),
                error,
            } if plugin == PLUGIN => Some(error),
            _ => None,
        })
        .collect()
}

fn record(store: &Store) -> Option<PluginCompiled> {
    store.plugin_compiled(PLUGIN).expect("the record reads")
}

const COMPILED_ONCE: CompileCounts = CompileCounts {
    compiled: 1,
    loaded: 0,
};
const LOADED_ONCE: CompileCounts = CompileCounts {
    compiled: 0,
    loaded: 1,
};

#[test]
fn a_warm_entry_is_loaded_instead_of_compiled() {
    let cached = Cached::new();
    warm(&cached);
    let store = cached.temp.open();
    let kept = record(&store).expect("a record");
    assert_eq!(kept.syrinx_version, VERSION);
    let length = fs::metadata(cached.entry()).expect("the entry").len();
    assert_eq!(length, kept.artifact_bytes);

    let mut runtime = cached.start(&store, VERSION, digest);
    load(&mut runtime);
    activate(&mut runtime);
    assert_eq!(runtime.compile_counts(), LOADED_ONCE);
    assert!(runtime.take_events().is_empty());
    close(runtime, store);
}

/// What a test does to the warm entry's file before the next start.
enum Damage {
    FlipOneByte,
    Truncate,
    Delete,
    SwapForAnother,
}

fn damage(cached: &Cached, damage: Damage) {
    let path = cached.entry();
    let mut bytes = fs::read(&path).expect("the entry");
    match damage {
        Damage::FlipOneByte => {
            let middle = bytes.len() / 2;
            bytes[middle] ^= 0x01;
        }
        Damage::Truncate => bytes.truncate(bytes.len() - 1),
        Damage::Delete => {
            fs::remove_file(&path).expect("delete the entry");
            return;
        }
        // Different bytes of the same length: only the SHA-256 tells them apart.
        Damage::SwapForAnother => bytes
            .iter_mut()
            .for_each(|byte| *byte = byte.wrapping_add(1)),
    }
    fs::write(&path, bytes).expect("rewrite the entry");
}

/// Damage the warm entry, start again and activate: the entry is refused for `expected`, the
/// plugin compiles and works, and the rewritten entry then loads.
fn refused_and_recompiled(how: Damage, expected: Refusal) {
    let cached = Cached::new();
    warm(&cached);
    damage(&cached, how);

    let store = cached.temp.open();
    let mut runtime = cached.start(&store, VERSION, digest);
    load(&mut runtime);
    activate(&mut runtime);
    assert_eq!(runtime.compile_counts(), COMPILED_ONCE);
    assert_eq!(
        cache_problems(&mut runtime),
        [CacheError::Refused(expected)]
    );
    close(runtime, store);

    let store = cached.temp.open();
    let mut runtime = cached.start(&store, VERSION, digest);
    load(&mut runtime);
    activate(&mut runtime);
    assert_eq!(
        runtime.compile_counts(),
        LOADED_ONCE,
        "the entry was rewritten"
    );
    close(runtime, store);
}

#[test]
fn a_flipped_byte_is_refused_before_deserialising_and_the_plugin_recompiles() {
    refused_and_recompiled(Damage::FlipOneByte, Refusal::Hash);
}

#[test]
fn a_truncated_entry_is_refused_and_the_plugin_recompiles() {
    refused_and_recompiled(Damage::Truncate, Refusal::Length);
}

#[test]
fn a_record_whose_file_is_gone_recompiles() {
    refused_and_recompiled(Damage::Delete, Refusal::Missing);
}

#[test]
fn a_swapped_entry_is_refused_and_the_plugin_recompiles() {
    refused_and_recompiled(Damage::SwapForAnother, Refusal::Hash);
}

#[test]
fn an_entry_wasmtime_refuses_is_deleted_and_the_plugin_recompiles() {
    let cached = Cached::new();
    warm(&cached);
    // Bytes that match their record but are not native code at all.
    let synthetic = b"synthetic bytes, not compiled code".to_vec();
    fs::write(cached.entry(), &synthetic).expect("rewrite the entry");
    let store = cached.temp.open();
    let matching = PluginCompiled {
        artifact_sha256: digest(&synthetic).expect("a digest"),
        artifact_bytes: synthetic.len() as u64,
        ..record(&store).expect("a record")
    };
    store
        .save_plugin_compiled(matching)
        .expect("match the record");

    let mut runtime = cached.start(&store, VERSION, digest);
    load(&mut runtime);
    activate(&mut runtime);
    assert_eq!(runtime.compile_counts(), COMPILED_ONCE);
    let problems = cache_problems(&mut runtime);
    assert!(
        matches!(
            problems.as_slice(),
            [CacheError::Refused(Refusal::Deserialize(_))]
        ),
        "{problems:?}"
    );
    let rewritten = fs::read(cached.entry()).expect("the rewritten entry");
    assert_ne!(rewritten, synthetic);
    close(runtime, store);
}

#[test]
fn an_entry_from_another_engine_configuration_is_cleared_at_start_and_recompiled() {
    let cached = Cached::new();
    warm(&cached);
    let store = cached.temp.open();
    let other = PluginCompiled {
        engine_key: [0; 32],
        ..record(&store).expect("a record")
    };
    store
        .save_plugin_compiled(other)
        .expect("re-key the record");

    let mut runtime = cached.start(&store, VERSION, digest);
    assert_eq!(record(&store), None, "the stale record is gone");
    assert!(cached.files().is_empty(), "and its file");
    load(&mut runtime);
    activate(&mut runtime);
    assert_eq!(runtime.compile_counts(), COMPILED_ONCE);
    assert!(runtime.take_events().is_empty());
    assert!(record(&store).is_some(), "a current entry replaces it");
    close(runtime, store);
}

#[test]
fn a_new_syrinx_version_misses_and_recompiles() {
    let cached = Cached::new();
    warm(&cached);
    let store = cached.temp.open();
    let mut runtime = cached.start(&store, "0.2.0-cache", digest);
    assert_eq!(record(&store), None, "the old version's record is gone");
    load(&mut runtime);
    activate(&mut runtime);
    assert_eq!(runtime.compile_counts(), COMPILED_ONCE);
    let kept = record(&store).expect("a record");
    assert_eq!(kept.syrinx_version, "0.2.0-cache");
    close(runtime, store);
}

#[test]
fn removing_a_plugin_deletes_its_entry_and_record() {
    let cached = Cached::new();
    warm(&cached);
    let store = cached.temp.open();
    let mut runtime = cached.start(&store, VERSION, digest);
    load(&mut runtime);
    runtime.remove(PLUGIN).expect("removed");
    assert_eq!(record(&store), None);
    assert!(cached.files().is_empty());
    assert!(runtime.take_events().is_empty());
    close(runtime, store);
}

#[test]
fn start_up_removes_orphans_and_partial_files_and_keeps_current_entries() {
    let cached = Cached::new();
    warm(&cached);
    for orphan in ["gone.cwasm", "cache-test.cwasm.partial", "stray.txt"] {
        fs::write(cached.folder.join(orphan), b"synthetic").expect("an orphan");
    }
    fs::create_dir(cached.folder.join("subfolder")).expect("a synthetic subfolder");

    let store = cached.temp.open();
    let mut runtime = cached.start(&store, VERSION, digest);
    assert_eq!(cached.files(), ["cache-test.cwasm", "subfolder"]);
    let whole_cache: Vec<PluginEvent> = runtime.take_events();
    assert_eq!(
        whole_cache,
        [PluginEvent::Cache {
            plugin: None,
            error: CacheError::NotAFile
        }],
        "the subfolder is reported and left alone"
    );
    load(&mut runtime);
    activate(&mut runtime);
    assert_eq!(runtime.compile_counts(), LOADED_ONCE);
    close(runtime, store);
}

/// Whether `gated_digest` lets hashing through; only the test below hashes through it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Gate {
    /// Through, as while the runtime starts and keys its cache.
    Unarmed,
    Closed,
    Open,
}

static GATE: (Mutex<Gate>, Condvar) = (Mutex::new(Gate::Unarmed), Condvar::new());

/// `digest`, except while the gate is closed: it stands in for a compile that takes as long
/// as the test wants, since the background compile hashes before it can finish.
fn gated_digest(bytes: &[u8]) -> Result<[u8; 32], String> {
    let (gate, changed) = &GATE;
    let mut gate = gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    while *gate == Gate::Closed {
        gate = changed
            .wait(gate)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
    digest(bytes)
}

fn set_gate(state: Gate) {
    let (gate, changed) = &GATE;
    *gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = state;
    changed.notify_all();
}

#[test]
fn install_returns_while_the_background_compile_is_still_running() {
    let cached = Cached::new();
    let store = cached.temp.open();
    let mut runtime = cached.start(&store, VERSION, gated_digest);
    set_gate(Gate::Closed);
    let started = Instant::now();
    install(&mut runtime, PLUGIN, STORAGE);
    let took = started.elapsed();
    let unfinished = record(&store).is_none();
    // Open before asserting, so a failure cannot leave the compile, and shutdown, waiting.
    set_gate(Gate::Open);
    assert!(
        took < Duration::from_secs(5),
        "install waited {took:?} for the compile"
    );
    assert!(
        unfinished,
        "the compile had not finished when install returned"
    );
    activate(&mut runtime);
    assert_eq!(
        runtime.compile_counts(),
        COMPILED_ONCE,
        "the activation waited for the background compile instead of compiling again"
    );
    assert!(record(&store).is_some());
    close(runtime, store);
}
