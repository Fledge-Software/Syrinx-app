//! Rejects fields that lack enough original ASCII digits for a budgeted query.

const DIGITS: usize = 10;
const MAX_QUERY_UNITS: usize = 63;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DigitCounts([u8; DIGITS]);

impl DigitCounts {
    pub(crate) fn new(original: &str) -> Self {
        let mut counts = [0u8; DIGITS];
        for byte in original.bytes().filter(u8::is_ascii_digit) {
            let count = &mut counts[usize::from(byte - b'0')];
            *count = count.saturating_add(1);
        }
        Self(counts)
    }
}

pub(crate) struct DigitQuery {
    required: [u8; DIGITS],
    present: u16,
    max_typos: u16,
}

impl DigitQuery {
    pub(crate) fn new(query: &str, max_typos: u16) -> Option<Self> {
        if max_typos > 1 {
            return None;
        }
        let mut required = [0; DIGITS];
        let mut present = 0;
        for (position, character) in query.chars().enumerate() {
            if position == MAX_QUERY_UNITS {
                return None;
            }
            if character.is_ascii_digit() {
                let digit = usize::from(character as u8 - b'0');
                required[digit] += 1;
                present |= 1 << digit;
            }
        }
        (present != 0).then_some(Self {
            required,
            present,
            max_typos,
        })
    }

    pub(crate) fn may_match(&self, counts: &DigitCounts) -> bool {
        let mut present = self.present;
        let mut deficit = 0;
        // Each absent occurrence needs its own query deletion, regardless of field ordering.
        while present != 0 {
            let digit = present.trailing_zeros() as usize;
            deficit += u16::from(self.required[digit].saturating_sub(counts.0[digit]));
            if deficit > self.max_typos {
                return false;
            }
            present &= present - 1;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use std::mem::{align_of, size_of};

    use frizbee::{Config, Matcher, SortStrategy};

    use super::*;

    fn permits(query: &str, field: &str, budget: u16) -> bool {
        DigitQuery::new(query, budget)
            .expect("supported digit query")
            .may_match(&DigitCounts::new(field))
    }

    #[test]
    fn original_ascii_counts_saturate_without_counting_unicode_digits_or_nul() {
        for length in [0usize, 62, 63, 64, 254, 255, 256, 1_024] {
            let field = format!("{}é١９\0{}", "1".repeat(length), "2".repeat(length / 2));
            let counts = DigitCounts::new(&field);
            assert_eq!(counts.0[1], length.min(255) as u8);
            assert_eq!(counts.0[2], (length / 2).min(255) as u8);
            for digit in [0, 3, 4, 5, 6, 7, 8, 9] {
                assert_eq!(counts.0[digit], 0);
            }
        }
        assert_eq!(size_of::<DigitCounts>(), 10);
        assert_eq!(align_of::<DigitCounts>(), 1);
        assert_eq!(size_of::<[DigitCounts; 2]>(), 20);
    }

    #[test]
    fn repeated_and_combined_deficits_use_one_shared_deletion_budget() {
        assert!(!permits("111111", "abc", 1));
        assert!(!permits("1122", "12", 1));
        assert!(!permits("1122", "11", 1));
        assert!(permits("1122", "122", 1));
        assert!(!permits("11", "1", 0));
        assert!(permits("11", "1", 1));
        assert!(permits("1", "", 1));
        assert!(!permits("1", "١９\0", 0));
        // This filter proves absence only; the existing matcher still enforces ordering.
        assert!(permits("12", "21", 0));
    }

    #[test]
    fn query_scalar_limit_and_unsupported_modes_preserve_fallback() {
        for budget in [0, 1] {
            assert!(DigitQuery::new(&"1".repeat(63), budget).is_some());
            assert!(DigitQuery::new(&format!("{}1", "é".repeat(62)), budget).is_some());
            assert!(DigitQuery::new(&"1".repeat(64), budget).is_none());
            assert!(DigitQuery::new(&format!("{}1", "é".repeat(63)), budget).is_none());
        }
        for query in ["", "letters", "é١９\0"] {
            assert!(DigitQuery::new(query, 0).is_none());
        }
        for budget in [2, u16::MAX] {
            assert!(DigitQuery::new("12", budget).is_none());
        }
    }

    #[test]
    fn field_count_boundaries_never_reject_available_query_digits() {
        let query = "1".repeat(63);
        for budget in [0, 1] {
            for length in [0usize, 61, 62, 63, 64, 254, 255, 256] {
                assert_eq!(
                    permits(&query, &"1".repeat(length), budget),
                    length + usize::from(budget) >= 63,
                );
            }
        }
    }

    fn words(alphabet: &[char], maximum: usize) -> Vec<String> {
        let mut result = vec![String::new()];
        let mut frontier = vec![String::new()];
        for _ in 0..maximum {
            frontier = frontier
                .iter()
                .flat_map(|prefix| {
                    alphabet
                        .iter()
                        .map(move |character| format!("{prefix}{character}"))
                })
                .collect();
            result.extend(frontier.iter().cloned());
        }
        result
    }

    // Enumerate deletions independently on a case-identical synthetic alphabet.
    fn scalar_match(query: &[char], field: &str, omitted: Option<usize>) -> bool {
        let mut field = field.chars();
        query
            .iter()
            .enumerate()
            .filter(|(position, _)| Some(*position) != omitted)
            .all(|(_, query_unit)| field.any(|field_unit| field_unit == *query_unit))
    }

    fn compare_scalar(query: &str, fields: &[String], counts: &[DigitCounts], budget: u16) {
        let Some(filter) = DigitQuery::new(query, budget) else {
            return;
        };
        let units: Vec<_> = query.chars().collect();
        for (field, counts) in fields.iter().zip(counts) {
            let accepted = scalar_match(&units, field, None)
                || (budget == 1
                    && (0..units.len())
                        .any(|position| scalar_match(&units, field, Some(position))));
            assert!(
                !accepted || filter.may_match(counts),
                "query={query:?}, field={field:?}, budget={budget}"
            );
        }
    }

    #[test]
    fn rejection_is_necessary_under_independent_scalar_deletion_enumeration() {
        let fields = words(&['a', '1', '2', 'é', '\0'], 3);
        let counts: Vec<_> = fields.iter().map(|field| DigitCounts::new(field)).collect();
        for query in words(&['a', '1', '2', 'é', '\0'], 3) {
            for budget in [0, 1] {
                compare_scalar(query.as_str(), &fields, &counts, budget);
            }
        }
    }

    fn compare_public(query: &str, fields: &[String], budget: u16) {
        let filter = DigitQuery::new(query, budget).expect("supported digit query");
        let config = Config::default()
            .max_typos(Some(budget))
            .sort(SortStrategy::IndexAsc);
        let borrowed: Vec<_> = fields.iter().map(String::as_str).collect();
        let mut accepted = vec![false; fields.len()];
        for found in Matcher::new(query, &config).match_list(&borrowed) {
            accepted[found.index as usize] = true;
        }
        for (field, accepted) in fields.iter().zip(accepted) {
            assert!(
                !accepted || filter.may_match(&DigitCounts::new(field)),
                "query={query:?}, field={field:?}, budget={budget}"
            );
        }
    }

    #[test]
    fn pinned_matcher_membership_survives_unicode_case_nul_and_count_boundaries() {
        let mut fields: Vec<_> = [
            "",
            "1",
            "2",
            "11",
            "12",
            "21",
            "1122",
            "a1é2",
            "A1É2",
            "é1",
            "É1",
            "é2",
            "\0",
            "a1\0b2",
            "a2\0b1",
            "🧭1東京2",
            "١1９",
            "İ1",
            "i1",
            "K1",
            "k1",
            "ẞ1",
            "ß1",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        for length in [15, 16, 31, 32, 61, 62, 63, 64, 254, 255, 256, 1_024, 1_025] {
            fields.extend([
                "1".repeat(length),
                format!("{}1\0", "é".repeat(length)),
                format!("{}2", "1".repeat(length)),
            ]);
        }
        for query in [
            "1",
            "11",
            "1122",
            "a1é2",
            "A1é2",
            "É1",
            "a1\0b2",
            "\u{0}1",
            "🧭1東京2",
            "İ1",
            "K1",
            "ß1",
        ] {
            for budget in [0, 1] {
                compare_public(query, &fields, budget);
            }
        }
        for query in ["1".repeat(63), format!("{}1", "é".repeat(62))] {
            for budget in [0, 1] {
                compare_public(query.as_str(), &fields, budget);
            }
        }
    }
}
