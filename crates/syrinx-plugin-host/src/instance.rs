//! One instantiated plugin in its own store: every export call runs under a
//! deadline, and a trap discards the store for good.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Engine, Store, StoreLimits, StoreLimitsBuilder};

use crate::bindings::syrinx::plugin::{host, types};
use crate::bindings::{LinkOptions, Plugin};
use crate::wasi::{self, WasiState};
use crate::{HostError, Runtime, limits};

/// What Syrinx answers when a plugin calls one of its `syrinx:plugin` imports.
pub trait PluginHost: types::Host + host::Host + Send + 'static {}

impl<H: types::Host + host::Host + Send + 'static> PluginHost for H {}

/// A store's data: the WASI state, the resource limits and the host's own state.
pub struct StoreData<H> {
    pub wasi: WasiState,
    limits: StoreLimits,
    pub host: H,
}

/// One plugin instance. A store holds exactly one, because a trap poisons the
/// whole store (`docs/dev/vendor-reviews/wasmtime-wit-prebuild-check.md`).
pub struct LoadedPlugin<H: 'static> {
    plugin: Plugin,
    store: Store<StoreData<H>>,
    discarded: bool,
    export_calls: u64,
}

impl<H: PluginHost> LoadedPlugin<H> {
    /// Link the WASI shim and the contract's imports, then instantiate under the limits.
    pub fn instantiate(
        runtime: &Runtime,
        component: &Component,
        host: H,
    ) -> Result<Self, HostError> {
        let linker = linker::<H>(runtime.engine())?;
        let data = StoreData {
            wasi: WasiState::new(),
            limits: store_limits(),
            host,
        };
        let mut store = Store::new(runtime.engine(), data);
        store.limiter(|data| &mut data.limits);
        store.set_epoch_deadline(limits::CALL_DEADLINE_TICKS);
        let plugin = {
            let _clock = EpochClock::start(runtime.engine());
            Plugin::instantiate(&mut store, component, &linker)
        }
        .map_err(|error| HostError::Instantiation(format!("{error:#}")))?;
        Ok(Self {
            plugin,
            store,
            discarded: false,
            export_calls: 0,
        })
    }

    /// Call one export under a fresh deadline. A trap discards this instance.
    pub fn call<R>(
        &mut self,
        export: impl FnOnce(&Plugin, &mut Store<StoreData<H>>) -> wasmtime::Result<R>,
    ) -> Result<R, HostError> {
        if self.discarded {
            return Err(HostError::Discarded);
        }
        self.export_calls += 1;
        self.store.set_epoch_deadline(limits::CALL_DEADLINE_TICKS);
        let result = {
            let _clock = EpochClock::start(self.store.engine());
            export(&self.plugin, &mut self.store)
        };
        result.map_err(|error| {
            self.discarded = true;
            HostError::from_trap(&error)
        })
    }

    pub fn host(&self) -> &H {
        &self.store.data().host
    }

    pub fn host_mut(&mut self) -> &mut H {
        &mut self.store.data_mut().host
    }

    pub fn wasi(&self) -> &WasiState {
        &self.store.data().wasi
    }

    /// Stop this instance for good, as a trap would.
    pub fn discard(&mut self) {
        self.discarded = true;
    }

    pub fn is_discarded(&self) -> bool {
        self.discarded
    }

    /// Export calls that reached the plugin, so a test can prove a refusal cost none.
    pub fn export_calls(&self) -> u64 {
        self.export_calls
    }
}

fn linker<H: PluginHost>(engine: &Engine) -> Result<Linker<StoreData<H>>, HostError> {
    let link_error = |error: wasmtime::Error| HostError::Instantiation(format!("{error:#}"));
    let mut linker = Linker::new(engine);
    wasi::add_to_linker(&mut linker, |data: &mut StoreData<H>| &mut data.wasi)
        .map_err(link_error)?;
    // Unstable interfaces (home widgets) are generated but never linked for a plugin.
    let options = LinkOptions::default();
    Plugin::add_to_linker::<_, HasSelf<H>>(&mut linker, &options, |data: &mut StoreData<H>| {
        &mut data.host
    })
    .map_err(link_error)?;
    Ok(linker)
}

fn store_limits() -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(limits::MEMORY_BYTES)
        .instances(limits::INSTANCES)
        .memories(limits::MEMORIES)
        .tables(limits::TABLES)
        .table_elements(limits::TABLE_ELEMENTS)
        .build()
}

/// Advances the engine's epoch only while a call is running, so an idle
/// Syrinx never wakes for it (D6). #135 replaces this with one shared ticker.
struct EpochClock {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl EpochClock {
    fn start(engine: &Engine) -> Self {
        let engine = engine.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !stop_flag.load(Ordering::Relaxed) {
                thread::park_timeout(limits::EPOCH_TICK);
                engine.increment_epoch();
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for EpochClock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let Some(thread) = self.thread.take() else {
            return;
        };
        thread.thread().unpark();
        if thread.join().is_err() {
            eprintln!("the plugin epoch clock thread panicked");
        }
    }
}

#[cfg(test)]
mod tests {
    use wasmtime::ResourceLimiter;

    use super::*;

    #[test]
    fn a_table_cannot_grow_past_its_element_limit() -> wasmtime::Result<()> {
        let mut limits = store_limits();
        assert!(limits.table_growing(0, limits::TABLE_ELEMENTS, None)?);
        assert!(!limits.table_growing(0, limits::TABLE_ELEMENTS + 1, None)?);
        assert!(!limits.table_growing(0, 10_000_000, None)?);
        Ok(())
    }
}
