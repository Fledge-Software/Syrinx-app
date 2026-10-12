//! Proves the journal's creation, exclusive ownership, durable appends and damage refusal.

use std::fs;

use atlas_store::deletion::{DeletionCategory, DeletionRange};

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

#[test]
fn an_empty_file_is_initialized_but_a_damaged_tail_is_refused() {
    let fixture = Fixture::new();
    let lease = WindowsDirectoryLease::prepare(&fixture.root).expect("prepare synthetic location");
    let path = fixture.root.join(JOURNAL_NAME);
    fs::write(&path, b"").expect("create synthetic empty journal");
    assert_eq!(read_deletion_log(&fixture.root), Ok(DeletionLog::default()));
    drop(DeletionJournal::open(&lease).expect("empty journal initializes"));
    assert_eq!(fs::read(&path).expect("read journal"), EMPTY_DELETION_LOG);

    // A torn append is refused by both readers, never silently truncated.
    let mut torn = EMPTY_DELETION_LOG.to_vec();
    torn.extend_from_slice(b"partial");
    fs::write(&path, &torn).expect("write synthetic torn journal");
    assert_eq!(
        read_deletion_log(&fixture.root),
        Err(JournalError::Deletion(DeletionError::DamagedRecord))
    );
    assert!(matches!(
        DeletionJournal::open(&lease),
        Err(JournalError::Deletion(DeletionError::DamagedRecord))
    ));
    assert_eq!(fs::read(&path).expect("reread journal"), torn);
}
