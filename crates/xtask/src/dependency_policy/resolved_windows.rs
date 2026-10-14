//! Checks the `windows` features Cargo resolves, so no third-party crate can widen the shared
//! bindings' networking beyond the update download's WinHTTP (D9, DW-68).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{
    ResolvedGraph, UPDATE_NET, WINHTTP_FEATURE, apply_feature_table, is_network_feature,
    normalize_name, read_declaration, read_feature_table, read_packages, required_array,
    required_string,
};

/// The binding crates whose every resolved version is read. Each version is a separate build,
/// and any crate that depends on it can call whatever namespace another crate switched on.
const BINDING_CRATES: [&str; 2] = ["windows", "windows-sys"];

/// One third-party networking feature the owner approved on 2026-10-14 (`docs/decisions.md`):
/// the binding build, the feature, and the only crates that may request it there.
pub(super) struct AllowedFeature {
    pub(super) binding: &'static str,
    pub(super) feature: &'static str,
    pub(super) requesters: &'static [&'static str],
}

/// GPUI's socket and system-information stack, exactly as resolved when DW-68 was built.
/// Each entry must still match, so the list cannot outlive the crates that needed it.
pub(super) const THIRD_PARTY_NETWORK_FEATURES: [AllowedFeature; 7] = [
    AllowedFeature {
        binding: "windows 0.62.2",
        feature: "Win32_Networking_WinSock",
        requesters: &["gpui-pre 0.3.8", "gpui-pre-windows 0.3.8"],
    },
    AllowedFeature {
        binding: "windows 0.57.0",
        feature: "Win32_NetworkManagement_IpHelper",
        requesters: &["sysinfo 0.31.4"],
    },
    AllowedFeature {
        binding: "windows 0.57.0",
        feature: "Win32_NetworkManagement_Ndis",
        requesters: &["sysinfo 0.31.4"],
    },
    AllowedFeature {
        binding: "windows 0.57.0",
        feature: "Win32_NetworkManagement_NetManagement",
        requesters: &["sysinfo 0.31.4"],
    },
    AllowedFeature {
        binding: "windows 0.57.0",
        feature: "Win32_Networking_WinSock",
        requesters: &["sysinfo 0.31.4"],
    },
    AllowedFeature {
        binding: "windows-sys 0.61.2",
        feature: "Win32_NetworkManagement_IpHelper",
        requesters: &["zbus 5.19.0"],
    },
    AllowedFeature {
        binding: "windows-sys 0.61.2",
        feature: "Win32_Networking_WinSock",
        requesters: &[
            "mio 1.2.4",
            "polling 3.11.0",
            "quinn-udp 0.5.16",
            "rustix 1.1.5",
            "socket2 0.6.5",
            "uds_windows 1.2.1",
            "zbus 5.19.0",
        ],
    },
];

/// Refuses each Windows networking feature resolved on any binding package, except WinHTTP on
/// the `windows` build the update download itself uses and the `allowed` third-party entries,
/// naming the crates that request it. An allowed entry that no longer matches is refused too.
pub(super) fn check(
    metadata: &Value,
    graph: &ResolvedGraph,
    allowed: &[AllowedFeature],
    violations: &mut BTreeSet<String>,
) -> Result<(), String> {
    let packages = read_packages(metadata)?;
    let winhttp_builds = winhttp_builds(metadata, graph)?;
    let mut matched = BTreeSet::new();
    for (id, name) in &graph.names {
        if !BINDING_CRATES.contains(&name.as_str()) {
            continue;
        }
        let Some(features) = graph.features.get(id) else {
            continue;
        };
        let approved = name == "windows" && winhttp_builds.contains(id);
        let binding = package_label(id, &packages)?;
        for feature in refused_features(features, approved) {
            let requesters = requesters(id, name, &feature, graph, &packages)?;
            let entry = allowed
                .iter()
                .position(|entry| entry.binding == binding && entry.feature == feature);
            let Some(index) = entry else {
                violations.insert(format!(
                    "{binding}: resolved feature {feature} exposes Windows networking (D9); requested by {}",
                    describe(&requesters)
                ));
                continue;
            };
            matched.insert(index);
            violations.extend(judge_allowed(&allowed[index], &requesters));
        }
    }
    violations.extend(unmatched_entries(allowed, &matched));
    Ok(())
}

/// An approved feature passes only while exactly its listed crates request it: a new requester
/// is refused, and a listed one that stopped asking makes the entry stale.
fn judge_allowed(entry: &AllowedFeature, requesters: &BTreeSet<String>) -> Vec<String> {
    let listed: BTreeSet<String> = entry
        .requesters
        .iter()
        .map(|&label| label.to_owned())
        .collect();
    let mut violations = Vec::new();
    let new: Vec<&String> = requesters.difference(&listed).collect();
    if !new.is_empty() {
        violations.push(format!(
            "{}: resolved feature {} is approved only from {}; also requested by {}",
            entry.binding,
            entry.feature,
            entry.requesters.join(", "),
            join(&new)
        ));
    }
    let gone: Vec<&String> = listed.difference(requesters).collect();
    if !gone.is_empty() {
        violations.push(format!(
            "{}: approved feature {} lists {}, which no longer request it; remove the stale requester (docs/decisions.md, 2026-10-14)",
            entry.binding,
            entry.feature,
            join(&gone)
        ));
    }
    violations
}

/// Approved entries whose build or feature is no longer resolved at all.
fn unmatched_entries(allowed: &[AllowedFeature], matched: &BTreeSet<usize>) -> Vec<String> {
    allowed
        .iter()
        .enumerate()
        .filter(|(index, _)| !matched.contains(index))
        .map(|(_, entry)| {
            format!(
                "{}: approved feature {} is no longer resolved; remove the stale entry (docs/decisions.md, 2026-10-14)",
                entry.binding, entry.feature
            )
        })
        .collect()
}

fn describe(requesters: &BTreeSet<String>) -> String {
    if requesters.is_empty() {
        return "no direct declaration".to_owned();
    }
    join(&requesters.iter().collect::<Vec<_>>())
}

fn join(labels: &[&String]) -> String {
    labels
        .iter()
        .map(|label| label.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The resolved `windows` packages the update download depends on: the only builds on which
/// WinHTTP is approved (P1-39's change 3). None when the download is not in the workspace.
fn winhttp_builds(metadata: &Value, graph: &ResolvedGraph) -> Result<BTreeSet<String>, String> {
    let is_named = |id: &str, wanted: &str| graph.names.get(id).is_some_and(|name| name == wanted);
    let builds = required_array(metadata, "workspace_members", "cargo metadata")?
        .iter()
        .filter_map(Value::as_str)
        .filter(|member| is_named(member, UPDATE_NET))
        .flat_map(|member| graph.edges.get(member).into_iter().flatten())
        .filter(|dependency| is_named(dependency, "windows"))
        .cloned()
        .collect();
    Ok(builds)
}

/// The networking features to refuse on one build. A namespace parent such as
/// `Win32_Networking` is passed when an enabled child implies it; the child is judged instead.
fn refused_features(features: &BTreeSet<String>, winhttp_approved: bool) -> Vec<String> {
    features
        .iter()
        .filter(|feature| is_network_feature(feature))
        .filter(|feature| !(winhttp_approved && feature.as_str() == WINHTTP_FEATURE))
        .filter(|feature| !is_implied_parent(feature, features))
        .cloned()
        .collect()
}

fn is_implied_parent(feature: &str, features: &BTreeSet<String>) -> bool {
    let child_prefix = format!("{feature}_");
    features
        .iter()
        .any(|other| other.starts_with(&child_prefix))
}

/// The packages with a resolved edge to this build whose declaration or `[features]` table
/// names the feature, each as `name version`; Cargo's metadata records nothing more direct.
fn requesters(
    binding_id: &str,
    binding_name: &str,
    feature: &str,
    graph: &ResolvedGraph,
    packages: &BTreeMap<&str, &Value>,
) -> Result<BTreeSet<String>, String> {
    let mut found = BTreeSet::new();
    for (id, dependencies) in &graph.edges {
        if !dependencies
            .iter()
            .any(|dependency| dependency == binding_id)
        {
            continue;
        }
        let Some(package) = packages.get(id.as_str()) else {
            continue;
        };
        if requests_feature(package, binding_name, feature)? {
            found.insert(package_label(id, packages)?);
        }
    }
    Ok(found)
}

/// Whether one package's declarations of the binding, or its own feature table, ask for it.
fn requests_feature(package: &Value, binding_name: &str, feature: &str) -> Result<bool, String> {
    let name = normalize_name(required_string(package, "name", "package")?);
    let mut declarations = required_array(package, "dependencies", &name)?
        .iter()
        .map(|dependency| read_declaration(dependency, &name))
        .collect::<Result<Vec<_>, _>>()?;
    let (_, entries) = read_feature_table(package, &name)?;
    apply_feature_table(&mut declarations, &entries);
    Ok(declarations
        .iter()
        .filter(|declaration| declaration.name == binding_name)
        .any(|declaration| declaration.features.contains(feature)))
}

/// `name version`, so an error points at the exact package among several versions.
fn package_label(id: &str, packages: &BTreeMap<&str, &Value>) -> Result<String, String> {
    let package = packages
        .get(id)
        .ok_or_else(|| format!("resolve node has no package: {id}"))?;
    let name = required_string(package, "name", "package")?;
    let version = required_string(package, "version", name)?;
    Ok(format!("{name} {version}"))
}
