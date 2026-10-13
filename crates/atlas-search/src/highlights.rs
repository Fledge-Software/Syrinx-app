//! Converts matcher byte positions into safe ranges of the original UTF-8 text.

use std::ops::Range;

use crate::SearchError;

pub(crate) fn ranges(text: &str, indices: &[u32]) -> Result<Vec<Range<usize>>, SearchError> {
    let mut ranges = Vec::with_capacity(indices.len());
    for &index in indices {
        ranges.push(scalar_range(text, index as usize)?);
    }
    ranges.sort_unstable_by_key(|range| range.start);

    // Merge repeated continuation bytes and adjoining matched scalars.
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        if let Some(previous) = merged.last_mut()
            && range.start <= previous.end
        {
            previous.end = previous.end.max(range.end);
            continue;
        }
        merged.push(range);
    }
    Ok(merged)
}

fn scalar_range(text: &str, index: usize) -> Result<Range<usize>, SearchError> {
    if index >= text.len() {
        return Err(SearchError::InvalidMatchRange);
    }
    // frizbee can return continuation bytes, so widen to the whole scalar.
    let mut start = index;
    while !text.is_char_boundary(start) {
        start -= 1;
    }
    let scalar = text[start..]
        .chars()
        .next()
        .ok_or(SearchError::InvalidMatchRange)?;
    Ok(start..start + scalar.len_utf8())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_bytes_merge_into_original_scalar_ranges() {
        assert_eq!(ranges("é😀x", &[6, 1, 0]), Ok(vec![0..2, 6..7]));
        assert_eq!(
            ranges("é😀x", &[5, 4, 3, 2, 1, 0]),
            Ok(vec![Range { start: 0, end: 6 }])
        );
    }

    #[test]
    fn combining_scalars_remain_valid_without_normalizing_text() {
        assert_eq!(
            ranges("e\u{301}x", &[2, 1, 0]),
            Ok(vec![Range { start: 0, end: 3 }])
        );
        assert_eq!(ranges("😀x", &[4, 3]), Ok(vec![Range { start: 0, end: 5 }]));
    }

    #[test]
    fn malformed_match_positions_are_errors() {
        assert_eq!(ranges("é", &[2]), Err(SearchError::InvalidMatchRange));
        assert_eq!(ranges("", &[0]), Err(SearchError::InvalidMatchRange));
        assert_eq!(
            ranges("x", &[u32::MAX]),
            Err(SearchError::InvalidMatchRange)
        );
    }
}
