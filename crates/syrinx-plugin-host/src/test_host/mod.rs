//! The test host: Syrinx's plugin host with synthetic capabilities, published
//! so plugin repositories run their contract tests against the real rules.
//!
//! It is a `Session` over synthetic services, so it keeps exactly the delivery
//! order and request rules the per-plugin workers keep.

mod synthetic;

use std::sync::Arc;

pub use synthetic::{SYNTHETIC_NOW, SyntheticHost, ViewRecord};

use crate::bindings::syrinx::plugin::host::LogLevel;
use crate::manifest::CapabilityKind;
pub use crate::session::{CallResult, Declarations, DeclaredCommand};
use crate::session::{Mediator, RequestIds, Session};
use crate::{LoadedPlugin, PreparedPlugin, Runtime};

/// The version the test host reports to `lifecycle.activate`.
pub const HOST_VERSION: &str = "0.1.0-test-host";

/// One loaded plugin under the test host.
pub type TestHost = Session<SyntheticHost>;

impl Session<SyntheticHost> {
    /// Check the declarations and the component, then instantiate it.
    pub fn load(bytes: &[u8], declarations: Declarations) -> CallResult<Self> {
        let runtime = Runtime::new()?;
        let prepared = Self::prepare(&runtime, bytes)?;
        Self::from_prepared(&runtime, &prepared, declarations)
    }

    /// Compile a component and resolve its imports once, to instantiate it many times.
    pub fn prepare(
        runtime: &Runtime,
        bytes: &[u8],
    ) -> CallResult<PreparedPlugin<Mediator<SyntheticHost>>> {
        let component = runtime.compile(bytes)?;
        LoadedPlugin::prepare(runtime, &component)
    }

    /// Instantiate a prepared component under checked declarations. The test host grants
    /// `storage`, so a plugin's settings can be tested; the owner's grants arrive with #136.
    pub fn from_prepared(
        runtime: &Runtime,
        prepared: &PreparedPlugin<Mediator<SyntheticHost>>,
        declarations: Declarations,
    ) -> CallResult<Self> {
        declarations.check()?;
        let services = Mediator::new(
            SyntheticHost::new(),
            declarations,
            vec![CapabilityKind::Storage],
        );
        let plugin = LoadedPlugin::from_prepared(runtime, prepared, services)?;
        Ok(Session::new(
            plugin,
            HOST_VERSION.into(),
            Arc::new(RequestIds::default()),
        ))
    }

    /// The view's interface reads what the plugin posted to it, oldest first;
    /// `None` when the view is not open.
    pub fn take_view_events(&mut self, view: u64) -> Option<Vec<String>> {
        let record = self.services_mut().views.get_mut(&view)?;
        Some(std::mem::take(&mut record.events))
    }

    pub fn logs(&self) -> &[(LogLevel, String)] {
        self.services().logs()
    }

    pub fn view(&self, view: u64) -> Option<&ViewRecord> {
        self.services().views.get(&view)
    }

    pub fn changed_feeds(&self) -> &[String] {
        &self.services().changed_feeds
    }
}
