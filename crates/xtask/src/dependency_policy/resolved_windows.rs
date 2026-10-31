//! Checks the Windows binding features Cargo resolves, so no third-party crate can widen the
//! shared bindings' networking beyond the update download's WinHTTP (D9, DW-68).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{
    ResolvedGraph, UPDATE_NET, WINHTTP_FEATURE, apply_feature_table, is_network_feature,
    normalize_name, read_declaration, read_feature_lists, read_packages, required_array,
    required_string, version_req,
};

/// The binding crates whose every resolved version is read. Each version is a separate build,
/// and any crate that depends on it can call whatever namespace another crate switched on.
const BINDING_CRATES: [&str; 3] = ["windows", "windows-sys", "winapi"];

/// winapi's modules that declare networking, HTTP, URL-download or remote-call functions. Its
/// features are module names, not namespaces. `inaddr` and `in6addr` are left out: they define
/// only the IPv4 and IPv6 address structs and declare no functions, like std's `Ipv4Addr`
/// (the agent's classification, 2026-10-14, after the owner deferred it).
const WINAPI_NETWORK_MODULES: [&str; 26] = [
    "httpapi", "iphlpapi", "ipexport", "iptypes", "lmserver", "lmshare", "lmuse", "lmwksta",
    "mstcpip", "mswsock", "netioapi", "nspapi", "rpc", "rpcdce", "rpcndr", "urlmon", "windns",
    "winhttp", "wininet", "winnetwk", "winsock2", "ws2def", "ws2ipdef", "ws2spi", "ws2tcpip",
    "wsipv6ok",
];

/// One third-party networking feature the owner approved on 2026-10-14 (`docs/decisions.md`):
/// the binding build, the feature, and the only crates that may request it there.
pub(super) struct AllowedFeature {
    pub(super) binding: &'static str,
    pub(super) feature: &'static str,
    pub(super) requesters: &'static [&'static str],
}

/// GPUI's socket and system-information stack, exactly as resolved when DW-68 was built.
/// Each entry must still match, so the list cannot outlive the crates that needed it.
pub(super) const THIRD_PARTY_NETWORK_FEATURES: [AllowedFeature; 10] = [
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
        binding: "windows 0.57.0",
        feature: "Win32_System_Rpc",
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
    // Linux-only crates (GTK's gio-sys, GPUI-X11's filedescriptor) name winapi's socket
    // modules for their own Windows builds, which Syrinx never compiles; metadata unifies
    // features across platforms (owner, 2026-10-31, as for zbus).
    AllowedFeature {
        binding: "winapi 0.3.9",
        feature: "winsock2",
        requesters: &["filedescriptor 0.8.3", "gio-sys 0.18.1"],
    },
    AllowedFeature {
        binding: "winapi 0.3.9",
        feature: "ws2def",
        requesters: &["gio-sys 0.18.1"],
    },
];

/// Refuses each Windows networking feature resolved on any binding package, except WinHTTP on
/// the `windows` build the update download itself uses and the `allowed` third-party entries,
/// naming the crates whose manifest activates it. An allowed entry that no longer matches is
/// refused too.
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
        let features = graph
            .features
            .get(id)
            .ok_or_else(|| format!("binding package has no resolve node: {id}"))?;
        // WinHTTP is approved per build, not per requester: once the download asks for it,
        // Cargo switches it on for every crate sharing that build, whoever else asks.
        let approved = name == "windows" && winhttp_builds.contains(id);
        let binding = package_label(id, &packages)?;
        for feature in refused_features(name, features, approved) {
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

/// The networking features to refuse on one build. A Win32 namespace parent such as
/// `Win32_Networking` is passed when an enabled child implies it; the child is judged instead.
fn refused_features(
    binding_name: &str,
    features: &BTreeSet<String>,
    winhttp_approved: bool,
) -> Vec<String> {
    features
        .iter()
        .filter(|feature| is_binding_network_feature(binding_name, feature))
        .filter(|feature| !(winhttp_approved && feature.as_str() == WINHTTP_FEATURE))
        .filter(|feature| !is_implied_parent(feature, features))
        .cloned()
        .collect()
}

/// `windows` and `windows-sys` name namespaces; winapi names modules.
fn is_binding_network_feature(binding_name: &str, feature: &str) -> bool {
    if binding_name == "winapi" {
        return WINAPI_NETWORK_MODULES.contains(&feature);
    }
    is_network_feature(feature)
}

/// Only Win32 parents hold nothing but child namespaces; WinRT's `Networking` and `Web`
/// declare types of their own, so they are judged even when a child is on.
fn is_implied_parent(feature: &str, features: &BTreeSet<String>) -> bool {
    if !feature.starts_with("Win32_") {
        return false;
    }
    let child_prefix = format!("{feature}_");
    features
        .iter()
        .any(|other| other.starts_with(&child_prefix))
}

/// The packages with a resolved edge to this build whose manifest activates the feature, each
/// as `name version`; Cargo's metadata records nothing more direct.
fn requesters(
    binding_id: &str,
    binding_name: &str,
    feature: &str,
    graph: &ResolvedGraph,
    packages: &BTreeMap<&str, &Value>,
) -> Result<BTreeSet<String>, String> {
    let binding_version = package_version(binding_id, packages)?;
    let mut found = BTreeSet::new();
    for (id, dependencies) in &graph.edges {
        if !dependencies
            .iter()
            .any(|dependency| dependency == binding_id)
        {
            continue;
        }
        let package = packages
            .get(id.as_str())
            .ok_or_else(|| format!("resolve node has no package: {id}"))?;
        let switched_on = graph
            .features
            .get(id)
            .ok_or_else(|| format!("package has no resolve node: {id}"))?;
        let binding = Binding {
            name: binding_name,
            version: binding_version,
        };
        if activates_feature(package, switched_on, &binding, feature)? {
            found.insert(package_label(id, packages)?);
        }
    }
    Ok(found)
}

/// One resolved binding package, as its requesters' declarations must match it.
struct Binding<'a> {
    name: &'a str,
    version: &'a str,
}

/// Whether a package asks for the feature through a declaration that resolved to this binding
/// version, or through an entry of its `[features]` table that its own build switched on.
fn activates_feature(
    package: &Value,
    switched_on: &BTreeSet<String>,
    binding: &Binding,
    feature: &str,
) -> Result<bool, String> {
    let name = normalize_name(required_string(package, "name", "package")?);
    let mut declarations = Vec::new();
    for dependency in required_array(package, "dependencies", &name)? {
        let declaration = read_declaration(dependency, &name)?;
        if declaration.name != binding.name {
            continue;
        }
        let requirement = required_string(dependency, "req", &format!("dependency of {name}"))?;
        if version_req::matches(requirement, binding.version)? {
            declarations.push(declaration);
        }
    }
    let entries: Vec<String> = read_feature_lists(package, &name)?
        .into_iter()
        .filter(|(table_feature, _)| switched_on.contains(table_feature))
        .flat_map(|(_, entries)| entries)
        .collect();
    apply_feature_table(&mut declarations, &entries);
    Ok(declarations
        .iter()
        .any(|declaration| declaration.features.contains(feature)))
}

fn package_version<'a>(id: &str, packages: &BTreeMap<&str, &'a Value>) -> Result<&'a str, String> {
    let package = packages
        .get(id)
        .ok_or_else(|| format!("resolve node has no package: {id}"))?;
    required_string(package, "version", id)
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
