//! Checks that incomplete clearing and damaged logs cannot bypass the restore barrier.

use super::{
    DeletionCategory, DeletionError, DeletionLog, DeletionRange, DeletionRecord,
    EMPTY_DELETION_LOG, MAX_LOG_BYTES, RECORD_SIZE, checksum,
};

fn range(start: i64, end: i64) -> DeletionRange {
    DeletionRange::new(start, end).expect("synthetic time interval")
}

fn append(log: &mut DeletionLog, bytes: &mut Vec<u8>, record: DeletionRecord) {
    // The in-memory transport represents a successful durable append, not real filesystem I/O.
    bytes.extend_from_slice(&record.encode());
    log.accept(record)
        .expect("accept persisted synthetic record");
}

fn clear(log: &mut DeletionLog, bytes: &mut Vec<u8>, interval: DeletionRange) {
    clear_with(log, bytes, DeletionCategory::History, interval);
}

fn clear_with(
    log: &mut DeletionLog,
    bytes: &mut Vec<u8>,
    category: DeletionCategory,
    interval: DeletionRange,
) {
    let requested = log
        .request(category, interval)
        .expect("prepare synthetic clear");
    append(log, bytes, requested);
    let completed = log.completion().expect("prepare completed synthetic clear");
    append(log, bytes, completed);
}

/// Recomputes a hand-edited record's checksum, so only the edited field can fail decoding.
fn resealed(mut record: [u8; RECORD_SIZE]) -> [u8; RECORD_SIZE] {
    let checksum = checksum(&record[..RECORD_SIZE - 4]);
    record[RECORD_SIZE - 4..].copy_from_slice(&checksum.to_be_bytes());
    record
}

#[test]
fn a_zoom_clear_record_differs_from_a_history_one_only_by_its_category_byte_and_checksum() {
    // The record carries no host: a zoom clear is the same 48 bytes as a history clear
    // except for the category byte, so nothing about the cleared sites can be in it.
    let history = DeletionLog::default()
        .request(DeletionCategory::History, range(10, 20))
        .expect("prepare synthetic history clear")
        .encode();
    let zoom = DeletionLog::default()
        .request(DeletionCategory::SiteZoom, range(10, 20))
        .expect("prepare synthetic zoom clear")
        .encode();
    assert_eq!(history[9], 1);
    assert_eq!(zoom[9], 2);
    assert_eq!(history[..9], zoom[..9]);
    assert_eq!(history[10..RECORD_SIZE - 4], zoom[10..RECORD_SIZE - 4]);
    assert_ne!(history[RECORD_SIZE - 4..], zoom[RECORD_SIZE - 4..]);
}

#[test]
fn zoom_clears_round_trip_and_an_unknown_category_still_fails_closed() {
    let mut log = DeletionLog::default();
    let mut bytes = EMPTY_DELETION_LOG.to_vec();
    clear(&mut log, &mut bytes, range(10, 20));
    clear_with(
        &mut log,
        &mut bytes,
        DeletionCategory::SiteZoom,
        range(30, 40),
    );

    let recovered = DeletionLog::decode(&bytes, 2).expect("mixed log reloads");
    assert_eq!(recovered, log);
    let categories: Vec<_> = recovered
        .unapplied(0)
        .expect("every clear")
        .iter()
        .map(|request| request.category())
        .collect();
    assert_eq!(
        categories,
        [DeletionCategory::History, DeletionCategory::SiteZoom]
    );
    assert_eq!(
        recovered.unapplied(1).expect("zoom clear")[0].range(),
        range(30, 40)
    );

    // A category this build does not know is refused even with a valid checksum (D47).
    let zoom_request = EMPTY_DELETION_LOG.len() + 2 * RECORD_SIZE;
    let mut future: [u8; RECORD_SIZE] = bytes[zoom_request..zoom_request + RECORD_SIZE]
        .try_into()
        .expect("one whole record");
    future[9] = 3;
    let mut unknown = bytes[..zoom_request].to_vec();
    unknown.extend_from_slice(&resealed(future));
    assert_eq!(
        DeletionLog::decode(&unknown, 1),
        Err(DeletionError::UnsupportedCategory)
    );
}

#[test]
fn older_backup_replays_every_later_completed_clear() {
    let mut log = DeletionLog::default();
    let mut bytes = EMPTY_DELETION_LOG.to_vec();
    clear(&mut log, &mut bytes, range(10, 20));
    clear(&mut log, &mut bytes, range(40, 50));
    clear(&mut log, &mut bytes, range(-20, -10));

    let recovered = DeletionLog::decode(&bytes, 3).expect("complete log reloads");
    assert_eq!(recovered, log);
    let replay = recovered.replay_after(1).expect("older backup can replay");
    assert_eq!(replay.len(), 2);
    assert_eq!(replay[0].generation(), 2);
    assert_eq!(replay[0].category(), DeletionCategory::History);
    assert_eq!(replay[0].range(), range(40, 50));
    assert_eq!(replay[1].generation(), 3);
    assert!(
        recovered
            .replay_after(3)
            .expect("current generation")
            .is_empty()
    );
    assert_eq!(
        recovered.replay_after(4),
        Err(DeletionError::MissingGeneration)
    );
}

#[test]
fn interrupted_clear_blocks_restore_and_new_requests_until_completion() {
    let mut log = DeletionLog::default();
    let mut bytes = EMPTY_DELETION_LOG.to_vec();
    let requested = log
        .request(DeletionCategory::History, range(10, 30))
        .unwrap();
    // Preparing a request does not mutate the log or claim that the write happened.
    assert_eq!(log.completed_generation(), 0);
    assert_eq!(log.pending(), None);
    append(&mut log, &mut bytes, requested);

    let mut reopened = DeletionLog::decode(&bytes, 1).expect("pending request survives restart");
    assert_eq!(reopened.replay_after(0), Err(DeletionError::PendingClear));
    assert_eq!(
        reopened.request(DeletionCategory::History, range(40, 50)),
        Err(DeletionError::PendingClear)
    );
    assert_eq!(reopened.pending().unwrap().range(), range(10, 30));
    let completion = reopened.completion().unwrap();
    append(&mut reopened, &mut bytes, completion);
    assert_eq!(reopened.pending(), None);
    assert_eq!(reopened.replay_after(0).unwrap().len(), 1);
}

#[test]
fn missing_truncated_and_bit_damaged_records_fail_closed() {
    let mut log = DeletionLog::default();
    let mut bytes = EMPTY_DELETION_LOG.to_vec();
    clear(&mut log, &mut bytes, range(10, 30));
    assert!(DeletionLog::decode(&[], 0).is_err());
    assert_eq!(
        DeletionLog::decode(EMPTY_DELETION_LOG, 1),
        Err(DeletionError::MissingGeneration)
    );
    for end in 0..bytes.len() {
        // Whole-frame loss is detected by the independently retained generation or pending state.
        let result = DeletionLog::decode(&bytes[..end], 1);
        if end == EMPTY_DELETION_LOG.len() + RECORD_SIZE {
            assert_eq!(
                result.unwrap().replay_after(0),
                Err(DeletionError::PendingClear)
            );
        } else {
            assert!(result.is_err(), "incomplete journal must be rejected");
        }
    }
    for index in 0..bytes.len() {
        let mut damaged = bytes.clone();
        damaged[index] ^= 1;
        assert!(
            DeletionLog::decode(&damaged, 1).is_err(),
            "damaged journal must be rejected"
        );
    }
}

#[test]
fn an_older_complete_log_cannot_replace_the_external_generation_barrier() {
    let mut log = DeletionLog::default();
    let mut bytes = EMPTY_DELETION_LOG.to_vec();
    clear(&mut log, &mut bytes, range(0, 10));
    let older_bytes = bytes.clone();
    clear(&mut log, &mut bytes, range(20, 30));
    assert_eq!(
        DeletionLog::decode(&older_bytes, 2),
        Err(DeletionError::MissingGeneration)
    );
    assert!(DeletionLog::decode(&bytes, 2).is_ok());
}

#[test]
fn generations_and_completion_pairs_cannot_be_skipped_or_reordered() {
    let mut log = DeletionLog::default();
    let mut bytes = EMPTY_DELETION_LOG.to_vec();
    clear(&mut log, &mut bytes, range(0, 10));
    let second = log
        .request(DeletionCategory::History, range(20, 30))
        .unwrap();
    let mut empty = DeletionLog::default();
    assert_eq!(empty.accept(second), Err(DeletionError::MissingGeneration));
    assert_eq!(empty, DeletionLog::default());
    assert_eq!(empty.completion(), Err(DeletionError::NoPendingClear));

    let completion_bytes = &bytes[EMPTY_DELETION_LOG.len() + RECORD_SIZE..];
    let mut out_of_order = EMPTY_DELETION_LOG.to_vec();
    out_of_order.extend_from_slice(completion_bytes);
    assert_eq!(
        DeletionLog::decode(&out_of_order, 0),
        Err(DeletionError::CompletionMismatch)
    );
    let previous = log.clone();
    let request = log.requests[0];
    assert_eq!(
        log.accept(DeletionRecord::Completed(request)),
        Err(DeletionError::CompletionMismatch)
    );
    assert_eq!(log, previous);
}

#[test]
fn unknown_kinds_and_versions_are_not_ignored_even_with_valid_checksums() {
    let requested = DeletionLog::default()
        .request(DeletionCategory::History, range(0, 10))
        .unwrap();
    for (offset, expected) in [
        (8, DeletionError::UnsupportedFormat),
        (9, DeletionError::UnsupportedCategory),
        (10, DeletionError::UnsupportedFormat),
    ] {
        let mut record = requested.encode();
        record[offset] = 99;
        let checksum = checksum(&record[..RECORD_SIZE - 4]);
        record[RECORD_SIZE - 4..].copy_from_slice(&checksum.to_be_bytes());
        let mut bytes = EMPTY_DELETION_LOG.to_vec();
        bytes.extend_from_slice(&record);
        assert_eq!(DeletionLog::decode(&bytes, 0), Err(expected));
    }
}

#[test]
fn invalid_ranges_limits_and_exhausted_generations_are_explicit() {
    assert_eq!(DeletionRange::new(1, 1), Err(DeletionError::InvalidRange));
    assert_eq!(DeletionRange::new(2, 1), Err(DeletionError::InvalidRange));
    assert_eq!(range(i64::MIN, i64::MAX).start(), i64::MIN);
    assert_eq!(range(i64::MIN, i64::MAX).end(), i64::MAX);
    assert_eq!(
        DeletionLog::decode(&vec![0; MAX_LOG_BYTES + 1], 0),
        Err(DeletionError::TooLarge)
    );
    let exhausted = DeletionLog {
        requests: Vec::new(),
        completed_generation: u64::MAX,
    };
    assert_eq!(
        exhausted.request(DeletionCategory::History, range(0, 10)),
        Err(DeletionError::GenerationExhausted)
    );
}

#[test]
fn checksum_matches_the_standard_crc32_check_value() {
    assert_eq!(checksum(b"123456789"), 0xcbf4_3926);
}

#[test]
fn diagnostics_do_not_disclose_clear_intervals() {
    let mut log = DeletionLog::default();
    let mut bytes = EMPTY_DELETION_LOG.to_vec();
    let interval = range(478_932_850, 478_933_210);
    clear(&mut log, &mut bytes, interval);
    let diagnostics = format!("{log:?} {interval:?} {:?}", log.requests[0]);
    assert!(!diagnostics.contains("478932850"));
    assert!(!diagnostics.contains("478933210"));
}

#[test]
fn unapplied_includes_a_pending_clear_but_never_a_generation_the_log_lacks() {
    let mut log = DeletionLog::default();
    let mut bytes = EMPTY_DELETION_LOG.to_vec();
    assert_eq!(log.last_generation(), 0);
    assert!(log.unapplied(0).expect("empty log").is_empty());
    clear(&mut log, &mut bytes, range(10, 20));
    let pending = log
        .request(DeletionCategory::History, range(30, 40))
        .expect("prepare pending synthetic clear");
    append(&mut log, &mut bytes, pending);

    // Recovery may apply the interrupted request; completed-only replay still refuses it.
    assert_eq!(log.last_generation(), 2);
    let all = log.unapplied(0).expect("copy without clears");
    assert_eq!(all.len(), 2);
    assert_eq!(all[1].range(), range(30, 40));
    assert_eq!(log.unapplied(1).expect("copy after first").len(), 1);
    assert!(log.unapplied(2).expect("copy with both").is_empty());
    assert_eq!(log.unapplied(3), Err(DeletionError::MissingGeneration));
    assert_eq!(log.replay_after(0), Err(DeletionError::PendingClear));
}
