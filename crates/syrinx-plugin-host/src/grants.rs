//! What the owner granted each plugin, which the host checks on every gated call (#136), and
//! the fixed deny list no folder grant may break.

mod deny;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, PoisonError, RwLock};

pub use deny::{DenyList, FolderRefusal, Protected};

use crate::manifest::CapabilityKind;

/// The owner's current grants, shared by every clone. The host reads it on every call into a
/// gated service, so a revocation stops the next call, not only the next activation. The
/// application fills it from its store; the test host starts it empty.
#[derive(Clone, Default)]
pub struct GrantSet(Arc<RwLock<BTreeMap<String, BTreeSet<CapabilityKind>>>>);

impl GrantSet {
    /// Nothing granted, as for a freshly installed plugin.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn grant(&self, plugin: &str, kind: CapabilityKind) {
        let mut held = self.0.write().unwrap_or_else(PoisonError::into_inner);
        held.entry(plugin.to_owned()).or_default().insert(kind);
    }

    /// Withdraw a grant. A running instance's next gated call answers `revoked`.
    pub fn revoke(&self, plugin: &str, kind: CapabilityKind) {
        let mut held = self.0.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(kinds) = held.get_mut(plugin) {
            kinds.remove(&kind);
        }
    }

    /// Withdraw every grant `plugin` holds.
    pub fn forget(&self, plugin: &str) {
        let mut held = self.0.write().unwrap_or_else(PoisonError::into_inner);
        held.remove(plugin);
    }

    pub fn is_granted(&self, plugin: &str, kind: CapabilityKind) -> bool {
        let held = self.0.read().unwrap_or_else(PoisonError::into_inner);
        held.get(plugin).is_some_and(|kinds| kinds.contains(&kind))
    }

    /// The kinds `plugin` holds now, in `CapabilityKind`'s order.
    pub fn kinds(&self, plugin: &str) -> Vec<CapabilityKind> {
        let held = self.0.read().unwrap_or_else(PoisonError::into_inner);
        held.get(plugin)
            .map(|kinds| kinds.iter().copied().collect())
            .unwrap_or_default()
    }
}
