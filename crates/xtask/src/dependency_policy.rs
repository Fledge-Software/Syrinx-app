//! Checks Cargo's workspace metadata so core logic cannot acquire shell dependencies.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::Value;

struct Package {
    dependencies: BTreeSet<String>,
    declarations: Vec<Declaration>,
}

/// One dependency declaration's requested features, kept for feature-restricted packages.
struct Declaration {
    name: String,
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
        check_shell_paths(name, &workspace, &mut violations);
    }
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
    let package = Package {
        dependencies,
        declarations,
    };
    if workspace.insert(name.clone(), package).is_some() {
        return Err(format!("ambiguous workspace package name: {name}"));
    }
    Ok(())
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
    Ok(Declaration {
        name,
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
        if is_gpui(dependency) && !matches!(name, "atlas-app" | "atlas-ui") {
            violations.insert(format!(
                "{name} -> {dependency}: only atlas-app and atlas-ui may directly depend on GPUI"
            ));
        }
        if is_platform_library(dependency) && !may_use_platform_library(name, dependency) {
            violations.insert(format!(
                "{name} -> {dependency}: only atlas-platform may directly depend on WebView2 or raw Win32"
            ));
        }
        if dependency == UPDATE_VERIFIER {
            violations.insert(format!(
                "{name} -> {dependency}: nothing may use the update verifier before P1-39 is approved"
            ));
        }
        check_plugin_dependency(name, dependency, workspace, violations);
    }
}

fn check_plugin_dependency(
    name: &str,
    dependency: &str,
    workspace: &Workspace,
    violations: &mut BTreeSet<String>,
) {
    // Plugins reach application services through their API, not another Atlas crate.
    if !is_atlas_dependency(dependency, workspace) {
        return;
    }
    if name == "atlas-plugin-api" {
        violations.insert(format!(
            "{name} -> {dependency}: atlas-plugin-api must not depend on another Atlas crate"
        ));
        return;
    }
    if name.starts_with("plugin-") && dependency != "atlas-plugin-api" {
        violations.insert(format!(
            "{name} -> {dependency}: a plugin's only Atlas dependency may be atlas-plugin-api"
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

/// The P1-38 update verifier, which no crate may use until P1-39's delivery is approved.
const UPDATE_VERIFIER: &str = "atlas-update";

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

/// The platform crate owns Win32; the update verifier may also call CNG through `windows`.
fn may_use_platform_library(name: &str, dependency: &str) -> bool {
    name == "atlas-platform" || (name == UPDATE_VERIFIER && dependency == "windows")
}

fn is_atlas_dependency(name: &str, workspace: &Workspace) -> bool {
    workspace.contains_key(name)
        || name.starts_with("atlas-")
        || name.starts_with("plugin-")
        || name == "fledge-client"
}

fn needs_shell_isolation(name: &str) -> bool {
    matches!(
        name,
        "atlas-core" | "atlas-store" | "atlas-search" | "atlas-plugin-api" | "atlas-update"
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
            "atlas-app" | "atlas-ui" | "atlas-platform"
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
        json!({"version": 1, "packages": packages, "workspace_members": members})
    }

    #[test]
    fn permits_shell_composition_and_headless_plugins() -> Result<(), String> {
        // The app can reach platform libraries through the designated workspace boundary.
        let input = metadata(vec![
            package(
                "atlas-app",
                json!([{"name": "atlas-ui"}, {"name": "atlas-platform"}]),
            ),
            package(
                "atlas-ui",
                json!([{"name": "gpui-pre"}, {"name": "gpui-component"}]),
            ),
            package(
                "atlas-platform",
                json!([{"name": "wry"}, {"name": "windows"}]),
            ),
            package(
                "atlas-core",
                json!([{"name": "atlas-store"}, {"name": "atlas-search"}]),
            ),
            package("atlas-store", json!([{"name": "rusqlite"}])),
            package("atlas-search", json!([{"name": "frizbee"}])),
            package(
                "plugin-notes",
                json!([{"name": "atlas-plugin-api"}, {"name": "serde"}]),
            ),
            package("atlas-plugin-api", json!([{"name": "serde"}])),
        ]);
        assert!(check_metadata(&input)?.is_empty());
        Ok(())
    }

    #[test]
    fn confines_the_update_verifier() -> Result<(), String> {
        // The verifier may call CNG through `windows`, but not WebView2, and nothing may use it yet.
        let permitted = metadata(vec![package(
            "atlas-update",
            json!([{
                "name": "windows",
                "uses_default_features": false,
                "features": ["Win32_Foundation", "Win32_Security_Cryptography"]
            }]),
        )]);
        assert!(check_metadata(&permitted)?.is_empty());

        let input = metadata(vec![
            package(
                "atlas-update",
                json!([
                    {"name": "windows", "uses_default_features": false},
                    {"name": "wry"},
                    {"name": "atlas-platform"}
                ]),
            ),
            package(
                "atlas-app",
                json!([{"name": "atlas-update", "kind": "dev"}]),
            ),
            package("atlas-platform", json!([])),
        ]);
        let violations = check_metadata(&input)?;
        assert_eq!(violations.len(), 3);
        assert!(
            violations
                .iter()
                .any(|message| message.contains("atlas-update -> wry"))
        );
        assert!(
            violations
                .iter()
                .any(|message| message.contains("atlas-app -> atlas-update"))
        );
        assert!(
            violations
                .iter()
                .any(|message| message.contains("atlas-update -> atlas-platform"))
        );
        Ok(())
    }

    #[test]
    fn restricts_the_update_verifier_to_cryptography_features() -> Result<(), String> {
        // A networking namespace or the default features must fail the dependency check.
        let input = metadata(vec![package(
            "atlas-update",
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
        assert_eq!(violations.len(), 2);
        assert!(
            violations
                .iter()
                .any(|message| message.contains("Win32_Networking_WinHttp"))
        );
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
            "atlas-core",
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
                .any(|message| message.contains("atlas-core -> gpui-pre"))
        );
        assert!(
            violations
                .iter()
                .any(|message| message.contains("atlas-core -> webview2-com-sys"))
        );
        assert!(
            violations
                .iter()
                .any(|message| message.contains("atlas-core -> windows-sys"))
        );
        Ok(())
    }

    #[test]
    fn rejects_workspace_facades_and_terminates_on_cycles() -> Result<(), String> {
        // A helper cannot smuggle shell dependencies into a headless crate.
        let input = metadata(vec![
            package("atlas-search", json!([{"name": "helper"}])),
            package(
                "helper",
                json!([{"name": "atlas-search"}, {"name": "atlas-ui"}]),
            ),
            package("atlas-ui", json!([{"name": "gpui-pre"}])),
        ]);
        let violations = check_metadata(&input)?;
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("atlas-search -> helper -> atlas-ui"));
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
            package("atlas-plugin-api", json!([{"name": "atlas-store"}])),
            package("atlas-store", json!([])),
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
                .any(|message| message.contains("atlas-plugin-api -> atlas-store"))
        );
        Ok(())
    }

    #[test]
    fn ignores_packages_outside_workspace_members() -> Result<(), String> {
        // External GPUI dependencies on Win32 do not make the application invalid.
        let mut input = metadata(vec![package("atlas-app", json!([{"name": "gpui-pre"}]))]);
        input["packages"] = json!([
            package("atlas-app", json!([{"name": "gpui-pre"}])),
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
            json!(["workspace:atlas-core", "workspace:atlas-core"]),
        ] {
            let mut input = metadata(vec![package("atlas-core", json!([]))]);
            input["workspace_members"] = members;
            assert!(check_metadata(&input).is_err());
        }
    }

    #[test]
    fn rejects_malformed_packages_and_dependencies() {
        // A renamed import still requires its real package name in Cargo metadata.
        for packages in [
            json!([{"id": "workspace:atlas-core", "dependencies": []}]),
            json!([package("atlas-core", json!(null))]),
            json!([package("atlas-core", json!([{"rename": "widgets"}]))]),
            json!([
                package("atlas-core", json!([])),
                package("atlas-core", json!([]))
            ]),
        ] {
            let mut input = metadata(vec![package("atlas-core", json!([]))]);
            input["packages"] = packages;
            assert!(check_metadata(&input).is_err());
        }
    }
}
