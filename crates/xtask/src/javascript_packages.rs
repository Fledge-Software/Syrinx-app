//! Holds the editor's npm packages to their audit (decision 12 of 2026-10-24): every lockfile
//! package must be listed in `docs/dev/codemirror-audit.md`, and a committed bundle must match it.

mod audit;
mod lockfile;

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::licences;
use crate::packaging::sha256;

/// The only two npm files the repository may track, the bundle, and the files the check reads.
const MANIFEST: &str = "editor/package.json";
const LOCKFILE: &str = "editor/package-lock.json";
const NPM_SETTINGS: &str = "editor/.npmrc";
const BUNDLE_FOLDER: &str = "editor/dist/";
const BUNDLE: &str = "editor/dist/syrinx-editor.js";
const AUDIT: &str = "docs/dev/codemirror-audit.md";

/// npm manifests and lockfiles. npm prefers `npm-shrinkwrap.json` over `package-lock.json`,
/// and the others belong to other package managers, so each would bypass the audited lockfile.
const NPM_FILES: [&str; 7] = [
    "package.json",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lock",
    "bun.lockb",
];

pub fn check(root: &Path) -> Result<(), String> {
    let allowed = licences::allowed_licences(root)?;
    let manifest = read_json(root, MANIFEST)?;
    let lockfile = read_json(root, LOCKFILE)?;
    let audit_text = read_text(root, AUDIT)?;
    let entries = lockfile::read_entries(&lockfile)?;
    let rows = audit::rows(&audit_text)?;

    let mut problems = tracked_file_problems(&tracked_files(root)?);
    problems.extend(npm_settings_problem(&read_text(root, NPM_SETTINGS)?));
    problems.extend(dependency_problems(&manifest, &lockfile));
    problems.extend(lockfile::conflicting_duplicates(&entries));
    for entry in &entries {
        problems.extend(lockfile::entry_problems(entry, &allowed));
        problems.extend(audit::row_problems(entry, &rows));
    }
    problems.extend(audit::stale_rows(&entries, &rows));
    let recorded = audit::recorded_bundle_hash(&audit_text)?;
    let summary =
        judge_bundle(&recorded, bundle_hash(root)?.as_deref()).unwrap_or_else(|problem| {
            problems.push(problem);
            ""
        });
    // Two paths to one package report the same row problem once.
    problems.sort();
    problems.dedup();
    if !problems.is_empty() {
        return Err(format!(
            "JavaScript packages do not match {AUDIT}:\n  {}",
            problems.join("\n  ")
        ));
    }

    println!(
        "JavaScript packages passed: {} locked packages are audited; {summary}.",
        entries.len()
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

/// The paths git tracks, with forward slashes; untracked scratch files never count.
fn tracked_files(root: &Path) -> Result<Vec<String>, String> {
    // A hook's GIT_DIR or GIT_INDEX_FILE would point git at another repository's index.
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .map_err(|error| format!("Could not run git ls-files: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git ls-files failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Tracked npm files other than the editor's manifest and lockfile, and stray bundle files.
fn tracked_file_problems(files: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for path in files {
        let name = path.rsplit('/').next().unwrap_or(path);
        if NPM_FILES.contains(&name) && path != MANIFEST && path != LOCKFILE {
            problems.push(format!(
                "{path}: only {MANIFEST} and {LOCKFILE} may hold npm packages"
            ));
        }
        if path.starts_with(BUNDLE_FOLDER) && path != BUNDLE {
            problems.push(format!("{path}: {BUNDLE_FOLDER} holds only {BUNDLE}"));
        }
    }
    problems
}

/// `editor/.npmrc` must end up with `ignore-scripts=true`, so installing runs no package's script.
fn npm_settings_problem(settings: &str) -> Option<String> {
    // npm reads the file top to bottom, so the last setting wins.
    let ignore_scripts = settings
        .lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.starts_with('#') && !line.starts_with(';'))
        .filter_map(|line| line.split_once('='))
        .filter(|(key, _)| key.trim() == "ignore-scripts")
        .map(|(_, value)| value.trim())
        .next();
    if ignore_scripts == Some("true") {
        return None;
    }
    Some(format!("{NPM_SETTINGS} must set ignore-scripts=true"))
}

/// Direct dependencies that are not exact versions, or that disagree with the lockfile.
fn dependency_problems(manifest: &Value, lockfile: &Value) -> Vec<String> {
    let sections = [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ];
    let mut problems = Vec::new();
    for section in sections {
        let Some(dependencies) = manifest[section].as_object() else {
            continue;
        };
        for (name, requirement) in dependencies {
            let requirement = requirement.as_str().unwrap_or_default();
            problems.extend(dependency_problem(section, name, requirement, lockfile));
        }
    }
    problems
}

fn dependency_problem(
    section: &str,
    name: &str,
    requirement: &str,
    lockfile: &Value,
) -> Option<String> {
    if !is_exact_version(requirement) {
        return Some(format!(
            "{MANIFEST}: {section}.{name} is `{requirement}`, not an exact version"
        ));
    }
    let locked = lockfile["packages"][format!("node_modules/{name}")]["version"].as_str();
    if locked == Some(requirement) {
        return None;
    }
    Some(format!(
        "{MANIFEST}: {section}.{name} is {requirement} but {LOCKFILE} locks {}",
        locked.unwrap_or("nothing")
    ))
}

/// `1.2.3`: three dot-separated numbers and nothing else, so no range or tag can float.
fn is_exact_version(requirement: &str) -> bool {
    let parts: Vec<&str> = requirement.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

/// The committed bundle's SHA-256, or `None` when there is no bundle yet.
fn bundle_hash(root: &Path) -> Result<Option<String>, String> {
    match fs::read(root.join(BUNDLE)) {
        Ok(bytes) => Ok(Some(sha256::hex_digest(&bytes))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Could not read {BUNDLE}: {error}")),
    }
}

/// A phrase for the summary when the bundle and the recorded hash agree; otherwise the problem.
fn judge_bundle(recorded: &str, actual: Option<&str>) -> Result<&'static str, String> {
    let none_recorded = recorded == audit::NO_BUNDLE;
    match actual {
        None if none_recorded => Ok("no bundle is committed yet"),
        None => Err(format!(
            "{AUDIT} records a bundle hash, but {BUNDLE} does not exist"
        )),
        Some(_) if none_recorded => Err(format!(
            "{BUNDLE} exists, but {AUDIT} records no bundle hash"
        )),
        Some(hash) if hash != recorded => Err(format!(
            "{BUNDLE} hashes to {hash}, but {AUDIT} records {recorded}"
        )),
        Some(_) => Ok("the committed bundle matches its recorded hash"),
    }
}

#[cfg(test)]
mod tests;
