//! Grows the in-memory catalogue with synthetic history rows for P1-33's palette acceptance run.

use std::{num::NonZeroU64, time::Duration};

use atlas_search::{Catalogue, Entry, Target, Usage};

use super::SessionError;

/// The largest synthetic catalogue P1-33 measures (D48's 100,000 rows).
pub(super) const MAX_ROWS: usize = 100_000;
/// Synthetic identities start far above any identity the store allocates.
const FIRST_ID: u64 = 1 << 62;
const TOPICS: [&str; 8] = [
    "amber", "birch", "cobalt", "delta", "ember", "forest", "granite", "harbor",
];
const PURPOSES: [&str; 8] = [
    "project guide",
    "release notes",
    "planning reference",
    "workshop manual",
    "example collection",
    "design journal",
    "archive index",
    "navigation handbook",
];
const UNICODE: [&str; 4] = ["café", "東京", "🧭", "naïve résumé"];
const LONG_FRAGMENT: &str = "detailed synthetic reference material for keyboard navigation";
/// Visits spread over the 90 days history retention keeps.
const RETAINED_SECONDS: usize = 90 * 24 * 60 * 60;

/// How large the synthetic part of the catalogue is after a grow request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyntheticCatalogue {
    pub synthetic_rows: usize,
    pub catalogue_rows: usize,
    pub elapsed: Duration,
}

/// Adds rows `present..total`; rows are never persisted and vanish when the worker stops.
pub(super) fn grow(
    catalogue: &mut Catalogue,
    present: &mut usize,
    total: usize,
    now: i64,
) -> Result<(), SessionError> {
    if total > MAX_ROWS || total < *present {
        return Err(SessionError::SyntheticCatalogue);
    }
    for index in *present..total {
        catalogue
            .upsert(entry(index, now))
            .map_err(SessionError::Search)?;
        *present = index + 1;
    }
    Ok(())
}

/// One deterministic history row with a reserved `.test` or `example.com` address.
fn entry(index: usize, now: i64) -> Entry {
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
    }
    let address = if index.is_multiple_of(5) {
        format!("https://www.example.com/{topic}/row-{index:06}")
    } else {
        format!(
            "https://catalogue-{:03}.test/{topic}/row-{index:06}",
            index % 127
        )
    };
    // Both values stay far below their types' limits, since `index` is at most 100,000.
    let age = ((index * 7_919) % RETAINED_SECONDS) as i64;
    Entry {
        target: Target::History(identity(index)),
        title,
        address,
        usage: Usage {
            visits: (1 + index % 40) as u32,
            last_used: Some(now - age),
        },
        open: false,
    }
}

fn identity(index: usize) -> NonZeroU64 {
    let offset = u64::try_from(index).expect("synthetic rows stay below 100,000");
    NonZeroU64::new(FIRST_ID + offset).expect("synthetic identities are never zero")
}

#[cfg(test)]
mod tests {
    use atlas_search::{BlankQuery, BoostWeights, Scope, SearchPolicy};

    use super::*;

    const NOW: i64 = 1_800_000_000;

    #[test]
    fn grows_in_steps_and_refuses_shrinking_or_oversized_requests() {
        let mut catalogue = Catalogue::new();
        let mut present = 0;
        grow(&mut catalogue, &mut present, 1_000, NOW).expect("first step");
        grow(&mut catalogue, &mut present, 2_500, NOW).expect("second step");
        assert_eq!((present, catalogue.len()), (2_500, 2_500));
        assert_eq!(
            grow(&mut catalogue, &mut present, 10, NOW),
            Err(SessionError::SyntheticCatalogue)
        );
        assert_eq!(
            grow(&mut catalogue, &mut present, MAX_ROWS + 1, NOW),
            Err(SessionError::SyntheticCatalogue)
        );
        assert_eq!(catalogue.len(), 2_500);
    }

    #[test]
    fn rows_use_reserved_addresses_and_are_searchable() {
        for index in [0, 1, 2, 5, 99_999] {
            let row = entry(index, NOW);
            let host = row.address.split('/').nth(2).expect("synthetic host");
            assert!(
                host == "www.example.com" || host.ends_with(".test"),
                "{host} is not reserved"
            );
        }
        let mut catalogue = Catalogue::new();
        let mut present = 0;
        grow(&mut catalogue, &mut present, 1_000, NOW).expect("synthetic rows");
        let policy = SearchPolicy {
            limit: 20,
            max_typos: 1,
            blank: BlankQuery::None,
            boosts: BoostWeights {
                open: 0,
                frequency: 0,
                recency: 0,
                recency_window_secs: NonZeroU64::new(86_400).expect("nonzero"),
            },
        };
        let results = catalogue
            .query("harbor guide", Scope::Everything, NOW, policy)
            .expect("query");
        assert!(!results.hits.is_empty());
    }
}
