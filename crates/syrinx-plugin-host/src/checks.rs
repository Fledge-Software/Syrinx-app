//! Checks made before instantiation: the component exports `syrinx:plugin@0.1.x`
//! and imports nothing the host does not provide.

use wasmtime::Engine;
use wasmtime::component::Component;

use crate::{HostError, wasi};

/// The contract track this host implements.
pub const CONTRACT_PACKAGE: &str = "syrinx:plugin";
pub const CONTRACT_TRACK: &str = "0.1";
/// The newest `syrinx:plugin` version this host links.
pub const CONTRACT_VERSION: &str = "0.1.0";

/// The interfaces every plugin exports.
const REQUIRED_EXPORTS: &[&str] = &[
    "lifecycle",
    "commands",
    "forms",
    "views",
    "search-feed",
    "events",
];

/// The `syrinx:plugin` interfaces a plugin may import.
const PROVIDED_IMPORTS: &[&str] = &["types", "host"];

/// List every reason the component does not fit, so one refusal names them all.
pub fn component_fits(engine: &Engine, component: &Component) -> Result<(), HostError> {
    let component_type = component.component_type();
    let exports: Vec<&str> = component_type
        .exports(engine)
        .map(|(name, _)| name)
        .collect();
    let mut reasons: Vec<String> = REQUIRED_EXPORTS
        .iter()
        .filter_map(|interface| missing_export(&exports, interface))
        .collect();
    reasons.extend(
        component_type
            .imports(engine)
            .filter_map(|(name, _)| unsupported_import(name)),
    );
    if reasons.is_empty() {
        return Ok(());
    }
    Err(HostError::Incompatible(reasons))
}

/// The export must exist at a 0.1.x version; a plugin built for another track is named as such.
fn missing_export(exports: &[&str], interface: &str) -> Option<String> {
    let prefix = format!("{CONTRACT_PACKAGE}/{interface}@");
    let versions: Vec<&str> = exports
        .iter()
        .filter_map(|name| name.strip_prefix(prefix.as_str()))
        .collect();
    if versions.iter().any(|version| on_this_track(version)) {
        return None;
    }
    if let Some(version) = versions.first() {
        return Some(format!(
            "it exports {CONTRACT_PACKAGE}/{interface}@{version}, but this host runs \
             {CONTRACT_PACKAGE}@{CONTRACT_TRACK}.x"
        ));
    }
    Some(format!(
        "it does not export {CONTRACT_PACKAGE}/{interface}@{CONTRACT_TRACK}.x"
    ))
}

fn unsupported_import(name: &str) -> Option<String> {
    if wasi::provides(name) {
        return None;
    }
    if let Some(rest) = name.strip_prefix(&format!("{CONTRACT_PACKAGE}/")) {
        return unsupported_contract_import(rest);
    }
    Some(format!("it imports {name}, which no plugin is given"))
}

/// A `syrinx:plugin` import must be one this host links, at a version it has.
fn unsupported_contract_import(rest: &str) -> Option<String> {
    let Some((interface, version)) = rest.split_once('@') else {
        return Some(format!(
            "it imports {CONTRACT_PACKAGE}/{rest} without a version"
        ));
    };
    if !on_this_track(version) {
        return Some(format!(
            "it imports {CONTRACT_PACKAGE}/{interface}@{version}, but this host runs \
             {CONTRACT_PACKAGE}@{CONTRACT_TRACK}.x"
        ));
    }
    if !PROVIDED_IMPORTS.contains(&interface) {
        return Some(format!(
            "it imports {CONTRACT_PACKAGE}/{interface}@{version}, which this host does not provide"
        ));
    }
    if is_newer_than_host(version) {
        return Some(format!(
            "it was built for {CONTRACT_PACKAGE}@{version}; this host provides {CONTRACT_VERSION}"
        ));
    }
    None
}

fn on_this_track(version: &str) -> bool {
    version
        .strip_prefix(CONTRACT_TRACK)
        .is_some_and(|rest| rest.starts_with('.'))
}

/// Within the track, a later patch may use `@since` imports this host lacks.
fn is_newer_than_host(version: &str) -> bool {
    patch(version) > patch(CONTRACT_VERSION)
}

fn patch(version: &str) -> Option<u64> {
    version.rsplit('.').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wasi_import_from_any_0_2_patch_is_provided() {
        assert_eq!(unsupported_import("wasi:cli/stdout@0.2.9"), None);
        assert_eq!(unsupported_import("wasi:io/streams@0.2.12"), None);
    }

    #[test]
    fn files_sockets_and_wasi_0_3_are_not() {
        for name in [
            "wasi:filesystem/types@0.2.9",
            "wasi:sockets/tcp@0.2.9",
            "wasi:cli/stdout@0.3.0",
        ] {
            assert!(unsupported_import(name).is_some(), "{name} was provided");
        }
    }

    #[test]
    fn a_contract_import_must_be_on_this_track_and_known() {
        assert_eq!(unsupported_import("syrinx:plugin/host@0.1.0"), None);
        let newer = unsupported_import("syrinx:plugin/host@0.1.1").expect("refused");
        assert!(newer.contains("built for syrinx:plugin@0.1.1"), "{newer}");
        let other_track = unsupported_import("syrinx:plugin/host@0.2.0").expect("refused");
        assert!(
            other_track.contains("runs syrinx:plugin@0.1.x"),
            "{other_track}"
        );
        let unknown = unsupported_import("syrinx:plugin/documents@0.1.0").expect("refused");
        assert!(unknown.contains("does not provide"), "{unknown}");
    }

    #[test]
    fn a_missing_export_names_the_track_it_found() {
        let exports = ["syrinx:plugin/commands@0.2.0"];
        let found = missing_export(&exports, "commands").expect("refused");
        assert!(
            found.contains("exports syrinx:plugin/commands@0.2.0"),
            "{found}"
        );
        assert!(missing_export(&["syrinx:plugin/commands@0.1.0"], "commands").is_none());
    }
}
