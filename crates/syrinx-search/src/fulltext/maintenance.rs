//! Makes commits durable and removes deleted text from disk, not just from results.

use tantivy::{Index, IndexWriter, SegmentMeta, index::SegmentId};

use super::types::FullTextError;

/// Prefix of the commit payload, which carries the applied clear generation with the data.
const PAYLOAD_PREFIX: &str = "syrinx-fulltext/1 cleared=";
/// Above this many segments every segment is merged, so queries do not slow down.
const MAX_SEGMENTS: usize = 8;

/// Reads the journal generation the last durable commit applied; a new index has none.
pub(super) fn cleared_generation(index: &Index) -> Result<u64, FullTextError> {
    let Some(payload) = index.load_metas()?.payload else {
        return Ok(0);
    };
    parse_payload(&payload).ok_or(FullTextError::Damaged)
}

fn parse_payload(payload: &str) -> Option<u64> {
    payload.strip_prefix(PAYLOAD_PREFIX)?.parse().ok()
}

fn payload(cleared: u64) -> String {
    format!("{PAYLOAD_PREFIX}{cleared}")
}

/// Commits everything staged together with `cleared`; returns only once it is on disk.
pub(super) fn commit(writer: &mut IndexWriter, cleared: u64) -> Result<(), FullTextError> {
    let mut prepared = writer.prepare_commit()?;
    prepared.set_payload(&payload(cleared));
    prepared.commit()?;
    Ok(())
}

/// Rewrites every segment holding deleted rows, folds a crowded index's smaller segments
/// together, then deletes the files no commit references. Callers must have released
/// every reader first.
pub(super) fn expunge(index: &Index, writer: &mut IndexWriter) -> Result<(), FullTextError> {
    // Held segment metas keep their files alive through garbage collection, so drop them first.
    let targets = merge_targets(&index.searchable_segment_metas()?);
    if !targets.is_empty() {
        writer.merge(&targets).wait()?;
    }
    collect_garbage(writer)
}

/// Segments holding deleted rows, plus all but the largest once there are too many.
///
/// Leaving the largest out keeps a routine commit from rewriting the whole index.
fn merge_targets(segments: &[SegmentMeta]) -> Vec<SegmentId> {
    let largest = segments
        .iter()
        .max_by_key(|segment| segment.max_doc())
        .map(|segment| segment.id());
    let crowded = segments.len() > MAX_SEGMENTS;
    segments
        .iter()
        .filter(|segment| {
            segment.num_deleted_docs() > 0 || (crowded && Some(segment.id()) != largest)
        })
        .map(|segment| segment.id())
        .collect()
}

/// Deletes unreferenced files; a file that could not be deleted still holds old text.
fn collect_garbage(writer: &IndexWriter) -> Result<(), FullTextError> {
    let garbage = writer.garbage_collect_files().wait()?;
    if !garbage.failed_to_delete_files.is_empty() {
        return Err(FullTextError::Io);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_foreign_payload_is_damage_not_generation_zero() {
        assert_eq!(parse_payload(&payload(42)), Some(42));
        assert_eq!(parse_payload("something else"), None);
        assert_eq!(parse_payload(&format!("{PAYLOAD_PREFIX}-1")), None);
    }
}
