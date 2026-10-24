//! Checks Cargo's workspace metadata so core logic cannot acquire shell dependencies.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::Value;

mod resolved_windows;
mod version_req;

struct Package {
    dependencies: BTreeSet<String>,
    declarations: Vec<Declaration>,
    /// Every entry of the package's own `[features]` table, such as `dep/feature`.
    feature_entries: Vec<String>,
    /// The names that table defines.
    feature_names: Vec<String>,
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
    check_metadata_against(metadata, &resolved_windows::THIRD_PARTY_NETWORK_FEATURES)
}

/// The same check with the approved third-party networking features passed in, so tests can
/// use synthetic graphs without the real list's entries reporting as stale.
fn check_metadata_against(
    metadata: &Value,
    allowed_network_features: &[resolved_windows::AllowedFeature],
) -> Result<Vec<String>, String> {
    // Validate membership before applying rules to the workspace packages.
    let workspace = read_workspace(metadata)?;
    let mut violations = BTreeSet::new();
    for (name, package) in &workspace {
        check_direct_dependencies(name, package, &workspace, &mut violations);
        check_windows_feature_lists(name, package, &mut violations);
        check_update_net_features(name, package, &mut violations);
        check_release_tool_feature(name, package, &mut violations);
        check_shell_paths(name, &workspace, &mut violations);
        check_network_declarations(name, package, &mut violations);
    }
    // Then Cargo's resolved graph, which third-party manifests also shape.
    let graph = read_resolved_graph(metadata)?;
    check_headless_network_reach(metadata, &graph, &mut violations)?;
    resolved_windows::check(metadata, &graph, allowed_network_features, &mut violations)?;
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
    let mut declarations = required_array(package, "dependencies", &name)?
        .iter()
        .map(|dependency| read_declaration(dependency, &name))
        .collect::<Result<Vec<_>, _>>()?;
    let dependencies = declarations
        .iter()
        .map(|declaration| declaration.name.clone())
        .collect();
    let (feature_names, feature_entries) = read_feature_table(package, &name)?;
    apply_feature_table(&mut declarations, &feature_entries);
    let package = Package {
        dependencies,
        declarations,
        feature_entries,
        feature_names,
    };
    if workspace.insert(name.clone(), package).is_some() {
        return Err(format!("ambiguous workspace package name: {name}"));
    }
    Ok(())
}

/// Count a dependency feature the package's own `[features]` table can switch on, such as
/// `windows/Win32_Networking_WinHttp` or `windows?/...`, as if its declaration requested it,
/// so every feature rule sees it. Entries name a dependency by its imported name.
fn apply_feature_table(declarations: &mut [Declaration], entries: &[String]) {
    for entry in entries {
        let Some((dependency, feature)) = entry.split_once('/') else {
            continue;
        };
        let dependency = normalize_name(dependency.strip_suffix('?').unwrap_or(dependency));
        for declaration in declarations
            .iter_mut()
            .filter(|declaration| imported_as(declaration).contains(&dependency))
        {
            declaration.features.insert(feature.to_owned());
        }
    }
}

/// The names a `[features]` entry may use for a dependency: its package name and any rename,
/// both normalised, as the release-tool rule compares them.
fn imported_as(declaration: &Declaration) -> Vec<String> {
    let mut names = vec![declaration.name.clone()];
    names.extend(declaration.alias.as_deref().map(normalize_name));
    names
}

/// The names and entries of a package's `[features]` table; a missing table means none.
fn read_feature_table(package: &Value, name: &str) -> Result<(Vec<String>, Vec<String>), String> {
    let table = read_feature_lists(package, name)?;
    let names = table.keys().cloned().collect();
    let entries = table.into_values().flatten().collect();
    Ok((names, entries))
}

/// Each feature of a package's `[features]` table with the entries it switches on.
fn read_feature_lists(
    package: &Value,
    name: &str,
) -> Result<BTreeMap<String, Vec<String>>, String> {
    let Some(features) = package.get("features") else {
        return Ok(BTreeMap::new());
    };
    let table = features
        .as_object()
        .ok_or_else(|| format!("{name}.features must be an object"))?;
    let mut lists = BTreeMap::new();
    for (feature, enabled) in table {
        let list = enabled
            .as_array()
            .ok_or_else(|| format!("{name}.features.{feature} must be an array"))?
            .iter()
            .map(|entry| {
                entry
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("{name}.features.{feature} must hold strings"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        lists.insert(feature.clone(), list);
    }
    Ok(lists)
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
                "{name} -> {dependency}: only syrinx-app and syrinx-update-net may use the update verifier (P1-39)"
            ));
        }
        if dependency == UPDATE_NET && name != "syrinx-app" {
            violations.insert(format!(
                "{name} -> {dependency}: only syrinx-app may use the update download (P1-39c)"
            ));
        }
        check_fulltext_quarantine(name, dependency, violations);
        check_plugin_dependency(name, dependency, workspace, violations);
    }
}

/// The P1-23 bake-off harness, which no product crate may use.
const FULLTEXT_BENCH: &str = "fulltext-bench";

/// Approved only for the bake-off until P1-24's production adapter (#53) brings it into Syrinx.
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
            "{name} -> {dependency}: only fulltext-bench may depend on Tantivy until #53 brings it into Syrinx"
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

/// P1-39c's owner-pressed update download, the one crate that may make a network call.
const UPDATE_NET: &str = "syrinx-update-net";

/// The crates that may depend on the verifier: P1-39's change 1.
const UPDATE_VERIFIER_USERS: [&str; 2] = ["syrinx-app", UPDATE_NET];

/// The verifier's feature that links Windows key storage for the release tool only.
const RELEASE_TOOL_FEATURE: &str = "release-tool";

/// The only `windows` features the update verifier may request: CNG and its NTSTATUS types.
const UPDATE_VERIFIER_WINDOWS_FEATURES: [&str; 2] =
    ["Win32_Foundation", "Win32_Security_Cryptography"];

/// WinHTTP, the one exception to D9's no-network rule (P1-39's change 3).
const WINHTTP_FEATURE: &str = "Win32_Networking_WinHttp";

/// The only `windows` features the update download may request (P1-39's change 2).
const UPDATE_NET_WINDOWS_FEATURES: [&str; 1] = [WINHTTP_FEATURE];

/// Each crate besides `syrinx-platform` that may use `windows`, with its exact feature list.
const WINDOWS_FEATURE_ALLOWLISTS: [(&str, &str, &[&str]); 2] = [
    (
        UPDATE_VERIFIER,
        "the update verifier",
        &UPDATE_VERIFIER_WINDOWS_FEATURES,
    ),
    (
        UPDATE_NET,
        "the update download",
        &UPDATE_NET_WINDOWS_FEATURES,
    ),
];

fn check_windows_feature_lists(name: &str, package: &Package, violations: &mut BTreeSet<String>) {
    // Keep each update crate to its own namespaces, with the default features off.
    let Some((_, role, allowed)) = WINDOWS_FEATURE_ALLOWLISTS
        .iter()
        .find(|(crate_name, _, _)| *crate_name == name)
    else {
        return;
    };
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
            if !allowed.contains(&feature.as_str()) {
                violations.insert(format!(
                    "{name} -> windows: feature {feature} is not allowed for {role}"
                ));
            }
        }
    }
}

/// The update download has no `[features]` table, so no build can switch on another endpoint.
fn check_update_net_features(name: &str, package: &Package, violations: &mut BTreeSet<String>) {
    if name == UPDATE_NET && !package.feature_names.is_empty() {
        violations.insert(format!(
            "{name}: the update download may not define features ({})",
            package.feature_names.join(", ")
        ));
    }
}

/// Refuse any dependency that turns on the release tool's key-storage signer: Syrinx itself
/// must never link code that opens Windows key storage (`docs/update-format.md`).
fn check_release_tool_feature(name: &str, package: &Package, violations: &mut BTreeSet<String>) {
    let verifier = package
        .declarations
        .iter()
        .filter(|declaration| declaration.name == UPDATE_VERIFIER);
    let mut imported_as = BTreeSet::new();
    let mut enabled = false;
    for declaration in verifier {
        enabled |= declaration.features.contains(RELEASE_TOOL_FEATURE);
        // Cargo treats `-` and `_` alike in names, so both sides are compared normalised.
        imported_as.extend(self::imported_as(declaration));
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

/// The platform crate owns Win32; the update crates may also use `windows`, each with its own
/// feature list.
fn may_use_platform_library(name: &str, dependency: &str) -> bool {
    name == "syrinx-platform"
        || (matches!(name, UPDATE_VERIFIER | UPDATE_NET) && dependency == "windows")
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
        "syrinx-core"
            | "syrinx-store"
            | "syrinx-search"
            | "syrinx-plugin-api"
            | "syrinx-update"
            | UPDATE_NET
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

/// `windows` feature prefixes that expose Windows networking, HTTP or remote-call APIs: URL
/// monikers download, RPC and WinRM reach other machines, and the web authentication broker
/// opens a sign-in page.
const NETWORK_WINDOWS_FEATURES: [&str; 9] = [
    "Win32_Networking",
    "Win32_NetworkManagement",
    "Win32_Web",
    "Win32_System_Com_Urlmon",
    "Win32_System_Rpc",
    "Win32_System_RemoteManagement",
    "Networking",
    "Web",
    "Security_Authentication_Web",
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
            .filter(|feature| !is_approved_network_feature(name, &declaration.name, feature))
        {
            violations.insert(format!(
                "{name} -> {}: feature {feature} exposes Windows networking (D9)",
                declaration.name
            ));
        }
    }
}

/// D9's one exception: WinHTTP through `windows`, in the update download alone (owner, 2026-10-13).
fn is_approved_network_feature(name: &str, dependency: &str, feature: &str) -> bool {
    name == UPDATE_NET && dependency == "windows" && feature == WINHTTP_FEATURE
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
    graph: &ResolvedGraph,
    violations: &mut BTreeSet<String>,
) -> Result<(), String> {
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
        if let Some(path) = network_path(id, graph) {
            violations.insert(format!(
                "{}: a headless crate reaches a network client (D9)",
                path.join(" -> ")
            ));
        }
    }
    Ok(())
}

/// Package names, resolved dependency edges and resolved features, keyed by Cargo's package ID.
struct ResolvedGraph {
    names: BTreeMap<String, String>,
    edges: BTreeMap<String, Vec<String>>,
    features: BTreeMap<String, BTreeSet<String>>,
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
    let mut features = BTreeMap::new();
    for node in required_array(resolve, "nodes", "cargo metadata.resolve")? {
        let id = required_string(node, "id", "resolve node")?;
        let dependencies = required_array(node, "deps", "resolve node")?
            .iter()
            .map(|dependency| required_string(dependency, "pkg", "resolve dependency"))
            .map(|pkg| pkg.map(str::to_owned))
            .collect::<Result<Vec<_>, _>>()?;
        edges.insert(id.to_owned(), dependencies);
        features.insert(id.to_owned(), read_node_features(node, id)?);
    }
    Ok(ResolvedGraph {
        names,
        edges,
        features,
    })
}

/// The features Cargo switched on for one resolved package, after unifying every request.
fn read_node_features(node: &Value, id: &str) -> Result<BTreeSet<String>, String> {
    required_array(node, "features", &format!("resolve node {id}"))?
        .iter()
        .map(|feature| {
            feature
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("resolve node {id}.features must hold strings"))
        })
        .collect()
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

    use super::check_metadata_against;
    use super::resolved_windows::AllowedFeature;

    /// The policy with no third-party networking approved, so synthetic graphs stand alone.
    fn check_metadata(metadata: &Value) -> Result<Vec<String>, String> {
        check_metadata_against(metadata, &[])
    }

    fn package(name: &str, dependencies: Value) -> Value {
        json!({
            "id": format!("workspace:{name}"),
            "name": name,
            "version": "0.1.0",
            "dependencies": dependencies
        })
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
        json!({"id": format!("workspace:{name}"), "deps": deps, "features": []})
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
            // A rename with an underscore, written either way in the feature entry.
            (
                json!({"sign": ["my_verifier/release-tool"]}),
                json!("my_verifier"),
            ),
            (
                json!({"sign": ["my-verifier?/release-tool"]}),
                json!("my_verifier"),
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

    fn update_net(dependencies: Value) -> Value {
        package("syrinx-update-net", dependencies)
    }

    fn winhttp(features: Value) -> Value {
        json!({
            "name": "windows",
            "req": "=0.62.2",
            "uses_default_features": false,
            "features": features
        })
    }

    #[test]
    fn permits_the_update_download_its_one_network_feature() -> Result<(), String> {
        // P1-39's changes 1 to 3: the verifier, `windows` and WinHTTP, for the app alone to use.
        let input = metadata(vec![
            update_net(json!([
                {"name": "syrinx-update"},
                winhttp(json!(["Win32_Networking_WinHttp"]))
            ])),
            package(
                "syrinx-app",
                json!([{"name": "syrinx-update-net"}, {"name": "syrinx-update"}]),
            ),
            package("syrinx-update", json!([])),
        ]);
        assert!(check_metadata(&input)?.is_empty());
        Ok(())
    }

    #[test]
    fn confines_winhttp_to_the_update_download() -> Result<(), String> {
        // No other crate gets WinHTTP, and the download gets nothing else.
        let input = metadata(vec![
            package(
                "syrinx-platform",
                json!([{"name": "windows", "features": ["Win32_Networking_WinHttp"]}]),
            ),
            package(
                "syrinx-app",
                json!([{"name": "windows-sys", "features": ["Win32_Networking_WinHttp"]}]),
            ),
            update_net(json!([
                winhttp(json!([
                    "Win32_Networking_WinHttp",
                    "Win32_Networking_WinSock",
                    "Win32_Foundation"
                ])),
                {"name": "windows", "kind": "dev"},
                {"name": "reqwest"}
            ])),
        ]);
        let violations = check_metadata(&input)?;
        let expected = [
            "syrinx-platform -> windows: feature Win32_Networking_WinHttp exposes Windows networking (D9)",
            "syrinx-app -> windows-sys: only syrinx-platform may directly depend",
            "syrinx-app -> windows-sys: feature Win32_Networking_WinHttp exposes Windows networking (D9)",
            "syrinx-update-net -> windows: feature Win32_Networking_WinSock is not allowed for the update download",
            "syrinx-update-net -> windows: feature Win32_Networking_WinSock exposes Windows networking (D9)",
            "syrinx-update-net -> windows: feature Win32_Foundation is not allowed for the update download",
            "syrinx-update-net -> windows: default features must be disabled",
            "syrinx-update-net -> reqwest: Syrinx's crates may not use a network client (D9)",
        ];
        for message in expected {
            assert!(
                violations.iter().any(|found| found.starts_with(message)),
                "{message}: {violations:?}"
            );
        }
        assert_eq!(violations.len(), expected.len(), "{violations:?}");
        Ok(())
    }

    #[test]
    fn only_the_app_may_use_the_update_download() -> Result<(), String> {
        // In every dependency kind, so not even a test or a build tool can make the calls.
        let input = metadata(vec![
            update_net(json!([])),
            package(
                "syrinx-ui",
                json!([{"name": "syrinx-update-net", "kind": "dev"}]),
            ),
            package(
                "xtask",
                json!([{"name": "syrinx_update_net", "kind": "build"}]),
            ),
            package("syrinx-update", json!([{"name": "syrinx-update-net"}])),
        ]);
        let violations = check_metadata(&input)?;
        for user in ["syrinx-ui", "xtask", "syrinx-update"] {
            assert!(
                violations.iter().any(|message| message
                    .starts_with(&format!("{user} -> syrinx-update-net: only syrinx-app"))),
                "{user}: {violations:?}"
            );
        }
        assert_eq!(violations.len(), 3, "{violations:?}");
        Ok(())
    }

    #[test]
    fn the_update_download_defines_no_features() -> Result<(), String> {
        // A feature could otherwise switch on another endpoint in a shipped build.
        let mut download = update_net(json!([]));
        download["features"] = json!({"test-endpoint": []});
        let violations = check_metadata(&metadata(vec![download]))?;
        assert_eq!(
            violations,
            ["syrinx-update-net: the update download may not define features (test-endpoint)"]
        );
        let mut empty = update_net(json!([]));
        empty["features"] = json!({});
        assert!(check_metadata(&metadata(vec![empty]))?.is_empty());
        Ok(())
    }

    /// A package whose own `[features]` table is `features`.
    fn with_features(name: &str, dependencies: Value, features: Value) -> Value {
        let mut package = package(name, dependencies);
        package["features"] = features;
        package
    }

    #[test]
    fn feature_tables_cannot_switch_on_networking_or_unlisted_features() -> Result<(), String> {
        // `dep/feature` and `dep?/feature` reach a dependency without its declaration naming it.
        let windows = json!([{"name": "windows", "uses_default_features": false}]);
        let cases = [
            (
                with_features(
                    "syrinx-update",
                    windows.clone(),
                    json!({"release-tool": ["windows/Win32_Networking_WinHttp"]}),
                ),
                vec![
                    // Violations come back sorted.
                    "syrinx-update -> windows: feature Win32_Networking_WinHttp exposes Windows networking (D9)",
                    "syrinx-update -> windows: feature Win32_Networking_WinHttp is not allowed for the update verifier",
                ],
            ),
            (
                with_features(
                    "syrinx-platform",
                    json!([{"name": "windows", "optional": true}]),
                    json!({"net": ["windows?/Win32_Networking_WinSock"]}),
                ),
                vec![
                    "syrinx-platform -> windows: feature Win32_Networking_WinSock exposes Windows networking (D9)",
                ],
            ),
            (
                // A renamed import, written with `_` in the table and `-` in the rename.
                with_features(
                    "syrinx-platform",
                    json!([{"name": "windows", "rename": "win-api"}]),
                    json!({"net": ["win_api/Web_Http"]}),
                ),
                vec![
                    "syrinx-platform -> windows: feature Web_Http exposes Windows networking (D9)",
                ],
            ),
            (
                with_features(
                    "syrinx-update-net",
                    json!([{"name": "windows", "uses_default_features": false, "features": ["Win32_Networking_WinHttp"]}]),
                    json!({}),
                ),
                vec![],
            ),
        ];
        for (input, expected) in cases {
            let violations = check_metadata(&metadata(vec![input]))?;
            assert_eq!(violations, expected, "{violations:?}");
        }

        // The download's own allowlist applies to its table too, which it may not have anyway.
        let download = with_features(
            "syrinx-update-net",
            windows,
            json!({"extra": ["windows/Win32_Foundation"]}),
        );
        let violations = check_metadata(&metadata(vec![download]))?;
        assert!(violations.iter().any(|message| {
            message.contains("feature Win32_Foundation is not allowed for the update download")
        }));
        assert!(
            violations
                .iter()
                .any(|message| message.contains("may not define features"))
        );
        Ok(())
    }

    #[test]
    fn the_update_download_stays_headless() -> Result<(), String> {
        // It cannot reach the shell, nor any network client through a third-party library.
        let mut input = metadata(vec![
            update_net(json!([{"name": "syrinx-platform"}, {"name": "helper"}])),
            package("syrinx-platform", json!([])),
        ]);
        input["packages"]
            .as_array_mut()
            .ok_or("synthetic packages")?
            .extend([package("helper", json!([])), package("hyper", json!([]))]);
        input["resolve"]["nodes"] = json!([
            node("syrinx-update-net", &["syrinx-platform", "helper"]),
            node("syrinx-platform", &[]),
            node("helper", &["hyper"]),
            node("hyper", &[]),
        ]);
        let violations = check_metadata(&input)?;
        assert_eq!(violations.len(), 2, "{violations:?}");
        assert!(violations.iter().any(|message| message.contains(
            "syrinx-update-net -> syrinx-platform: core and plugin crates must not reach the shell"
        )));
        assert!(violations.iter().any(|message| message.contains(
            "syrinx-update-net -> helper -> hyper: a headless crate reaches a network client"
        )));
        Ok(())
    }

    /// A third-party package, identified as Cargo's registry IDs are, by name and version.
    fn versioned(name: &str, version: &str, dependencies: Value) -> Value {
        json!({
            "id": format!("{name}@{version}"),
            "name": name,
            "version": version,
            "dependencies": dependencies
        })
    }

    /// A resolved node with Cargo's unified feature list for that package.
    fn resolved(id: &str, dependencies: &[&str], features: &[&str]) -> Value {
        let deps: Vec<Value> = dependencies
            .iter()
            .map(|dependency| json!({"pkg": dependency}))
            .collect();
        json!({"id": id, "deps": deps, "features": features})
    }

    /// The update download and the platform crate's wry sharing one `windows 0.62.2` build,
    /// whose resolved features are `windows_features`; wry asks for `wry_features`.
    fn shared_windows_graph(
        windows_features: &[&str],
        wry_features: Value,
    ) -> Result<Value, String> {
        let mut input = metadata(vec![
            update_net(json!([winhttp(json!(["Win32_Networking_WinHttp"]))])),
            package("syrinx-platform", json!([{"name": "wry"}])),
        ]);
        extend_graph(
            &mut input,
            vec![
                versioned(
                    "wry",
                    "0.57.0",
                    json!([{"name": "windows", "req": "^0.62", "features": wry_features}]),
                ),
                versioned("windows", "0.62.2", json!([])),
            ],
            vec![
                resolved("workspace:syrinx-update-net", &["windows@0.62.2"], &[]),
                resolved("workspace:syrinx-platform", &["wry@0.57.0"], &[]),
                resolved("wry@0.57.0", &["windows@0.62.2"], &[]),
                resolved("windows@0.62.2", &[], windows_features),
            ],
        )?;
        Ok(input)
    }

    /// Adds third-party packages and their resolved nodes to a synthetic graph.
    fn extend_graph(
        input: &mut Value,
        packages: Vec<Value>,
        nodes: Vec<Value>,
    ) -> Result<(), String> {
        input["packages"]
            .as_array_mut()
            .ok_or("synthetic packages")?
            .extend(packages);
        input["resolve"]["nodes"]
            .as_array_mut()
            .ok_or("synthetic nodes")?
            .extend(nodes);
        Ok(())
    }

    #[test]
    fn the_resolved_windows_build_may_carry_only_winhttp() -> Result<(), String> {
        // WinHTTP and the `Win32_Networking` parent it implies, on the download's own build.
        let passing = shared_windows_graph(
            &[
                "Win32",
                "Win32_Foundation",
                "Win32_Networking",
                "Win32_Networking_WinHttp",
            ],
            json!(["Win32_Foundation"]),
        )?;
        assert!(check_metadata(&passing)?.is_empty());

        // A resolve node without its feature list cannot be checked, so it is an error.
        let mut missing = passing.clone();
        missing["resolve"]["nodes"][3]
            .as_object_mut()
            .ok_or("synthetic node")?
            .remove("features");
        assert!(check_metadata(&missing).is_err());
        Ok(())
    }

    #[test]
    fn a_third_party_crate_cannot_widen_the_shared_windows_build() -> Result<(), String> {
        // Cargo unifies wry's request into the build every Syrinx crate shares.
        let mut input = shared_windows_graph(
            &[
                "Win32_Foundation",
                "Win32_Networking",
                "Win32_Networking_WinHttp",
                "Win32_Networking_WinSock",
            ],
            json!(["Win32_Foundation", "Win32_Networking_WinSock"]),
        )?;
        // A third party's own `[features]` table reaches a binding too, here an optional one.
        let mut helper = versioned(
            "helper",
            "1.0.0",
            json!([{"name": "windows-sys", "req": "^0.61", "optional": true}]),
        );
        helper["features"] = json!({"net": ["windows-sys?/Win32_NetworkManagement_IpHelper"]});
        extend_graph(
            &mut input,
            vec![helper, versioned("windows-sys", "0.61.2", json!([]))],
            vec![
                resolved("helper@1.0.0", &["windows-sys@0.61.2"], &["net"]),
                resolved(
                    "windows-sys@0.61.2",
                    &[],
                    &[
                        "Win32_NetworkManagement",
                        "Win32_NetworkManagement_IpHelper",
                    ],
                ),
            ],
        )?;
        let violations = check_metadata(&input)?;
        assert_eq!(
            violations,
            [
                "windows 0.62.2: resolved feature Win32_Networking_WinSock exposes Windows networking (D9); requested by wry 0.57.0",
                "windows-sys 0.61.2: resolved feature Win32_NetworkManagement_IpHelper exposes Windows networking (D9); requested by helper 1.0.0",
            ]
        );
        Ok(())
    }

    #[test]
    fn every_resolved_windows_version_is_checked() -> Result<(), String> {
        // WinHTTP is approved only on the build the download uses, not on an older `windows`.
        let mut input =
            shared_windows_graph(&["Win32_Networking", "Win32_Networking_WinHttp"], json!([]))?;
        extend_graph(
            &mut input,
            vec![
                versioned(
                    "gpui-base",
                    "0.7.1",
                    json!([
                        {"name": "windows", "req": "^0.58", "features": ["Win32_Networking_WinHttp"]},
                        {"name": "windows-sys", "req": "^0.52"}
                    ]),
                ),
                versioned("windows", "0.58.0", json!([])),
                versioned("windows-sys", "0.52.0", json!([])),
            ],
            vec![
                resolved(
                    "gpui-base@0.7.1",
                    &["windows@0.58.0", "windows-sys@0.52.0"],
                    &[],
                ),
                resolved(
                    "windows@0.58.0",
                    &[],
                    &["Win32_Networking", "Win32_Networking_WinHttp"],
                ),
                // A namespace parent with no child is refused, with no declaration to blame.
                resolved(
                    "windows-sys@0.52.0",
                    &[],
                    &["Win32_Networking", "Networking"],
                ),
            ],
        )?;
        let violations = check_metadata(&input)?;
        assert_eq!(
            violations,
            [
                "windows 0.58.0: resolved feature Win32_Networking_WinHttp exposes Windows networking (D9); requested by gpui-base 0.7.1",
                "windows-sys 0.52.0: resolved feature Networking exposes Windows networking (D9); requested by no direct declaration",
                "windows-sys 0.52.0: resolved feature Win32_Networking exposes Windows networking (D9); requested by no direct declaration",
            ]
        );

        // Without the download in the workspace, no build may carry WinHTTP at all.
        let mut without_download = shared_windows_graph(&["Win32_Networking_WinHttp"], json!([]))?;
        without_download["workspace_members"] = json!(["workspace:syrinx-platform"]);
        let violations = check_metadata(&without_download)?;
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(violations[0].starts_with(
            "windows 0.62.2: resolved feature Win32_Networking_WinHttp exposes Windows networking"
        ));
        Ok(())
    }

    /// wry's WinSock on the shared build, approved from `requesters` only.
    fn approved_winsock(requesters: &'static [&'static str]) -> [AllowedFeature; 1] {
        [AllowedFeature {
            binding: "windows 0.62.2",
            feature: "Win32_Networking_WinSock",
            requesters,
        }]
    }

    fn wry_winsock_graph() -> Result<Value, String> {
        shared_windows_graph(
            &[
                "Win32_Networking",
                "Win32_Networking_WinHttp",
                "Win32_Networking_WinSock",
            ],
            json!(["Win32_Networking_WinSock"]),
        )
    }

    #[test]
    fn an_approved_third_party_feature_passes_only_from_its_listed_crates() -> Result<(), String> {
        // The owner's 2026-10-14 approval names each requesting crate and version.
        let input = wry_winsock_graph()?;
        let allowed = approved_winsock(&["wry 0.57.0"]);
        assert!(check_metadata_against(&input, &allowed)?.is_empty());

        // The same feature from a crate the approval does not list is refused.
        let mut widened = wry_winsock_graph()?;
        extend_graph(
            &mut widened,
            vec![versioned(
                "helper",
                "1.0.0",
                json!([{"name": "windows", "req": "^0.62", "features": ["Win32_Networking_WinSock"]}]),
            )],
            vec![resolved("helper@1.0.0", &["windows@0.62.2"], &[])],
        )?;
        assert_eq!(
            check_metadata_against(&widened, &allowed)?,
            [
                "windows 0.62.2: resolved feature Win32_Networking_WinSock is approved only from wry 0.57.0; also requested by helper 1.0.0"
            ]
        );
        Ok(())
    }

    #[test]
    fn a_stale_approval_fails() -> Result<(), String> {
        // A feature no longer resolved, or a listed crate that stopped asking, must be removed.
        let without_winsock =
            shared_windows_graph(&["Win32_Networking", "Win32_Networking_WinHttp"], json!([]))?;
        let allowed = approved_winsock(&["wry 0.57.0"]);
        assert_eq!(
            check_metadata_against(&without_winsock, &allowed)?,
            [
                "windows 0.62.2: approved feature Win32_Networking_WinSock is no longer resolved; remove the stale entry (docs/decisions.md, 2026-10-14)"
            ]
        );

        let allowed = approved_winsock(&["retired 2.0.0", "wry 0.57.0"]);
        assert_eq!(
            check_metadata_against(&wry_winsock_graph()?, &allowed)?,
            [
                "windows 0.62.2: approved feature Win32_Networking_WinSock lists retired 2.0.0, which no longer request it; remove the stale requester (docs/decisions.md, 2026-10-14)"
            ]
        );
        Ok(())
    }

    #[test]
    fn winhttp_is_approved_per_build_whoever_else_asks() -> Result<(), String> {
        // Cargo switches WinHTTP on for everyone sharing the download's build, so a third party
        // asking for it there changes nothing and passes; the source scan confines its use.
        let input = shared_windows_graph(
            &["Win32_Networking", "Win32_Networking_WinHttp"],
            json!(["Win32_Networking_WinHttp"]),
        )?;
        assert!(check_metadata(&input)?.is_empty());
        Ok(())
    }

    #[test]
    fn checks_winapi_modules_and_the_wider_network_namespaces() -> Result<(), String> {
        // winapi names modules; its address-struct modules declare no functions and pass.
        let mut input = shared_windows_graph(&["Win32_Networking_WinHttp"], json!([]))?;
        extend_graph(
            &mut input,
            vec![
                versioned(
                    "helper",
                    "1.0.0",
                    json!([
                        {"name": "winapi", "req": "0.3.9", "features": ["inaddr", "winhttp"]},
                        {"name": "windows-sys", "req": "^0.59", "features": ["Win32_System_Rpc"]}
                    ]),
                ),
                versioned("winapi", "0.3.9", json!([])),
                versioned("windows-sys", "0.59.0", json!([])),
            ],
            vec![
                resolved("helper@1.0.0", &["winapi@0.3.9", "windows-sys@0.59.0"], &[]),
                resolved("winapi@0.3.9", &[], &["in6addr", "inaddr", "winhttp"]),
                // WinRT's `Networking` holds types of its own, so a child does not excuse it.
                resolved(
                    "windows-sys@0.59.0",
                    &[],
                    &["Networking", "Networking_Sockets", "Win32_System_Rpc"],
                ),
            ],
        )?;
        assert_eq!(
            check_metadata(&input)?,
            [
                "winapi 0.3.9: resolved feature winhttp exposes Windows networking (D9); requested by helper 1.0.0",
                "windows-sys 0.59.0: resolved feature Networking exposes Windows networking (D9); requested by no direct declaration",
                "windows-sys 0.59.0: resolved feature Networking_Sockets exposes Windows networking (D9); requested by no direct declaration",
                "windows-sys 0.59.0: resolved feature Win32_System_Rpc exposes Windows networking (D9); requested by helper 1.0.0",
            ]
        );
        Ok(())
    }

    #[test]
    fn credits_only_manifests_that_activate_the_feature_on_that_build() -> Result<(), String> {
        // A declaration of another version, or a `[features]` entry whose own feature is off,
        // does not make a crate a requester of the resolved feature.
        let mut input = wry_winsock_graph()?;
        let mut helper = versioned(
            "helper",
            "1.0.0",
            json!([
                {"name": "windows", "req": "^0.58", "features": ["Win32_Networking_WinSock"]},
                {"name": "windows", "req": "^0.62", "rename": "windows-new"}
            ]),
        );
        helper["features"] = json!({"sockets": ["windows-new/Win32_Networking_WinSock"]});
        extend_graph(
            &mut input,
            vec![helper],
            vec![resolved("helper@1.0.0", &["windows@0.62.2"], &[])],
        )?;
        let allowed = approved_winsock(&["wry 0.57.0"]);
        assert!(check_metadata_against(&input, &allowed)?.is_empty());

        // Once its own `sockets` feature is on, the same entry makes it a new requester.
        input["resolve"]["nodes"][4]["features"] = json!(["sockets"]);
        let violations = check_metadata_against(&input, &allowed)?;
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(violations[0].ends_with("also requested by helper 1.0.0"));
        Ok(())
    }

    #[test]
    fn a_binding_or_requester_missing_from_the_graph_is_an_error() -> Result<(), String> {
        // Fail closed: a part of the graph that cannot be read is never treated as clean.
        let mut unresolved = shared_windows_graph(&["Win32_Networking_WinHttp"], json!([]))?;
        extend_graph(
            &mut unresolved,
            vec![versioned("windows", "0.58.0", json!([]))],
            vec![],
        )?;
        assert!(check_metadata(&unresolved).is_err());

        let mut orphan = wry_winsock_graph()?;
        extend_graph(
            &mut orphan,
            vec![],
            vec![resolved("ghost@1.0.0", &["windows@0.62.2"], &[])],
        )?;
        assert!(check_metadata(&orphan).is_err());
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
