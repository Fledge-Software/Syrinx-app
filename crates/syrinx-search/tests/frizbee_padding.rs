//! Verifies the approved padding correction through frizbee's public matching APIs.

use frizbee::{Config, Matcher, Scoring, SortStrategy};

const LENGTHS: &[usize] = &[
    0, 1, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65, 1023, 1024, 1025,
];

fn unit_scoring() -> Scoring {
    Scoring {
        match_score: 1,
        mismatch_penalty: 0,
        gap_open_penalty: 0,
        gap_extend_penalty: 0,
        prefix_bonus: 0,
        capitalization_bonus: 0,
        matching_case_bonus: 0,
        exact_match_bonus: 0,
        delimiter_bonus: 0,
    }
}

fn config(budget: Option<u16>, scoring: Scoring) -> Config {
    Config::default()
        .max_typos(budget)
        .scoring(scoring)
        .sort(SortStrategy::IndexAsc)
}

fn assert_case(
    query: &str,
    field: &str,
    config: &Config,
    score: Option<u16>,
    indices: Option<&[u32]>,
) {
    let mut matcher = Matcher::new(query, config);
    let list = matcher.match_list(&[field]);
    let indexed = matcher.match_list_indices(&[field]);
    let single = matcher.match_one(field, 37);
    let single_indexed = matcher.match_one_indices(field, 37);
    assert_eq!(
        list.len(),
        usize::from(score.is_some()),
        "query={query:?}, field length={}",
        field.len()
    );
    assert_eq!(indexed.len(), list.len());
    assert_eq!(single.as_ref().map(|found| found.score), score);
    assert_eq!(single_indexed.as_ref().map(|found| found.score), score);
    let Some(score) = score else {
        return;
    };
    assert_eq!(list[0].score, score);
    assert_eq!(indexed[0].score, score);
    assert_eq!(list[0].index, 0);
    assert_eq!(indexed[0].index, 0);
    let single = single.expect("accepted single result");
    let single_indexed = single_indexed.expect("accepted indexed result");
    assert_eq!(single.index, 37);
    assert_eq!(single_indexed.index, 37);
    assert_eq!(single.exact, query == field);
    assert_eq!(list[0].exact, single.exact);
    assert_eq!(indexed[0].exact, single.exact);
    assert_eq!(single_indexed.exact, single.exact);
    assert_eq!(indexed[0].indices, single_indexed.indices);
    for index in &single_indexed.indices {
        assert!(
            (*index as usize) < field.len(),
            "trace must address a real field byte"
        );
    }
    if let Some(indices) = indices {
        assert_eq!(single_indexed.indices, indices);
    }
}

#[test]
fn absent_nul_retains_zero_score_typo_membership_without_padding_indices() {
    for length in LENGTHS {
        let ascii = "x".repeat(*length);
        let unicode = format!("{}{}", "é".repeat(length / 2), "x".repeat(length % 2));
        for field in [ascii, unicode] {
            assert_absent_nul(field.as_str());
        }
    }
    assert_absent_nul("東京😀");
}

fn assert_absent_nul(field: &str) {
    for scoring in [
        Scoring::default(),
        unit_scoring(),
        Scoring {
            mismatch_penalty: 300,
            ..unit_scoring()
        },
    ] {
        assert_case("\0", field, &config(Some(0), scoring.clone()), None, None);
        assert_case("\0", field, &config(Some(1), scoring), Some(0), Some(&[]));
    }
}

#[test]
fn genuine_nul_beginning_middle_and_end_keep_exact_scores_and_real_indices() {
    for length in LENGTHS.iter().copied().filter(|length| *length > 0) {
        for position in [0, length / 2, length - 1] {
            assert_real_nul(length, position);
        }
    }
}

fn assert_real_nul(length: usize, position: usize) {
    let field = format!(
        "{}\0{}",
        "x".repeat(position),
        "x".repeat(length - position - 1)
    );
    let expected = 16 + if position == 0 { 12 } else { 0 } + if length == 1 { 8 } else { 0 };
    for budget in [0, 1] {
        assert_case(
            "\0",
            field.as_str(),
            &config(Some(budget), Scoring::default()),
            Some(expected),
            Some(&[position as u32]),
        );
        assert_case(
            "\0",
            field.as_str(),
            &config(Some(budget), unit_scoring()),
            Some(1),
            Some(&[position as u32]),
        );
    }
}

#[test]
fn repeated_and_mixed_nul_queries_score_only_genuine_characters() {
    for query in ["\0\0", "a\0", "\0a", "a\0a", "\0a\0"] {
        for prefix in [0usize, 7, 8, 15, 16, 31, 32, 63, 64] {
            let field = format!("{}{query}x", "x".repeat(prefix));
            let indices: Vec<_> = (prefix..prefix + query.len())
                .rev()
                .map(|index| index as u32)
                .collect();
            assert_case(
                query,
                field.as_str(),
                &config(Some(0), unit_scoring()),
                Some(query.len() as u16),
                Some(&indices),
            );
        }
    }
    for query in ["a\0", "\0a"] {
        assert_case(query, "a", &config(Some(0), unit_scoring()), None, None);
        assert_case(query, "a", &config(Some(1), unit_scoring()), Some(1), None);
    }
    assert_case("\0\0", "x", &config(Some(1), unit_scoring()), None, None);
    assert_case(
        "\0\0",
        "\0",
        &config(Some(1), unit_scoring()),
        Some(1),
        None,
    );
}

#[test]
fn unicode_queries_and_zero_scores_preserve_real_byte_boundaries() {
    for query in ["é\0", "\0é", "東京\0", "🧭\0é"] {
        let field = format!("x{query}x");
        assert_case(
            query,
            field.as_str(),
            &config(Some(0), unit_scoring()),
            Some(query.chars().count() as u16),
            None,
        );
    }
    for query in ["é\0", "\0é"] {
        assert_case(query, "é", &config(Some(0), unit_scoring()), None, None);
        assert_case(query, "é", &config(Some(1), unit_scoring()), Some(1), None);
    }
    let zero = Scoring {
        match_score: 0,
        ..unit_scoring()
    };
    for field in ["\0", "x\0x", "東京\0😀"] {
        assert_case(
            "\0",
            field,
            &config(Some(0), zero.clone()),
            Some(0),
            Some(&[]),
        );
    }
}

#[test]
fn untrimmed_greedy_boundary_keeps_real_nul_and_absence_distinct() {
    // Disabling the prefilter keeps the full field at the 1024-byte algorithm boundary.
    for length in [1023, 1024, 1025] {
        let absent = "x".repeat(length);
        assert_case(
            "\0",
            absent.as_str(),
            &config(None, unit_scoring()),
            Some(0),
            Some(&[]),
        );
        let present = format!("{}\0", "x".repeat(length - 1));
        assert_case(
            "\0",
            present.as_str(),
            &config(None, unit_scoring()),
            Some(1),
            Some(&[(length - 1) as u32]),
        );
    }
}
