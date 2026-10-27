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
