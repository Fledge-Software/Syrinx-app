//! Cross-checks a package's three sources of identity before its component is instantiated:
//! `check_signed` compares the signed `package` lines with the manifest, and
//! `check_component` compares the component's own interface names with the manifest.

use std::fmt;

use wasmtime::Engine;
use wasmtime::component::Component;

use crate::checks::{CONTRACT_PACKAGE, REQUIRED_EXPORTS};
use crate::manifest::{CapabilityKind, PluginManifest};

/// The `id`, `version` and `world` lines of a verified `package` list. `syrinx-update` verifies
/// the signature; the application hands these lines across, since the host never links it.
#[derive(Debug, PartialEq, Eq)]
pub struct SignedIdentity {
    id: String,
    version: String,
    world: String,
}

impl SignedIdentity {
    /// The one constructor, and the one glue point between the verifier and the host. Call it
    /// only with the `id`, `version` and `world` lines of a `syrinx_update::VerifiedPluginPackage`,
    /// after `verify_plugin_package` accepted its signature, slot and version (#140 wires it).
    /// The host cannot check where the lines came from.
    pub fn from_signed_lines(id: &str, version: &str, world: &str) -> Self {
        Self {
            id: id.to_owned(),
            version: version.to_owned(),
            world: world.to_owned(),
        }
    }

    /// The signed plugin id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The signed version, `major.minor.patch`.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The signed world, such as `syrinx:plugin/plugin@0.1.0`.
    pub fn world(&self) -> &str {
        &self.world
    }
}

/// Where the three sources disagree, or what the component imports without asking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityMismatch {
    // The signed lines against the manifest, from `check_signed`.
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
    // The component against the manifest, from `Runtime::compile_package`.
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
    /// The component imports `watch`, but its `folder` request does not set `watch = true`.
    UnrequestedWatch {
        interface: String,
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
            Self::UnrequestedWatch { interface } => write!(
                formatter,
                "the component imports {interface}, but its folder request does not set watch = true"
            ),
        }
    }
}

/// A manifest whose package's signed lines name the same id, version and world. Only
/// `check_signed` makes one, and only one can be registered.
#[derive(Debug, PartialEq, Eq)]
pub struct SignedManifest {
    manifest: PluginManifest,
}

impl SignedManifest {
    /// The manifest the signed lines vouch for.
    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    /// Give up the proof, keeping the manifest.
    pub(crate) fn into_manifest(self) -> PluginManifest {
        self.manifest
    }
}

/// The signed lines and the manifest must name the same id, version and world. This is the
/// one place they are compared.
pub fn check_signed(
    signed: &SignedIdentity,
    manifest: PluginManifest,
) -> Result<SignedManifest, IdentityMismatch> {
    compare_signed(signed, &manifest)?;
    Ok(SignedManifest { manifest })
}

/// Name the first line on which the signed lines and the manifest disagree.
fn compare_signed(
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
pub(crate) fn check_component(
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

/// The interface a folder request unlocks only with `watch = true`.
const WATCH_INTERFACE: &str = "watch";

/// A capability interface may be imported only when its kind is requested, and `watch` only
/// when the folder request asks for change events.
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
    if !manifest.requests(kind) {
        return Err(IdentityMismatch::UnrequestedCapability {
            interface: name.to_owned(),
            kind,
        });
    }
    if interface == WATCH_INTERFACE && !manifest.watches_folders() {
        return Err(IdentityMismatch::UnrequestedWatch {
            interface: name.to_owned(),
        });
    }
    Ok(())
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
        SignedIdentity::from_signed_lines(id, version, world)
    }

    #[test]
    fn signed_lines_must_match_the_manifest() {
        let manifest = manifest("");
        let world = "syrinx:plugin/plugin@0.1.0";
        assert_eq!(
            compare_signed(&signed("sample", "1.2.3", world), &manifest),
            Ok(())
        );
        assert!(matches!(
            compare_signed(&signed("other", "1.2.3", world), &manifest),
            Err(IdentityMismatch::Id { .. })
        ));
        assert!(matches!(
            compare_signed(&signed("sample", "1.2.4", world), &manifest),
            Err(IdentityMismatch::Version { .. })
        ));
        assert!(matches!(
            compare_signed(
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
        // `watch` needs the folder request's `watch = true` as well.
        let watch = "syrinx:plugin/watch@0.1.0";
        assert_eq!(
            check_import(watch, &with),
            Err(IdentityMismatch::UnrequestedWatch {
                interface: watch.into()
            })
        );
        let watching = manifest(
            "[[capabilities]]\nkind = \"folder\"\naccess = \"read\"\nwatch = true\nreason = \"Follow synthetic notes.\"\n",
        );
        assert_eq!(check_import(watch, &watching), Ok(()));
        // The plain host interfaces and WASI need no capability.
        assert_eq!(check_import("syrinx:plugin/host@0.1.0", &without), Ok(()));
        assert_eq!(check_import("wasi:cli/stdout@0.2.9", &without), Ok(()));
    }
}
