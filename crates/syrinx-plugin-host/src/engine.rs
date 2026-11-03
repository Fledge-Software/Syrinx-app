//! The one wasmtime engine Syrinx configures, and compiling a component on it.

use wasmtime::component::Component;
use wasmtime::{Config, Engine, Strategy};

use crate::HostError;
use crate::checks;
use crate::identity::{self, SignedIdentity};
use crate::manifest::PluginManifest;

/// The engine every plugin runs on: Cranelift, epoch interruption, the default
/// (not pooling) allocator and stack, and no on-disk cache (#137 adds Syrinx's own).
#[derive(Clone)]
pub struct Runtime {
    engine: Engine,
}

impl Runtime {
    pub fn new() -> Result<Self, HostError> {
        let mut config = Config::new();
        config.strategy(Strategy::Cranelift);
        config.epoch_interruption(true);
        let engine =
            Engine::new(&config).map_err(|error| HostError::Malformed(format!("{error:#}")))?;
        Ok(Self { engine })
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Compile a component, refusing precompiled code and anything that does
    /// not fit `syrinx:plugin@0.1.x` before any of its code runs.
    pub fn compile(&self, bytes: &[u8]) -> Result<Component, HostError> {
        if Engine::detect_precompiled(bytes).is_some() {
            return Err(HostError::Precompiled);
        }
        let component = Component::new(&self.engine, bytes)
            .map_err(|error| HostError::Malformed(format!("{error:#}")))?;
        checks::component_fits(&self.engine, &component)?;
        Ok(component)
    }

    /// Compile a verified package's component after its signed lines, its manifest and its
    /// interface names agree; the identity checks run before the contract's own, so a
    /// disagreement is named as such. Nothing is instantiated.
    pub fn compile_package(
        &self,
        signed: &SignedIdentity,
        manifest: &PluginManifest,
        bytes: &[u8],
    ) -> Result<Component, HostError> {
        identity::check_signed(signed, manifest).map_err(HostError::Identity)?;
        if Engine::detect_precompiled(bytes).is_some() {
            return Err(HostError::Precompiled);
        }
        let component = Component::new(&self.engine, bytes)
            .map_err(|error| HostError::Malformed(format!("{error:#}")))?;
        identity::check_component(&self.engine, &component, manifest)
            .map_err(HostError::Identity)?;
        checks::component_fits(&self.engine, &component)?;
        Ok(component)
    }
}
