//! Checks every third-party package's licence against `licences.toml`, so no GPL or unreviewed licence slips in (#28, G4).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

/// The policy file: licences allowed for any package, and exact-version approvals.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    allowed: Vec<String>,
    #[serde(default)]
    exceptions: Vec<Exception>,
}

/// One package, at one exact version, allowed under a licence the list above does not cover.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Exception {
    name: String,
    version: String,
    licence: String,
    reason: String,
}

/// Fails listing every third-party package `target` compiles whose licence the policy refuses.
pub fn check(root: &Path, target: &str, metadata: &Value) -> Result<(), String> {
    let policy = read_policy(root)?;
    let packages = third_party_packages(metadata)?;
    let mut refused = Vec::new();
    for package in &packages {
        if let Some(problem) = judge(&policy, package)? {
            refused.push(problem);
        }
    }
    if !refused.is_empty() {
        return Err(format!(
            "Licences not allowed by licences.toml for {target}:\n  {}",
            refused.join("\n  ")
        ));
    }

    println!(
        "Licences passed for {target}: {} third-party packages checked.",
        packages.len()
    );
    Ok(())
}

fn read_policy(root: &Path) -> Result<Policy, String> {
    // Refuse an approval without a reason rather than letting it count silently.
    let path = root.join("licences.toml");
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    parse_policy(&text)
}

fn parse_policy(text: &str) -> Result<Policy, String> {
    let policy: Policy = toml::from_str(text).map_err(|error| format!("licences.toml: {error}"))?;
    if let Some(bare) = policy
        .exceptions
        .iter()
        .find(|exception| exception.reason.trim().is_empty())
    {
        return Err(format!(
            "licences.toml: the approval of {} {} gives no reason",
            bare.name, bare.version
        ));
    }
    Ok(policy)
}

/// Every package in the metadata except the workspace's own crates.
fn third_party_packages(metadata: &Value) -> Result<Vec<&Value>, String> {
    let members: BTreeSet<&str> = metadata["workspace_members"]
        .as_array()
        .ok_or("cargo metadata has no workspace members")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let packages = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata has no packages")?;
    Ok(packages
        .iter()
        .filter(|package| {
            !package["id"]
                .as_str()
                .is_some_and(|id| members.contains(id))
        })
        .collect())
}

/// `None` when the package is allowed; otherwise why not.
fn judge(policy: &Policy, package: &Value) -> Result<Option<String>, String> {
    let name = required_str(package, "name")?;
    let version = required_str(package, "version")?;
    let Some(licence) = package["license"].as_str() else {
        return Ok(Some(format!("{name} {version}: no SPDX licence field")));
    };
    if satisfies(licence, &policy.allowed) || is_approved(policy, name, version, licence) {
        return Ok(None);
    }
    Ok(Some(format!("{name} {version}: {licence}")))
}

/// Whether an approval names this exact package, version and licence string.
fn is_approved(policy: &Policy, name: &str, version: &str, licence: &str) -> bool {
    policy.exceptions.iter().any(|exception| {
        exception.name == name && exception.version == version && exception.licence == licence
    })
}

fn required_str<'a>(package: &'a Value, field: &str) -> Result<&'a str, String> {
    package[field]
        .as_str()
        .ok_or_else(|| format!("cargo metadata has a package without a {field}"))
}

/// Whether an SPDX expression can be met using only allowed licences.
/// `OR` needs one side allowed, `AND` needs both; `A WITH B` is one licence.
fn satisfies(expression: &str, allowed: &[String]) -> bool {
    let normalised = expression
        .replace('/', " OR ")
        .replace('(', " ( ")
        .replace(')', " ) ");
    let tokens: Vec<&str> = normalised.split_whitespace().collect();
    let mut parser = Parser {
        tokens: &tokens,
        position: 0,
        allowed,
    };
    let result = parser.or_expression();
    result == Some(true) && parser.position == tokens.len()
}

/// A tiny recursive-descent reader for SPDX expressions; `None` means unreadable.
struct Parser<'a> {
    tokens: &'a [&'a str],
    position: usize,
    allowed: &'a [String],
}

impl Parser<'_> {
    fn or_expression(&mut self) -> Option<bool> {
        let mut value = self.and_expression()?;
        while self.take("OR") {
            value |= self.and_expression()?;
        }
        Some(value)
    }

    fn and_expression(&mut self) -> Option<bool> {
        let mut value = self.atom()?;
        while self.take("AND") {
            value &= self.atom()?;
        }
        Some(value)
    }

    fn atom(&mut self) -> Option<bool> {
        // A parenthesised group, or one licence with an optional `WITH` exception.
        if self.take("(") {
            let value = self.or_expression()?;
            return self.take(")").then_some(value);
        }
        let mut licence = (*self.tokens.get(self.position)?).to_owned();
        self.position += 1;
        if self.take("WITH") {
            let exception = self.tokens.get(self.position)?;
            self.position += 1;
            licence = format!("{licence} WITH {exception}");
        }
        Some(self.allowed.contains(&licence))
    }

    fn take(&mut self, token: &str) -> bool {
        if self.tokens.get(self.position) == Some(&token) {
            self.position += 1;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const POLICY: &str = r#"
        allowed = ["MIT", "Apache-2.0", "BSD-3-Clause"]

        [[exceptions]]
        name = "unicode-ident"
        version = "1.0.26"
        licence = "(MIT OR Apache-2.0) AND Unicode-3.0"
        reason = "approved 2026-10-10"
    "#;

    fn policy() -> Policy {
        parse_policy(POLICY).expect("the test policy parses")
    }

    fn package(name: &str, version: &str, licence: &str) -> Value {
        json!({"id": format!("{name}@{version}"), "name": name, "version": version, "license": licence})
    }

    fn verdict(name: &str, version: &str, licence: &str) -> Option<String> {
        judge(&policy(), &package(name, version, licence)).expect("a well-formed package")
    }

    #[test]
    fn an_approved_exact_version_passes() {
        let licence = "(MIT OR Apache-2.0) AND Unicode-3.0";
        assert_eq!(verdict("unicode-ident", "1.0.26", licence), None);
    }

    #[test]
    fn the_same_package_at_another_version_fails() {
        let licence = "(MIT OR Apache-2.0) AND Unicode-3.0";
        let problem = verdict("unicode-ident", "1.0.27", licence);
        assert_eq!(
            problem.as_deref(),
            Some("unicode-ident 1.0.27: (MIT OR Apache-2.0) AND Unicode-3.0")
        );
    }

    #[test]
    fn an_unknown_licence_fails_naming_package_and_licence() {
        let problem = verdict("copyleft", "2.0.0", "GPL-3.0-only").expect("refused");
        assert!(problem.contains("copyleft 2.0.0"), "{problem}");
        assert!(problem.contains("GPL-3.0-only"), "{problem}");
    }

    #[test]
    fn an_or_expression_passes_with_one_allowed_side() {
        assert_eq!(verdict("dual", "1.0.0", "Apache-2.0 OR GPL-2.0-only"), None);
        assert_eq!(verdict("legacy", "1.0.0", "Unlicense/MIT"), None);
    }

    #[test]
    fn an_and_expression_needs_every_side_allowed() {
        assert_eq!(
            verdict("both", "1.0.0", "(Apache-2.0 OR MIT) AND BSD-3-Clause"),
            None
        );
        assert!(verdict("half", "1.0.0", "(MIT OR Apache-2.0) AND Unicode-3.0").is_some());
    }

    #[test]
    fn spdx_expressions_are_judged_by_their_operators() {
        let allowed = ["MIT", "Apache-2.0 WITH LLVM-exception"].map(String::from);
        assert!(satisfies("Apache-2.0 WITH LLVM-exception", &allowed));
        assert!(!satisfies("GPL-2.0 WITH LLVM-exception", &allowed));
        assert!(!satisfies("MIT AND", &allowed));
        assert!(!satisfies("(MIT", &allowed));
    }

    #[test]
    fn workspace_members_and_reasonless_approvals_are_handled() {
        let metadata = json!({
            "workspace_members": ["ours@0.1.0"],
            "packages": [package("ours", "0.1.0", "MPL-2.0"), package("theirs", "1.0.0", "MIT")],
        });
        let packages = third_party_packages(&metadata).expect("well-formed metadata");
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0]["name"], "theirs");

        let bare = POLICY.replace("approved 2026-10-10", " ");
        assert!(parse_policy(&bare).is_err_and(|error| error.contains("unicode-ident 1.0.26")));
    }
}
