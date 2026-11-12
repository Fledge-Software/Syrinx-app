//! The one wasmtime engine Syrinx configures, and compiling a component on it.

use std::sync::Arc;

use wasmtime::component::Component;
use wasmtime::{Config, Engine, Strategy};

use crate::HostError;
use crate::checks;
use crate::identity;
use crate::manifest::PluginManifest;
use crate::registry::RegisteredPlugin;
use crate::ticker::{EpochTicker, InFlight};

/// The engine every plugin runs on: Cranelift, epoch interruption, the default
/// (not pooling) allocator and stack, and no on-disk cache (#137 adds Syrinx's own).
/// Its one epoch ticker serves every plugin on the engine.
#[derive(Clone)]
pub struct Runtime {
    engine: Engine,
    ticker: Arc<EpochTicker>,
}

impl Runtime {
    pub fn new() -> Result<Self, HostError> {
        let mut config = Config::new();
        config.strategy(Strategy::Cranelift);
        config.epoch_interruption(true);
        let engine =
            Engine::new(&config).map_err(|error| HostError::Malformed(format!("{error:#}")))?;
        let ticker = EpochTicker::start(&engine)
            .map_err(|error| HostError::Instantiation(format!("epoch ticker: {error}")))?;
        Ok(Self {
            engine,
            ticker: Arc::new(ticker),
        })
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
        let component = self.build_component(bytes)?;
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
        self.compile_manifest(plugin.manifest(), bytes)
    }

    /// `compile_package` for a manifest the caller kept from a registered plugin.
    pub(crate) fn compile_manifest(
        &self,
        manifest: &PluginManifest,
        bytes: &[u8],
    ) -> Result<Component, HostError> {
        let component = self.build_component(bytes)?;
        identity::check_component(&self.engine, &component, manifest)
            .map_err(HostError::Identity)?;
        checks::component_fits(&self.engine, &component)?;
        Ok(component)
    }

    /// Build a component from its bytes, refusing precompiled code first.
    fn build_component(&self, bytes: &[u8]) -> Result<Component, HostError> {
        if Engine::detect_precompiled(bytes).is_some() {
            return Err(HostError::Precompiled);
        }
        Component::new(&self.engine, bytes)
            .map_err(|error| HostError::Malformed(format!("{error:#}")))
    }
}
