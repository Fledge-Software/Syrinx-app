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
        let component = self.build_component(bytes)?;
        checks::component_fits(&self.engine, &component)?;
        Ok(component)
    }

    /// Load a verified package: its signed lines, its manifest and its component's interface
    /// names must agree, and the component must fit the contract. Every package load goes
    /// through here, and only its result can be registered, so a plugin's commands appear
    /// only after these checks pass. The identity checks run before the contract's own, so a
    /// disagreement is named as such. Nothing is instantiated.
    pub fn compile_package(
        &self,
        signed: &SignedIdentity,
        manifest: PluginManifest,
        bytes: &[u8],
    ) -> Result<CheckedPackage, HostError> {
        identity::check_signed(signed, &manifest).map_err(HostError::Identity)?;
        let component = self.build_component(bytes)?;
        identity::check_component(&self.engine, &component, &manifest)
            .map_err(HostError::Identity)?;
        checks::component_fits(&self.engine, &component)?;
        Ok(CheckedPackage {
            manifest,
            component,
        })
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

/// A package that passed every load check: compiled, never instantiated. Only
/// `Runtime::compile_package` makes one, and only one can be registered.
#[derive(Clone)]
pub struct CheckedPackage {
    manifest: PluginManifest,
    component: Component,
}

impl std::fmt::Debug for CheckedPackage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CheckedPackage")
            .field("manifest", &self.manifest)
            .finish_non_exhaustive()
    }
}

impl CheckedPackage {
    /// The checked manifest.
    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    /// The compiled component, which activation (2a) instantiates.
    pub fn component(&self) -> &Component {
        &self.component
    }

    /// Split into the manifest and the component.
    pub(crate) fn into_parts(self) -> (PluginManifest, Component) {
        (self.manifest, self.component)
    }
}
