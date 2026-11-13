//! The test host: Syrinx's plugin host with synthetic capabilities, published
//! so plugin repositories run their contract tests against the real rules.
//!
//! It is a `Session` over synthetic services, so it keeps exactly the delivery
//! order and request rules the per-plugin workers keep.

mod synthetic;

use std::sync::Arc;

/// The test host's grant store: the host's own `GrantSet`, empty at first, so plugin
/// repositories can test what their plugin does when a capability is `denied` or `revoked`.
pub use crate::grants::GrantSet as SyntheticGrants;
pub use crate::views::ViewRecord;
pub use synthetic::{SYNTHETIC_NOW, SyntheticHost};

use crate::bindings::syrinx::plugin::host::LogLevel;
pub use crate::session::{CallResult, Declarations, DeclaredCommand};
use crate::session::{Mediator, RequestIds, Session};
use crate::{LoadedPlugin, PreparedPlugin, Runtime};

/// The version the test host reports to `lifecycle.activate`.
pub const HOST_VERSION: &str = "0.1.0-test-host";

/// One loaded plugin under the test host.
pub type TestHost = Session<SyntheticHost>;

impl Session<SyntheticHost> {
    /// Check the declarations and the component, then instantiate it with nothing granted.
    pub fn load(bytes: &[u8], declarations: Declarations) -> CallResult<Self> {
        Self::load_with_grants(bytes, declarations, SyntheticGrants::new())
    }

    /// As `load`, under `grants`; keep a clone to grant or revoke while the plugin runs.
    pub fn load_with_grants(
        bytes: &[u8],
        declarations: Declarations,
        grants: SyntheticGrants,
    ) -> CallResult<Self> {
        let runtime = Runtime::new()?;
        let prepared = Self::prepare(&runtime, bytes)?;
        Self::from_prepared(&runtime, &prepared, declarations, grants)
    }

    /// Compile a component and resolve its imports once, to instantiate it many times.
    pub fn prepare(
        runtime: &Runtime,
        bytes: &[u8],
    ) -> CallResult<PreparedPlugin<Mediator<SyntheticHost>>> {
        let component = runtime.compile(bytes)?;
        LoadedPlugin::prepare(runtime, &component)
    }

    /// Instantiate a prepared component under checked declarations and synthetic grants,
    /// which the host checks on every gated call as it checks the owner's.
    pub fn from_prepared(
        runtime: &Runtime,
        prepared: &PreparedPlugin<Mediator<SyntheticHost>>,
        declarations: Declarations,
        grants: SyntheticGrants,
    ) -> CallResult<Self> {
        declarations.check()?;
        let services = Mediator::new(SyntheticHost::new(), declarations, grants);
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
        self.services_mut().book.take_view_events(view)
    }

    pub fn logs(&self) -> &[(LogLevel, String)] {
        self.services().logs()
    }

    pub fn view(&self, view: u64) -> Option<&ViewRecord> {
        self.services().book.view(view)
    }

    pub fn changed_feeds(&self) -> &[String] {
        self.services().book.changed_feeds()
    }
}
