//! Checks Cargo's workspace metadata so core logic cannot acquire shell dependencies.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::Value;

struct Package {
    dependencies: BTreeSet<String>,
    declarations: Vec<Declaration>,
    /// Every entry of the package's own `[features]` table, such as `dep/feature`.
    feature_entries: Vec<String>,
}

/// One dependency declaration's requested features, kept for feature-restricted packages.
struct Declaration {
    name: String,
    /// The name the package imports it under, when renamed.
    alias: Option<String>,
    features: BTreeSet<String>,
    default_features: bool,
}

type Workspace = BTreeMap<String, Package>;

/// Returns policy violations, or an error when Cargo's metadata cannot be read.
pub fn check_metadata(metadata: &Value) -> Result<Vec<String>, String> {
    // Validate membership before applying rules to the workspace packages.
    let workspace = read_workspace(metadata)?;
    let mut violations = BTreeSet::new();
    for (name, package) in &workspace {
        check_direct_dependencies(name, package, &workspace, &mut violations);
        check_update_verifier_features(name, package, &mut violations);
        check_release_tool_feature(name, package, &mut violations);
        check_shell_paths(name, &workspace, &mut violations);
        check_network_declarations(name, package, &mut violations);
    }
    check_headless_network_reach(metadata, &mut violations)?;
    Ok(violations.into_iter().collect())
}

fn read_workspace(metadata: &Value) -> Result<Workspace, String> {
    // Require the versioned format before reading workspace membership.
    if metadata.get("version").and_then(Value::as_u64) != Some(1) {
        return Err("cargo metadata must have format version 1".to_owned());
    }
    let packages = read_packages(metadata)?;
    let members = required_array(metadata, "workspace_members", "cargo metadata")?;
    if members.is_empty() {
        return Err("cargo metadata has no workspace members".to_owned());
    }

    // Package IDs, rather than the entire packages array, define membership.
    let mut workspace = BTreeMap::new();
    let mut seen_members = BTreeSet::new();
    for member in members {
        let member_id = member
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or("workspace_members must contain nonempty package ID strings")?;
        if !seen_members.insert(member_id) {
            return Err(format!("duplicate workspace member: {member_id}"));
        }
        let package = packages
            .get(member_id)
            .ok_or_else(|| format!("workspace member has no package: {member_id}"))?;
        insert_package(&mut workspace, package)?;
    }
    Ok(workspace)
}

fn read_packages(metadata: &Value) -> Result<BTreeMap<&str, &Value>, String> {
    // Index Cargo's opaque IDs without assuming their path or registry format.
    let mut packages = BTreeMap::new();
    for package in required_array(metadata, "packages", "cargo metadata")? {
        let id = required_string(package, "id", "package")?;
        if packages.insert(id, package).is_some() {
            return Err(format!("duplicate package ID: {id}"));
        }
    }
    Ok(packages)
}

fn insert_package(workspace: &mut Workspace, package: &Value) -> Result<(), String> {
    // Cargo reports the original package name even when an import is renamed.
    let name = normalize_name(required_string(package, "name", "workspace package")?);
    let declarations = required_array(package, "dependencies", &name)?
        .iter()
        .map(|dependency| read_declaration(dependency, &name))
        .collect::<Result<Vec<_>, _>>()?;
    let dependencies = declarations
        .iter()
        .map(|declaration| declaration.name.clone())
        .collect();
    let feature_entries = read_feature_entries(package, &name)?;
    let package = Package {
        dependencies,
        declarations,
        feature_entries,
    };
    if workspace.insert(name.clone(), package).is_some() {
        return Err(format!("ambiguous workspace package name: {name}"));
    }
    Ok(())
}

/// The entries of a package's `[features]` table; a missing table means none.
fn read_feature_entries(package: &Value, name: &str) -> Result<Vec<String>, String> {
    let Some(features) = package.get("features") else {
        return Ok(Vec::new());
    };
    let table = features
        .as_object()
        .ok_or_else(|| format!("{name}.features must be an object"))?;
    let mut entries = Vec::new();
    for (feature, enabled) in table {
        let list = enabled
            .as_array()
            .ok_or_else(|| format!("{name}.features.{feature} must be an array"))?;
        for entry in list {
            let entry = entry
                .as_str()
                .ok_or_else(|| format!("{name}.features.{feature} must hold strings"))?;
            entries.push(entry.to_owned());
        }
    }
    Ok(entries)
}

fn read_declaration(dependency: &Value, package: &str) -> Result<Declaration, String> {
    // Missing features mean none; missing default-feature data counts as defaults enabled.
    let context = format!("dependency of {package}");
    let name = normalize_name(required_string(dependency, "name", &context)?);
    let features = match dependency.get("features") {
        None => BTreeSet::new(),
        Some(value) => value
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .map(Value::as_str)
                    .collect::<Option<BTreeSet<_>>>()
            })
            .ok_or_else(|| format!("{context}.features must be an array of strings"))?
            .into_iter()
            .map(str::to_owned)
            .collect(),
    };
    let default_features = match dependency.get("uses_default_features") {
        None => true,
        Some(value) => value
            .as_bool()
            .ok_or_else(|| format!("{context}.uses_default_features must be a boolean"))?,
    };
    let alias = match dependency.get("rename") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or_else(|| format!("{context}.rename must be a string"))?
                .to_owned(),
        ),
    };
    Ok(Declaration {
        name,
        alias,
        features,
        default_features,
    })
}

fn required_array<'a>(value: &'a Value, field: &str, context: &str) -> Result<&'a [Value], String> {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{context}.{field} must be an array"))
}

fn required_string<'a>(value: &'a Value, field: &str, context: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| format!("{context}.{field} must be a nonempty string"))
}

fn normalize_name(name: &str) -> String {
    name.replace('_', "-")
}

fn check_direct_dependencies(
    name: &str,
    package: &Package,
    workspace: &Workspace,
    violations: &mut BTreeSet<String>,
) {
    // All declarations count, including optional, target, dev and build dependencies.
    for dependency in &package.dependencies {
        if is_gpui(dependency) && !matches!(name, "syrinx-app" | "syrinx-ui") {
            violations.insert(format!(
                "{name} -> {dependency}: only syrinx-app and syrinx-ui may directly depend on GPUI"
            ));
        }
        if is_platform_library(dependency) && !may_use_platform_library(name, dependency) {
            violations.insert(format!(
                "{name} -> {dependency}: only syrinx-platform may directly depend on WebView2 or raw Win32"
            ));
        }
        if dependency == UPDATE_VERIFIER && !UPDATE_VERIFIER_USERS.contains(&name) {
            violations.insert(format!(
                "{name} -> {dependency}: only syrinx-app may use the update verifier (P1-39b)"
            ));
        }
        check_fulltext_quarantine(name, dependency, violations);
        check_plugin_dependency(name, dependency, workspace, violations);
    }
}

/// The P1-23 bake-off harness, which no product crate may use.
const FULLTEXT_BENCH: &str = "fulltext-bench";

/// Approved only for the bake-off until P1-24 chooses an engine and RUSTSEC-2026-0253 is resolved.
const TANTIVY: &str = "tantivy";

fn check_fulltext_quarantine(name: &str, dependency: &str, violations: &mut BTreeSet<String>) {
    // Keep the benchmark and its Tantivy graph out of everything Syrinx ships.
    if dependency == FULLTEXT_BENCH {
        violations.insert(format!(
            "{name} -> {dependency}: nothing may depend on the P1-23 full-text bake-off harness"
        ));
    }
    if dependency == TANTIVY && name != FULLTEXT_BENCH {
        violations.insert(format!(
            "{name} -> {dependency}: only fulltext-bench may depend on Tantivy before P1-24 chooses it"
        ));
    }
}

fn check_plugin_dependency(
    name: &str,
    dependency: &str,
    workspace: &Workspace,
    violations: &mut BTreeSet<String>,
) {
    // Plugins reach application services through their API, not another Syrinx crate.
    if !is_syrinx_dependency(dependency, workspace) {
        return;
    }
    if name == "syrinx-plugin-api" {
        violations.insert(format!(
            "{name} -> {dependency}: syrinx-plugin-api must not depend on another Syrinx crate"
        ));
        return;
    }
    if name.starts_with("plugin-") && dependency != "syrinx-plugin-api" {
        violations.insert(format!(
            "{name} -> {dependency}: a plugin's only Syrinx dependency may be syrinx-plugin-api"
        ));
    }
}

fn is_gpui(name: &str) -> bool {
    name == "gpui" || name.starts_with("gpui-")
}

fn is_platform_library(name: &str) -> bool {
    matches!(
        name,
        "wry" | "webview2-com" | "webview2-com-sys" | "windows" | "windows-sys"
    )
}

/// The P1-38 update verifier and P1-39b's installer.
const UPDATE_VERIFIER: &str = "syrinx-update";

/// The crates that may depend on the verifier: P1-39's change 1, for P1-39b. P1-39c adds
/// `syrinx-update-net` in its own slice.
const UPDATE_VERIFIER_USERS: [&str; 1] = ["syrinx-app"];

/// The verifier's feature that links Windows key storage for the release tool only.
const RELEASE_TOOL_FEATURE: &str = "release-tool";

/// The only `windows` features the update verifier may request: CNG and its NTSTATUS types.
const UPDATE_VERIFIER_WINDOWS_FEATURES: [&str; 2] =
    ["Win32_Foundation", "Win32_Security_Cryptography"];

fn check_update_verifier_features(
    name: &str,
    package: &Package,
    violations: &mut BTreeSet<String>,
) {
    // Keep the verifier to cryptography: no default features, no networking or other namespaces.
    if name != UPDATE_VERIFIER {
        return;
    }
    for declaration in package
        .declarations
        .iter()
        .filter(|item| item.name == "windows")
    {
        if declaration.default_features {
            violations.insert(format!(
                "{name} -> windows: default features must be disabled"
            ));
        }
        for feature in &declaration.features {
            if !UPDATE_VERIFIER_WINDOWS_FEATURES.contains(&feature.as_str()) {
                violations.insert(format!(
                    "{name} -> windows: feature {feature} is not allowed for the update verifier"
                ));
            }
        }
    }
}

/// Refuse any dependency that turns on the release tool's key-storage signer: Syrinx itself
/// must never link code that opens Windows key storage (`docs/p1-38-update-format.md`).
fn check_release_tool_feature(name: &str, package: &Package, violations: &mut BTreeSet<String>) {
    let verifier = package
        .declarations
        .iter()
        .filter(|declaration| declaration.name == UPDATE_VERIFIER);
    let mut imported_as = BTreeSet::new();
    let mut enabled = false;
    for declaration in verifier {
        enabled |= declaration.features.contains(RELEASE_TOOL_FEATURE);
        imported_as.insert(declaration.name.clone());
        imported_as.extend(declaration.alias.clone());
    }
    // A package's own features can switch it on too: `dep/release-tool` or `dep?/release-tool`.
    enabled |= package
        .feature_entries
        .iter()
        .any(|entry| enables_release_tool(entry, &imported_as));
    if enabled {
        violations.insert(format!(
            "{name} -> {UPDATE_VERIFIER}: the {RELEASE_TOOL_FEATURE} feature is for the release tool alone"
        ));
    }
}

/// Whether a `[features]` entry turns on the verifier's release tool under any imported name.
fn enables_release_tool(entry: &str, imported_as: &BTreeSet<String>) -> bool {
    let Some((dependency, feature)) = entry.split_once('/') else {
        return false;
    };
    let dependency = dependency.strip_suffix('?').unwrap_or(dependency);
    feature == RELEASE_TOOL_FEATURE && imported_as.contains(&normalize_name(dependency))
}

/// The platform crate owns Win32; the update verifier may also call CNG through `windows`.
fn may_use_platform_library(name: &str, dependency: &str) -> bool {
    name == "syrinx-platform" || (name == UPDATE_VERIFIER && dependency == "windows")
}

fn is_syrinx_dependency(name: &str, workspace: &Workspace) -> bool {
    workspace.contains_key(name)
        || name.starts_with("syrinx-")
        || name.starts_with("plugin-")
        || name == "fledge-client"
}

fn needs_shell_isolation(name: &str) -> bool {
    matches!(
        name,
        "syrinx-core" | "syrinx-store" | "syrinx-search" | "syrinx-plugin-api" | "syrinx-update"
    ) || name.starts_with("plugin-")
}

fn check_shell_paths(name: &str, workspace: &Workspace, violations: &mut BTreeSet<String>) {
    // Follow workspace facades without inspecting third-party transitive libraries.
    if !needs_shell_isolation(name) {
        return;
    }
    let mut pending = VecDeque::from([(name.to_owned(), vec![name.to_owned()])]);
    let mut visited = BTreeSet::new();
    while let Some((current, path)) = pending.pop_front() {
        if !visited.insert(current.clone()) {
            continue;
        }
        if matches!(
            current.as_str(),
            "syrinx-app" | "syrinx-ui" | "syrinx-platform"
        ) {
            violations.insert(format!(
                "{}: core and plugin crates must not reach the shell through workspace dependencies",
                path.join(" -> ")
            ));
            continue;
        }
        append_workspace_dependencies(&current, &path, workspace, &mut pending);
    }
}

fn append_workspace_dependencies(
    name: &str,
    path: &[String],
    workspace: &Workspace,
    pending: &mut VecDeque<(String, Vec<String>)>,
) {
    // External packages are leaves because --no-deps excludes their dependency graph.
    let Some(package) = workspace.get(name) else {
        return;
    };
    for dependency in &package.dependencies {
        if !workspace.contains_key(dependency) {
            continue;
        }
        let mut next_path = path.to_vec();
        next_path.push(dependency.clone());
        pending.push_back((dependency.clone(), next_path));
    }
}

/// Crates that open network connections. Typing must stay on the machine (D9) and Syrinx makes no
/// network call of its own, so Syrinx's crates never depend on them; WebView2 loads every page.
const NETWORK_CRATES: [&str; 22] = [
    "reqwest",
    "gpui-pre-reqwest",
    "gpui-pre-http-client",
    "http-client",
    "hyper",
    "hyper-util",
    "h2",
    "ureq",
    "curl",
    "isahc",
    "surf",
    "attohttpc",
    "minreq",
    "tokio",
    "async-net",
    "async-std",
    "socket2",
    "mio",
    "tungstenite",
    "quinn",
    "trust-dns-resolver",
    "hickory-resolver",
];

/// `windows` feature prefixes that expose Windows networking or HTTP APIs.
const NETWORK_WINDOWS_FEATURES: [&str; 5] = [
    "Win32_Networking",
    "Win32_NetworkManagement",
    "Win32_Web",
    "Networking",
    "Web",
];

fn check_network_declarations(name: &str, package: &Package, violations: &mut BTreeSet<String>) {
    // Neither a network crate nor a Windows networking namespace may be declared by Syrinx.
    for declaration in &package.declarations {
        if NETWORK_CRATES.contains(&declaration.name.as_str()) {
            violations.insert(format!(
                "{name} -> {}: Syrinx's crates may not use a network client (D9)",
                declaration.name
            ));
        }
        if !matches!(declaration.name.as_str(), "windows" | "windows-sys") {
            continue;
        }
        for feature in declaration
            .features
            .iter()
            .filter(|feature| is_network_feature(feature))
        {
            violations.insert(format!(
                "{name} -> {}: feature {feature} exposes Windows networking (D9)",
                declaration.name
            ));
        }
    }
}

fn is_network_feature(feature: &str) -> bool {
    NETWORK_WINDOWS_FEATURES.iter().any(|prefix| {
        feature == *prefix
            || feature
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('_'))
    })
}

/// Follows Cargo's resolved graph from each headless crate, so the catalogue, store and core
/// that rank typed text cannot reach a network crate even through a third-party library.
fn check_headless_network_reach(
    metadata: &Value,
    violations: &mut BTreeSet<String>,
) -> Result<(), String> {
    let graph = read_resolved_graph(metadata)?;
    for member in required_array(metadata, "workspace_members", "cargo metadata")? {
        let Some(id) = member.as_str() else {
            continue;
        };
        let Some(name) = graph.names.get(id) else {
            continue;
        };
        if !needs_shell_isolation(name) {
            continue;
        }
        if let Some(path) = network_path(id, &graph) {
            violations.insert(format!(
                "{}: a headless crate reaches a network client (D9)",
                path.join(" -> ")
            ));
        }
    }
    Ok(())
}

/// Package names and resolved dependency edges, keyed by Cargo's package ID.
struct ResolvedGraph {
    names: BTreeMap<String, String>,
    edges: BTreeMap<String, Vec<String>>,
}

fn read_resolved_graph(metadata: &Value) -> Result<ResolvedGraph, String> {
    // The full (not `--no-deps`) metadata carries the resolved graph this check needs.
    let mut names = BTreeMap::new();
    for package in required_array(metadata, "packages", "cargo metadata")? {
        let id = required_string(package, "id", "package")?;
        let name = normalize_name(required_string(package, "name", "package")?);
        names.insert(id.to_owned(), name);
    }
    let resolve = metadata
        .get("resolve")
        .filter(|value| value.is_object())
        .ok_or("cargo metadata must include the resolved dependency graph")?;
    let mut edges = BTreeMap::new();
    for node in required_array(resolve, "nodes", "cargo metadata.resolve")? {
        let id = required_string(node, "id", "resolve node")?;
        let dependencies = required_array(node, "deps", "resolve node")?
            .iter()
            .map(|dependency| required_string(dependency, "pkg", "resolve dependency"))
            .map(|pkg| pkg.map(str::to_owned))
            .collect::<Result<Vec<_>, _>>()?;
        edges.insert(id.to_owned(), dependencies);
    }
    Ok(ResolvedGraph { names, edges })
}

// Breadth-first search for the shortest package path to a network crate.
fn network_path(start: &str, graph: &ResolvedGraph) -> Option<Vec<String>> {
    let mut pending = VecDeque::from([(start.to_owned(), vec![start.to_owned()])]);
    let mut visited = BTreeSet::new();
    while let Some((id, path)) = pending.pop_front() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let name = graph.names.get(&id).map_or(id.as_str(), String::as_str);
        if NETWORK_CRATES.contains(&name) {
            return Some(path.iter().map(|step| display_name(step, graph)).collect());
        }
        for dependency in graph.edges.get(&id).into_iter().flatten() {
            let mut next = path.clone();
            next.push(dependency.clone());
            pending.push_back((dependency.clone(), next));
        }
    }
    None
}

fn display_name(id: &str, graph: &ResolvedGraph) -> String {
    graph
        .names
        .get(id)
        .cloned()
        .unwrap_or_else(|| id.to_owned())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::check_metadata;

    fn package(name: &str, dependencies: Value) -> Value {
        json!({"id": format!("workspace:{name}"), "name": name, "dependencies": dependencies})
    }

    fn metadata(packages: Vec<Value>) -> Value {
        let members: Vec<Value> = packages
            .iter()
            .map(|package| package["id"].clone())
            .collect();
        json!({
            "version": 1,
            "packages": packages,
            "workspace_members": members,
            "resolve": {"nodes": []}
        })
    }

    fn node(name: &str, dependencies: &[&str]) -> Value {
        let deps: Vec<Value> = dependencies
            .iter()
            .map(|dependency| json!({"pkg": format!("workspace:{dependency}")}))
            .collect();
        json!({"id": format!("workspace:{name}"), "deps": deps})
    }

    #[test]
    fn rejects_network_clients_and_windows_networking() -> Result<(), String> {
        // D9: no Syrinx crate may declare a network client or a Windows networking namespace.
        let input = metadata(vec![
            package("syrinx-app", json!([{"name": "reqwest", "kind": "dev"}])),
            package(
                "syrinx-platform",
                json!([{
                    "name": "windows",
                    "features": [
                        "Win32_Foundation",
                        "Win32_Networking_WinSock",
                        "Web_Http",
                        "Win32_WebUi",
                        "Networkingish"
                    ]
                }]),
            ),
        ]);
        let violations = check_metadata(&input)?;
        assert_eq!(violations.len(), 3, "{violations:?}");
        assert!(
            violations
                .iter()
                .any(|message| message.contains("syrinx-app -> reqwest"))
        );
        assert!(
            violations
                .iter()
                .any(|message| message.contains("Win32_Networking_WinSock"))
        );
        assert!(
            violations
                .iter()
                .any(|message| message.contains("Web_Http"))
        );
        Ok(())
    }

    #[test]
    fn headless_crates_cannot_reach_a_network_client_transitively() -> Result<(), String> {
        // The shell may inherit GPUI's client; the crates that rank typed text may not.
        let mut input = metadata(vec![
            package("syrinx-search", json!([{"name": "helper"}])),
            package("syrinx-app", json!([{"name": "gpui-pre"}])),
        ]);
        input["packages"]
            .as_array_mut()
            .ok_or("synthetic packages")?
            .extend([
                package("helper", json!([])),
                package("gpui-pre", json!([])),
                package("hyper", json!([])),
            ]);
        input["resolve"]["nodes"] = json!([
            node("syrinx-search", &["helper"]),
            node("helper", &["hyper"]),
            node("syrinx-app", &["gpui-pre"]),
            node("gpui-pre", &["hyper"]),
            node("hyper", &[]),
        ]);
        let violations = check_metadata(&input)?;
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(violations[0].contains("syrinx-search -> helper -> hyper"));

        input
            .as_object_mut()
            .ok_or("synthetic metadata")?
            .remove("resolve");
        assert!(check_metadata(&input).is_err());
        Ok(())
    }

    #[test]
    fn permits_shell_composition_and_headless_plugins() -> Result<(), String> {
        // The app can reach platform libraries through the designated workspace boundary.
        let input = metadata(vec![
            package(
                "syrinx-app",
                json!([{"name": "syrinx-ui"}, {"name": "syrinx-platform"}]),
            ),
            package(
                "syrinx-ui",
                json!([{"name": "gpui-pre"}, {"name": "gpui-component"}]),
            ),
            package(
                "syrinx-platform",
                json!([{"name": "wry"}, {"name": "windows"}]),
            ),
            package(
                "syrinx-core",
                json!([{"name": "syrinx-store"}, {"name": "syrinx-search"}]),
            ),
            package("syrinx-store", json!([{"name": "rusqlite"}])),
            package("syrinx-search", json!([{"name": "frizbee"}])),
            package(
                "plugin-notes",
                json!([{"name": "syrinx-plugin-api"}, {"name": "serde"}]),
            ),
            package("syrinx-plugin-api", json!([{"name": "serde"}])),
        ]);
        assert!(check_metadata(&input)?.is_empty());
        Ok(())
    }

    #[test]
    fn confines_the_update_verifier() -> Result<(), String> {
        // The verifier may call CNG through `windows`, but not WebView2; only the app may use it.
        let permitted = metadata(vec![package(
            "syrinx-update",
            json!([{
                "name": "windows",
                "uses_default_features": false,
                "features": ["Win32_Foundation", "Win32_Security_Cryptography"]
            }]),
        )]);
        assert!(check_metadata(&permitted)?.is_empty());

        let input = metadata(vec![
            package(
                "syrinx-update",
                json!([
                    {"name": "windows", "uses_default_features": false},
                    {"name": "wry"},
                    {"name": "syrinx-platform"}
                ]),
            ),
            package("syrinx-app", json!([{"name": "syrinx-update"}])),
            package(
                "syrinx-ui",
                json!([{"name": "syrinx-update", "kind": "dev"}]),
            ),
            package("syrinx-platform", json!([])),
        ]);
        let violations = check_metadata(&input)?;
        assert_eq!(violations.len(), 3, "{violations:?}");
        assert!(
            violations
                .iter()
                .any(|message| message.contains("syrinx-update -> wry"))
        );
        assert!(
            violations
                .iter()
                .any(|message| message.contains("syrinx-ui -> syrinx-update"))
        );
        assert!(
            violations
                .iter()
                .any(|message| message.contains("syrinx-update -> syrinx-platform"))
        );
        Ok(())
    }

    #[test]
    fn no_dependency_may_enable_the_release_tool() -> Result<(), String> {
        // Syrinx links the verifier without the key-storage signer, in every dependency kind.
        let permitted = metadata(vec![package(
            "syrinx-app",
            json!([{"name": "syrinx-update", "features": []}]),
        )]);
        assert!(check_metadata(&permitted)?.is_empty());
        for kind in [Value::Null, json!("dev"), json!("build")] {
            let input = metadata(vec![package(
                "syrinx-app",
                json!([{"name": "syrinx-update", "kind": kind, "features": ["release-tool"]}]),
            )]);
            let violations = check_metadata(&input)?;
            assert_eq!(violations.len(), 1, "{violations:?}");
            assert!(violations[0].contains("release-tool feature is for the release tool alone"));
        }
        Ok(())
    }

    #[test]
    fn no_feature_table_may_enable_the_release_tool() -> Result<(), String> {
        // `sign = ["syrinx-update/release-tool"]` reaches the signer without a declared feature.
        let with_features = |features: Value, rename: Value| {
            let mut app = package(
                "syrinx-app",
                json!([{"name": "syrinx-update", "rename": rename}]),
            );
            app["features"] = features;
            metadata(vec![app])
        };
        let harmless = with_features(json!({"sign": ["syrinx-update/other"]}), Value::Null);
        assert!(check_metadata(&harmless)?.is_empty());
        for (features, rename) in [
            (json!({"sign": ["syrinx-update/release-tool"]}), Value::Null),
            (
                json!({"sign": ["syrinx-update?/release-tool"]}),
                Value::Null,
            ),
            (json!({"sign": ["syrinx_update/release-tool"]}), Value::Null),
            (
                json!({"sign": ["verifier/release-tool"]}),
                json!("verifier"),
            ),
        ] {
            let violations = check_metadata(&with_features(features.clone(), rename))?;
            assert_eq!(violations.len(), 1, "{features}: {violations:?}");
            assert!(violations[0].contains("release-tool feature is for the release tool alone"));
        }
        let malformed = with_features(json!({"sign": "syrinx-update/release-tool"}), Value::Null);
        assert!(check_metadata(&malformed).is_err());
        Ok(())
    }

    #[test]
    fn restricts_the_update_verifier_to_cryptography_features() -> Result<(), String> {
        // A networking namespace or the default features must fail the dependency check.
        let input = metadata(vec![package(
            "syrinx-update",
            json!([
                {
                    "name": "windows",
                    "uses_default_features": false,
                    "features": ["Win32_Security_Cryptography", "Win32_Networking_WinHttp"]
                },
                {"name": "windows", "kind": "dev"}
            ]),
        )]);
        let violations = check_metadata(&input)?;
        // WinHTTP breaks both the verifier's feature list and D9's networking rule.
        assert_eq!(violations.len(), 3, "{violations:?}");
        assert!(violations.iter().any(|message| {
            message.contains("Win32_Networking_WinHttp is not allowed for the update verifier")
        }));
        assert!(violations.iter().any(|message| {
            message.contains("Win32_Networking_WinHttp exposes Windows networking (D9)")
        }));
        assert!(
            violations
                .iter()
                .any(|message| message.contains("default features must be disabled"))
        );
        Ok(())
    }

    #[test]
    fn rejects_renamed_optional_dev_build_and_target_dependencies() -> Result<(), String> {
        // Renaming and non-default dependency declarations cannot hide forbidden imports.
        let input = metadata(vec![package(
            "syrinx-core",
            json!([
                {"name": "gpui_pre", "rename": "widgets", "kind": "dev"},
                {"name": "webview2-com-sys", "rename": "engine", "kind": "build"},
                {"name": "windows-sys", "target": "cfg(windows)", "optional": true}
            ]),
        )]);
        let violations = check_metadata(&input)?;
        assert_eq!(violations.len(), 3);
        assert!(
            violations
                .iter()
                .any(|message| message.contains("syrinx-core -> gpui-pre"))
        );
        assert!(
            violations
                .iter()
                .any(|message| message.contains("syrinx-core -> webview2-com-sys"))
        );
        assert!(
            violations
                .iter()
                .any(|message| message.contains("syrinx-core -> windows-sys"))
        );
        Ok(())
    }

    #[test]
    fn quarantines_the_fulltext_bench_and_tantivy() -> Result<(), String> {
        // The bake-off may use Tantivy; nothing else may use either, in any dependency kind.
        let permitted = metadata(vec![package(
            "fulltext-bench",
            json!([{"name": "tantivy"}, {"name": "rusqlite"}]),
        )]);
        assert!(check_metadata(&permitted)?.is_empty());

        let input = metadata(vec![
            package("fulltext-bench", json!([{"name": "tantivy"}])),
            package(
                "syrinx-search",
                json!([{"name": "tantivy", "rename": "fulltext", "optional": true}]),
            ),
            package(
                "syrinx-app",
                json!([{"name": "fulltext_bench", "kind": "dev"}]),
            ),
            package(
                "xtask",
                json!([{"name": "fulltext-bench", "kind": "build"}]),
            ),
        ]);
        let violations = check_metadata(&input)?;
        assert_eq!(violations.len(), 3);
        assert!(
            violations
                .iter()
                .any(|message| message.contains("syrinx-search -> tantivy"))
        );
        assert!(
            violations
                .iter()
                .any(|message| message.contains("syrinx-app -> fulltext-bench"))
        );
        assert!(
            violations
                .iter()
                .any(|message| message.contains("xtask -> fulltext-bench"))
        );
        Ok(())
    }

    #[test]
    fn rejects_workspace_facades_and_terminates_on_cycles() -> Result<(), String> {
        // A helper cannot smuggle shell dependencies into a headless crate.
        let input = metadata(vec![
            package("syrinx-search", json!([{"name": "helper"}])),
            package(
                "helper",
                json!([{"name": "syrinx-search"}, {"name": "syrinx-ui"}]),
            ),
            package("syrinx-ui", json!([{"name": "gpui-pre"}])),
        ]);
        let violations = check_metadata(&input)?;
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("syrinx-search -> helper -> syrinx-ui"));
        Ok(())
    }

    #[test]
    fn rejects_plugin_service_imports_and_api_back_edges() -> Result<(), String> {
        // Third-party helpers are allowed, but the Fledge client belongs behind Host.
        let input = metadata(vec![
            package(
                "plugin-fledge",
                json!([{"name": "fledge-client"}, {"name": "serde"}]),
            ),
            package("fledge-client", json!([])),
            package("syrinx-plugin-api", json!([{"name": "syrinx-store"}])),
            package("syrinx-store", json!([])),
        ]);
        let violations = check_metadata(&input)?;
        assert_eq!(violations.len(), 2);
        assert!(
            violations
                .iter()
                .any(|message| message.contains("plugin-fledge -> fledge-client"))
        );
        assert!(
            violations
                .iter()
                .any(|message| message.contains("syrinx-plugin-api -> syrinx-store"))
        );
        Ok(())
    }

    #[test]
    fn ignores_packages_outside_workspace_members() -> Result<(), String> {
        // External GPUI dependencies on Win32 do not make the application invalid.
        let mut input = metadata(vec![package("syrinx-app", json!([{"name": "gpui-pre"}]))]);
        input["packages"] = json!([
            package("syrinx-app", json!([{"name": "gpui-pre"}])),
            package("gpui-pre", json!([{"name": "windows"}]))
        ]);
        assert!(check_metadata(&input)?.is_empty());
        Ok(())
    }

    #[test]
    fn rejects_missing_unknown_and_duplicate_members() {
        // Bad membership must fail parsing, not silently omit architectural checks.
        for members in [
            json!(null),
            json!([]),
            json!([7]),
            json!(["missing"]),
            json!(["workspace:syrinx-core", "workspace:syrinx-core"]),
        ] {
            let mut input = metadata(vec![package("syrinx-core", json!([]))]);
            input["workspace_members"] = members;
            assert!(check_metadata(&input).is_err());
        }
    }

    #[test]
    fn rejects_malformed_packages_and_dependencies() {
        // A renamed import still requires its real package name in Cargo metadata.
        for packages in [
            json!([{"id": "workspace:syrinx-core", "dependencies": []}]),
            json!([package("syrinx-core", json!(null))]),
            json!([package("syrinx-core", json!([{"rename": "widgets"}]))]),
            json!([
                package("syrinx-core", json!([])),
                package("syrinx-core", json!([]))
            ]),
        ] {
            let mut input = metadata(vec![package("syrinx-core", json!([]))]);
            input["packages"] = packages;
            assert!(check_metadata(&input).is_err());
        }
    }
}
