//! Reads the machine-read parts of `docs/dev/codemirror-audit.md`: its package table and its
//! bundle hash line, and compares each lockfile entry with its row.

use std::collections::BTreeMap;

use super::AUDIT;
use super::lockfile::LockedEntry;

const PACKAGE_TABLE_HEADING: &str = "## Package table";
pub const BUNDLE_HASH_LABEL: &str = "Bundle SHA-256:";
pub const NO_BUNDLE: &str = "none";

/// The Install script column: no script, or a script reviewed and accepted because npm never
/// runs it here (`editor/.npmrc` sets `ignore-scripts`).
const NO_SCRIPT: &str = "none";
const SCRIPT_NOT_RUN: &str = "not run";

/// One row of the package table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRow {
    licence: String,
    integrity: String,
    install_script: String,
}

/// Rows keyed by package name and exact version.
pub type AuditRows = BTreeMap<(String, String), AuditRow>;

/// The rows of the audit's package table, read by their column names.
pub fn rows(audit: &str) -> Result<AuditRows, String> {
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

    let mut rows = AuditRows::new();
    for cells in lines.map(table_cells).filter(|cells| !is_separator(cells)) {
        let cell = |index: usize| cells.get(index).cloned().unwrap_or_default();
        let key = (cell(columns.package), cell(columns.version));
        let row = AuditRow {
            licence: cell(columns.licence),
            integrity: cell(columns.integrity),
            install_script: cell(columns.install_script),
        };
        if rows.insert(key.clone(), row).is_some() {
            return Err(format!("{AUDIT} lists {} {} twice", key.0, key.1));
        }
    }
    Ok(rows)
}

/// Where the checked columns sit in the package table.
struct TableColumns {
    package: usize,
    version: usize,
    licence: usize,
    install_script: usize,
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
            install_script: position("Install script")?,
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

/// Whether a row is the table's `|---|---|` line under the header.
fn is_separator(cells: &[String]) -> bool {
    cells.iter().all(|cell| {
        !cell.is_empty() && cell.chars().all(|character| matches!(character, '-' | ':'))
    })
}

/// How one lockfile entry differs from its audit row; every difference is reported.
pub fn row_problems(entry: &LockedEntry, rows: &AuditRows) -> Vec<String> {
    let label = format!("{} {}", entry.name, entry.version);
    let Some(row) = rows.get(&(entry.name.clone(), entry.version.clone())) else {
        return vec![format!(
            "{label} is locked but not in the audit's package table"
        )];
    };
    let mut problems = Vec::new();
    if row.licence != entry.licence {
        problems.push(format!(
            "{label}: the audit says licence `{}`, the lockfile `{}`",
            row.licence, entry.licence
        ));
    }
    if row.integrity != entry.integrity {
        problems.push(format!(
            "{label}: the audit's integrity hash differs from the lockfile's at {}",
            entry.path
        ));
    }
    problems.extend(install_script_problem(
        &label,
        entry.has_install_script,
        &row.install_script,
    ));
    problems
}

/// A script must be marked `not run` after review, and a package without one `none`.
fn install_script_problem(label: &str, has_script: bool, recorded: &str) -> Option<String> {
    let expected = if has_script {
        SCRIPT_NOT_RUN
    } else {
        NO_SCRIPT
    };
    if recorded == expected {
        return None;
    }
    if has_script {
        return Some(format!(
            "{label} has an install script the audit has not accepted (Install script `{recorded}`, expected `{SCRIPT_NOT_RUN}` after review)"
        ));
    }
    Some(format!(
        "{label}: the audit's Install script column says `{recorded}`, but the lockfile records none"
    ))
}

/// Rows for a name and version the lockfile no longer holds.
pub fn stale_rows(entries: &[LockedEntry], rows: &AuditRows) -> Vec<String> {
    rows.keys()
        .filter(|(name, version)| {
            !entries
                .iter()
                .any(|entry| entry.name == *name && entry.version == *version)
        })
        .map(|(name, version)| {
            format!("{name} {version} is in the audit but not locked; remove the stale row")
        })
        .collect()
}

/// The value after the audit's one `Bundle SHA-256:` label: `none` or 64 lowercase hex digits.
pub fn recorded_bundle_hash(audit: &str) -> Result<String, String> {
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
    use super::*;

    fn table(rows_text: &[&str]) -> String {
        format!(
            "# Audit\n\n{PACKAGE_TABLE_HEADING}\n\n| Package | Version | Licence | Install script | Integrity |\n|---|---|---|---|---|\n{}\n",
            rows_text.join("\n")
        )
    }

    fn entry(name: &str, version: &str, has_install_script: bool) -> LockedEntry {
        LockedEntry {
            path: format!("node_modules/{name}"),
            name: name.to_owned(),
            version: version.to_owned(),
            licence: "MIT".to_owned(),
            integrity: "sha512-AAAA".to_owned(),
            resolved: String::new(),
            has_install_script,
            declared_name: None,
            link: false,
        }
    }

    fn problems(entry: &LockedEntry, rows_text: &[&str]) -> Vec<String> {
        row_problems(entry, &rows(&table(rows_text)).expect("a table"))
    }

    #[test]
    fn a_matching_row_passes_and_a_missing_one_fails() {
        let crelt = entry("crelt", "1.0.7", false);
        assert!(
            problems(
                &crelt,
                &["| `crelt` | 1.0.7 | MIT | none | `sha512-AAAA` |"]
            )
            .is_empty()
        );
        assert_eq!(
            problems(
                &entry("style-mod", "4.1.4", false),
                &["| crelt | 1.0.7 | MIT | none | sha512-AAAA |"]
            ),
            ["style-mod 4.1.4 is locked but not in the audit's package table"]
        );
    }

    #[test]
    fn a_licence_and_a_hash_mismatch_are_both_reported() {
        let found = problems(
            &entry("crelt", "1.0.7", false),
            &["| crelt | 1.0.7 | ISC | none | sha512-BBBB |"],
        );
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found[0].contains("the audit says licence `ISC`"),
            "{found:?}"
        );
        assert!(found[1].contains("integrity hash differs"), "{found:?}");
    }

    #[test]
    fn an_install_script_needs_the_audits_acceptance() {
        let esbuild = entry("esbuild", "0.28.2", true);
        assert!(
            problems(
                &esbuild,
                &["| esbuild | 0.28.2 | MIT | not run | sha512-AAAA |"]
            )
            .is_empty()
        );
        let unaccepted = problems(
            &esbuild,
            &["| esbuild | 0.28.2 | MIT | none | sha512-AAAA |"],
        );
        assert!(
            unaccepted[0].contains("has an install script the audit has not accepted"),
            "{unaccepted:?}"
        );
        let stale = problems(
            &entry("crelt", "1.0.7", false),
            &["| crelt | 1.0.7 | MIT | not run | sha512-AAAA |"],
        );
        assert!(stale[0].contains("the lockfile records none"), "{stale:?}");
    }

    #[test]
    fn a_row_without_a_locked_package_is_stale() {
        let audited = rows(&table(&[
            "| crelt | 1.0.7 | MIT | none | x |",
            "| crelt | 1.0.8 | MIT | none | x |",
        ]))
        .expect("a table");
        assert_eq!(
            stale_rows(&[entry("crelt", "1.0.7", false)], &audited),
            ["crelt 1.0.8 is in the audit but not locked; remove the stale row"]
        );
    }

    #[test]
    fn the_table_needs_its_columns_and_no_duplicates() {
        let missing =
            "## Package table\n\n| Package | Version | Licence | Integrity |\n|---|---|---|---|\n";
        assert!(
            rows(missing)
                .expect_err("refused")
                .contains("`Install script`")
        );
        let twice = table(&[
            "| crelt | 1.0.7 | MIT | none | x |",
            "| crelt | 1.0.7 | MIT | none | x |",
        ]);
        assert!(rows(&twice).expect_err("refused").contains("twice"));
        assert!(rows("# No table\n").is_err());
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
}
