//! Keeps every deferred-work marker in Syrinx's own code tied to a GitHub issue listed in the register.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// File holding the deferred-work register, relative to the workspace root.
pub const REGISTER: &str = "docs/dev/deferred-work.md";

/// Heading that starts the register's table.
const REGISTER_HEADING: &str = "## Deferred work";

/// Prefix of every register identifier, a GitHub issue number as in `#12`.
const ID_PREFIX: &str = "#";

/// Marker words that record deferred work. Built in pieces so this file does not flag itself.
const MARKERS: [&str; 3] = [
    concat!("TO", "DO"),
    concat!("FIX", "ME"),
    concat!("HA", "CK"),
];

/// Directories under the workspace root that hold Syrinx-authored source.
const SOURCE_ROOTS: [&str; 1] = ["crates"];

/// Check every Syrinx-authored Rust file against the deferred-work register.
pub fn check(root: &Path) -> Result<Vec<String>, String> {
    let register = fs::read_to_string(root.join(REGISTER))
        .map_err(|error| format!("Could not read {REGISTER}: {error}"))?;
    let registered = registered_ids(&register);

    let mut violations = Vec::new();
    for file in rust_sources(root)? {
        let text = fs::read_to_string(&file)
            .map_err(|error| format!("Could not read {}: {error}", file.display()))?;
        let relative = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .display()
            .to_string();
        violations.extend(file_violations(&relative, &text, &registered));
    }
    Ok(violations)
}

/// Collect `#n` identifiers from table rows in the register section.
fn registered_ids(register: &str) -> BTreeSet<String> {
    let Some(start) = register.find(REGISTER_HEADING) else {
        return BTreeSet::new();
    };
    let section = &register[start + REGISTER_HEADING.len()..];
    let end = section.find("\n## ").unwrap_or(section.len());

    section[..end]
        .lines()
        .filter_map(|line| line.trim().strip_prefix('|'))
        .filter_map(|row| row.split('|').next())
        .map(str::trim)
        .filter(|cell| is_register_id(cell))
        .map(str::to_owned)
        .collect()
}

/// Report each marker that lacks a `(#n)` tag or names an unregistered entry.
fn file_violations(path: &str, text: &str, registered: &BTreeSet<String>) -> Vec<String> {
    let mut violations = Vec::new();
    for (index, line) in text.lines().enumerate() {
        for marker in markers_in(line) {
            let location = format!("{path}:{}", index + 1);
            match marker.id {
                None => violations.push(format!(
                    "{location}: {} needs a register tag, e.g. {}({ID_PREFIX}1), listed under \"{REGISTER_HEADING}\" in {REGISTER}.",
                    marker.word, marker.word
                )),
                Some(id) if !registered.contains(id) => violations.push(format!(
                    "{location}: {id} is not listed under \"{REGISTER_HEADING}\" in {REGISTER}."
                )),
                Some(_) => {}
            }
        }
    }
    violations
}

/// One marker word found on a line, with its tag if it has one.
struct Marker<'line> {
    word: &'static str,
    id: Option<&'line str>,
}

/// Find whole-word markers on one line and read an immediately following `(#n)` tag.
fn markers_in(line: &str) -> Vec<Marker<'_>> {
    let mut found = Vec::new();
    for word in MARKERS {
        for (offset, _) in line.match_indices(word) {
            let after = offset + word.len();
            if !is_word_boundary(line, offset, after) {
                continue;
            }
            found.push(Marker {
                word,
                id: tag_after(&line[after..]),
            });
        }
    }
    found
}

/// True when the match is not part of a longer identifier.
fn is_word_boundary(line: &str, start: usize, end: usize) -> bool {
    let before = line[..start].chars().next_back();
    let after = line[end..].chars().next();
    !before.is_some_and(is_identifier_char) && !after.is_some_and(is_identifier_char)
}

fn is_identifier_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// Read `(#n)` directly after a marker.
fn tag_after(rest: &str) -> Option<&str> {
    let inner = rest.strip_prefix('(')?;
    let id = &inner[..inner.find(')')?];
    is_register_id(id).then_some(id)
}

fn is_register_id(text: &str) -> bool {
    text.strip_prefix(ID_PREFIX).is_some_and(|number| {
        !number.is_empty() && number.chars().all(|digit| digit.is_ascii_digit())
    })
}

/// List Rust files under the source roots, skipping build output.
fn rust_sources(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut pending: Vec<PathBuf> = SOURCE_ROOTS.iter().map(|dir| root.join(dir)).collect();
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory)
            .map_err(|error| format!("Could not list {}: {error}", directory.display()))?;
        for entry in entries {
            let path = entry
                .map_err(|error| format!("Could not list {}: {error}", directory.display()))?
                .path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name != "target") {
                    pending.push(path);
                }
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn todo() -> &'static str {
        MARKERS[0]
    }

    fn register(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn reads_ids_only_from_the_register_section() {
        let text = "## Other\n| #9 | elsewhere |\n## Deferred work\n| Issue | Work | Status |\n| --- | --- | --- |\n| #1 | first | Open |\n| #22 | second | Open |\n| DW-5 | old tag | Open |\n## Next\n| #3 | after |\n";
        assert_eq!(registered_ids(text), register(&["#1", "#22"]));
    }

    #[test]
    fn missing_register_means_no_ids() {
        assert!(registered_ids("# Deferred work\n").is_empty());
    }

    #[test]
    fn accepts_a_registered_tag() {
        let line = format!("// {}(#1): resume suspended tabs", todo());
        assert!(file_violations("a.rs", &line, &register(&["#1"])).is_empty());
    }

    #[test]
    fn rejects_an_untagged_marker() {
        let line = format!("// {}: later", todo());
        let violations = file_violations("a.rs", &line, &register(&["#1"]));
        assert_eq!(violations.len(), 1);
        assert!(violations[0].starts_with("a.rs:1:"));
    }

    #[test]
    fn rejects_an_unregistered_tag() {
        let line = format!("// {}(#7): later", MARKERS[1]);
        let violations = file_violations("a.rs", &line, &register(&["#1"]));
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("#7"));
    }

    #[test]
    fn ignores_markers_inside_longer_identifiers() {
        let line = format!("let {}_count = MY{}S;", todo(), todo());
        assert!(file_violations("a.rs", &line, &BTreeSet::new()).is_empty());
    }

    #[test]
    fn rejects_malformed_tags() {
        // An old `DW-n` tag is malformed now, even if a stray register row named it.
        for tag in ["(#)", "(#x)", "(# 1)", "(DW-1)", " (#1)"] {
            let line = format!("// {}{tag}", todo());
            assert_eq!(
                file_violations("a.rs", &line, &register(&["#1", "DW-1"])).len(),
                1,
                "{tag}"
            );
        }
    }
}
