//! Shares the unchanged synthetic tier-one workload between benchmark and search diagnostics.

use std::num::NonZeroU64;

use syrinx_core::{SavedPageId, TabId};

use super::{BlankQuery, BoostWeights, Catalogue, Entry, Scope, SearchPolicy, Target, Usage};

pub(super) const ROWS: usize = 100_000;
pub(super) const TABS: usize = 256;
pub(super) const SAVED: usize = 20_000;
pub(super) const COMMANDS: usize = 64;
pub(super) const HISTORY: usize = ROWS - TABS - SAVED - COMMANDS;
pub(super) const NOW: i64 = 1_800_000_000;
pub(super) const LIMIT: usize = 50;
pub(super) const WARMUPS: usize = 5;
pub(super) const PASSES: usize = 40;
pub(super) const BUDGET_NS: u64 = 5_000_000;
pub(super) const LONG_FRAGMENT: &str = "reproducible synthetic catalogue exploration includes detailed release notes and project reference material for keyboard navigation";
pub(super) const TOPICS: [&str; 8] = [
    "amber", "birch", "cobalt", "delta", "ember", "forest", "granite", "harbor",
];
pub(super) const PURPOSES: [&str; 8] = [
    "project guide",
    "release notes",
    "planning reference",
    "workshop manual",
    "example collection",
    "design journal",
    "archive index",
    "navigation handbook",
];
pub(super) const UNICODE: [&str; 5] = ["café", "cafe\u{301}", "東京", "🧭", "naïve résumé"];

pub(super) struct Case {
    pub(super) label: &'static str,
    pub(super) text: String,
    pub(super) scope: Scope,
}

pub(super) fn build_catalogue() -> Result<Catalogue, String> {
    // This permutation puts each identity into the catalogue once, across all four kinds.
    let mut catalogue = Catalogue::new();
    for position in 0..ROWS {
        let index = (position * 7_919) % ROWS;
        catalogue
            .upsert(entry(index)?)
            .map_err(|error| format!("Synthetic catalogue insertion failed: {error:?}"))?;
    }
    if catalogue.len() != ROWS {
        return Err("Synthetic catalogue has the wrong row count.".to_owned());
    }
    Ok(catalogue)
}

pub(super) fn entry(index: usize) -> Result<Entry, String> {
    let topic = TOPICS[index % TOPICS.len()];
    let purpose = PURPOSES[(index / TOPICS.len()) % PURPOSES.len()];
    let mut title = format!("{topic} {purpose} row-{index:06}");
    if index % 10 < 2 {
        title.push(' ');
        title.push_str(UNICODE[(index / 10) % UNICODE.len()]);
    }
    if index % 10 == 2 {
        title.push(' ');
        title.push_str(LONG_FRAGMENT);
        title.push(' ');
        title.push_str(LONG_FRAGMENT);
    }
    if index.is_multiple_of(101) {
        title.make_ascii_uppercase();
    }

    let target = target(index)?;
    let address = match target {
        Target::Command(_) => String::new(),
        _ => format!(
            "https://catalogue-{:03}.test/{topic}/{}/row-{index:06}",
            index % 127,
            if index.is_multiple_of(97) {
                "urlmarker"
            } else {
                "reference"
            }
        ),
    };
    Ok(Entry {
        target,
        title,
        address,
        usage: Usage {
            visits: ((index * 73) % 1_001) as u32,
            last_used: (!index.is_multiple_of(17)).then_some(NOW - ((index % 9) as i64 * 86_400)),
        },
        open: matches!(target, Target::Tab(_))
            || (matches!(target, Target::Saved(_)) && index.is_multiple_of(13)),
    })
}

pub(super) fn target(index: usize) -> Result<Target, String> {
    let value = u64::try_from(index)
        .map_err(|_| "Synthetic identity exceeds the numeric range.".to_owned())?
        .checked_add(1)
        .and_then(NonZeroU64::new)
        .ok_or_else(|| "Synthetic identity is out of range.".to_owned())?;
    if index < TABS {
        return TabId::new(value.get())
            .map(Target::Tab)
            .map_err(|_| "Synthetic tab identity is invalid.".to_owned());
    }
    if index < TABS + SAVED {
        return SavedPageId::new(value.get())
            .map(Target::Saved)
            .map_err(|_| "Synthetic saved identity is invalid.".to_owned());
    }
    if index < ROWS - COMMANDS {
        return Ok(Target::History(value));
    }
    Ok(Target::Command(value))
}

pub(super) fn cases() -> Vec<Case> {
    let mut cases: Vec<Case> = [
        ("blank", ""),
        ("broad-character", "a"),
        ("typed-two", "pr"),
        ("typed-three", "pro"),
        ("typed-four", "proj"),
        ("word", "project"),
        ("missing-character", "prject"),
        ("transposed-characters", "porject"),
        ("uppercase", "PROJECT"),
        ("subsequence", "pjct"),
        ("common-word", "guide"),
        ("two-words", "release notes"),
        ("address-only", "urlmarker"),
        ("address-suffix", ".test"),
        ("rare-identity", "row-099983"),
        ("composed-accent", "café"),
        ("decomposed-accent", "cafe\u{301}"),
        ("cjk", "東京"),
        ("emoji", "🧭"),
        ("no-match", "qzxvjk-no-result"),
    ]
    .into_iter()
    .map(|(label, text)| Case {
        label,
        text: text.to_owned(),
        scope: Scope::Everything,
    })
    .collect();
    cases.push(Case {
        label: "long-match",
        text: LONG_FRAGMENT[..96].to_owned(),
        scope: Scope::Everything,
    });
    cases.push(Case {
        label: "long-near-match",
        text: format!("{}xyz", &LONG_FRAGMENT[..93]),
        scope: Scope::Everything,
    });
    for (label, scope) in [
        ("tabs-scope", Scope::Tabs),
        ("saved-scope", Scope::Saved),
        ("history-scope", Scope::History),
        ("commands-scope", Scope::Commands),
    ] {
        cases.push(Case {
            label,
            text: "project".to_owned(),
            scope,
        });
    }
    cases
}

pub(super) fn policy(text: &str) -> Result<SearchPolicy, String> {
    // These workload settings are explicit inputs, not palette defaults.
    let recency_window_secs = NonZeroU64::new(7 * 86_400)
        .ok_or_else(|| "Synthetic recency window is invalid.".to_owned())?;
    Ok(SearchPolicy {
        limit: LIMIT,
        max_typos: u16::from(text.chars().count() >= 4),
        blank: BlankQuery::All,
        boosts: BoostWeights {
            open: 120,
            frequency: 30,
            recency: 30,
            recency_window_secs,
        },
    })
}

pub(super) fn refresh_generation(
    catalogue: &mut Catalogue,
    revision: &mut u64,
) -> Result<(), String> {
    // Change one existing row before each query so answer caching cannot replace ranking.
    *revision = revision
        .checked_add(1)
        .ok_or_else(|| "Synthetic revision is exhausted.".to_owned())?;
    let mut changed = entry(ROWS - 1)?;
    changed.title.push_str(&format!(" revision-{revision}"));
    catalogue
        .upsert(changed)
        .map_err(|error| format!("Synthetic incremental update failed: {error:?}"))?;
    Ok(())
}

pub(super) fn case_index(offset: usize, pass: usize, count: usize) -> usize {
    (offset + pass * 7) % count
}
