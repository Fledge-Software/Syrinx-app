//! The one wasmtime engine Syrinx configures, and compiling a component on it.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use wasmtime::component::Component;
use wasmtime::{Config, Engine, Strategy};

use crate::HostError;
use crate::cache::{CompiledCache, Refusal};
use crate::checks;
use crate::identity;
use crate::manifest::PluginManifest;
use crate::registry::RegisteredPlugin;
use crate::ticker::{EpochTicker, InFlight};

/// The engine every plugin runs on: Cranelift, epoch interruption, the default
/// (not pooling) allocator and stack, and none of wasmtime's own on-disk cache: Syrinx's
/// compiled-code cache (#137), when the runtime has one, is the only one.
/// Its one epoch ticker serves every plugin on the engine.
#[derive(Clone)]
pub struct Runtime {
    engine: Engine,
    ticker: Arc<EpochTicker>,
    cache: Option<Arc<dyn CompiledCache>>,
    counts: Arc<Counts>,
}

/// How many components the runtime compiled from `plugin.wasm` and loaded from the cache; a
/// diagnostic, not part of the application's contract.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompileCounts {
    pub compiled: u64,
    pub loaded: u64,
}

/// What `compile_counts` reports, shared by every clone of the runtime.
#[derive(Default)]
struct Counts {
    compiled: AtomicU64,
    loaded: AtomicU64,
}

/// The engine configuration every plugin compiles and runs under.
pub(crate) fn configured_engine() -> Result<Engine, HostError> {
    let mut config = Config::new();
    config.strategy(Strategy::Cranelift);
    config.epoch_interruption(true);
    Engine::new(&config).map_err(|error| HostError::Malformed(format!("{error:#}")))
}

impl Runtime {
    pub fn new() -> Result<Self, HostError> {
        Self::on(configured_engine()?)
    }

    /// A runtime on an engine built by `configured_engine`, with its own epoch ticker.
    pub(crate) fn on(engine: Engine) -> Result<Self, HostError> {
        let ticker = EpochTicker::start(&engine)
            .map_err(|error| HostError::Instantiation(format!("epoch ticker: {error}")))?;
        Ok(Self {
            engine,
            ticker: Arc::new(ticker),
            cache: None,
            counts: Arc::new(Counts::default()),
        })
    }

    /// Load and keep plugins' native code through `cache` when they activate.
    #[cfg(feature = "store")]
    pub(crate) fn with_cache(mut self, cache: Arc<dyn CompiledCache>) -> Self {
        self.cache = Some(cache);
        self
    }

    /// How many components this runtime and its clones compiled and loaded from the cache: a
    /// diagnostic for tests and measurements, not part of the application's contract.
    pub fn compile_counts(&self) -> CompileCounts {
        CompileCounts {
            compiled: self.counts.compiled.load(Ordering::Relaxed),
            loaded: self.counts.loaded.load(Ordering::Relaxed),
        }
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// How many times the epoch has advanced; it stays flat while every plugin is idle.
    pub fn epoch_ticks(&self) -> u64 {
        self.ticker.ticks()
    }

    /// Keep the epoch advancing while the returned guard lives.
    pub(crate) fn in_flight(&self) -> InFlight<'_> {
        self.ticker.in_flight()
    }

    /// Compile a component, refusing precompiled code and anything that does
    /// not fit `syrinx:plugin@0.1.x` before any of its code runs.
    pub fn compile(&self, bytes: &[u8]) -> Result<Component, HostError> {
        refuse_precompiled(bytes)?;
        let component = self.new_component(bytes)?;
        checks::component_fits(&self.engine, &component)?;
        Ok(component)
    }

    /// Compile a registered plugin's component when it loads or activates: its interface names
    /// must carry the manifest's world and import only the capabilities the manifest requests,
    /// and it must fit the contract. Loading or activating a package compiles it through here;
    /// registration never does, so discovery compiles nothing. Nothing is instantiated.
    pub fn compile_package(
        &self,
        plugin: &RegisteredPlugin,
        bytes: &[u8],
    ) -> Result<Component, HostError> {
        refuse_precompiled(bytes)?;
        self.compile_checked(plugin.manifest(), bytes)
    }

    /// Compile a plugin as `compile_package` does, through the runtime's cache when it has
    /// one: a current entry that passes every check is loaded instead of compiled, and a
    /// component compiled here is kept for next time. The cache reports its own problems;
    /// none stops the plugin.
    pub(crate) fn compile_cached(
        &self,
        manifest: &PluginManifest,
        bytes: &[u8],
    ) -> Result<Component, HostError> {
        refuse_precompiled(bytes)?;
        let Some(cache) = self.cache.as_deref() else {
            return self.compile_checked(manifest, bytes);
        };
        if let Some(component) = self.load_cached(cache, manifest, bytes) {
            return Ok(component);
        }
        let component = self.compile_checked(manifest, bytes)?;
        cache.keep(manifest.id(), bytes, &component);
        Ok(component)
    }

    /// Whether the plugin has no current cache entry, so compiling it ahead of activation
    /// saves its first activation the compile. Without a cache nothing is saved.
    #[cfg(feature = "store")]
    pub(crate) fn needs_compiling(&self, manifest: &PluginManifest, bytes: &[u8]) -> bool {
        self.cache
            .as_ref()
            .is_some_and(|cache| !cache.is_current(manifest.id(), bytes))
    }

    /// Whether plugins' native code is kept in a compiled-code cache.
    #[cfg(feature = "store")]
    pub(crate) fn has_cache(&self) -> bool {
        self.cache.is_some()
    }

    /// The cached component, once it passes the same checks as code compiled now; one that
    /// fails them is refused, so the plugin is compiled afresh.
    fn load_cached(
        &self,
        cache: &dyn CompiledCache,
        manifest: &PluginManifest,
        bytes: &[u8],
    ) -> Option<Component> {
        let component = cache.fetch(&self.engine, manifest.id(), bytes)?;
        if let Err(error) = self.check_component(manifest, &component) {
            cache.refuse(manifest.id(), Refusal::Checks(error.to_string()));
            return None;
        }
        self.counts.loaded.fetch_add(1, Ordering::Relaxed);
        Some(component)
    }

    /// Compile bytes already known not to be precompiled, then check them against the manifest.
    fn compile_checked(
        &self,
        manifest: &PluginManifest,
        bytes: &[u8],
    ) -> Result<Component, HostError> {
        let component = self.new_component(bytes)?;
        self.check_component(manifest, &component)?;
        Ok(component)
    }

    /// The registered manifest's identity, then the contract.
    fn check_component(
        &self,
        manifest: &PluginManifest,
        component: &Component,
    ) -> Result<(), HostError> {
        identity::check_component(&self.engine, component, manifest)
            .map_err(HostError::Identity)?;
        checks::component_fits(&self.engine, component)
    }

    /// Compile a component from bytes already known not to be precompiled.
    fn new_component(&self, bytes: &[u8]) -> Result<Component, HostError> {
        let component = Component::new(&self.engine, bytes)
            .map_err(|error| HostError::Malformed(format!("{error:#}")))?;
        self.counts.compiled.fetch_add(1, Ordering::Relaxed);
        Ok(component)
    }
}

/// A package's bytes are compiled by Syrinx itself: native code in them is refused.
fn refuse_precompiled(bytes: &[u8]) -> Result<(), HostError> {
    match Engine::detect_precompiled(bytes) {
        Some(_) => Err(HostError::Precompiled),
        None => Ok(()),
    }
}
