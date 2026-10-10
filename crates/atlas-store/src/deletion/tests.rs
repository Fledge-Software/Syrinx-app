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
    let requested = log
        .request(DeletionCategory::History, interval)
        .expect("prepare synthetic clear");
    append(log, bytes, requested);
    let completed = log.completion().expect("prepare completed synthetic clear");
    append(log, bytes, completed);
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
