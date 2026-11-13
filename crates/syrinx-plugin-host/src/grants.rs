//! What the owner granted each plugin, which the host checks on every gated call (#136), and
//! the fixed deny list no folder grant may break.

mod deny;

pub use deny::{DenyList, FolderRefusal, Protected};

use crate::manifest::CapabilityKind;

/// The owner's current grants. The host asks on every call into a gated service, so a
/// revocation stops the next call, not only the next activation.
pub trait Grants: Send + Sync {
    fn is_granted(&self, plugin: &str, kind: CapabilityKind) -> bool;
}

/// The kinds `plugin` holds now, in `CapabilityKind::ALL`'s order.
pub fn granted_kinds(grants: &dyn Grants, plugin: &str) -> Vec<CapabilityKind> {
    CapabilityKind::ALL
        .into_iter()
        .filter(|kind| grants.is_granted(plugin, *kind))
        .collect()
}
