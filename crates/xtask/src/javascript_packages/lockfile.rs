//! Reads `editor/package-lock.json` entry by entry and refuses any that is not an exact,
//! hashed, allowed package from npm's registry.

use std::collections::BTreeMap;

use serde_json::Value;

use super::LOCKFILE;
use crate::licences;

/// Every package must come from npm's public registry with a SHA-512 integrity hash.
const REGISTRY: &str = "https://registry.npmjs.org/";
const INTEGRITY_PREFIX: &str = "sha512-";

/// One lockfile path's entry; two paths can hold the same name and version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedEntry {
    pub path: String,
    pub name: String,
    pub version: String,
    pub licence: String,
    pub integrity: String,
    pub resolved: String,
    pub has_install_script: bool,
    /// The entry's own `name` field, which npm writes only for an alias.
    pub declared_name: Option<String>,
    pub link: bool,
}

/// Every entry of a version 3 lockfile except the root project's own.
pub fn read_entries(lockfile: &Value) -> Result<Vec<LockedEntry>, String> {
    // Version 3 is the only format whose `packages` map lists every installed package.
    if lockfile["lockfileVersion"].as_u64() != Some(3) {
        return Err(format!("{LOCKFILE} must use lockfileVersion 3"));
    }
    let packages = lockfile["packages"]
        .as_object()
        .ok_or_else(|| format!("{LOCKFILE} has no packages map"))?;
    let text = |entry: &Value, field: &str| entry[field].as_str().unwrap_or_default().to_owned();
    Ok(packages
        .iter()
        .filter(|(path, _)| !path.is_empty())
        .map(|(path, entry)| LockedEntry {
            path: path.clone(),
            name: package_name(path).to_owned(),
            version: text(entry, "version"),
            licence: text(entry, "license"),
            integrity: text(entry, "integrity"),
            resolved: text(entry, "resolved"),
            has_install_script: entry["hasInstallScript"].as_bool() == Some(true),
            declared_name: entry["name"].as_str().map(str::to_owned),
            link: entry["link"].as_bool() == Some(true),
        })
        .collect())
}

/// `node_modules/a/node_modules/@scope/b` is package `@scope/b`.
fn package_name(path: &str) -> &str {
    path.rsplit_once("node_modules/")
        .map_or(path, |(_, name)| name)
}

/// The only tarball address npm's registry gives a name and version.
fn registry_tarball(name: &str, version: &str) -> String {
    let basename = name.rsplit('/').next().unwrap_or(name);
    format!("{REGISTRY}{name}/-/{basename}-{version}.tgz")
}

/// Why one entry is refused, naming its path; empty when it is acceptable.
pub fn entry_problems(entry: &LockedEntry, allowed: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut refuse = |problem: String| {
        problems.push(format!(
            "{} {} ({}): {problem}",
            entry.name, entry.version, entry.path
        ));
    };
    if entry.link {
        refuse("a linked folder, not a registry package".to_owned());
        return problems;
    }
    if let Some(declared) = entry
        .declared_name
        .as_ref()
        .filter(|declared| **declared != entry.name)
    {
        refuse(format!(
            "an alias of `{declared}`; install packages under their own name"
        ));
    }
    let expected = registry_tarball(&entry.name, &entry.version);
    if entry.resolved != expected {
        refuse(format!(
            "resolved from `{}`, not {expected}",
            entry.resolved
        ));
    }
    if !entry.integrity.starts_with(INTEGRITY_PREFIX) {
        refuse("no SHA-512 integrity hash".to_owned());
    }
    if !licences::satisfies(&entry.licence, allowed) {
        refuse(format!(
            "licence `{}` is not allowed by licences.toml",
            entry.licence
        ));
    }
    problems
}

/// A name and version locked at two paths with different sources, hashes or licences.
pub fn conflicting_duplicates(entries: &[LockedEntry]) -> Vec<String> {
    let mut first_seen: BTreeMap<(&str, &str), &LockedEntry> = BTreeMap::new();
    let mut problems = Vec::new();
    for entry in entries {
        let key = (entry.name.as_str(), entry.version.as_str());
        let Some(first) = first_seen.get(&key) else {
            first_seen.insert(key, entry);
            continue;
        };
        let same = first.integrity == entry.integrity
            && first.resolved == entry.resolved
            && first.licence == entry.licence;
        if !same {
            // Name the paths in sorted order, whichever order serde_json kept the map in.
            let (one, other) = if first.path <= entry.path {
                (&first.path, &entry.path)
            } else {
                (&entry.path, &first.path)
            };
            problems.push(format!(
                "{} {} is locked at {one} and {other} with a different source, hash or licence",
                entry.name, entry.version
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const HASH: &str = "sha512-AAAA";

    fn allowed() -> Vec<String> {
        vec!["MIT".to_owned(), "Apache-2.0".to_owned()]
    }

    fn registry_entry(name: &str, version: &str) -> Value {
        json!({
            "version": version,
            "resolved": registry_tarball(name, version),
            "integrity": HASH,
            "license": "MIT",
        })
    }

    fn entries(packages: Value) -> Vec<LockedEntry> {
        let mut all = json!({"": {"name": "syrinx-editor"}});
        for (path, entry) in packages.as_object().expect("an object") {
            all[path] = entry.clone();
        }
        read_entries(&json!({"lockfileVersion": 3, "packages": all})).expect("a version 3 lockfile")
    }

    fn problems_of(packages: Value) -> Vec<String> {
        entries(packages)
            .iter()
            .flat_map(|entry| entry_problems(entry, &allowed()))
            .collect()
    }

    #[test]
    fn scoped_and_nested_scoped_paths_name_their_package() {
        let found = entries(json!({
            "node_modules/@codemirror/view": registry_entry("@codemirror/view", "6.43.15"),
            "node_modules/a/node_modules/@scope/b": registry_entry("@scope/b", "1.0.0"),
        }));
        let names: Vec<&str> = found.iter().map(|entry| entry.name.as_str()).collect();
        assert!(
            names.contains(&"@codemirror/view") && names.contains(&"@scope/b"),
            "{names:?}"
        );
        assert_eq!(
            registry_tarball("@codemirror/view", "6.43.15"),
            "https://registry.npmjs.org/@codemirror/view/-/view-6.43.15.tgz"
        );
        assert!(
            problems_of(json!({
                "node_modules/a/node_modules/@scope/b": registry_entry("@scope/b", "1.0.0"),
            }))
            .is_empty()
        );
    }

    #[test]
    fn a_second_path_with_another_source_and_hash_is_refused() {
        let mut evil = registry_entry("crelt", "1.0.7");
        evil["resolved"] = json!("https://registry.npmjs.org/evil/-/evil-1.0.7.tgz");
        evil["integrity"] = json!("sha512-EVIL");
        let found = entries(json!({
            "node_modules/crelt": registry_entry("crelt", "1.0.7"),
            "node_modules/@codemirror/view/node_modules/crelt": evil,
        }));
        let conflicts = conflicting_duplicates(&found);
        assert_eq!(conflicts.len(), 1, "{conflicts:?}");
        assert!(
            conflicts[0].contains("crelt 1.0.7 is locked at"),
            "{conflicts:?}"
        );
        let source = found
            .iter()
            .flat_map(|entry| entry_problems(entry, &allowed()))
            .collect::<Vec<_>>();
        assert!(
            source
                .iter()
                .any(|problem| problem.contains("/node_modules/crelt): resolved from")),
            "{source:?}"
        );

        let twin = entries(json!({
            "node_modules/crelt": registry_entry("crelt", "1.0.7"),
            "node_modules/a/node_modules/crelt": registry_entry("crelt", "1.0.7"),
        }));
        assert!(conflicting_duplicates(&twin).is_empty());
    }

    #[test]
    fn an_alias_or_another_tarball_address_is_refused() {
        let mut alias = registry_entry("crelt", "1.0.7");
        alias["name"] = json!("evil-crelt");
        alias["resolved"] = json!(registry_tarball("evil-crelt", "1.0.7"));
        let found = problems_of(json!({"node_modules/crelt": alias}));
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].contains("an alias of `evil-crelt`"), "{found:?}");
        assert!(
            found[1].contains("not https://registry.npmjs.org/crelt/-/crelt-1.0.7.tgz"),
            "{found:?}"
        );
    }

    #[test]
    fn a_link_a_file_source_or_a_bare_entry_is_refused() {
        let mut file = registry_entry("x", "1.0.0");
        file["resolved"] = json!("file:../x");
        let found = problems_of(json!({
            "node_modules/local": {"link": true, "resolved": "../local"},
            "node_modules/x": file,
            "node_modules/bare": {"version": "1.0.0", "license": "MIT"},
        }));
        assert_eq!(found.len(), 4, "{found:?}");
        assert!(
            found
                .iter()
                .any(|problem| problem.contains("(node_modules/local): a linked folder"))
        );
        assert!(
            found
                .iter()
                .any(|problem| problem.contains("resolved from `file:../x`"))
        );
        assert!(
            found
                .iter()
                .any(|problem| problem.contains("(node_modules/bare): resolved from ``"))
        );
        assert!(
            found
                .iter()
                .any(|problem| problem.contains("(node_modules/bare): no SHA-512"))
        );
    }

    #[test]
    fn licences_follow_the_spdx_operators() {
        let licensed = |licence: &str| {
            let mut entry = registry_entry("crelt", "1.0.7");
            entry["license"] = json!(licence);
            problems_of(json!({"node_modules/crelt": entry}))
        };
        assert!(licensed("(MIT OR Apache-2.0)").is_empty());
        assert_eq!(
            licensed("MIT AND GPL-3.0"),
            [
                "crelt 1.0.7 (node_modules/crelt): licence `MIT AND GPL-3.0` is not allowed by licences.toml"
            ]
        );
        assert_eq!(licensed("").len(), 1);
    }

    #[test]
    fn an_older_lockfile_format_is_refused() {
        let error =
            read_entries(&json!({"lockfileVersion": 2, "packages": {}})).expect_err("refused");
        assert!(error.contains("lockfileVersion 3"), "{error}");
    }
}
