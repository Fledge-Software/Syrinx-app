//! Holds the editor's npm packages to their audit (decision 12 of 2026-10-24): every lockfile
//! package must be listed in `docs/dev/codemirror-audit.md`, and a committed bundle must match it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::licences;
use crate::packaging::sha256;

/// The one folder npm packages may enter through, and the files the check reads.
const EDITOR_FOLDER: &str = "editor";
const MANIFEST: &str = "editor/package.json";
const LOCKFILE: &str = "editor/package-lock.json";
const BUNDLE: &str = "editor/dist/syrinx-editor.js";
const AUDIT: &str = "docs/dev/codemirror-audit.md";

/// The audit's machine-read parts: the package table and the bundle's hash line.
const PACKAGE_TABLE_HEADING: &str = "## Package table";
const BUNDLE_HASH_LABEL: &str = "Bundle SHA-256:";
const NO_BUNDLE: &str = "none";

/// Every package must come from npm's public registry with a SHA-512 integrity hash.
const REGISTRY: &str = "https://registry.npmjs.org/";
const INTEGRITY_PREFIX: &str = "sha512-";

/// npm manifests and lockfiles; any of these outside `editor/` would bring unaudited packages.
const NPM_FILES: [&str; 7] = [
    "package.json",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lock",
    "bun.lockb",
];

/// Folders the stray-manifest search never enters: build output, git, installed packages and
/// agent tooling, none of which ships.
const UNSEARCHED_FOLDERS: [&str; 4] = [".git", "target", "node_modules", ".claude"];

/// One package as the lockfile or the audit records it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Package {
    licence: String,
    integrity: String,
}

/// Packages keyed by name and exact version, so two versions of one name stay apart.
type Packages = BTreeMap<(String, String), Package>;

pub fn check(root: &Path) -> Result<(), String> {
    let allowed = licences::allowed_licences(root)?;
    let manifest = read_json(root, MANIFEST)?;
    let lockfile = read_json(root, LOCKFILE)?;
    let audit = read_text(root, AUDIT)?;

    let mut problems = stray_npm_files(root)?;
    let (locked, lockfile_problems) = locked_packages(&lockfile, &allowed)?;
    problems.extend(lockfile_problems);
    problems.extend(unpinned_dependencies(&manifest, &lockfile));
    problems.extend(audit_mismatches(&locked, &audited_packages(&audit)?));
    let bundle = bundle_state(root, &audit)?;
    problems.extend(bundle.problem);
    if !problems.is_empty() {
        return Err(format!(
            "JavaScript packages do not match {AUDIT}:\n  {}",
            problems.join("\n  ")
        ));
    }

    println!(
        "JavaScript packages passed: {} locked packages are audited; {}.",
        locked.len(),
        bundle.summary
    );
    Ok(())
}

fn read_text(root: &Path, relative: &str) -> Result<String, String> {
    fs::read_to_string(root.join(relative))
        .map_err(|error| format!("Could not read {relative}: {error}"))
}

fn read_json(root: &Path, relative: &str) -> Result<Value, String> {
    serde_json::from_str(&read_text(root, relative)?)
        .map_err(|error| format!("{relative} is not valid JSON: {error}"))
}

/// Every npm manifest or lockfile outside `editor/`.
fn stray_npm_files(root: &Path) -> Result<Vec<String>, String> {
    let mut problems: Vec<String> = npm_files(root)?
        .iter()
        .filter_map(|path| path.strip_prefix(root).ok())
        .filter(|relative| relative.parent() != Some(Path::new(EDITOR_FOLDER)))
        .map(|relative| {
            format!(
                "{}: npm packages enter only through {EDITOR_FOLDER}/",
                relative.display()
            )
        })
        .collect();
    problems.sort();
    Ok(problems)
}

/// Every npm manifest or lockfile under `root`, outside the unsearched folders.
fn npm_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for (path, is_folder) in folder_entries(&folder)? {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if is_folder && !UNSEARCHED_FOLDERS.contains(&name) {
                folders.push(path);
            } else if !is_folder && NPM_FILES.contains(&name) {
                found.push(path);
            }
        }
    }
    Ok(found)
}

/// A folder's entries, each with whether it is itself a folder (a link counts as a file).
fn folder_entries(folder: &Path) -> Result<Vec<(PathBuf, bool)>, String> {
    let failed = |error: std::io::Error| format!("Could not list {}: {error}", folder.display());
    fs::read_dir(folder)
        .map_err(failed)?
        .map(|entry| {
            let entry = entry.map_err(failed)?;
            let kind = entry.file_type().map_err(failed)?;
            Ok((entry.path(), kind.is_dir()))
        })
        .collect()
}

/// The lockfile's packages, and every entry that is not an exact, hashed, allowed registry package.
fn locked_packages(
    lockfile: &Value,
    allowed: &[String],
) -> Result<(Packages, Vec<String>), String> {
    // Version 3 is the only format whose `packages` map lists every installed package.
    if lockfile["lockfileVersion"].as_u64() != Some(3) {
        return Err(format!("{LOCKFILE} must use lockfileVersion 3"));
    }
    let entries = lockfile["packages"]
        .as_object()
        .ok_or_else(|| format!("{LOCKFILE} has no packages map"))?;

    let mut packages = Packages::new();
    let mut problems = Vec::new();
    for (path, entry) in entries.iter().filter(|(path, _)| !path.is_empty()) {
        let name = package_name(path);
        let version = entry["version"].as_str().unwrap_or_default().to_owned();
        let licence = entry["license"].as_str().unwrap_or_default().to_owned();
        let integrity = entry["integrity"].as_str().unwrap_or_default().to_owned();
        if let Some(problem) = locked_entry_problem(entry, &licence, &integrity, allowed) {
            problems.push(format!("{name} {version} ({path}): {problem}"));
        }
        packages.insert((name.to_owned(), version), Package { licence, integrity });
    }
    // serde_json keeps file order when another crate enables `preserve_order`; report sorted.
    problems.sort();
    Ok((packages, problems))
}

/// A nested `node_modules/a/node_modules/b` entry is package `b`.
fn package_name(path: &str) -> &str {
    path.rsplit_once("node_modules/")
        .map_or(path, |(_, name)| name)
}

/// Why one lockfile entry is refused, or `None` when it is acceptable.
fn locked_entry_problem(
    entry: &Value,
    licence: &str,
    integrity: &str,
    allowed: &[String],
) -> Option<String> {
    if entry["link"].as_bool() == Some(true) {
        return Some("a linked folder, not a registry package".to_owned());
    }
    let resolved = entry["resolved"].as_str().unwrap_or_default();
    if !resolved.starts_with(REGISTRY) {
        return Some(format!("resolved from `{resolved}`, not {REGISTRY}"));
    }
    if !integrity.starts_with(INTEGRITY_PREFIX) {
        return Some("no SHA-512 integrity hash".to_owned());
    }
    if !licences::satisfies(licence, allowed) {
        return Some(format!(
            "licence `{licence}` is not allowed by licences.toml"
        ));
    }
    None
}

/// Direct dependencies that are not exact versions, or that disagree with the lockfile.
fn unpinned_dependencies(manifest: &Value, lockfile: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    for section in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        let Some(dependencies) = manifest[section].as_object() else {
            continue;
        };
        for (name, requirement) in dependencies {
            let requirement = requirement.as_str().unwrap_or_default();
            let locked = lockfile["packages"][format!("node_modules/{name}")]["version"].as_str();
            if !is_exact_version(requirement) {
                problems.push(format!(
                    "{MANIFEST}: {section}.{name} is `{requirement}`, not an exact version"
                ));
            } else if locked != Some(requirement) {
                problems.push(format!(
                    "{MANIFEST}: {section}.{name} is {requirement} but {LOCKFILE} locks {}",
                    locked.unwrap_or("nothing")
                ));
            }
        }
    }
    problems
}

/// `1.2.3`: three dot-separated numbers and nothing else, so no range or tag can float.
fn is_exact_version(requirement: &str) -> bool {
    let parts: Vec<&str> = requirement.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

/// The rows of the audit's package table, read by their column names.
fn audited_packages(audit: &str) -> Result<Packages, String> {
    let mut lines = audit
        .lines()
        .map(str::trim)
        .skip_while(|line| *line != PACKAGE_TABLE_HEADING)
        .skip(1)
        .skip_while(|line| !line.starts_with('|'))
        .take_while(|line| line.starts_with('|'));
    let header = lines
        .next()
        .ok_or_else(|| format!("{AUDIT} has no table under `{PACKAGE_TABLE_HEADING}`"))?;
    let columns = TableColumns::find(&table_cells(header))?;

    let mut packages = Packages::new();
    for row in lines.map(table_cells).filter(|cells| !is_separator(cells)) {
        let cell = |index: usize| row.get(index).cloned().unwrap_or_default();
        let key = (cell(columns.package), cell(columns.version));
        let package = Package {
            licence: cell(columns.licence),
            integrity: cell(columns.integrity),
        };
        if packages.insert(key.clone(), package).is_some() {
            return Err(format!("{AUDIT} lists {} {} twice", key.0, key.1));
        }
    }
    Ok(packages)
}

/// Where the four checked columns sit in the package table.
struct TableColumns {
    package: usize,
    version: usize,
    licence: usize,
    integrity: usize,
}

impl TableColumns {
    fn find(header: &[String]) -> Result<Self, String> {
        let position = |name: &str| {
            header
                .iter()
                .position(|cell| cell == name)
                .ok_or_else(|| format!("{AUDIT}'s package table has no `{name}` column"))
        };
        Ok(Self {
            package: position("Package")?,
            version: position("Version")?,
            licence: position("Licence")?,
            integrity: position("Integrity")?,
        })
    }
}

/// A Markdown table row's cells, trimmed and without code backticks.
fn table_cells(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    inner
        .split('|')
        .map(|cell| cell.trim().trim_matches('`').to_owned())
        .collect()
}

fn is_separator(cells: &[String]) -> bool {
    cells.iter().all(|cell| {
        !cell.is_empty() && cell.chars().all(|character| matches!(character, '-' | ':'))
    })
}

/// Lockfile packages missing from the audit or recorded differently, and stale audit rows.
fn audit_mismatches(locked: &Packages, audited: &Packages) -> Vec<String> {
    let mut problems = Vec::new();
    for ((name, version), package) in locked {
        match audited.get(&(name.clone(), version.clone())) {
            None => problems.push(format!(
                "{name} {version} is locked but not in the audit's package table"
            )),
            Some(row) if row.licence != package.licence => problems.push(format!(
                "{name} {version}: the audit says licence `{}`, the lockfile `{}`",
                row.licence, package.licence
            )),
            Some(row) if row.integrity != package.integrity => problems.push(format!(
                "{name} {version}: the audit's integrity hash differs from the lockfile's"
            )),
            Some(_) => {}
        }
    }
    for (name, version) in audited.keys().filter(|key| !locked.contains_key(*key)) {
        problems.push(format!(
            "{name} {version} is in the audit but not locked; remove the stale row"
        ));
    }
    problems
}

/// What the bundle check found: one problem at most, and a phrase for the summary.
#[derive(Debug, PartialEq, Eq)]
struct BundleState {
    problem: Option<String>,
    summary: String,
}

/// Compare the committed bundle, if any, with the hash the audit records.
fn bundle_state(root: &Path, audit: &str) -> Result<BundleState, String> {
    let recorded = recorded_bundle_hash(audit)?;
    let path = root.join(BUNDLE);
    let actual = match fs::read(&path) {
        Ok(bytes) => Some(sha256::hex_digest(&bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Could not read {BUNDLE}: {error}")),
    };
    Ok(judge_bundle(&recorded, actual.as_deref()))
}

fn judge_bundle(recorded: &str, actual: Option<&str>) -> BundleState {
    let state = |problem: Option<String>, summary: &str| BundleState {
        problem,
        summary: summary.to_owned(),
    };
    match actual {
        None if recorded == NO_BUNDLE => state(None, "no bundle is committed yet"),
        None => state(
            Some(format!(
                "{AUDIT} records a bundle hash, but {BUNDLE} does not exist"
            )),
            "",
        ),
        Some(_) if recorded == NO_BUNDLE => state(
            Some(format!(
                "{BUNDLE} exists, but {AUDIT} records no bundle hash"
            )),
            "",
        ),
        Some(hash) if hash != recorded => state(
            Some(format!(
                "{BUNDLE} hashes to {hash}, but {AUDIT} records {recorded}"
            )),
            "",
        ),
        Some(_) => state(None, "the committed bundle matches its recorded hash"),
    }
}

/// The value after the audit's one `Bundle SHA-256:` label: `none` or 64 lowercase hex digits.
fn recorded_bundle_hash(audit: &str) -> Result<String, String> {
    let values: Vec<String> = audit
        .lines()
        .filter_map(|line| line.trim().strip_prefix(BUNDLE_HASH_LABEL))
        .map(|value| value.trim().trim_matches('`').to_owned())
        .collect();
    let [value] = values.as_slice() else {
        return Err(format!(
            "{AUDIT} must have exactly one `{BUNDLE_HASH_LABEL}` line"
        ));
    };
    let is_hash = value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if value != NO_BUNDLE && !is_hash {
        return Err(format!(
            "{AUDIT}: `{BUNDLE_HASH_LABEL}` must be `{NO_BUNDLE}` or a lowercase SHA-256, not `{value}`"
        ));
    }
    Ok(value.clone())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const HASH: &str = "sha512-AAAA";

    fn allowed() -> Vec<String> {
        vec!["MIT".to_owned(), "Apache-2.0".to_owned()]
    }

    fn lockfile(entries: Value) -> Value {
        let mut packages = json!({"": {"name": "syrinx-editor"}});
        for (path, entry) in entries.as_object().expect("an object") {
            packages[path] = entry.clone();
        }
        json!({"lockfileVersion": 3, "packages": packages})
    }

    fn registry_entry(name: &str, version: &str, licence: &str) -> Value {
        json!({
            "version": version,
            "resolved": format!("{REGISTRY}{name}/-/{name}-{version}.tgz"),
            "integrity": HASH,
            "license": licence,
        })
    }

    fn audit(rows: &[&str]) -> String {
        format!(
            "# Audit\n\n{PACKAGE_TABLE_HEADING}\n\n| Package | Version | Licence | Integrity |\n|---|---|---|---|\n{}\n\n{BUNDLE_HASH_LABEL} `none`\n",
            rows.join("\n")
        )
    }

    fn problems(lock: &Value, audit_text: &str) -> Vec<String> {
        let (locked, mut problems) =
            locked_packages(lock, &allowed()).expect("a version 3 lockfile");
        problems.extend(audit_mismatches(
            &locked,
            &audited_packages(audit_text).expect("a table"),
        ));
        problems
    }

    #[test]
    fn an_audited_package_passes() {
        let lock = lockfile(json!({"node_modules/crelt": registry_entry("crelt", "1.0.7", "MIT")}));
        let audit_text = audit(&["| `crelt` | 1.0.7 | MIT | `sha512-AAAA` |"]);
        assert_eq!(problems(&lock, &audit_text), Vec::<String>::new());
    }

    #[test]
    fn a_locked_package_missing_from_the_audit_fails() {
        let lock = lockfile(json!({
            "node_modules/crelt": registry_entry("crelt", "1.0.7", "MIT"),
            "node_modules/a/node_modules/style-mod": registry_entry("style-mod", "4.1.4", "MIT"),
        }));
        let audit_text = audit(&["| crelt | 1.0.7 | MIT | sha512-AAAA |"]);
        assert_eq!(
            problems(&lock, &audit_text),
            ["style-mod 4.1.4 is locked but not in the audit's package table"]
        );
    }

    #[test]
    fn another_version_hash_or_licence_than_the_audit_records_fails() {
        let lock = lockfile(json!({"node_modules/crelt": registry_entry("crelt", "1.0.8", "MIT")}));
        let stale = problems(&lock, &audit(&["| crelt | 1.0.7 | MIT | sha512-AAAA |"]));
        assert_eq!(stale.len(), 2, "{stale:?}");
        assert!(stale[1].contains("remove the stale row"), "{stale:?}");

        let lock = lockfile(json!({"node_modules/crelt": registry_entry("crelt", "1.0.7", "MIT")}));
        let hash = problems(&lock, &audit(&["| crelt | 1.0.7 | MIT | sha512-BBBB |"]));
        assert!(hash[0].contains("integrity hash differs"), "{hash:?}");
        let licence = problems(&lock, &audit(&["| crelt | 1.0.7 | ISC | sha512-AAAA |"]));
        assert!(
            licence[0].contains("the audit says licence `ISC`"),
            "{licence:?}"
        );
    }

    #[test]
    fn a_gpl_package_fails_even_when_audited() {
        let lock = lockfile(
            json!({"node_modules/copyleft": registry_entry("copyleft", "1.0.0", "GPL-3.0")}),
        );
        let found = problems(
            &lock,
            &audit(&["| copyleft | 1.0.0 | GPL-3.0 | sha512-AAAA |"]),
        );
        assert_eq!(
            found,
            [
                "copyleft 1.0.0 (node_modules/copyleft): licence `GPL-3.0` is not allowed by licences.toml"
            ]
        );
    }

    #[test]
    fn a_link_another_source_or_a_missing_hash_fails() {
        let mut git = registry_entry("crelt", "1.0.7", "MIT");
        git["resolved"] = json!("git+ssh://git@example.invalid/crelt.git");
        let mut unhashed = registry_entry("style-mod", "4.1.4", "MIT");
        unhashed["integrity"] = json!("sha1-AAAA");
        let lock = lockfile(json!({
            "node_modules/crelt": git,
            "node_modules/style-mod": unhashed,
            "node_modules/local": {"link": true, "resolved": "../local"},
        }));
        let (_, found) = locked_packages(&lock, &allowed()).expect("a version 3 lockfile");
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(
            found[0].contains("not https://registry.npmjs.org/"),
            "{found:?}"
        );
        assert!(found[1].contains("a linked folder"), "{found:?}");
        assert!(found[2].contains("no SHA-512 integrity hash"), "{found:?}");
    }

    #[test]
    fn an_older_lockfile_format_is_refused() {
        let error = locked_packages(&json!({"lockfileVersion": 2, "packages": {}}), &allowed())
            .expect_err("refused");
        assert!(error.contains("lockfileVersion 3"), "{error}");
    }

    #[test]
    fn a_range_or_a_version_the_lockfile_does_not_hold_fails() {
        let lock = lockfile(json!({"node_modules/crelt": registry_entry("crelt", "1.0.7", "MIT")}));
        let manifest = json!({
            "dependencies": {"crelt": "^1.0.7"},
            "devDependencies": {"esbuild": "0.28.2"},
        });
        let found = unpinned_dependencies(&manifest, &lock);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found[0].contains("`^1.0.7`, not an exact version"),
            "{found:?}"
        );
        assert!(found[1].contains("locks nothing"), "{found:?}");
        assert!(
            unpinned_dependencies(&json!({"dependencies": {"crelt": "1.0.7"}}), &lock).is_empty()
        );
    }

    #[test]
    fn exact_versions_are_three_numbers() {
        for exact in ["1.0.7", "0.28.2", "10.20.30"] {
            assert!(is_exact_version(exact), "{exact}");
        }
        for loose in [
            "^1.0.7",
            "~1.0",
            "1.0",
            "1.0.x",
            "latest",
            "1.0.7-beta.1",
            ">=1.0.0",
            "",
        ] {
            assert!(!is_exact_version(loose), "{loose}");
        }
    }

    #[test]
    fn the_audit_table_needs_its_columns_and_no_duplicates() {
        let missing = "## Package table\n\n| Package | Version | Licence |\n|---|---|---|\n";
        assert!(
            audited_packages(missing)
                .expect_err("refused")
                .contains("`Integrity`")
        );
        let twice = audit(&["| crelt | 1.0.7 | MIT | x |", "| crelt | 1.0.7 | MIT | x |"]);
        assert!(
            audited_packages(&twice)
                .expect_err("refused")
                .contains("twice")
        );
        assert!(audited_packages("# No table\n").is_err());
    }

    #[test]
    fn the_bundle_hash_activates_once_a_bundle_exists() {
        let hash = "a".repeat(64);
        assert_eq!(judge_bundle(NO_BUNDLE, None).problem, None);
        assert_eq!(judge_bundle(&hash, Some(&hash)).problem, None);
        let unrecorded = judge_bundle(NO_BUNDLE, Some(&hash))
            .problem
            .expect("refused");
        assert!(
            unrecorded.contains("records no bundle hash"),
            "{unrecorded}"
        );
        let missing = judge_bundle(&hash, None).problem.expect("refused");
        assert!(missing.contains("does not exist"), "{missing}");
        let changed = judge_bundle(&hash, Some(&"b".repeat(64)))
            .problem
            .expect("refused");
        assert!(
            changed.contains("but docs/dev/codemirror-audit.md records"),
            "{changed}"
        );
    }

    #[test]
    fn the_bundle_line_is_none_or_one_lowercase_hash() {
        assert_eq!(
            recorded_bundle_hash("Bundle SHA-256: `none`\n"),
            Ok("none".to_owned())
        );
        let hash = "0123456789abcdef".repeat(4);
        assert_eq!(
            recorded_bundle_hash(&format!("Bundle SHA-256: `{hash}`")),
            Ok(hash.clone())
        );
        for refused in [
            String::new(),
            "Bundle SHA-256: `none`\nBundle SHA-256: `none`\n".to_owned(),
            format!("Bundle SHA-256: `{}`", hash.to_uppercase()),
            "Bundle SHA-256: `pending`".to_owned(),
        ] {
            assert!(recorded_bundle_hash(&refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn a_committed_bundle_is_hashed_from_its_bytes() {
        let root = scratch("bundle");
        fs::create_dir_all(root.join("editor/dist")).expect("bundle folder");
        fs::write(root.join(BUNDLE), b"abc").expect("bundle");
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let state = bundle_state(&root, &format!("{BUNDLE_HASH_LABEL} `{abc}`")).expect("read");
        assert_eq!(state.problem, None);
        fs::remove_dir_all(root).expect("scratch removed");
    }

    #[test]
    fn an_npm_file_outside_the_editor_folder_fails() {
        let root = scratch("stray");
        for folder in ["editor", "crates/app", "target/debug", "node_modules/x"] {
            fs::create_dir_all(root.join(folder)).expect("folder");
        }
        for file in [
            "editor/package.json",
            "editor/package-lock.json",
            "crates/app/package.json",
            "yarn.lock",
            "target/debug/package.json",
            "node_modules/x/package.json",
        ] {
            fs::write(root.join(file), "{}").expect("file");
        }
        let found = stray_npm_files(&root).expect("listed");
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found.iter().any(|problem| problem.starts_with("yarn.lock")),
            "{found:?}"
        );
        assert!(
            found.iter().any(|problem| problem.contains("package.json")),
            "{found:?}"
        );
        fs::remove_dir_all(root).expect("scratch removed");
    }

    #[test]
    fn the_repository_packages_match_their_audit() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert_eq!(check(&root), Ok(()));
    }

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "syrinx-xtask-javascript-{name}-{}",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root).expect("old scratch removed");
        }
        fs::create_dir_all(&root).expect("scratch folder");
        root
    }
}
