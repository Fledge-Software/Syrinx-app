//! The owner's grants as every gated call reads them: an in-memory copy of the store's grant
//! rows, so a worker thread never waits on the database to learn what it may do.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{PoisonError, RwLock};

use syrinx_store::StoredGrant;

use crate::grants::Grants;
use crate::manifest::CapabilityKind;

#[derive(Default)]
pub struct OwnerGrants(RwLock<BTreeMap<String, BTreeSet<CapabilityKind>>>);

impl OwnerGrants {
    /// The grants the store holds. A row naming a kind this Syrinx does not know grants nothing.
    pub(super) fn from_stored(rows: &[StoredGrant]) -> Self {
        let mut held: BTreeMap<String, BTreeSet<CapabilityKind>> = BTreeMap::new();
        for row in rows {
            if let Some(kind) = CapabilityKind::from_name(&row.grant.capability) {
                held.entry(row.plugin_id.clone()).or_default().insert(kind);
            }
        }
        Self(RwLock::new(held))
    }

    pub(super) fn insert(&self, plugin: &str, kind: CapabilityKind) {
        let mut held = self.0.write().unwrap_or_else(PoisonError::into_inner);
        held.entry(plugin.to_owned()).or_default().insert(kind);
    }

    pub(super) fn remove(&self, plugin: &str, kind: CapabilityKind) {
        let mut held = self.0.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(kinds) = held.get_mut(plugin) {
            kinds.remove(&kind);
        }
    }

    pub(super) fn forget(&self, plugin: &str) {
        let mut held = self.0.write().unwrap_or_else(PoisonError::into_inner);
        held.remove(plugin);
    }
}

impl Grants for OwnerGrants {
    fn is_granted(&self, plugin: &str, kind: CapabilityKind) -> bool {
        let held = self.0.read().unwrap_or_else(PoisonError::into_inner);
        held.get(plugin).is_some_and(|kinds| kinds.contains(&kind))
    }
}
