//! Cross-checks a package's three sources of identity before its component is instantiated:
//! the signed `package` list, the manifest, and the component's own interface names.

use std::fmt;

use wasmtime::Engine;
use wasmtime::component::Component;

use crate::checks::{CONTRACT_PACKAGE, REQUIRED_EXPORTS};
use crate::manifest::{CapabilityKind, PluginManifest};

/// The `id`, `version` and `world` lines of a verified `package` list. `syrinx-update` verifies
/// the signature; the application hands these lines across, since the host never links it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedIdentity {
    pub id: String,
    pub version: String,
    pub world: String,
}

/// Where the three sources disagree, or what the component imports without asking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityMismatch {
    Id {
        signed: String,
        manifest: String,
    },
    Version {
        signed: String,
        manifest: String,
    },
    World {
        signed: String,
        manifest: String,
    },
    /// The component's interface names carry another version than the manifest's world.
    ComponentWorld {
        manifest: String,
        component: Vec<String>,
    },
    /// The component imports a capability interface its manifest does not request.
    UnrequestedCapability {
        interface: String,
        kind: CapabilityKind,
    },
}

impl fmt::Display for IdentityMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Id { signed, manifest } => {
                write!(
                    formatter,
                    "the package is signed as {signed} but its manifest says {manifest}"
                )
            }
            Self::Version { signed, manifest } => write!(
                formatter,
                "the package is signed as version {signed} but its manifest says {manifest}"
            ),
            Self::World { signed, manifest } => write!(
                formatter,
                "the package is signed for {signed} but its manifest says {manifest}"
            ),
            Self::ComponentWorld {
                manifest,
                component,
            } => write!(
                formatter,
                "the manifest says {manifest} but the component exports {}",
                component.join(", ")
            ),
            Self::UnrequestedCapability { interface, kind } => write!(
                formatter,
                "the component imports {interface}, but its manifest does not request {kind}"
            ),
        }
    }
}

/// The signed lines and the manifest must name the same id, version and world.
pub fn check_signed(
    signed: &SignedIdentity,
    manifest: &PluginManifest,
) -> Result<(), IdentityMismatch> {
    if signed.id != manifest.id() {
        return Err(IdentityMismatch::Id {
            signed: signed.id.clone(),
            manifest: manifest.id().to_owned(),
        });
    }
    if signed.version != manifest.version() {
        return Err(IdentityMismatch::Version {
            signed: signed.version.clone(),
            manifest: manifest.version().to_owned(),
        });
    }
    if signed.world != manifest.world() {
        return Err(IdentityMismatch::World {
            signed: signed.world.clone(),
            manifest: manifest.world().to_owned(),
        });
    }
    Ok(())
}

/// The component, read with `component_type()` before instantiating, must export the
/// manifest's world version and import no capability interface the manifest left out.
pub fn check_component(
    engine: &Engine,
    component: &Component,
    manifest: &PluginManifest,
) -> Result<(), IdentityMismatch> {
    let component_type = component.component_type();
    let exports: Vec<&str> = component_type
        .exports(engine)
        .map(|(name, _)| name)
        .collect();
    check_exported_world(&exports, manifest)?;
    for (name, _) in component_type.imports(engine) {
        check_import(name, manifest)?;
    }
    Ok(())
}

/// Every required export must carry the version the manifest's world names.
fn check_exported_world(
    exports: &[&str],
    manifest: &PluginManifest,
) -> Result<(), IdentityMismatch> {
    let version = manifest
        .world()
        .rsplit_once('@')
        .map_or("", |(_, version)| version);
    let package = format!("{CONTRACT_PACKAGE}/");
    let contract: Vec<String> = exports
        .iter()
        .filter(|name| name.starts_with(&package))
        .map(|name| (*name).to_owned())
        .collect();
    let matches = REQUIRED_EXPORTS.iter().all(|interface| {
        let expected = format!("{package}{interface}@{version}");
        contract.contains(&expected)
    });
    if matches {
        return Ok(());
    }
    Err(IdentityMismatch::ComponentWorld {
        manifest: manifest.world().to_owned(),
        component: contract,
    })
}

/// A capability interface may be imported only when its kind is requested.
fn check_import(name: &str, manifest: &PluginManifest) -> Result<(), IdentityMismatch> {
    let Some(rest) = name.strip_prefix(&format!("{CONTRACT_PACKAGE}/")) else {
        return Ok(());
    };
    let interface = rest
        .split_once('@')
        .map_or(rest, |(interface, _)| interface);
    let Some(kind) = CapabilityKind::unlocking(interface) else {
        return Ok(());
    };
    if manifest.requests(kind) {
        return Ok(());
    }
    Err(IdentityMismatch::UnrequestedCapability {
        interface: name.to_owned(),
        kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::DiscoveryOptions;

    const MANIFEST: &str = r#"
schema = 1
[plugin]
id = "sample"
name = "Sample"
version = "1.2.3"
description = "A synthetic plugin."
authors = ["Example Author <author@example.invalid>"]
repository = "https://example.invalid/sample"
licence = "MPL-2.0"
min-syrinx = "0.1.0"
[interface]
world = "syrinx:plugin/plugin@0.1.0"
[entry]
module = "plugin.wasm"
"#;

    fn manifest(extra: &str) -> PluginManifest {
        let text = format!("{MANIFEST}{extra}");
        PluginManifest::parse(text.as_bytes(), DiscoveryOptions::default()).expect("valid")
    }

    fn signed(id: &str, version: &str, world: &str) -> SignedIdentity {
        SignedIdentity {
            id: id.into(),
            version: version.into(),
            world: world.into(),
        }
    }

    #[test]
    fn signed_lines_must_match_the_manifest() {
        let manifest = manifest("");
        let world = "syrinx:plugin/plugin@0.1.0";
        assert_eq!(
            check_signed(&signed("sample", "1.2.3", world), &manifest),
            Ok(())
        );
        assert!(matches!(
            check_signed(&signed("other", "1.2.3", world), &manifest),
            Err(IdentityMismatch::Id { .. })
        ));
        assert!(matches!(
            check_signed(&signed("sample", "1.2.4", world), &manifest),
            Err(IdentityMismatch::Version { .. })
        ));
        assert!(matches!(
            check_signed(
                &signed("sample", "1.2.3", "syrinx:plugin/plugin@0.2.0"),
                &manifest
            ),
            Err(IdentityMismatch::World { .. })
        ));
    }

    #[test]
    fn a_capability_interface_needs_its_kind_requested() {
        let without = manifest("");
        let import = "syrinx:plugin/documents@0.1.1";
        assert_eq!(
            check_import(import, &without),
            Err(IdentityMismatch::UnrequestedCapability {
                interface: import.into(),
                kind: CapabilityKind::Folder,
            })
        );
        let with = manifest(
            "[[capabilities]]\nkind = \"folder\"\naccess = \"read\"\nreason = \"Read synthetic notes.\"\n",
        );
        assert_eq!(check_import(import, &with), Ok(()));
        // The plain host interfaces and WASI need no capability.
        assert_eq!(check_import("syrinx:plugin/host@0.1.0", &without), Ok(()));
        assert_eq!(check_import("wasi:cli/stdout@0.2.9", &without), Ok(()));
    }
}
