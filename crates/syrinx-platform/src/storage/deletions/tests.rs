//! Proves the journal's creation, exclusive ownership, durable appends and damage refusal.

use std::fs;

use syrinx_store::deletion::{DeletionCategory, DeletionRange};

use super::*;
use crate::storage::{WindowsDirectoryLease, tests::Fixture};

fn range() -> DeletionRange {
    DeletionRange::new(100, 200).expect("synthetic interval")
}

#[test]
fn journal_is_created_once_appended_durably_and_reloaded() {
    let fixture = Fixture::new();
    assert_eq!(
        read_deletion_log(&fixture.root.join("not-yet-created")),
        Ok(DeletionLog::default())
    );
    let lease = WindowsDirectoryLease::prepare(&fixture.root).expect("prepare synthetic location");
    let path = fixture.root.join(JOURNAL_NAME);
    drop(DeletionJournal::open(&lease).expect("journal is created"));
    assert_eq!(fs::read(&path).expect("read journal"), EMPTY_DELETION_LOG);

    // The running session holds the journal exclusively, even against plain readers.
    let mut journal = DeletionJournal::open(&lease).expect("journal reopens");
    assert!(matches!(
        DeletionJournal::open(&lease),
        Err(JournalError::Location(LocationError::StorageBusy))
    ));
    assert!(fs::read(&path).is_err());

    let request = journal
        .log()
        .request(DeletionCategory::History, range())
        .expect("prepare synthetic request");
    journal.append(request).expect("request appended");
    let completion = journal.log().completion().expect("prepare completion");
    journal.append(completion).expect("completion appended");

    // A record the log refuses is never written.
    assert_eq!(
        journal.append(completion),
        Err(JournalError::Deletion(DeletionError::CompletionMismatch))
    );
    drop(journal);
    let expected = EMPTY_DELETION_LOG.len() + 2 * request.encode().len();
    assert_eq!(fs::read(&path).expect("read journal").len(), expected);

    let read = read_deletion_log(&fixture.root).expect("journal reads back");
    assert_eq!(read.completed_generation(), 1);
    assert!(read.pending().is_none());
    let reopened = DeletionJournal::open(&lease).expect("journal reopens");
    assert_eq!(reopened.log(), &read);
}

/// Writes one completed synthetic clear into the journal, then closes it.
fn one_clear(lease: &WindowsDirectoryLease) -> Vec<u8> {
    let mut journal = DeletionJournal::open(lease).expect("journal opens");
    let request = journal
        .log()
        .request(DeletionCategory::History, range())
        .expect("prepare synthetic request");
    journal.append(request).expect("request appended");
    let completion = journal.log().completion().expect("prepare completion");
    journal.append(completion).expect("completion appended");
    let mut bytes = EMPTY_DELETION_LOG.to_vec();
    bytes.extend_from_slice(&request.encode());
    bytes.extend_from_slice(&completion.encode());
    bytes
}

#[test]
fn empty_or_torn_header_files_are_initialized_and_a_torn_tail_is_cut_off() {
    let fixture = Fixture::new();
    let lease = WindowsDirectoryLease::prepare(&fixture.root).expect("prepare synthetic location");
    let path = fixture.root.join(JOURNAL_NAME);
    for torn_header in [&b""[..], &EMPTY_DELETION_LOG[..5]] {
        fs::write(&path, torn_header).expect("write synthetic torn header");
        assert_eq!(read_deletion_log(&fixture.root), Ok(DeletionLog::default()));
        drop(DeletionJournal::open(&lease).expect("journal initializes"));
        assert_eq!(fs::read(&path).expect("read journal"), EMPTY_DELETION_LOG);
    }

    // A power cut during an append leaves part of a record nobody acted on.
    let complete = one_clear(&lease);
    let mut torn = complete.clone();
    torn.extend_from_slice(&[7; DELETION_RECORD_BYTES - 1]);
    fs::write(&path, &torn).expect("write synthetic torn append");
    let read = read_deletion_log(&fixture.root).expect("torn tail is ignored when reading");
    assert_eq!(read.completed_generation(), 1);
    let journal = DeletionJournal::open(&lease).expect("torn tail is cut off");
    assert_eq!(journal.log(), &read);
    drop(journal);
    assert_eq!(fs::read(&path).expect("reread journal"), complete);
}

#[test]
fn a_damaged_complete_record_is_refused_until_an_explicit_rebuild() {
    let fixture = Fixture::new();
    let lease = WindowsDirectoryLease::prepare(&fixture.root).expect("prepare synthetic location");
    let path = fixture.root.join(JOURNAL_NAME);
    let mut damaged = one_clear(&lease);
    let last = damaged.len() - 1;
    damaged[last] ^= 0xff;
    fs::write(&path, &damaged).expect("write synthetic damaged record");
    assert_eq!(
        read_deletion_log(&fixture.root),
        Err(JournalError::Deletion(DeletionError::DamagedRecord))
    );
    assert!(matches!(
        DeletionJournal::open(&lease),
        Err(JournalError::Deletion(DeletionError::DamagedRecord))
    ));
    assert_eq!(fs::read(&path).expect("damage is left alone"), damaged);
    assert_eq!(
        JournalError::Deletion(DeletionError::DamagedRecord).to_string(),
        "Syrinx's record of cleared history is damaged"
    );

    // The confirmed repair keeps the damaged bytes, records two all-time clears and
    // requests a third, which the start-up then applies.
    let rebuilt = DeletionJournal::rebuild(&lease, 2).expect("journal rebuilds");
    assert_eq!(rebuilt.log().completed_generation(), 2);
    assert_eq!(
        rebuilt.log().pending().map(|request| request.generation()),
        Some(3)
    );
    let replay = rebuilt.log().unapplied(0).expect("rebuilt log replays");
    assert_eq!(replay.len(), 3);
    assert!(
        replay
            .iter()
            .all(|request| request.range() == DeletionRange::all_time())
    );
    drop(rebuilt);
    let copies: Vec<_> = fs::read_dir(&fixture.root)
        .expect("list synthetic directory")
        .map(|entry| entry.expect("synthetic entry").path())
        .filter(|entry| {
            entry.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with("syrinx.deletions.damaged-")
            })
        })
        .collect();
    assert_eq!(copies.len(), 1);
    assert_eq!(fs::read(&copies[0]).expect("read preserved copy"), damaged);
    assert_eq!(
        read_deletion_log(&fixture.root)
            .expect("rebuilt journal reads")
            .completed_generation(),
        2
    );
}

#[test]
fn a_repair_interrupted_at_any_step_leaves_the_old_journal_or_the_complete_new_one() {
    let fixture = Fixture::new();
    let lease = WindowsDirectoryLease::prepare(&fixture.root).expect("prepare synthetic location");
    let path = fixture.root.join(JOURNAL_NAME);
    let replacement = replacement_path(&path);
    let mut damaged = one_clear(&lease);
    let last = damaged.len() - 1;
    damaged[last] ^= 0xff;
    fs::write(&path, &damaged).expect("write synthetic damaged record");

    // Stopped after writing the replacement: the old journal is untouched and still
    // refused, so the owner is asked again.
    let bytes = repaired_bytes(1).expect("synthetic repaired bytes");
    drop(write_replacement(&path, &bytes).expect("write synthetic replacement"));
    assert_eq!(fs::read(&replacement).expect("read replacement"), bytes);
    assert_eq!(fs::read(&path).expect("old journal remains"), damaged);
    assert!(matches!(
        DeletionJournal::open(&lease),
        Err(JournalError::Deletion(DeletionError::DamagedRecord))
    ));

    // The repeated repair discards the earlier attempt and installs a complete journal.
    drop(DeletionJournal::rebuild(&lease, 1).expect("journal rebuilds"));
    assert!(!replacement.exists());
    assert_eq!(fs::read(&path).expect("read rebuilt journal"), bytes);

    // Stopped before the start-up applied the requested clear: reopening still has it.
    let reopened = DeletionJournal::open(&lease).expect("rebuilt journal reopens");
    assert_eq!(reopened.log().completed_generation(), 1);
    let pending = reopened
        .log()
        .pending()
        .expect("all-time clear still requested");
    assert_eq!(
        (pending.generation(), pending.range()),
        (2, DeletionRange::all_time())
    );
}
