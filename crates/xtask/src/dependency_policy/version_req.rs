//! Matches a Cargo version requirement against a resolved version, so a package that declares
//! one binding twice is credited only for the declaration Cargo resolved to that version.
//! It is not full Cargo semver: pre-release and build tags are refused, never guessed at.

/// A release version as three numbers.
type Version = (u64, u64, u64);

/// Whether `version` satisfies every comma-separated comparator of `requirement`, read as
/// Cargo reads them (`^` by default). An unreadable requirement or version, including one with
/// a pre-release or build tag, is an error, never a match.
pub(super) fn matches(requirement: &str, version: &str) -> Result<bool, String> {
    let resolved =
        parse_version(version).ok_or_else(|| format!("unreadable resolved version: {version}"))?;
    for comparator in requirement.split(',') {
        let (lower, upper) = comparator_range(comparator.trim())
            .ok_or_else(|| format!("unreadable version requirement: {requirement}"))?;
        if resolved < lower || upper.is_some_and(|upper| resolved >= upper) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn parse_version(version: &str) -> Option<Version> {
    if has_tag(version) {
        return None;
    }
    let mut parts = version.split('.').map(|part| part.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// A pre-release (`-rc.1`) or build (`+meta`) tag, which this matcher does not interpret.
fn has_tag(text: &str) -> bool {
    text.contains(['-', '+'])
}

/// One comparator as a half-open range: an inclusive lower bound and an exclusive upper one.
fn comparator_range(comparator: &str) -> Option<(Version, Option<Version>)> {
    let (operator, rest) = split_operator(comparator);
    if is_wildcard(rest.trim()) {
        return Some(((0, 0, 0), None));
    }
    let (partial, wildcard) = parse_partial(rest.trim())?;
    // With no operator, `1.2.*` means exactly the 1.2 series, as `=1.2` does.
    let operator = if wildcard && operator.is_empty() {
        "="
    } else {
        operator
    };
    let floor = (partial.0, partial.1.unwrap_or(0), partial.2.unwrap_or(0));
    match operator {
        "^" | "" => Some((floor, Some(caret_ceiling(partial)))),
        "~" => Some((floor, Some(next_unspecified(partial, 1)))),
        "=" => Some((floor, Some(next_unspecified(partial, 2)))),
        ">=" => Some((floor, None)),
        ">" => Some((next_unspecified(partial, 2), None)),
        "<" => Some(((0, 0, 0), Some(floor))),
        "<=" => Some(((0, 0, 0), Some(next_unspecified(partial, 2)))),
        _ => None,
    }
}

/// The comparator's operator, or an empty one when it has none (Cargo's default caret).
fn split_operator(comparator: &str) -> (&str, &str) {
    for operator in [">=", "<=", "^", "~", "=", ">", "<"] {
        if let Some(rest) = comparator.strip_prefix(operator) {
            return (operator, rest);
        }
    }
    ("", comparator)
}

/// `1`, `1.2` or `1.2.3`; a `*` or `x` part ends the version as a wildcard.
type Partial = (u64, Option<u64>, Option<u64>);

fn is_wildcard(part: &str) -> bool {
    matches!(part, "*" | "x" | "X")
}

/// The numbers given and whether a wildcard part ended them; a tag makes it unreadable.
fn parse_partial(text: &str) -> Option<(Partial, bool)> {
    if has_tag(text) {
        return None;
    }
    let mut numbers = Vec::new();
    let mut wildcard = false;
    for part in text.split('.') {
        if is_wildcard(part) {
            wildcard = true;
            break;
        }
        numbers.push(part.parse::<u64>().ok()?);
    }
    let partial = match numbers.as_slice() {
        [major] => (*major, None, None),
        [major, minor] => (*major, Some(*minor), None),
        [major, minor, patch] => (*major, Some(*minor), Some(*patch)),
        _ => return None,
    };
    Some((partial, wildcard))
}

/// Cargo's caret: the left-most nonzero part given may not change.
fn caret_ceiling(partial: Partial) -> Version {
    match partial {
        (0, Some(0), Some(patch)) => (0, 0, patch + 1),
        (0, Some(0), None) => (0, 1, 0),
        (0, Some(minor), _) => (0, minor + 1, 0),
        (0, None, _) => (1, 0, 0),
        (major, _, _) => (major + 1, 0, 0),
    }
}

/// The first version past every one the partial names: `1.2` gives `1.3.0`. With a full version,
/// `depth` picks the part to increase (1 for minor, 2 for patch).
fn next_unspecified(partial: Partial, depth: usize) -> Version {
    match partial {
        (major, None, _) => (major + 1, 0, 0),
        (major, Some(minor), None) => (major, minor + 1, 0),
        (major, Some(minor), Some(_)) if depth == 1 => (major, minor + 1, 0),
        (major, Some(minor), Some(patch)) => (major, minor, patch + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn reads_the_requirements_in_the_resolved_graph() -> Result<(), String> {
        // The forms Syrinx's graph declares for its Windows bindings, and Cargo's defaults.
        let cases = [
            ("^0.62", "0.62.2", true),
            ("^0.62", "0.63.0", false),
            ("=0.62.2", "0.62.2", true),
            ("=0.62.2", "0.62.3", false),
            (">=0.52, <=0.61", "0.61.2", true),
            (">=0.52, <=0.61", "0.62.0", false),
            (">=0.60, <0.62", "0.61.2", true),
            (">=0.60, <0.62", "0.62.2", false),
            ("0.3.9", "0.3.9", true),
            ("0.3", "0.4.0", false),
            ("1", "1.9.0", true),
            ("~1.2.3", "1.2.9", true),
            ("~1.2.3", "1.3.0", false),
            ("*", "0.57.0", true),
            ("0.62.*", "0.62.9", true),
            ("^0.0.3", "0.0.4", false),
            // Caret on a bare zero major or minor.
            ("^0", "0.9.0", true),
            ("^0", "1.0.0", false),
            ("^0.0", "0.0.9", true),
            ("^0.0", "0.1.0", false),
            // `>` and `<=` with partial versions step past the whole series named.
            (">0.62", "0.62.9", false),
            (">0.62", "0.63.0", true),
            (">1", "2.0.0", true),
            ("<=1", "1.9.9", true),
            ("<=1", "2.0.0", false),
            ("<=1.2", "1.2.9", true),
            ("<=1.2", "1.3.0", false),
            // A wildcard with no operator pins the series it names.
            ("1.2.*", "1.2.5", true),
            ("1.2.*", "1.9.0", false),
            ("1.2.x", "1.9.0", false),
            ("1.x", "1.9.0", true),
            ("1.x", "2.0.0", false),
        ];
        for (requirement, version, expected) in cases {
            assert_eq!(
                matches(requirement, version)?,
                expected,
                "{requirement} {version}"
            );
        }
        assert!(matches("~>1", "1.0.0").is_err());
        assert!(matches("^0.62", "zero").is_err());
        // Pre-release and build tags are refused rather than read as the release.
        assert!(matches("=0.62.0-rc.1", "0.62.0").is_err());
        assert!(matches("^0.62+build", "0.62.0").is_err());
        assert!(matches("^0.62", "0.62.0-rc.1").is_err());
        Ok(())
    }
}
