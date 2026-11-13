//! The test host's grant store: in memory, empty at first, so plugin repositories can test
//! what their plugin does when a capability is `denied` or `revoked`.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};

use crate::grants::Grants;
use crate::manifest::CapabilityKind;

/// Synthetic grants, shared by every clone, so a test keeps a handle while the host uses it.
#[derive(Clone, Default)]
pub struct SyntheticGrants(Arc<Mutex<BTreeSet<(String, CapabilityKind)>>>);

impl SyntheticGrants {
    /// Nothing is granted, as for a freshly installed plugin.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn grant(&self, plugin: &str, kind: CapabilityKind) {
        self.held().insert((plugin.to_owned(), kind));
    }

    /// Withdraw a grant. A running instance's next gated call answers `revoked`; under the
    /// test host, `TestHost::apply_revocations` then closes what the services opened under it.
    pub fn revoke(&self, plugin: &str, kind: CapabilityKind) {
        self.held().remove(&(plugin.to_owned(), kind));
    }

    fn held(&self) -> std::sync::MutexGuard<'_, BTreeSet<(String, CapabilityKind)>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Grants for SyntheticGrants {
    fn is_granted(&self, plugin: &str, kind: CapabilityKind) -> bool {
        self.held().contains(&(plugin.to_owned(), kind))
    }
}
