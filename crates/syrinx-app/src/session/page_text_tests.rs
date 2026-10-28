//! Proves the full-text index's page text follows D47's clears, restores and D40's expiry (#53).

use syrinx_search::fulltext::{
    Change, ClearRequest, ClearScope, FullText, FullTextDocument, Namespace, NamespaceFilter,
    PAGE_RETENTION_SECONDS, SearchRequest,
};
use syrinx_store::deletion::{DeletionCategory, DeletionRange};

use super::*;

const DAY: i64 = retention::DAY_SECS;
/// Every synthetic document carries this word, so one search finds them all.
const WORD: &str = "indexsigil";

fn index_folder(fixture: &Fixture) -> PathBuf {
    fixture.0.join(page_text::INDEX_FOLDER)
}

fn page(id: &str, captured_at: i64) -> Change {
    Change::Upsert(FullTextDocument {
        namespace: Namespace::page(),
        id: id.to_owned(),
        revision: 1,
        captured_at,
        title: "Synthetic page".to_owned(),
        body: format!("{WORD} synthetic page text"),
    })
}

fn note(id: &str) -> Change {
    Change::Upsert(FullTextDocument {
        namespace: notes(),
        id: id.to_owned(),
        revision: 1,
        captured_at: 0,
        title: "Synthetic note".to_owned(),
        body: format!("{WORD} synthetic note text"),
    })
}

fn notes() -> Namespace {
    Namespace::feed("notes").expect("valid synthetic feed name")
}

/// Writes documents into the fixture's index through the same guarded folder Syrinx uses.
/// Each is staged as of its own capture, so an old page is not refused as expired.
fn seed_index(fixture: &Fixture, changes: &[Change]) {
    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data");
    let folder = lease
        .cache_folder(page_text::INDEX_FOLDER)
        .expect("guard synthetic index folder");
    let mut index = FullText::open(folder.path()).expect("open synthetic index");
    for change in changes {
        let Change::Upsert(document) = change else {
            panic!("only synthetic documents are seeded");
        };
        index
            .stage(std::slice::from_ref(change), document.captured_at)
            .expect("stage synthetic document");
    }
    index.commit().expect("commit synthetic documents");
}

/// Sets the index's applied generation directly, as an index ahead of its journal would hold.
fn advance_index(fixture: &Fixture, generations: u64) {
    let mut index = FullText::open(&index_folder(fixture)).expect("open synthetic index");
    let clears: Vec<_> = (1..=generations)
        .map(|generation| ClearRequest {
            generation,
            scope: ClearScope::Elsewhere,
        })
        .collect();
    index
        .apply_clears(&clears)
        .expect("advance synthetic index");
}

/// The ids in one namespace, read while no session holds the index.
fn stored_ids(fixture: &Fixture, namespace: Namespace) -> (Vec<String>, u64) {
    let index = FullText::open(&index_folder(fixture)).expect("reopen synthetic index");
    (ids_in(&index, namespace), index.cleared_generation())
}

fn ids_in(index: &FullText, namespace: Namespace) -> Vec<String> {
    let request = SearchRequest {
        text: WORD.to_owned(),
        prefix_last: false,
        namespaces: NamespaceFilter::Only(vec![namespace]),
        limit: 100,
    };
    let mut ids: Vec<_> = index
        .search(&request)
        .expect("search synthetic index")
        .into_iter()
        .map(|hit| hit.id)
        .collect();
    ids.sort();
    ids
}

fn clear_history(
    handle: &SessionHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
    start: i64,
    end: i64,
) {
    send(
        handle,
        SessionCommand::ClearHistory {
            request_id: 53,
            start,
            end,
        },
    );
    match next_event(events) {
        SessionEvent::HistoryCleared {
            request_id: 53,
            result: Ok(_),
        } => {}
        event => panic!("expected a synthetic history clear, got {event:?}"),
    }
}

fn clear_site_zooms(
    handle: &SessionHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
    start: i64,
    end: i64,
) {
    send(
        handle,
        SessionCommand::ClearSiteZooms {
            request_id: 54,
            start,
            end,
        },
    );
    match next_event(events) {
        SessionEvent::SiteZoomsCleared {
            request_id: 54,
            result: Ok(_),
        } => {}
        event => panic!("expected a synthetic zoom clear, got {event:?}"),
    }
}

/// Journals a history clear and stops, as a crash before any participant applied it would.
fn journal_unapplied_clear(fixture: &Fixture, start: i64, end: i64) {
    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data");
    let mut journal = DeletionJournal::open(&lease).expect("open synthetic journal");
    let range = DeletionRange::new(start, end).expect("valid synthetic range");
    let request = journal
        .log()
        .request(DeletionCategory::History, range)
        .expect("prepare synthetic request");
    journal.append(request).expect("journal synthetic request");
}

fn completed_clears(fixture: &Fixture) -> u64 {
    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data");
    let journal = DeletionJournal::open(&lease).expect("reopen synthetic journal");
    assert_eq!(journal.log().pending(), None);
    journal.log().completed_generation()
}

#[test]
fn history_clears_reach_page_text_and_zoom_clears_do_not() {
    let fixture = Fixture::new();
    let now = utc_seconds().expect("synthetic clock");
    seed_index(
        &fixture,
        &[
            page("inside", now - 600),
            page("before", now - 5_000),
            note("kept"),
        ],
    );

    let (handle, mut events) = fixture.start();
    state(&mut events);
    // A zoom clear of the same hour takes a journal generation but no page text.
    clear_site_zooms(&handle, &mut events, now - 3_600, now + 1);
    clear_history(&handle, &mut events, now - 3_600, now + 1);
    stop(handle, &mut events);

    let (pages, generation) = stored_ids(&fixture, Namespace::page());
    assert_eq!(pages, ["before"]);
    assert_eq!(generation, 2);
    assert_eq!(stored_ids(&fixture, notes()).0, ["kept"]);
    fixture.cleanup();
}

#[test]
fn a_restored_backup_replays_the_journal_onto_the_index_and_keeps_notes() {
    let fixture = Fixture::new();
    let now = utc_seconds().expect("synthetic clock");
    seed_recovery(&fixture);
    seed_index(
        &fixture,
        &[
            page("inside", now - 600),
            page("earlier", now - 50_000),
            page("kept", now - 100_000),
            note("kept"),
        ],
    );

    // The owner clears the last hour, then a second clear is journaled and Syrinx stops
    // before the database or the index applies it.
    let (handle, mut events) = fixture.start();
    state(&mut events);
    clear_history(&handle, &mut events, now - 3_600, now + 1);
    stop(handle, &mut events);
    journal_unapplied_clear(&fixture, now - 60_000, now - 40_000);

    // The database is damaged and the owner restores a backup older than both clears.
    damage_recovery_primary(&fixture);
    let (handle, mut events) = fixture.start();
    let offer_id = recovery_offer(&mut events);
    send(&handle, SessionCommand::ConfirmRecovery { offer_id });
    state(&mut events);
    stop(handle, &mut events);

    // The replay removed the missed range; an index ahead of the restored database but not
    // of the journal kept the rest, and notes are untouched.
    let (pages, generation) = stored_ids(&fixture, Namespace::page());
    assert_eq!(pages, ["kept"]);
    assert_eq!(generation, 2);
    assert_eq!(stored_ids(&fixture, notes()).0, ["kept"]);
    assert_eq!(completed_clears(&fixture), 2);
    remove_preserved_recovery_bundle(&fixture, Some(DAMAGED_RECOVERY_PRIMARY));
    fixture.cleanup();
}

#[test]
fn an_index_ahead_of_a_recreated_journal_loses_page_text_but_keeps_notes() {
    let fixture = Fixture::new();
    let now = utc_seconds().expect("synthetic clock");
    seed_index(&fixture, &[page("orphan", now - 600), note("kept")]);
    // The index applied three clears that this new, empty journal has never recorded.
    advance_index(&fixture, 3);

    let (handle, mut events) = fixture.start();
    state(&mut events);
    stop(handle, &mut events);

    let (pages, generation) = stored_ids(&fixture, Namespace::page());
    assert!(pages.is_empty());
    assert_eq!(generation, 0);
    assert_eq!(stored_ids(&fixture, notes()).0, ["kept"]);

    // Counting again from zero, the next clear lines up with the journal.
    let (handle, mut events) = fixture.start();
    state(&mut events);
    clear_history(&handle, &mut events, now - 3_600, now + 1);
    stop(handle, &mut events);
    assert_eq!(stored_ids(&fixture, Namespace::page()).1, 1);
    fixture.cleanup();
}

#[test]
fn page_text_expires_at_start_up_and_then_once_a_day() {
    let fixture = Fixture::new();
    let now = utc_seconds().expect("synthetic clock");
    let half_day = DAY / 2;
    seed_index(
        &fixture,
        &[
            page("stale", now - PAGE_RETENTION_SECONDS - 60),
            // Seven days old half a day from now: kept at start-up, gone a day later.
            page("ageing", now - PAGE_RETENTION_SECONDS + half_day),
            page("fresh", now - 60),
            note("kept"),
        ],
    );
    let lease =
        Arc::new(WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic store"));
    let journal = DeletionJournal::open(&lease).expect("open synthetic journal");
    let store = Store::open(lease.clone()).expect("open guarded synthetic store");
    let mut worker = Worker::restore(&store, journal, page_text::PageText::open(&lease))
        .expect("restore synthetic worker");
    let pages = |worker: &Worker| {
        ids_in(
            worker.page_text.index().expect("the index is open"),
            Namespace::page(),
        )
    };
    assert_eq!(pages(&worker), ["ageing", "fresh"]);

    // The "ageing" page passes seven days before the next daily run, which waits for its day.
    worker.page_text.expire_if_due(now + half_day + 60);
    assert_eq!(pages(&worker), ["ageing", "fresh"]);
    // A minute past the day, since the restore read the clock just after `now`.
    worker.page_text.expire_if_due(now + DAY + 60);
    assert_eq!(pages(&worker), ["fresh"]);
    assert_eq!(
        ids_in(
            worker.page_text.index().expect("the index is open"),
            notes()
        ),
        ["kept"]
    );
    assert_eq!(worker.page_text.take_problem(), Ok(()));
    drop(worker);
    store.shutdown().expect("close synthetic store");
    drop(lease);
    fixture.cleanup();
}

#[test]
fn an_index_that_cannot_open_is_reported_and_the_session_goes_on() {
    let fixture = Fixture::new();
    let now = utc_seconds().expect("synthetic clock");
    // A file where the index folder belongs: the guard refuses it rather than write elsewhere.
    drop(WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data"));
    checked_io(fs::write(index_folder(&fixture), b"synthetic blocker"));

    let (handle, mut events) = fixture.start();
    state(&mut events);
    match next_event(&mut events) {
        SessionEvent::Failure {
            error: SessionError::During { operation, error },
            fatal: false,
        } => {
            assert_eq!(operation, "opening the search index");
            assert_eq!(
                *error,
                SessionError::IndexFolder(LocationError::UnexpectedEntryKind)
            );
        }
        event => panic!("expected the index failure to be reported, got {event:?}"),
    }
    // Clearing still works and completes: the journal brings the index level later.
    clear_history(&handle, &mut events, now - 3_600, now + 1);
    stop(handle, &mut events);
    assert_eq!(completed_clears(&fixture), 1);
    checked_io(fs::remove_file(index_folder(&fixture)));
    fixture.cleanup();
}

#[test]
fn a_confirmed_history_repair_clears_page_text_even_when_the_index_matches_the_new_journal() {
    let fixture = Fixture::new();
    // The rebuilt journal of a database at generation 0 ends with a pending all-time clear at
    // generation 1; an index that already counted to 1 would otherwise skip that clear.
    seed_index_ahead_of_a_repair(&fixture);

    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data");
    let (store, worker) = open_session(lease, true).expect("open with a confirmed repair");
    let index = worker.page_text.index().expect("the index is open");
    assert!(ids_in(index, Namespace::page()).is_empty());
    assert_eq!(ids_in(index, notes()), ["kept"]);
    assert_eq!(index.cleared_generation(), 1);
    finish_repaired_session(fixture, store, worker);
}

/// An index one generation ahead of the journal a repair of a database at generation 0
/// rebuilds, holding page text the repair must remove and a note a restart keeps.
fn seed_index_ahead_of_a_repair(fixture: &Fixture) {
    let now = utc_seconds().expect("synthetic clock");
    seed_index(fixture, &[page("cleared", now - 600), note("kept")]);
    advance_index(fixture, 1);
}

/// Closes a session opened with a confirmed repair, checks its one rebuilt clear completed,
/// and removes the fixture.
fn finish_repaired_session(fixture: Fixture, store: Store, worker: Worker) {
    drop(worker);
    store.shutdown().expect("close synthetic store");
    assert_eq!(completed_clears(&fixture), 1);
    fixture.cleanup();
}

/// Windows' `FILE_SHARE_DELETE`: others may delete the file but not read it.
const SHARE_DELETE_ONLY: u32 = 0x4;
/// No sharing: others may neither read nor delete the file.
const SHARE_NOTHING: u32 = 0;
/// Windows' `FILE_SHARE_READ`: others may read the file but not delete it.
const SHARE_READ_ONLY: u32 = 0x1;

/// Holds the index's `meta.json` open, sharing only `share_mode`, so Tantivy cannot read it.
fn hold_index_metadata(fixture: &Fixture, share_mode: u32) -> fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .share_mode(share_mode)
        .open(index_folder(fixture).join("meta.json"))
        .expect("hold synthetic index metadata")
}

#[test]
fn a_repair_whose_index_cannot_open_empties_its_folder_and_starts_afresh() {
    let fixture = Fixture::new();
    seed_index_ahead_of_a_repair(&fixture);
    // The index cannot open, so it cannot restart its count, but its files can be deleted.
    // NTFS's POSIX-style deletion frees the held file's name at once, so the fresh index can
    // write its own `meta.json` while this handle is still open.
    let held = hold_index_metadata(&fixture, SHARE_DELETE_ONLY);

    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data");
    let (store, mut worker) = open_session(lease, true).expect("open with a confirmed repair");
    drop(held);
    let index = worker.page_text.index().expect("a fresh index is open");
    assert!(ids_in(index, Namespace::page()).is_empty());
    // Emptying cannot spare a feed: nothing in an index that will not open can be read.
    assert!(ids_in(index, notes()).is_empty());
    assert_eq!(index.cleared_generation(), 1);
    // The owner still hears that the old index could not open.
    match worker.page_text.take_problem() {
        Err(SessionError::During { operation, .. }) => {
            assert_eq!(operation, "opening the search index");
        }
        problem => panic!("expected the open failure to be reported, got {problem:?}"),
    }
    finish_repaired_session(fixture, store, worker);
}

#[test]
fn a_repair_whose_index_opens_but_cannot_restart_empties_its_folder_and_starts_afresh() {
    let fixture = Fixture::new();
    seed_index_ahead_of_a_repair(&fixture);
    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data");
    let mut page_text = page_text::PageText::open(&lease);
    // Uncommitted work makes the restart refuse; the open index must then let go of its files.
    let now = utc_seconds().expect("synthetic clock");
    page_text
        .index_mut()
        .expect("the seeded index opens")
        .stage(&[page("staged", now)], now)
        .expect("stage synthetic document");

    page_text
        .restart_before_repair(&lease)
        .expect("the folder is emptied instead");
    let index = page_text.index().expect("a fresh index is open");
    assert!(ids_in(index, Namespace::page()).is_empty());
    assert!(ids_in(index, notes()).is_empty());
    assert_eq!(index.cleared_generation(), 0);
    match page_text.take_problem() {
        Err(SessionError::During { operation, .. }) => {
            assert_eq!(
                operation,
                "restarting the search index for a history repair"
            );
        }
        problem => panic!("expected the restart failure to be reported, got {problem:?}"),
    }
    drop(page_text);
    drop(lease);
    fixture.cleanup();
}

#[test]
fn a_repair_whose_index_can_neither_open_nor_be_emptied_is_refused_until_it_can() {
    let fixture = Fixture::new();
    seed_index_ahead_of_a_repair(&fixture);
    // Nothing may read or delete the metadata, so the old page text cannot be removed.
    let held = hold_index_metadata(&fixture, SHARE_NOTHING);

    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data");
    let refusal = match open_session(lease, true) {
        Err(error) => error,
        Ok(_) => panic!("a repair went ahead while the old page text could not be removed"),
    };
    // Both causes reach the owner: the open failure, then the refusal naming what to clear.
    let SessionError::AfterEarlier { earlier, error } = &refusal else {
        panic!("expected both failures to be kept, got {refusal:?}");
    };
    assert!(matches!(
        **earlier,
        SessionError::During {
            operation: "opening the search index",
            ..
        }
    ));
    assert!(matches!(**error, SessionError::IndexNotEmptied(_)));
    assert!(refusal.to_string().contains("\"fulltext\" folder"));
    // Nothing was rebuilt, so a damaged journal would still offer the repair at the next start.
    assert!(!fixture.0.join("syrinx.deletions").exists());
    drop(held);

    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data");
    let (store, worker) = open_session(lease, true).expect("repair once the index is free");
    let index = worker.page_text.index().expect("the index is open");
    assert!(ids_in(index, Namespace::page()).is_empty());
    assert_eq!(index.cleared_generation(), 1);
    finish_repaired_session(fixture, store, worker);
}

/// The index's segment files in listing order: not a `.tantivy-*` lock and not `*.json`.
fn segment_files(fixture: &Fixture) -> Vec<PathBuf> {
    checked_io(fs::read_dir(index_folder(fixture)))
        .map(|entry| checked_io(entry).path())
        .filter(|path| is_segment_file(path))
        .collect()
}

fn is_segment_file(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        let name = name.to_string_lossy();
        !name.starts_with('.') && !name.ends_with(".json")
    })
}

/// Holds one of the index's segment files open so it cannot be deleted, and names it.
fn hold_a_segment(fixture: &Fixture) -> (PathBuf, fs::File) {
    use std::os::windows::fs::OpenOptionsExt;
    let segment = segment_files(fixture)
        .into_iter()
        .next()
        .expect("the synthetic index has a segment file");
    let held = checked_io(
        fs::OpenOptions::new()
            .read(true)
            .share_mode(SHARE_READ_ONLY)
            .open(&segment),
    );
    (segment, held)
}

#[test]
fn an_ordinary_start_empties_an_index_a_failed_empty_left_without_some_segment_files() {
    let fixture = Fixture::new();
    let now = utc_seconds().expect("synthetic clock");
    seed_index(&fixture, &[page("leftover", now - 600), note("lost")]);
    // A failed empty deletes in listing order and stops at the held file, leaving the
    // metadata and the rest of the segment behind.
    let segment = segment_files(&fixture);
    let (deleted, kept) = segment
        .split_first()
        .expect("the synthetic index has a segment");
    checked_io(fs::remove_file(deleted));

    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data");
    let mut page_text = page_text::PageText::open(&lease);
    let index = page_text.index().expect("a fresh index is open");
    assert!(ids_in(index, Namespace::page()).is_empty());
    assert!(ids_in(index, notes()).is_empty());
    assert!(
        kept.iter().all(|file| !file.exists()),
        "the leftover segment was removed"
    );
    assert_eq!(page_text.take_problem(), Ok(()));
    drop(page_text);
    drop(lease);
    fixture.cleanup();
}

#[test]
fn a_repair_whose_index_is_only_partly_emptied_is_refused_and_retried_until_it_is_gone() {
    let fixture = Fixture::new();
    seed_index_ahead_of_a_repair(&fixture);
    // The index cannot open, so the repair empties it, but one segment cannot be deleted.
    let metadata = hold_index_metadata(&fixture, SHARE_DELETE_ONLY);
    let (segment, held) = hold_a_segment(&fixture);

    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data");
    let refusal = match open_session(lease, true) {
        Err(error) => error,
        Ok(_) => panic!("a repair went ahead while a segment of old page text was left"),
    };
    let SessionError::AfterEarlier { error, .. } = &refusal else {
        panic!("expected both failures to be kept, got {refusal:?}");
    };
    assert!(matches!(**error, SessionError::IndexNotEmptied(_)));
    drop(metadata);
    drop(held);
    // The metadata went last, so it survives: the next open finds the old index damaged,
    // not a fresh one that would never track the leftover segment.
    assert!(index_folder(&fixture).join("meta.json").is_file());
    assert!(index_folder(&fixture).join(".managed.json").is_file());
    assert_eq!(segment_files(&fixture), std::slice::from_ref(&segment));

    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data");
    let (store, worker) = open_session(lease, true).expect("repair once the segment is free");
    let index = worker.page_text.index().expect("the index is open");
    assert!(ids_in(index, Namespace::page()).is_empty());
    assert_eq!(index.cleared_generation(), 1);
    assert!(!segment.exists(), "the leftover segment was removed");
    finish_repaired_session(fixture, store, worker);
}

#[test]
fn a_repair_goes_ahead_when_a_file_stands_where_the_index_folder_belongs() {
    let fixture = Fixture::new();
    // Syrinx never opens an index through a file, so refusing the repair would protect nothing.
    drop(WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data"));
    checked_io(fs::write(index_folder(&fixture), b"synthetic blocker"));

    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data");
    let (store, mut worker) = open_session(lease, true).expect("open with a confirmed repair");
    assert!(worker.page_text.index().is_none());
    // The guard's refusal at opening and again at the repair are both kept.
    let blocked = |operation| SessionError::During {
        operation,
        error: Box::new(SessionError::IndexFolder(
            LocationError::UnexpectedEntryKind,
        )),
    };
    assert_eq!(
        worker.page_text.take_problem(),
        Err(SessionError::AfterEarlier {
            earlier: Box::new(blocked("opening the search index")),
            error: Box::new(blocked("emptying the search index before a history repair")),
        })
    );
    drop(worker);
    store.shutdown().expect("close synthetic store");
    assert_eq!(completed_clears(&fixture), 1);
    checked_io(fs::remove_file(index_folder(&fixture)));
    fixture.cleanup();
}

#[test]
fn a_repair_goes_ahead_when_a_junction_stands_where_the_index_folder_belongs() {
    let fixture = Fixture::new();
    drop(WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data"));
    let target = fixture.1.join("junction-target");
    checked_io(fs::create_dir(&target));
    // Only fixed relative names reach the shell; its working directory is this owned fixture.
    let output = std::process::Command::new("cmd.exe")
        .current_dir(&fixture.0)
        .args([
            "/d",
            "/c",
            "mklink",
            "/J",
            page_text::INDEX_FOLDER,
            r"..\junction-target",
        ])
        .output()
        .expect("create disposable directory junction");
    assert!(
        output.status.success(),
        "synthetic junction creation must succeed"
    );

    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data");
    let (store, mut worker) = open_session(lease, true).expect("open with a confirmed repair");
    assert!(worker.page_text.index().is_none());
    let Err(SessionError::AfterEarlier { error, .. }) = worker.page_text.take_problem() else {
        panic!("expected the junction to be reported");
    };
    assert_eq!(
        *error,
        SessionError::During {
            operation: "emptying the search index before a history repair",
            error: Box::new(SessionError::IndexFolder(LocationError::ReparsePoint)),
        }
    );
    // Nothing was written through the junction.
    assert_eq!(checked_io(fs::read_dir(&target)).count(), 0);
    drop(worker);
    store.shutdown().expect("close synthetic store");
    assert_eq!(completed_clears(&fixture), 1);
    checked_io(fs::remove_dir(index_folder(&fixture)));
    checked_io(fs::remove_dir(&target));
    fixture.cleanup();
}

#[test]
fn a_repair_is_refused_when_the_data_folder_fails_its_guard_though_the_index_folder_is_fine() {
    let fixture = Fixture::new();
    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data");
    // A folder where SQLite's journal belongs fails the data folder's own guard with the same
    // kind of error a misplaced index entry gives; it is not the index's to report and go on.
    // The index's own folder is a plain folder, so only the data folder's fault remains.
    checked_io(fs::create_dir(index_folder(&fixture)));
    let sidecar = fixture.0.join("syrinx.db-journal");
    checked_io(fs::create_dir(&sidecar));
    let mut page_text = page_text::PageText::open(&lease);
    assert!(page_text.index().is_none());

    match page_text.restart_before_repair(&lease) {
        Err(SessionError::AfterEarlier { error, .. }) => assert_eq!(
            *error,
            SessionError::During {
                operation: "emptying the search index before a history repair",
                error: Box::new(SessionError::IndexFolder(
                    LocationError::UnexpectedEntryKind,
                )),
            }
        ),
        result => panic!("expected the repair to be refused, got {result:?}"),
    }
    drop(page_text);
    drop(lease);
    checked_io(fs::remove_dir(&sidecar));
    fixture.cleanup();
}

#[test]
fn a_clear_the_index_applied_before_a_crash_is_completed_but_not_applied_again() {
    let fixture = Fixture::new();
    let now = utc_seconds().expect("synthetic clock");
    journal_unapplied_clear(&fixture, now - 3_600, now + 1);
    // The index applied generation 1, then a page dated inside its range was captured again;
    // Syrinx stopped before the database or the journal's completion caught up.
    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data");
    drop(
        lease
            .cache_folder(page_text::INDEX_FOLDER)
            .expect("guard synthetic index folder"),
    );
    drop(lease);
    let mut index = FullText::open(&index_folder(&fixture)).expect("open synthetic index");
    let clear = ClearRequest {
        generation: 1,
        scope: ClearScope::Pages {
            start: now - 3_600,
            end: now + 1,
        },
    };
    index
        .apply_clears(&[clear])
        .expect("apply the synthetic clear");
    drop(index);
    seed_index(&fixture, &[page("recaptured", now - 600)]);

    let (handle, mut events) = fixture.start();
    state(&mut events);
    stop(handle, &mut events);

    let (pages, generation) = stored_ids(&fixture, Namespace::page());
    assert_eq!(pages, ["recaptured"], "the clear was applied twice");
    assert_eq!(generation, 1);
    assert_eq!(completed_clears(&fixture), 1);
    fixture.cleanup();
}

#[test]
fn a_damaged_index_is_emptied_and_started_afresh() {
    let fixture = Fixture::new();
    let now = utc_seconds().expect("synthetic clock");
    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data");
    drop(
        lease
            .cache_folder(page_text::INDEX_FOLDER)
            .expect("guard synthetic index folder"),
    );
    drop(lease);
    // Unreadable metadata beside a stray file that stands in for an old segment's text.
    checked_io(fs::write(
        index_folder(&fixture).join("meta.json"),
        b"synthetic damaged metadata",
    ));
    let stray = index_folder(&fixture).join("synthetic-segment.store");
    checked_io(fs::write(&stray, b"synthetic stale page text"));

    let (handle, mut events) = fixture.start();
    state(&mut events);
    clear_history(&handle, &mut events, now - 3_600, now + 1);
    stop(handle, &mut events);

    // The cache was rebuilt: nothing old is left, and it follows the journal from there.
    assert!(!stray.exists());
    assert_eq!(stored_ids(&fixture, Namespace::page()).1, 1);
    fixture.cleanup();
}

/// A synthetic capture as the window hands it over, of a page that loaded a minute ago.
fn captured(address: &str, body: &str) -> CapturedPage {
    let now = utc_seconds().expect("synthetic clock");
    visited(address, body, now - 60)
}

/// A synthetic capture of a page visited (loaded, or moved to by the History API) at `visited_at`.
fn visited(address: &str, body: &str, visited_at: i64) -> CapturedPage {
    CapturedPage {
        address: address.to_owned(),
        title: "Synthetic captured page".to_owned(),
        text: body.to_owned(),
        visited_at,
    }
}

/// The page ids whose title or body holds `word`.
fn pages_with(index: &FullText, word: &str) -> Vec<String> {
    let request = SearchRequest {
        text: word.to_owned(),
        prefix_last: false,
        namespaces: NamespaceFilter::Only(vec![Namespace::page()]),
        limit: 100,
    };
    let hits = index.search(&request).expect("search synthetic index");
    hits.into_iter().map(|hit| hit.id).collect()
}

/// Every file's bytes under `folder`, however deep.
fn files_under(folder: &Path) -> Vec<Vec<u8>> {
    let mut files = Vec::new();
    for entry in checked_io(fs::read_dir(folder)) {
        let path = checked_io(entry).path();
        if path.is_dir() {
            files.extend(files_under(&path));
        } else {
            files.push(checked_io(fs::read(&path)));
        }
    }
    files
}

/// A worker's page text opened on the fixture's guarded folder, without a session.
fn open_page_text(fixture: &Fixture) -> (Arc<WindowsDirectoryLease>, page_text::PageText) {
    let lease = Arc::new(WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data"));
    let page_text = page_text::PageText::open(&lease);
    (lease, page_text)
}

#[test]
fn a_captured_page_is_kept_found_by_a_body_word_and_never_backed_up() {
    let fixture = Fixture::new();
    let address = "https://kept.example.test/article";
    let (handle, mut events) = fixture.start();
    state(&mut events);
    let body = format!("{WORD} synthetic captured words");
    send(
        &handle,
        SessionCommand::StorePageText(captured(address, &body)),
    );
    // A snapshot taken after the capture holds the database only (D40).
    send(&handle, SessionCommand::SnapshotBeforeUpdate);
    match next_event(&mut events) {
        SessionEvent::UpdateSnapshot(Ok(())) => {}
        event => panic!("expected a synthetic snapshot, got {event:?}"),
    }
    stop(handle, &mut events);

    let index = FullText::open(&index_folder(&fixture)).expect("reopen synthetic index");
    assert_eq!(pages_with(&index, "captured"), [address]);
    drop(index);
    let files = files_under(&fixture.0.join("backups"));
    assert!(!files.is_empty(), "the snapshot was taken");
    for bytes in files {
        let found = bytes
            .windows(WORD.len())
            .any(|window| window == WORD.as_bytes());
        assert!(!found, "page text reached a backup");
    }
    fixture.cleanup();
}

#[test]
fn a_later_capture_replaces_the_text_and_seven_days_remove_it() {
    let fixture = Fixture::new();
    let now = utc_seconds().expect("synthetic clock");
    let address = "https://replaced.example.test/";
    let (lease, mut page_text) = open_page_text(&fixture);
    page_text.store(visited(address, "firstcapture words", now), now);
    // Within the same second, the later capture still replaces the earlier one.
    page_text.store(visited(address, "secondcapture words", now), now);
    let index = page_text.index().expect("the index is open");
    assert!(pages_with(index, "firstcapture").is_empty());
    assert_eq!(pages_with(index, "secondcapture"), [address]);

    page_text.expire_if_due(now + PAGE_RETENTION_SECONDS + 1);
    let index = page_text.index().expect("the index is open");
    assert!(pages_with(index, "secondcapture").is_empty());
    assert_eq!(page_text.take_problem(), Ok(()));
    drop(page_text);
    drop(lease);
    fixture.cleanup();
}

#[test]
fn an_address_too_long_to_key_is_left_unread_reported_and_the_index_stays_open() {
    let fixture = Fixture::new();
    let now = utc_seconds().expect("synthetic clock");
    let long = format!("https://long.example.test/{}", "a".repeat(4_096));
    let (lease, mut page_text) = open_page_text(&fixture);
    page_text.store(captured(&long, "longaddress words"), now);
    page_text.store(
        captured("https://short.example.test/", "shortaddress words"),
        now,
    );
    let index = page_text.index().expect("the index is still open");
    assert!(pages_with(index, "longaddress").is_empty());
    assert_eq!(
        pages_with(index, "shortaddress"),
        ["https://short.example.test/"]
    );
    // The skipped page is reported, without its address.
    let problem = page_text.take_problem().expect_err("the skip is reported");
    assert!(
        !problem.to_string().contains("long.example.test"),
        "{problem}"
    );
    drop(page_text);
    drop(lease);
    fixture.cleanup();
}

/// The setting as the window sends it, with no excluded sites.
fn capture(on: bool) -> SessionCommand {
    excluding(on, &[])
}

fn excluding(on: bool, sites: &[&str]) -> SessionCommand {
    SessionCommand::SavePageTextSetting(PageTextSetting {
        on,
        excluded: sites.iter().map(|site| (*site).to_owned()).collect(),
    })
}

#[test]
fn switching_capture_off_erases_page_text_at_once_and_keeps_notes() {
    let fixture = Fixture::new();
    seed_index(&fixture, &[note("kept")]);
    let (handle, mut events) = fixture.start();
    state(&mut events);
    send(&handle, capture(true));
    let body = format!("{WORD} erasedcapture words");
    send(
        &handle,
        SessionCommand::StorePageText(captured("https://erased.example.test/", &body)),
    );
    send(&handle, capture(false));
    stop(handle, &mut events);

    let index = FullText::open(&index_folder(&fixture)).expect("reopen synthetic index");
    assert!(pages_with(&index, "erasedcapture").is_empty());
    assert_eq!(ids_in(&index, notes()), ["kept"]);
    drop(index);

    // A restart neither brings the text back nor erases notes.
    let (handle, mut events) = fixture.start();
    state(&mut events);
    stop(handle, &mut events);
    assert!(stored_ids(&fixture, Namespace::page()).0.is_empty());
    assert_eq!(stored_ids(&fixture, notes()).0, ["kept"]);
    fixture.cleanup();
}

#[test]
fn page_text_left_behind_while_capture_is_off_is_erased_at_start_up() {
    let fixture = Fixture::new();
    let now = utc_seconds().expect("synthetic clock");
    // An erase cut short: the setting is saved off, the text is still there.
    let (handle, mut events) = fixture.start();
    state(&mut events);
    send(&handle, capture(false));
    stop(handle, &mut events);
    seed_index(&fixture, &[page("left", now - 60), note("kept")]);

    let (handle, mut events) = fixture.start();
    state(&mut events);
    stop(handle, &mut events);
    assert!(stored_ids(&fixture, Namespace::page()).0.is_empty());
    assert_eq!(stored_ids(&fixture, notes()).0, ["kept"]);
    fixture.cleanup();
}

#[test]
fn erased_page_text_does_not_return_with_a_restored_backup() {
    let fixture = Fixture::new();
    let now = utc_seconds().expect("synthetic clock");
    seed_recovery(&fixture);
    seed_index(&fixture, &[page("before", now - 600)]);
    let (handle, mut events) = fixture.start();
    state(&mut events);
    send(&handle, capture(false));
    stop(handle, &mut events);
    assert!(stored_ids(&fixture, Namespace::page()).0.is_empty());

    // The database is damaged and the owner restores a backup taken before the switch-off.
    damage_recovery_primary(&fixture);
    let (handle, mut events) = fixture.start();
    let offer_id = recovery_offer(&mut events);
    send(&handle, SessionCommand::ConfirmRecovery { offer_id });
    state(&mut events);
    stop(handle, &mut events);
    assert!(stored_ids(&fixture, Namespace::page()).0.is_empty());
    remove_preserved_recovery_bundle(&fixture, Some(DAMAGED_RECOVERY_PRIMARY));
    fixture.cleanup();
}

#[test]
fn an_erase_the_index_cannot_make_is_reported_not_swallowed() {
    let fixture = Fixture::new();
    let (lease, mut page_text) = open_page_text(&fixture);
    let now = utc_seconds().expect("synthetic clock");
    // Close the index the way a failed operation does, then switch capture off.
    page_text.store(captured("https://kept.example.test/", "words"), now);
    page_text.close_for_test();
    page_text.erase(now);
    assert!(page_text.take_problem().is_err());
    drop(page_text);
    drop(lease);
    fixture.cleanup();
}

#[test]
fn excluding_a_site_erases_its_text_and_its_subdomains_and_spares_lookalikes() {
    let fixture = Fixture::new();
    seed_index(&fixture, &[note("kept")]);
    let (handle, mut events) = fixture.start();
    state(&mut events);
    send(&handle, capture(true));
    for address in [
        "https://owner.test/account",
        "https://www.owner.test/",
        "https://notowner.test/",
        "https://owner.test.example/",
    ] {
        let body = format!("{WORD} exclusioncapture words");
        send(
            &handle,
            SessionCommand::StorePageText(captured(address, &body)),
        );
    }
    send(&handle, excluding(true, &["owner.test"]));
    stop(handle, &mut events);

    let mut kept = stored_ids(&fixture, Namespace::page()).0;
    kept.sort();
    assert_eq!(
        kept,
        ["https://notowner.test/", "https://owner.test.example/"]
    );
    assert_eq!(stored_ids(&fixture, notes()).0, ["kept"]);
    fixture.cleanup();
}

#[test]
fn text_of_an_excluded_site_left_behind_is_erased_at_start_up() {
    let fixture = Fixture::new();
    let now = utc_seconds().expect("synthetic clock");
    let (handle, mut events) = fixture.start();
    state(&mut events);
    send(&handle, excluding(true, &["owner.test"]));
    stop(handle, &mut events);
    // An erase cut short: the site is excluded, its text is still there.
    seed_index(
        &fixture,
        &[
            page("https://www.owner.test/", now - 60),
            page("https://other.test/", now - 60),
        ],
    );

    let (handle, mut events) = fixture.start();
    state(&mut events);
    stop(handle, &mut events);
    assert_eq!(
        stored_ids(&fixture, Namespace::page()).0,
        ["https://other.test/"]
    );
    fixture.cleanup();
}

#[test]
fn a_capture_of_a_page_visited_before_a_history_clear_never_lands_after_it() {
    let fixture = Fixture::new();
    let now = utc_seconds().expect("synthetic clock");
    let (handle, mut events) = fixture.start();
    state(&mut events);
    send(&handle, capture(true));
    // Clear browsing data's last hour, as the window computes it.
    clear_history(&handle, &mut events, now - 3_600, now + 1);
    // A capture that was still in flight arrives after the clear; one of a page visited
    // after the clear is kept.
    let before = format!("{WORD} inflightcapture words");
    send(
        &handle,
        SessionCommand::StorePageText(visited("https://flight.example.test/", &before, now - 2)),
    );
    let after = format!("{WORD} aftercapture words");
    send(
        &handle,
        SessionCommand::StorePageText(visited("https://after.example.test/", &after, now + 5)),
    );
    stop(handle, &mut events);

    let index = FullText::open(&index_folder(&fixture)).expect("reopen synthetic index");
    assert!(pages_with(&index, "inflightcapture").is_empty());
    assert_eq!(
        pages_with(&index, "aftercapture"),
        ["https://after.example.test/"]
    );
    fixture.cleanup();
}

#[test]
fn a_capture_stored_just_before_a_clear_is_dated_by_its_visit_and_cleared() {
    let fixture = Fixture::new();
    let now = utc_seconds().expect("synthetic clock");
    let (handle, mut events) = fixture.start();
    state(&mut events);
    send(&handle, capture(true));
    let body = format!("{WORD} visitdatedcapture words");
    send(
        &handle,
        SessionCommand::StorePageText(visited("https://dated.example.test/", &body, now - 30)),
    );
    // A range that holds the visit but ends before the store: only the visit's date matches.
    clear_history(&handle, &mut events, now - 60, now - 10);
    stop(handle, &mut events);
    assert!(stored_ids(&fixture, Namespace::page()).0.is_empty());
    fixture.cleanup();
}

#[test]
fn a_capture_from_before_a_switch_off_is_not_kept_after_capture_comes_back_on() {
    let fixture = Fixture::new();
    let now = utc_seconds().expect("synthetic clock");
    let (handle, mut events) = fixture.start();
    state(&mut events);
    send(&handle, capture(true));
    send(&handle, capture(false));
    send(&handle, capture(true));
    let body = format!("{WORD} togglecapture words");
    send(
        &handle,
        SessionCommand::StorePageText(visited("https://toggle.example.test/", &body, now - 2)),
    );
    stop(handle, &mut events);
    assert!(stored_ids(&fixture, Namespace::page()).0.is_empty());
    fixture.cleanup();
}

#[test]
fn a_captured_page_never_shows_its_address_title_or_text_in_debug_output() {
    let page = visited(
        "https://debug-sentinel.example.test/",
        "debug-sentinel words",
        0,
    );
    assert!(!format!("{page:?}").contains("sentinel"));
    let command = SessionCommand::StorePageText(page);
    assert!(!format!("{command:?}").contains("sentinel"));
}
