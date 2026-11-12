//! One instantiated plugin in its own store: every export call runs under a
//! deadline, and a trap discards the store for good.

use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Engine, Store, StoreLimits, StoreLimitsBuilder};

use crate::bindings::syrinx::plugin::{host, types};
use crate::bindings::{LinkOptions, Plugin, PluginPre};
use crate::deadline::{CallDeadline, Export};
use crate::wasi::{self, WasiState};
use crate::{Failure, HostError, Runtime, limits};

/// What Syrinx answers when a plugin calls one of its `syrinx:plugin` imports.
pub trait PluginHost: types::Host + host::Host + Send + 'static {
    /// An export is about to run until `deadline`. The epoch deadline cannot interrupt a
    /// host function the guest waits on, so a service that may block answers by then.
    fn begin_export(&mut self, _deadline: CallDeadline) {}

    /// The host closed `view`: the owner closed its tab, or the plugin refused to open it.
    fn view_closed(&mut self, _view: u64) {}
}

/// A store's data: the WASI state, the resource limits and the host's own state.
pub struct StoreData<H> {
    pub wasi: WasiState,
    limits: StoreLimits,
    pub host: H,
}

/// A component with its imports resolved once (`Linker::instantiate_pre`), ready to
/// instantiate again after a trap without compiling or linking again.
pub type PreparedPlugin<H> = PluginPre<StoreData<H>>;

/// One plugin instance. A store holds exactly one, because a trap poisons the
/// whole store (`docs/dev/vendor-reviews/wasmtime-wit-prebuild-check.md`).
pub struct LoadedPlugin<H: 'static> {
    runtime: Runtime,
    plugin: Plugin,
    store: Store<StoreData<H>>,
    failure: Option<Failure>,
    export_calls: u64,
}

impl<H: PluginHost> LoadedPlugin<H> {
    /// Link the WASI shim and the contract's imports, then instantiate under the limits.
    pub fn instantiate(
        runtime: &Runtime,
        component: &Component,
        host: H,
    ) -> Result<Self, HostError> {
        let prepared = Self::prepare(runtime, component)?;
        Self::from_prepared(runtime, &prepared, host)
    }

    /// Resolve the component's imports against the WASI shim and the contract once.
    pub fn prepare(
        runtime: &Runtime,
        component: &Component,
    ) -> Result<PreparedPlugin<H>, HostError> {
        let instantiation = |error: wasmtime::Error| HostError::Instantiation(format!("{error:#}"));
        let linker = linker::<H>(runtime.engine())?;
        let pre = linker.instantiate_pre(component).map_err(instantiation)?;
        PluginPre::new(pre).map_err(instantiation)
    }

    /// Instantiate a prepared component in a fresh store, under the activation deadline.
    pub fn from_prepared(
        runtime: &Runtime,
        prepared: &PreparedPlugin<H>,
        host: H,
    ) -> Result<Self, HostError> {
        let data = StoreData {
            wasi: WasiState::new(),
            limits: store_limits(),
            host,
        };
        let mut store = Store::new(runtime.engine(), data);
        store.limiter(|data| &mut data.limits);
        store.set_epoch_deadline(Export::Activate.deadline_ticks());
        let plugin = {
            let _in_flight = runtime.in_flight();
            prepared.instantiate(&mut store)
        }
        .map_err(|error| HostError::Instantiation(format!("{error:#}")))?;
        Ok(Self {
            runtime: runtime.clone(),
            plugin,
            store,
            failure: None,
            export_calls: 0,
        })
    }

    /// Call one export under its own deadline. A trap discards this instance.
    pub fn call<R>(
        &mut self,
        export: Export,
        call: impl FnOnce(&Plugin, &mut Store<StoreData<H>>) -> wasmtime::Result<R>,
    ) -> Result<R, HostError> {
        if self.failure.is_some() {
            return Err(HostError::Discarded);
        }
        self.export_calls += 1;
        self.store.set_epoch_deadline(export.deadline_ticks());
        self.store
            .data_mut()
            .host
            .begin_export(CallDeadline::starting_now(export));
        let result = {
            let _in_flight = self.runtime.in_flight();
            call(&self.plugin, &mut self.store)
        };
        result.map_err(|error| {
            let stopped = HostError::from_trap(&error);
            self.failure = Some(Failure::from_trap(export, &stopped));
            stopped
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
    pub fn discard(&mut self, failure: Failure) {
        self.failure.get_or_insert(failure);
    }

    pub fn is_discarded(&self) -> bool {
        self.failure.is_some()
    }

    /// Why the instance was stopped, once it has been.
    pub fn failure(&self) -> Option<&Failure> {
        self.failure.as_ref()
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
