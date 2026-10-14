//! Matches a Cargo version requirement against a resolved version, so a package that declares
//! one binding twice is credited only for the declaration Cargo resolved to that version.

/// A version as three numbers; a pre-release or build suffix is ignored.
type Version = (u64, u64, u64);

/// Whether `version` satisfies every comma-separated comparator of `requirement`, read as
/// Cargo reads them (`^` by default). An unreadable requirement is an error, never a match.
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
    let core = version.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// One comparator as a half-open range: an inclusive lower bound and an exclusive upper one.
fn comparator_range(comparator: &str) -> Option<(Version, Option<Version>)> {
    let (operator, rest) = split_operator(comparator);
    if is_wildcard(rest.trim()) {
        return Some(((0, 0, 0), None));
    }
    let partial = parse_partial(rest.trim())?;
    let floor = (partial.0, partial.1.unwrap_or(0), partial.2.unwrap_or(0));
    match operator {
        "^" => Some((floor, Some(caret_ceiling(partial)))),
        "~" => Some((floor, Some(next_unspecified(partial, 1)))),
        "=" => Some((floor, Some(next_unspecified(partial, 2)))),
        ">=" => Some((floor, None)),
        ">" => Some((next_unspecified(partial, 2), None)),
        "<" => Some(((0, 0, 0), Some(floor))),
        "<=" => Some(((0, 0, 0), Some(next_unspecified(partial, 2)))),
        _ => None,
    }
}

fn split_operator(comparator: &str) -> (&str, &str) {
    for operator in [">=", "<=", "^", "~", "=", ">", "<"] {
        if let Some(rest) = comparator.strip_prefix(operator) {
            return (operator, rest);
        }
    }
    ("^", comparator)
}

/// `1`, `1.2` or `1.2.3`; a `*` or `x` part ends the version as a wildcard.
type Partial = (u64, Option<u64>, Option<u64>);

fn is_wildcard(part: &str) -> bool {
    matches!(part, "*" | "x" | "X")
}

fn parse_partial(text: &str) -> Option<Partial> {
    let core = text.split(['-', '+']).next()?;
    let mut numbers = Vec::new();
    for part in core.split('.') {
        if is_wildcard(part) {
            break;
        }
        numbers.push(part.parse::<u64>().ok()?);
    }
    match numbers.as_slice() {
        [major] => Some((*major, None, None)),
        [major, minor] => Some((*major, Some(*minor), None)),
        [major, minor, patch] => Some((*major, Some(*minor), Some(*patch))),
        _ => None,
    }
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
        Ok(())
    }
}
