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
            error: SessionError::During { error, .. },
            fatal: false,
        } => assert_eq!(
            *error,
            SessionError::IndexFolder(LocationError::UnexpectedEntryKind)
        ),
        event => panic!("expected the index failure to be reported, got {event:?}"),
    }
    // Clearing still works and completes: the journal brings the index level later.
    clear_history(&handle, &mut events, now - 3_600, now + 1);
    stop(handle, &mut events);
    assert_eq!(completed_clears(&fixture), 1);
    checked_io(fs::remove_file(index_folder(&fixture)));
    fixture.cleanup();
}
