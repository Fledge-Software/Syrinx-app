//! Checks exact zero/one-typo membership without computing fuzzy scores.

mod bitset;
#[cfg(test)]
mod grouped;
#[cfg(test)]
pub(crate) mod indexed;
#[cfg(test)]
mod numeric;

use bitset::ShortAscii;

pub(crate) enum Membership {
    ShortAscii(Box<ShortAscii>),
    Ascii(Vec<(u8, u8)>),
    Unicode {
        needle: Vec<(char, char)>,
        ascii: Vec<(u8, u8)>,
        non_ascii_units: usize,
    },
}

impl Membership {
    pub(crate) fn new(query: &str) -> Self {
        // Match frizbee 0.13.0's default smart-case and smart-Unicode selection.
        let case_sensitive = query.chars().any(char::is_uppercase);
        if query.is_ascii() {
            if let Some(needle) = ShortAscii::new(query, case_sensitive) {
                return Self::ShortAscii(Box::new(needle));
            }
            return Self::Ascii(
                query
                    .bytes()
                    .map(|byte| (byte, ascii_counterpart(byte, case_sensitive)))
                    .collect(),
            );
        }
        let needle: Vec<_> = query
            .chars()
            .map(|character| (character, unicode_counterpart(character, case_sensitive)))
            .collect();
        // Retain the original whole-query case pairs when non-ASCII units must be deleted.
        let ascii: Vec<_> = needle
            .iter()
            .filter(|(original, _)| original.is_ascii())
            .map(|&(original, counterpart)| (original as u8, counterpart as u8))
            .collect();
        let non_ascii_units = needle.len() - ascii.len();
        Self::Unicode {
            needle,
            ascii,
            non_ascii_units,
        }
    }

    pub(crate) fn matches(&self, field: &str, max_typos: u16) -> Option<bool> {
        let allow_deletion = deletion_budget(max_typos)?;
        Some(match self {
            Self::ShortAscii(needle) => needle.matches(field.as_bytes(), allow_deletion),
            Self::Ascii(needle) => {
                // An ASCII alignment consumes at least one field byte per retained query byte.
                if needle.len().saturating_sub(usize::from(allow_deletion)) > field.len() {
                    return Some(false);
                }
                ordered_match(needle, field.bytes(), allow_deletion)
            }
            Self::Unicode { needle, .. } => ordered_match(needle, field.chars(), allow_deletion),
        })
    }

    pub(crate) fn matches_ascii(&self, field: &str, max_typos: u16) -> Option<bool> {
        let Self::Unicode { ascii, .. } = self else {
            return self.matches(field, max_typos);
        };
        deletion_budget(max_typos)?;
        if self.ascii_field_budget(max_typos).is_none() || ascii.len() > field.len() {
            return Some(false);
        }
        Some(ordered_match(ascii, field.bytes(), false))
    }

    /// None means an unsupported budget or too many forced deletions for an ASCII field.
    pub(crate) fn ascii_field_budget(&self, max_typos: u16) -> Option<u16> {
        deletion_budget(max_typos)?;
        let non_ascii_units = match self {
            Self::Unicode {
                non_ascii_units, ..
            } => *non_ascii_units,
            _ => 0,
        };
        // Width-preserving case pairs cannot turn a non-ASCII scalar into an ASCII match.
        usize::from(max_typos)
            .checked_sub(non_ascii_units)
            .map(|remaining| remaining as u16)
    }
}

fn deletion_budget(max_typos: u16) -> Option<bool> {
    // Other budgets must use the existing matcher until independently implemented.
    match max_typos {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn ascii_counterpart(byte: u8, case_sensitive: bool) -> u8 {
    if case_sensitive {
        return byte;
    }
    if byte.is_ascii_lowercase() {
        byte.to_ascii_uppercase()
    } else {
        byte.to_ascii_lowercase()
    }
}

fn unicode_counterpart(character: char, case_sensitive: bool) -> char {
    if case_sensitive {
        return character;
    }
    // The pinned prefilter ignores expanding or UTF-8-width-changing case mappings.
    if character.is_uppercase() {
        return single_same_width(character, character.to_lowercase());
    }
    if character.is_lowercase() {
        return single_same_width(character, character.to_uppercase());
    }
    character
}

fn single_same_width(original: char, mut variants: impl Iterator<Item = char>) -> char {
    let Some(variant) = variants.next() else {
        return original;
    };
    if variants.next().is_some() || variant.len_utf8() != original.len_utf8() {
        return original;
    }
    variant
}

fn ordered_match<Unit: Copy + Eq>(
    needle: &[(Unit, Unit)],
    field: impl Iterator<Item = Unit>,
    allow_deletion: bool,
) -> bool {
    let mut exact_prefix = 0;
    let mut deleted_prefix = usize::from(allow_deletion);
    if needle.len() <= deleted_prefix {
        return true;
    }

    // The furthest matched prefix dominates shorter prefixes with the same deletion budget.
    for unit in field {
        if allow_deletion {
            deleted_prefix = advance(needle, deleted_prefix, unit);
        }
        exact_prefix = advance(needle, exact_prefix, unit);
        if exact_prefix == needle.len() {
            return true;
        }
        if allow_deletion {
            deleted_prefix = deleted_prefix.max(exact_prefix + 1);
            if deleted_prefix == needle.len() {
                return true;
            }
        }
    }
    false
}

fn advance<Unit: Copy + Eq>(needle: &[(Unit, Unit)], prefix: usize, unit: Unit) -> usize {
    let (original, counterpart) = needle[prefix];
    prefix + usize::from(unit == original || unit == counterpart)
}

#[cfg(test)]
mod tests {
    use frizbee::{Config, Matcher, SortStrategy};

    use super::{Membership, ascii_counterpart, ordered_match};

    fn compare(query: &str, fields: &[String], max_typos: u16) {
        let config = Config::default()
            .max_typos(Some(max_typos))
            .sort(SortStrategy::IndexAsc);
        let borrowed_fields: Vec<&str> = fields.iter().map(String::as_str).collect();
        let matches = Matcher::new(query, &config).match_list(&borrowed_fields);
        let mut expected = vec![false; fields.len()];
        for found in matches {
            let index = usize::try_from(found.index).expect("synthetic index fits usize");
            assert!(index < expected.len(), "matcher returned an invalid row");
            expected[index] = true;
        }
        let membership = Membership::new(query);
        for (field, expected) in fields.iter().zip(expected) {
            let actual = if field.is_ascii() {
                membership.matches_ascii(field, max_typos)
            } else {
                membership.matches(field, max_typos)
            };
            assert_eq!(
                actual,
                Some(expected),
                "synthetic query {query:?}, field {field:?}, typos {max_typos}"
            );
        }
    }

    fn compare_both_budgets(query: &str, fields: &[String]) {
        compare(query, fields, 0);
        compare(query, fields, 1);
    }

    fn next_words(prefixes: &[String], alphabet: &[char]) -> Vec<String> {
        let mut words = Vec::new();
        for prefix in prefixes {
            for character in alphabet {
                let mut word = prefix.clone();
                word.push(*character);
                words.push(word);
            }
        }
        words
    }

    fn words(alphabet: &[char], max_len: usize) -> Vec<String> {
        let mut all = vec![String::new()];
        let mut frontier = all.clone();
        for _ in 0..max_len {
            frontier = next_words(&frontier, alphabet);
            all.extend(frontier.iter().cloned());
        }
        all
    }

    #[test]
    fn ordered_membership_matches_public_api_exhaustively_for_short_ascii() {
        let fields = words(&['a', 'B', '_', '\0'], 4);
        for query in words(&['a', 'B', '_', '\0'], 3) {
            compare_both_budgets(&query, &fields);
        }
    }

    #[test]
    fn short_bitsets_match_the_previous_scalar_algorithm_exhaustively() {
        let mut fields = words(&['a', 'B', '_', '\0'], 5);
        fields.extend(["éa\0B😀", "東京", "aéB_😀\0"].map(str::to_owned));
        for query in words(&['a', 'B', '_', '\0'], 4) {
            compare_scalar(&query, &fields);
        }
    }

    fn compare_scalar(query: &str, fields: &[String]) {
        let case_sensitive = query.chars().any(char::is_uppercase);
        let pairs: Vec<_> = query
            .bytes()
            .map(|byte| (byte, ascii_counterpart(byte, case_sensitive)))
            .collect();
        let membership = Membership::new(query);
        for field in fields {
            for typos in [0, 1] {
                let expected = ordered_match(&pairs, field.bytes(), typos == 1);
                assert_eq!(
                    membership.matches(field, typos),
                    Some(expected),
                    "synthetic query {query:?}, field {field:?}, typos {typos}"
                );
            }
        }
    }

    fn deletion_fields(query: &str) -> Vec<String> {
        if query.is_empty() {
            return Vec::new();
        }
        let mut fields = Vec::new();
        for position in [0, query.len() / 2, query.len() - 1] {
            let mut deleted = query.to_owned();
            deleted.remove(position);
            fields.push(deleted);
        }
        fields
    }

    #[test]
    fn bitset_boundaries_keep_all_prefix_bits_and_the_long_query_fallback() {
        for length in [0, 1, 62, 63, 64] {
            let query: String = "aB_\0".chars().cycle().take(length).collect();
            let membership = Membership::new(&query);
            assert_eq!(
                matches!(&membership, Membership::ShortAscii(_)),
                (1..=63).contains(&length)
            );
            let mut fields = vec![
                String::new(),
                query.clone(),
                format!("prefix{query}suffix"),
                query
                    .chars()
                    .map(|character| format!("é{character}"))
                    .collect(),
                query.to_ascii_lowercase(),
                query.to_ascii_uppercase(),
            ];
            fields.extend(deletion_fields(&query));
            compare_scalar(&query, &fields);
            compare_both_budgets(&query, &fields);
            assert_eq!(membership.matches(&query, 2), None);
        }
    }

    #[test]
    fn bitsets_never_consume_a_field_byte_twice_or_spend_two_deletions() {
        for (query, field, typos, expected) in [
            ("aa", "a", 0, false),
            ("aa", "a", 1, true),
            ("aaa", "a", 1, false),
            ("aba", "a", 1, false),
            ("abc", "bc", 1, true),
            ("abc", "ac", 1, true),
            ("abc", "ab", 1, true),
            ("abc", "b", 1, false),
            ("a", "", 1, true),
            ("\0", "", 0, false),
            ("\0", "", 1, true),
            ("\0\0", "\0", 0, false),
            ("\0\0", "\0", 1, true),
            ("aB", "aéB", 0, true),
            ("aB", "aéb", 0, false),
        ] {
            assert_eq!(Membership::new(query).matches(field, typos), Some(expected));
            compare(query, &[field.to_owned()], typos);
        }
        for length in [62, 63, 64] {
            let query = "a".repeat(length);
            let fields = ["a".repeat(length - 1), "a".repeat(length - 2)];
            let membership = Membership::new(&query);
            assert_eq!(membership.matches(&fields[0], 0), Some(false));
            assert_eq!(membership.matches(&fields[0], 1), Some(true));
            assert_eq!(membership.matches(&fields[1], 1), Some(false));
            compare_both_budgets(&query, &fields);
        }
    }

    #[test]
    fn every_ascii_byte_is_literal_and_not_padding() {
        let mut fields = vec![String::new(), "ordinary".to_owned(), "東京😀".to_owned()];
        for byte in 0u8..=127 {
            let character = char::from(byte);
            fields.push(character.to_string());
            fields.push(format!("a{character}b"));
        }
        for byte in 0u8..=127 {
            compare_both_budgets(&char::from(byte).to_string(), &fields);
        }
        compare_both_budgets("a\0b", &fields);
        assert_eq!(Membership::new("\0").matches("ordinary", 0), Some(false));
        assert_eq!(Membership::new("\0").matches("ordinary", 1), Some(true));
        assert_eq!(Membership::new("\0").matches("", 1), Some(true));
    }

    #[test]
    fn unicode_membership_matches_public_api_across_mixed_scalar_widths() {
        let alphabet = ['a', 'B', 'é', 'É', '東', '😀', '\u{301}', '\0'];
        let fields = words(&alphabet, 3);
        for query in words(&alphabet, 2) {
            compare_both_budgets(&query, &fields);
        }
    }

    #[test]
    fn unicode_case_rules_do_not_expand_normalize_or_change_utf8_width() {
        let fields: Vec<String> = [
            "", "é", "É", "e\u{301}", "ß", "SS", "ss", "σ", "ς", "Σ", "ı", "I", "i", "İ", "ſ", "S",
            "s", "K", "K", "k", "ǅ", "ǆ", "aß", "aSS", "aſ", "aS", "aé", "aÉ",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        for query in &fields {
            compare_both_budgets(query, &fields);
        }
        assert_eq!(Membership::new("σ").matches("Σ", 0), Some(true));
        assert_eq!(Membership::new("σ").matches("ς", 0), Some(false));
        assert_eq!(Membership::new("ſ").matches("S", 0), Some(false));
        assert_eq!(Membership::new("aß").matches("aSS", 0), Some(false));
        assert_eq!(Membership::new("é").matches("e\u{301}", 0), Some(false));
    }

    #[test]
    fn long_fields_and_byte_chunk_boundaries_preserve_membership() {
        let queries = ["abc", "a\0b", "é東😀", "Aa", "aaaa", "abé"];
        let mut fields = vec![String::new()];
        for padding in [0, 15, 16, 17, 31, 32, 33, 63, 64, 65, 1_023, 1_024, 1_100] {
            let gap = "_".repeat(padding);
            fields.extend([
                format!("{gap}abc{gap}"),
                format!("a{gap}c"),
                format!("{gap}a\0b"),
                format!("{gap}é{gap}東{gap}😀"),
                format!("{gap}é{gap}😀"),
                format!("{gap}aabé"),
            ]);
        }
        for query in queries {
            compare_both_budgets(query, &fields);
        }
    }

    #[test]
    fn unsupported_budgets_require_an_explicit_fallback() {
        for query in ["", "a", "abc", "東京"] {
            let membership = Membership::new(query);
            assert_eq!(membership.matches("synthetic", 2), None);
            assert_eq!(membership.matches("", u16::MAX), None);
        }
    }

    #[test]
    fn ascii_length_rejection_respects_deletion_boundaries_and_unicode_fields() {
        let membership = Membership::new("abcd");
        assert_eq!(membership.matches("abc", 0), Some(false));
        assert_eq!(membership.matches("ab", 1), Some(false));
        assert_eq!(membership.matches("abc", 1), Some(true));
        assert_eq!(membership.matches("abcd", 0), Some(true));
        assert_eq!(membership.matches("éab", 1), Some(false));
        assert_eq!(membership.matches("éabc", 1), Some(true));
        compare_both_budgets(
            "abcd",
            &["ab", "abc", "abcd", "éab", "éabc"].map(str::to_owned),
        );
    }

    #[test]
    fn literal_and_nonliteral_matches_preserve_case_unicode_and_deletions() {
        for (query, field, typos, expected) in [
            ("project", "prefix project suffix", 0, true),
            ("project", "PROJECT", 0, true),
            ("PROJECT", "project", 0, false),
            ("café", "prefix café suffix", 0, true),
            ("café", "caf_É", 0, true),
            ("café", "cafe\u{301}", 0, false),
            ("東😀", "prefix 東😀 suffix", 0, true),
            ("東😀", "東_😀", 0, true),
            ("\0", "a\0b", 0, true),
            ("\0", "ordinary", 0, false),
            ("\0", "ordinary", 1, true),
            ("\0", "", 1, true),
            ("a\0b", "ab", 1, true),
            ("abc", "a-b-c", 0, true),
            ("abc", "ac", 1, true),
            ("abc", "ac", 0, false),
            ("", "", 0, true),
            ("", "ordinary", 0, true),
        ] {
            assert_eq!(Membership::new(query).matches(field, typos), Some(expected));
            compare(query, &[field.to_owned()], typos);
        }
    }

    #[test]
    fn ascii_fields_force_unicode_deletions_without_recomputing_smart_case() {
        for (query, field, typos, expected) in [
            ("café", "caf", 0, false),
            ("café", "caf", 1, true),
            ("café", "CAF", 1, true),
            ("Café", "caf", 1, false),
            ("Café", "Caf", 1, true),
            ("Éa", "A", 1, false),
            ("Éa", "a", 1, true),
            ("cafe\u{301}", "CAFE", 1, true),
            ("ſa", "Sa", 0, false),
            ("ſa", "a", 1, true),
            ("éé", "", 1, false),
            ("東京", "ordinary", 1, false),
            ("é", "", 1, true),
            ("é", "ordinary", 1, true),
            ("\0é", "\0", 1, true),
            ("\0é", "", 1, false),
            ("éab", "axb", 1, true),
            ("éab", "a", 1, false),
            ("éa_b", "a-b", 1, false),
        ] {
            assert!(field.is_ascii());
            let membership = Membership::new(query);
            assert_eq!(membership.matches_ascii(field, typos), Some(expected));
            assert_eq!(membership.matches(field, typos), Some(expected));
            compare(query, &[field.to_owned()], typos);
            assert_eq!(membership.matches_ascii(field, 2), None);
        }
    }
}
