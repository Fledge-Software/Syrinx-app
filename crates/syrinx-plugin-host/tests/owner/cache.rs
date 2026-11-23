//! The compiled-code cache under the runtime the application owns (#137): a warm entry is
//! loaded instead of compiled, every damaged or stale entry is refused and compiled afresh,
//! and install and removal keep the folder and the store's records level.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use syrinx_plugin_host::grants::DenyList;
use syrinx_plugin_host::owner::{CacheSetup, OwnerRuntime, Sha256};
use syrinx_plugin_host::worker::PluginEvent;
use syrinx_plugin_host::{CacheError, CompileCounts, Refusal, Runtime};
use syrinx_store::{PluginCompiled, PluginInstall, Store};
use wasmtime::component::Component;

use super::{STORAGE, TempStore, call, close, install_record, registry};
use crate::support::sha256::sha256;
use crate::support::{self, FULL, variant};

const PLUGIN: &str = "cache-test";
const VERSION: &str = "0.1.0-cache";

const COMPILED_ONCE: CompileCounts = CompileCounts {
    compiled: 1,
    loaded: 0,
};
const LOADED_ONCE: CompileCounts = CompileCounts {
    compiled: 0,
    loaded: 1,
};

/// How long a test waits for a background compile before it fails.
const PATIENCE: Duration = Duration::from_secs(60);

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
        self.folder.join(format!("plugin-{PLUGIN}.cwasm"))
    }

    fn partial(&self) -> PathBuf {
        self.folder.join(format!("plugin-{PLUGIN}.cwasm.partial"))
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

fn full() -> Vec<u8> {
    support::bytes(FULL)
}

/// The install record for `bytes`, whose SHA-256 it names, as a verified install records it.
fn install_for(bytes: &[u8]) -> PluginInstall {
    PluginInstall {
        module_sha256: sha256(bytes).expect("a digest"),
        ..install_record(PLUGIN)
    }
}

/// Install the plugin with `bytes`, recording their SHA-256.
fn install_with(runtime: &mut OwnerRuntime, bytes: &[u8]) {
    let registry = registry(PLUGIN, STORAGE);
    let registered = registry.get(PLUGIN).expect("registered");
    runtime
        .install(registered, Arc::from(bytes), install_for(bytes))
        .expect("installed");
}

/// Load the installed plugin with `bytes`, as at a start-up.
fn load_with(runtime: &mut OwnerRuntime, bytes: &[u8]) {
    let registry = registry(PLUGIN, STORAGE);
    let registered = registry.get(PLUGIN).expect("registered");
    runtime.load(registered, Arc::from(bytes)).expect("loaded");
}

fn load(runtime: &mut OwnerRuntime) {
    load_with(runtime, &full());
}

/// Activate the plugin with an empty call; it answers once activation is done.
fn activate(runtime: &mut OwnerRuntime) {
    assert_eq!(call(runtime, PLUGIN, |_session| Ok(())), Ok(()));
}

/// Install and activate the plugin on a fresh runtime, leaving a warm entry, then close.
fn warm(cached: &Cached) {
    let store = cached.temp.open();
    let mut runtime = cached.start(&store, VERSION, sha256);
    install_with(&mut runtime, &full());
    activate(&mut runtime);
    assert_eq!(
        runtime.compile_counts(),
        COMPILED_ONCE,
        "the background compile and the activation compiled once between them"
    );
    assert!(runtime.take_events().is_empty());
    close(runtime, store);
}

/// The cache problems the runtime reported for the plugin.
fn cache_problems(runtime: &OwnerRuntime) -> Vec<CacheError> {
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

/// Poll until `done` holds, failing after `PATIENCE`.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !done() {
        assert!(Instant::now() < deadline, "{what} did not happen in time");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Wait until the background compile has kept a complete entry, without activating.
fn wait_for_entry(cached: &Cached, runtime: &OwnerRuntime, store: &Store) {
    wait_until("the background compile", || {
        runtime.compile_counts() == COMPILED_ONCE
            && record(store).is_some()
            && cached.entry().is_file()
    });
}

/// Activate on a fresh runtime and check that the entry is loaded, not compiled.
fn loads_after_restart(cached: &Cached, bytes: &[u8]) {
    let store = cached.temp.open();
    let mut runtime = cached.start(&store, VERSION, sha256);
    load_with(&mut runtime, bytes);
    activate(&mut runtime);
    assert_eq!(runtime.compile_counts(), LOADED_ONCE);
    assert!(runtime.take_events().is_empty());
    close(runtime, store);
}

#[test]
fn a_warm_entry_is_loaded_instead_of_compiled() {
    let cached = Cached::new();
    warm(&cached);
    let store = cached.temp.open();
    let kept = record(&store).expect("a record");
    assert_eq!(kept.syrinx_version, VERSION);
    let length = fs::metadata(cached.entry()).expect("the entry").len();
    assert_eq!(length, kept.artifact_bytes);

    drop(store);
    loads_after_restart(&cached, &full());
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
    let mut runtime = cached.start(&store, VERSION, sha256);
    load(&mut runtime);
    activate(&mut runtime);
    assert_eq!(runtime.compile_counts(), COMPILED_ONCE);
    assert_eq!(cache_problems(&runtime), [CacheError::Refused(expected)]);
    close(runtime, store);
    loads_after_restart(&cached, &full());
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
        artifact_sha256: sha256(&synthetic).expect("a digest"),
        artifact_bytes: synthetic.len() as u64,
        ..record(&store).expect("a record")
    };
    store
        .save_plugin_compiled(matching)
        .expect("match the record");

    let mut runtime = cached.start(&store, VERSION, sha256);
    load(&mut runtime);
    activate(&mut runtime);
    assert_eq!(runtime.compile_counts(), COMPILED_ONCE);
    let problems = cache_problems(&runtime);
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

    let mut runtime = cached.start(&store, VERSION, sha256);
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
    let mut runtime = cached.start(&store, "0.2.0-cache", sha256);
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
    let mut runtime = cached.start(&store, VERSION, sha256);
    load(&mut runtime);
    runtime.remove(PLUGIN).expect("removed");
    assert_eq!(record(&store), None);
    assert!(cached.files().is_empty());
    // No cache problem is reported; only the removal itself is announced (#141).
    let events = runtime.take_events();
    assert!(
        matches!(events.as_slice(), [PluginEvent::Removed { plugin }] if plugin == PLUGIN),
        "{events:?}"
    );
    close(runtime, store);
}

#[test]
fn start_up_removes_orphans_and_partial_files_and_keeps_current_entries() {
    let cached = Cached::new();
    warm(&cached);
    for orphan in [
        "plugin-gone.cwasm",
        "plugin-cache-test.cwasm.partial",
        "cache-test.cwasm",
        "stray.txt",
    ] {
        fs::write(cached.folder.join(orphan), b"synthetic").expect("an orphan");
    }
    fs::create_dir(cached.folder.join("subfolder")).expect("a synthetic subfolder");

    let store = cached.temp.open();
    let mut runtime = cached.start(&store, VERSION, sha256);
    assert_eq!(cached.files(), ["plugin-cache-test.cwasm", "subfolder"]);
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

/// Whether `gated_sha256` holds hashing back; only the test below hashes through it.
static GATE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

/// SHA-256, held back while the gate is closed, for at most `PATIENCE`: it stands in for a
/// compile that takes as long as the test wants, since the background compile hashes before
/// it can finish.
fn gated_sha256(bytes: &[u8]) -> Result<[u8; 32], String> {
    let (closed, changed) = &GATE;
    let deadline = Instant::now() + PATIENCE;
    let mut closed = closed
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    while *closed && Instant::now() < deadline {
        let left = deadline.saturating_duration_since(Instant::now());
        closed = changed
            .wait_timeout(closed, left)
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .0;
    }
    sha256(bytes)
}

/// Closes the gate until dropped, so a failing assertion cannot leave a compile waiting.
struct ClosedGate;

impl ClosedGate {
    fn close() -> Self {
        set_gate(true);
        Self
    }
}

impl Drop for ClosedGate {
    fn drop(&mut self) {
        set_gate(false);
    }
}

fn set_gate(closed: bool) {
    let (gate, changed) = &GATE;
    *gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = closed;
    changed.notify_all();
}

#[test]
fn install_returns_while_the_background_compile_is_still_running() {
    let cached = Cached::new();
    let store = cached.temp.open();
    let mut runtime = cached.start(&store, VERSION, gated_sha256);
    let gate = ClosedGate::close();
    let started = Instant::now();
    install_with(&mut runtime, &full());
    let took = started.elapsed();
    // An install that waited would wait for the gate's whole `PATIENCE`.
    assert!(
        took < PATIENCE / 2,
        "install waited {took:?} for the compile"
    );
    // The compile runs after install returned, and is held before it can keep its entry.
    wait_until("the background compile", || {
        runtime.compile_counts() == COMPILED_ONCE
    });
    assert_eq!(record(&store), None, "the entry waits behind the gate");
    drop(gate);
    activate(&mut runtime);
    assert_eq!(
        runtime.compile_counts(),
        COMPILED_ONCE,
        "the activation waited for the background compile instead of compiling again"
    );
    assert!(record(&store).is_some());
    close(runtime, store);
}

#[test]
fn install_compiles_in_the_background_without_an_activation() {
    let cached = Cached::new();
    let store = cached.temp.open();
    let mut runtime = cached.start(&store, VERSION, sha256);
    install_with(&mut runtime, &full());
    wait_for_entry(&cached, &runtime, &store);
    close(runtime, store);
    loads_after_restart(&cached, &full());
}

#[test]
fn loading_an_installed_plugin_with_no_entry_compiles_it_in_the_background() {
    let cached = Cached::new();
    let store = cached.temp.open();
    store
        .save_plugin_install(install_for(&full()))
        .expect("a bundled plugin's record");
    let mut runtime = cached.start(&store, VERSION, sha256);
    load(&mut runtime);
    wait_for_entry(&cached, &runtime, &store);
    activate(&mut runtime);
    assert_eq!(
        runtime.compile_counts(),
        COMPILED_ONCE,
        "the activation used the background compile"
    );
    close(runtime, store);
    loads_after_restart(&cached, &full());
}

/// `bytes` with a custom section appended: the same plugin, different bytes.
fn with_custom_section(mut bytes: Vec<u8>) -> Vec<u8> {
    let name = b"syrinx-cache-test";
    let payload = b"synthetic";
    let size = 1 + name.len() + payload.len();
    // Section id 0 is a custom section, which a component may carry anywhere.
    bytes.push(0);
    bytes.push(u8::try_from(size).expect("a one-byte size"));
    bytes.push(u8::try_from(name.len()).expect("a one-byte name length"));
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(payload);
    bytes
}

#[test]
fn other_bytes_for_the_same_plugin_miss_and_rewrite_the_record() {
    let cached = Cached::new();
    warm(&cached);
    let updated = with_custom_section(full());
    let store = cached.temp.open();
    let mut runtime = cached.start(&store, VERSION, sha256);
    load_with(&mut runtime, &updated);
    activate(&mut runtime);
    assert_eq!(runtime.compile_counts(), COMPILED_ONCE);
    let kept = record(&store).expect("a record");
    assert_eq!(kept.module_sha256, sha256(&updated).expect("a digest"));
    close(runtime, store);

    // Once the update is recorded, the next start loads the new bytes' entry.
    let store = cached.temp.open();
    store
        .save_plugin_install(install_for(&updated))
        .expect("record the update");
    drop(store);
    loads_after_restart(&cached, &updated);
}

#[test]
fn an_entry_for_other_bytes_than_the_install_records_is_cleared_at_start() {
    let cached = Cached::new();
    warm(&cached);
    let store = cached.temp.open();
    let updated = with_custom_section(full());
    store
        .save_plugin_install(install_for(&updated))
        .expect("record an update");
    let runtime = cached.start(&store, VERSION, sha256);
    assert_eq!(record(&store), None);
    assert!(cached.files().is_empty());
    assert_eq!(
        cache_problems(&runtime),
        [CacheError::Refused(Refusal::OtherBytes)],
        "a disagreement with the install record is reported, not silent"
    );
    close(runtime, store);
}

#[test]
fn an_entry_that_cannot_be_read_is_a_miss_that_is_never_discarded() {
    let cached = Cached::new();
    warm(&cached);
    // A folder where the entry belongs cannot be read or written as a file.
    fs::remove_file(cached.entry()).expect("remove the entry");
    fs::create_dir(cached.entry()).expect("a folder at the entry's name");

    let store = cached.temp.open();
    let mut runtime = cached.start(&store, VERSION, sha256);
    let whole_cache = runtime.take_events();
    assert_eq!(
        whole_cache,
        [PluginEvent::Cache {
            plugin: None,
            error: CacheError::NotAFile
        }]
    );
    load(&mut runtime);
    activate(&mut runtime);
    assert_eq!(runtime.compile_counts(), COMPILED_ONCE);
    // Exactly the read and the rename over the folder fail; discarding the entry would add a
    // third failure, removing the folder as a file.
    let problems = cache_problems(&runtime);
    assert!(
        matches!(
            problems.as_slice(),
            [CacheError::File(_), CacheError::File(_)]
        ),
        "{problems:?}"
    );
    assert!(cached.entry().is_dir(), "the folder is left alone");
    close(runtime, store);
}

#[test]
fn a_cached_component_that_fails_the_plugins_checks_is_refused_and_recompiled() {
    let cached = Cached::new();
    warm(&cached);
    // Another component's genuine code, under this plugin's record: it asks for a capability
    // the manifest does not request, so the identity check refuses it.
    let compiler = Runtime::new().expect("a runtime like the owner's");
    let other = Component::new(
        compiler.engine(),
        support::bytes(variant("imports-documents")),
    )
    .expect("the variant compiles")
    .serialize()
    .expect("it serialises");
    fs::write(cached.entry(), &other).expect("swap the entry");
    let store = cached.temp.open();
    let matching = PluginCompiled {
        artifact_sha256: sha256(&other).expect("a digest"),
        artifact_bytes: other.len() as u64,
        ..record(&store).expect("a record")
    };
    store
        .save_plugin_compiled(matching)
        .expect("match the record");

    let mut runtime = cached.start(&store, VERSION, sha256);
    load(&mut runtime);
    activate(&mut runtime);
    assert_eq!(runtime.compile_counts(), COMPILED_ONCE);
    let problems = cache_problems(&runtime);
    assert!(
        matches!(
            problems.as_slice(),
            [CacheError::Refused(Refusal::Checks(_))]
        ),
        "{problems:?}"
    );
    close(runtime, store);
    loads_after_restart(&cached, &full());
}

#[test]
fn a_function_that_is_not_sha256_turns_the_cache_off() {
    let cached = Cached::new();
    let store = cached.temp.open();
    let not_sha256: Sha256 = |_| Ok([7; 32]);
    let mut runtime = cached.start(&store, VERSION, not_sha256);
    let reported = runtime.take_events();
    assert!(
        matches!(
            reported.as_slice(),
            [PluginEvent::Cache {
                plugin: None,
                error: CacheError::Hash(_)
            }]
        ),
        "{reported:?}"
    );
    install_with(&mut runtime, &full());
    activate(&mut runtime);
    assert_eq!(runtime.compile_counts(), COMPILED_ONCE);
    assert_eq!(record(&store), None);
    assert!(cached.files().is_empty());
    close(runtime, store);
}

/// Start a cache, `plant` something at the partial file's name, then install and activate:
/// the entry is written fresh and the file outside the folder keeps its bytes.
fn planted_partial_not_written_through(plant: impl FnOnce(&std::path::Path, &std::path::Path)) {
    let cached = Cached::new();
    let outside = cached.temp.folder.join("outside.txt");
    fs::write(&outside, b"synthetic outside").expect("a file outside the cache");
    let store = cached.temp.open();
    let mut runtime = cached.start(&store, VERSION, sha256);
    plant(&outside, &cached.partial());
    install_with(&mut runtime, &full());
    activate(&mut runtime);
    assert_eq!(
        fs::read(&outside).expect("the outside file"),
        b"synthetic outside"
    );
    assert!(cached.entry().is_file());
    assert!(!cached.partial().exists());
    close(runtime, store);
    loads_after_restart(&cached, &full());
}

#[test]
fn a_hard_linked_partial_file_is_replaced_not_written_through() {
    planted_partial_not_written_through(|outside, partial| {
        fs::hard_link(outside, partial).expect("plant a hard link");
    });
}

#[cfg(unix)]
#[test]
fn a_symlinked_partial_file_is_replaced_not_followed() {
    planted_partial_not_written_through(|outside, partial| {
        std::os::unix::fs::symlink(outside, partial).expect("plant a symbolic link");
    });
}
