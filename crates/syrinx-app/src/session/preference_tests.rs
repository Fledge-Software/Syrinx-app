//! Proves that appearance settings, tile edits and site zoom levels survive a real worker
//! restart, and that a zoom clear goes through D47's journal (#121).

use syrinx_core::{BuiltInPlugin, Destination, DestinationTarget};
use syrinx_store::deletion::{DeletionCategory, DeletionRange};

use super::*;

fn preferences(
    events: &mut UnboundedReceiver<SessionEvent>,
) -> (BTreeMap<String, String>, QuickDestinations) {
    // The usage status (AN-01) is reported first at start-up and has its own tests.
    loop {
        match next_any_event(events) {
            SessionEvent::Preferences {
                settings,
                destinations,
                ..
            } => return (settings, destinations),
            SessionEvent::Usage { .. } => continue,
            event => panic!("expected synthetic preferences, got {event:?}"),
        }
    }
}

fn edited(events: &mut UnboundedReceiver<SessionEvent>) -> QuickDestinations {
    match next_any_event(events) {
        SessionEvent::Destinations(destinations) => destinations,
        event => panic!("expected synthetic tile lists, got {event:?}"),
    }
}

fn tile(label: &str) -> Destination {
    Destination::new(
        label,
        DestinationTarget::Web("https://tile.example.test/".to_owned()),
    )
    .expect("synthetic tile")
}

fn edit(surface: DestinationSurface, edit: DestinationEdit) -> SessionCommand {
    SessionCommand::EditDestinations { surface, edit }
}

#[test]
fn preferences_arrive_before_state_and_survive_restart() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    let (settings, destinations) = preferences(&mut events);
    assert!(settings.is_empty());
    assert_eq!(destinations, QuickDestinations::default());
    assert_eq!(state(&mut events).revision, 0);

    send(
        &handle,
        edit(
            DestinationSurface::QuickAccess,
            DestinationEdit::Add(tile("Synthetic alpha")),
        ),
    );
    edited(&mut events);
    send(
        &handle,
        edit(
            DestinationSurface::QuickAccess,
            DestinationEdit::Add(tile("Synthetic bravo")),
        ),
    );
    edited(&mut events);
    send(
        &handle,
        edit(
            DestinationSurface::Favorites,
            DestinationEdit::Add(
                Destination::new(
                    "Synthetic plugin",
                    DestinationTarget::Plugin(BuiltInPlugin::Fledge),
                )
                .expect("plugin tile"),
            ),
        ),
    );
    edited(&mut events);
    send(
        &handle,
        edit(
            DestinationSurface::QuickAccess,
            DestinationEdit::Move {
                from: 1,
                to: 0,
                expected: tile("Synthetic bravo"),
            },
        ),
    );
    let latest = edited(&mut events);
    send(
        &handle,
        SessionCommand::SaveSettings(vec![("appearance.palette".to_owned(), "haze".to_owned())]),
    );
    // The sidebar presentation (DW-2) uses the same table through its documented key.
    send(
        &handle,
        SessionCommand::SaveSettings(vec![syrinx_ui::sidebar_setting(
            syrinx_ui::SidebarWidthState::Compact,
        )]),
    );
    stop(handle, &mut events);

    let (handle, mut events) = fixture.start();
    let (settings, destinations) = preferences(&mut events);
    assert_eq!(
        settings.get("appearance.palette").map(String::as_str),
        Some("haze")
    );
    assert_eq!(
        settings
            .get(syrinx_ui::SIDEBAR_WIDTH_STATE_KEY)
            .and_then(|value| syrinx_ui::parse_sidebar_setting(value)),
        Some(syrinx_ui::SidebarWidthState::Compact)
    );
    assert_eq!(destinations, latest);
    let labels: Vec<_> = destinations
        .list(DestinationSurface::QuickAccess)
        .iter()
        .map(|tile| tile.label.as_str())
        .collect();
    assert_eq!(labels, ["Synthetic bravo", "Synthetic alpha"]);
    state(&mut events);
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn refused_tile_edits_report_without_stopping_the_worker() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    preferences(&mut events);
    state(&mut events);
    send(
        &handle,
        edit(
            DestinationSurface::Favorites,
            DestinationEdit::Remove {
                index: 0,
                expected: tile("Synthetic absent"),
            },
        ),
    );
    assert!(matches!(
        next_any_event(&mut events),
        SessionEvent::Failure {
            error: SessionError::Destination(syrinx_core::DestinationError::UnknownPosition),
            fatal: false,
        }
    ));
    // The worker still accepts tab commands after the refused edit.
    let opened = change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: "about:blank".to_owned(),
        },
    );
    assert_eq!(opened.model.tabs.len(), 1);
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn a_double_clicked_remove_queued_twice_removes_one_tile() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    preferences(&mut events);
    state(&mut events);
    for label in ["Synthetic one", "Synthetic two"] {
        send(
            &handle,
            edit(
                DestinationSurface::QuickAccess,
                DestinationEdit::Add(tile(label)),
            ),
        );
        edited(&mut events);
    }
    // Both clicks were made against "Synthetic one" before the first was acknowledged.
    let remove = DestinationEdit::Remove {
        index: 0,
        expected: tile("Synthetic one"),
    };
    send(
        &handle,
        edit(DestinationSurface::QuickAccess, remove.clone()),
    );
    send(&handle, edit(DestinationSurface::QuickAccess, remove));
    let after_first = edited(&mut events);
    assert!(matches!(
        next_any_event(&mut events),
        SessionEvent::Failure {
            error: SessionError::Destination(syrinx_core::DestinationError::Changed),
            fatal: false,
        }
    ));
    let labels: Vec<_> = after_first
        .list(DestinationSurface::QuickAccess)
        .iter()
        .map(|tile| tile.label.as_str())
        .collect();
    assert_eq!(labels, ["Synthetic two"]);
    stop(handle, &mut events);
    fixture.cleanup();
}

/// The site zoom levels the worker hands the window at start-up (#90).
fn site_zooms(events: &mut UnboundedReceiver<SessionEvent>) -> BTreeMap<String, f64> {
    loop {
        match next_any_event(events) {
            SessionEvent::Preferences { site_zooms, .. } => return site_zooms.0,
            SessionEvent::Usage { .. } => continue,
            event => panic!("expected synthetic preferences, got {event:?}"),
        }
    }
}

fn zoom(host: &str, factor: Option<f64>) -> SessionCommand {
    SessionCommand::SiteZoom {
        host: host.to_owned(),
        factor,
    }
}

#[test]
fn site_zoom_levels_survive_restart_and_a_reset_forgets_one() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    assert!(site_zooms(&mut events).is_empty());
    state(&mut events);
    send(&handle, zoom("kept.example.test", Some(1.25)));
    send(&handle, zoom("reset.example.test", Some(0.75)));
    send(&handle, zoom("reset.example.test", None));
    stop(handle, &mut events);

    let (handle, mut events) = fixture.start();
    let restored = site_zooms(&mut events);
    assert_eq!(
        restored.into_iter().collect::<Vec<_>>(),
        [("kept.example.test".to_owned(), 1.25)]
    );
    state(&mut events);
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn a_refused_site_zoom_reports_without_stopping_the_worker() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    site_zooms(&mut events);
    state(&mut events);
    send(&handle, zoom("https://address.example.test/", Some(1.25)));
    assert!(matches!(
        next_any_event(&mut events),
        SessionEvent::Failure {
            error: SessionError::Store(syrinx_store::StoreError::InvalidEncoding(
                syrinx_store::EncodingField::SiteZoom
            )),
            fatal: false,
        }
    ));
    let opened = change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: "about:blank".to_owned(),
        },
    );
    assert_eq!(opened.model.tabs.len(), 1);
    stop(handle, &mut events);
    fixture.cleanup();
}

#[test]
fn clearing_every_site_zoom_reports_the_count_and_what_remains() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    site_zooms(&mut events);
    state(&mut events);
    send(&handle, zoom("one.example.test", Some(1.25)));
    send(&handle, zoom("two.example.test", Some(0.9)));
    send(
        &handle,
        SessionCommand::ClearSiteZooms {
            request_id: 7,
            start: 0,
            end: i64::MAX,
        },
    );
    match next_any_event(&mut events) {
        SessionEvent::SiteZoomsCleared {
            request_id: 7,
            result: Ok(cleared),
        } => {
            assert_eq!(cleared.sites, 2);
            assert!(cleared.remaining.0.is_empty());
        }
        event => panic!("expected the synthetic zoom clear, got {event:?}"),
    }
    stop(handle, &mut events);
    // The clear is journaled, so a restored backup replays it (D47, #121).
    assert_eq!(
        completed_clears(&fixture),
        (1, vec![DeletionCategory::SiteZoom])
    );

    let (handle, mut events) = fixture.start();
    assert!(site_zooms(&mut events).is_empty(), "the clear is durable");
    state(&mut events);
    stop(handle, &mut events);
    fixture.cleanup();
}

/// The journal after the worker stopped: its completed generation and every clear's category.
/// Panics on a pending clear, since a stopped worker leaves none.
fn completed_clears(fixture: &Fixture) -> (u64, Vec<DeletionCategory>) {
    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data");
    let journal = DeletionJournal::open(&lease).expect("reopen synthetic journal");
    assert_eq!(journal.log().pending(), None);
    let categories = journal
        .log()
        .unapplied(0)
        .expect("every synthetic clear")
        .iter()
        .map(|request| request.category())
        .collect();
    (journal.log().completed_generation(), categories)
}

#[test]
fn a_zoom_clear_requested_before_a_crash_finishes_at_the_next_start() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    site_zooms(&mut events);
    state(&mut events);
    send(&handle, zoom("crashed.example.test", Some(1.25)));
    stop(handle, &mut events);

    // Step 1 alone is durable: the request is journaled, nothing is applied or completed.
    let now = utc_seconds().expect("synthetic clock");
    let lease = WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic data");
    let mut journal = DeletionJournal::open(&lease).expect("open synthetic journal");
    let range = DeletionRange::new(now - 3_600, now + 1).expect("valid synthetic range");
    let request = journal
        .log()
        .request(DeletionCategory::SiteZoom, range)
        .expect("prepare synthetic request");
    journal.append(request).expect("journal synthetic request");
    drop(journal);
    drop(lease);

    // The start-up finishes the clear before the window hears any level.
    let (handle, mut events) = fixture.start();
    assert!(site_zooms(&mut events).is_empty());
    state(&mut events);
    stop(handle, &mut events);
    assert_eq!(
        completed_clears(&fixture),
        (1, vec![DeletionCategory::SiteZoom])
    );
    fixture.cleanup();
}

/// A worker whose journal holds a zoom clear that stopped after its request, as a busy
/// database leaves one: `kept` changed long before the clear's range, `cleared` inside it.
fn worker_with_pending_zoom_clear(
    fixture: &Fixture,
) -> (
    Worker,
    Store,
    UnboundedSender<SessionEvent>,
    UnboundedReceiver<SessionEvent>,
) {
    let lease =
        Arc::new(WindowsDirectoryLease::prepare(&fixture.0).expect("guard synthetic store"));
    let journal = DeletionJournal::open(&lease).expect("open synthetic journal");
    let page_text = page_text::PageText::open(&lease);
    let store = Store::open(lease).expect("open synthetic store");
    let mut worker = Worker::restore(&store, journal, page_text).expect("restore synthetic worker");
    let now = utc_seconds().expect("synthetic clock");
    store
        .save_site_zoom("kept.example.test".to_owned(), Some(1.25), 100)
        .expect("save synthetic kept level");
    store
        .save_site_zoom("cleared.example.test".to_owned(), Some(0.9), now)
        .expect("save synthetic cleared level");
    let range = DeletionRange::new(now - 3_600, now + 1).expect("valid synthetic range");
    let request = worker
        .deletions
        .log()
        .request(DeletionCategory::SiteZoom, range)
        .expect("prepare synthetic request");
    worker
        .deletions
        .append(request)
        .expect("journal synthetic request");
    let (sender, events) = unbounded();
    (worker, store, sender, events)
}

/// The levels the window was handed, among the events already sent.
fn replaced_levels(events: &mut UnboundedReceiver<SessionEvent>) -> Option<BTreeMap<String, f64>> {
    while let Ok(event) = events.try_recv() {
        if let SessionEvent::SiteZoomsReplaced(levels) = event {
            return Some(levels.0);
        }
    }
    None
}

fn kept_level() -> Option<BTreeMap<String, f64>> {
    Some(BTreeMap::from([("kept.example.test".to_owned(), 1.25)]))
}

#[test]
fn a_pending_zoom_clear_finished_while_idle_replaces_the_windows_levels() {
    let fixture = Fixture::new();
    let (mut worker, store, sender, mut events) = worker_with_pending_zoom_clear(&fixture);
    // The same branch the idle timer runs, without waiting for it.
    worker
        .idle(&store, &sender)
        .expect("finish the synthetic clear while idle");
    assert_eq!(replaced_levels(&mut events), kept_level());
    assert_eq!(worker.deletions.log().pending(), None);
    drop(worker);
    store.shutdown().expect("close synthetic store");
    fixture.cleanup();
}

/// Whether the window was told a failure, which the worker reports without stopping.
fn heard_failure(events: &mut UnboundedReceiver<SessionEvent>) -> bool {
    let mut heard = false;
    while let Ok(event) = events.try_recv() {
        heard |= matches!(event, SessionEvent::Failure { fatal: false, .. });
    }
    heard
}

#[test]
fn a_levels_read_that_fails_after_the_apply_is_retried_on_a_later_idle_tick() {
    let fixture = Fixture::new();
    let (mut worker, store, sender, mut events) = worker_with_pending_zoom_clear(&fixture);
    worker.zoom_levels.refuse_reads(1);
    worker
        .idle(&store, &sender)
        .expect("the worker survives a refused read");
    // The clear is applied and complete, but the window has heard nothing yet.
    assert_eq!(worker.deletions.log().pending(), None);
    assert!(worker.zoom_levels.is_stale());
    assert!(heard_failure(&mut events));
    worker
        .idle(&store, &sender)
        .expect("resend the synthetic levels");
    assert_eq!(replaced_levels(&mut events), kept_level());
    assert!(!worker.zoom_levels.is_stale());
    drop(worker);
    store.shutdown().expect("close synthetic store");
    fixture.cleanup();
}

#[test]
fn a_levels_read_that_fails_after_a_requested_zoom_clear_is_retried_on_an_idle_tick() {
    let fixture = Fixture::new();
    let (mut worker, store, sender, mut events) = worker_with_pending_zoom_clear(&fixture);
    // Finish the fixture's pending clear first, so the requested clear is the only one left.
    worker
        .idle(&store, &sender)
        .expect("finish the synthetic clear");
    assert_eq!(replaced_levels(&mut events), kept_level());
    let now = utc_seconds().expect("synthetic clock");
    // A level inside the requested range, so stale levels would differ from the right ones.
    store
        .save_site_zoom("fresh.example.test".to_owned(), Some(1.5), now)
        .expect("save synthetic in-range level");
    worker.zoom_levels.refuse_reads(1);
    worker
        .handle(
            &store,
            SessionCommand::ClearSiteZooms {
                request_id: 21,
                start: now - 3_600,
                end: now + 1,
            },
            &sender,
        )
        .expect("the worker reports a failed read instead of stopping");
    assert!(worker.zoom_levels.is_stale());
    assert_eq!(worker.deletions.log().pending(), None);
    worker
        .idle(&store, &sender)
        .expect("resend the synthetic levels");
    // The resend holds the levels left after the clear: the in-range level is gone.
    assert_eq!(replaced_levels(&mut events), kept_level());
    assert!(!worker.zoom_levels.is_stale());
    drop(worker);
    store.shutdown().expect("close synthetic store");
    fixture.cleanup();
}

#[test]
fn a_resend_to_a_window_that_has_gone_is_retried_a_bounded_number_of_times() {
    let fixture = Fixture::new();
    let (mut worker, store, sender, events) = worker_with_pending_zoom_clear(&fixture);
    worker.zoom_levels.refuse_reads(1);
    worker
        .idle(&store, &sender)
        .expect("the worker survives a refused read");
    assert!(worker.zoom_levels.is_stale());
    // The window's event stream closes; the worker keeps serving, but cannot resend.
    drop(events);
    for _ in 1..zoom_levels::MAX_ATTEMPTS {
        worker
            .idle(&store, &sender)
            .expect("the worker survives a closed window");
    }
    assert!(!worker.zoom_levels.is_stale(), "the retry stopped");
    drop(worker);
    store.shutdown().expect("close synthetic store");
    fixture.cleanup();
}

#[test]
fn a_levels_read_that_keeps_failing_is_retried_a_bounded_number_of_times() {
    let fixture = Fixture::new();
    let (mut worker, store, sender, mut events) = worker_with_pending_zoom_clear(&fixture);
    worker.zoom_levels.refuse_reads(u32::MAX);
    // The first send fails inside the clear, and each idle tick after it is one retry.
    worker
        .idle(&store, &sender)
        .expect("the worker survives a refused read");
    for retry in 1..zoom_levels::MAX_ATTEMPTS {
        assert!(worker.zoom_levels.is_stale());
        if retry == zoom_levels::MAX_ATTEMPTS - 1 {
            // Drop the earlier failures, so only the give-up can satisfy the check below.
            heard_failure(&mut events);
        }
        worker
            .idle(&store, &sender)
            .expect("the worker survives a refused retry");
    }
    // The last retry gave up and told the window; later ticks read nothing more.
    assert!(!worker.zoom_levels.is_stale());
    assert!(heard_failure(&mut events), "the give-up reached the window");
    worker
        .idle(&store, &sender)
        .expect("an idle tick with nothing stale");
    assert_eq!(replaced_levels(&mut events), None);
    drop(worker);
    store.shutdown().expect("close synthetic store");
    fixture.cleanup();
}

#[test]
fn a_history_clear_that_finishes_a_pending_zoom_clear_replaces_the_windows_levels() {
    let fixture = Fixture::new();
    let (mut worker, store, sender, mut events) = worker_with_pending_zoom_clear(&fixture);
    worker
        .handle(
            &store,
            SessionCommand::ClearHistory {
                request_id: 9,
                start: 0,
                end: i64::MAX,
            },
            &sender,
        )
        .expect("clear synthetic history");
    // The window hears its levels before the dialog hears the history count.
    assert_eq!(replaced_levels(&mut events), kept_level());
    assert!(matches!(
        next_any_event(&mut events),
        SessionEvent::HistoryCleared {
            request_id: 9,
            result: Ok(_),
        }
    ));
    drop(worker);
    store.shutdown().expect("close synthetic store");
    fixture.cleanup();
}

#[test]
fn a_zoom_clear_finished_before_a_failing_history_clear_still_replaces_the_windows_levels() {
    let fixture = Fixture::new();
    let (mut worker, store, sender, mut events) = worker_with_pending_zoom_clear(&fixture);
    // A second link to the journal makes every later append refuse it (hard-linked file), so
    // the pending zoom clear is applied but its completion fails, and the history clear is
    // never journaled. This is the completion-failure variant of #159: a failure after an
    // earlier clear completes is not practical here: the link refuses that append too, and the
    // journal's size cap would need about 175,000 requests.
    fs::hard_link(
        fixture.0.join("syrinx.deletions"),
        fixture.1.join("journal-alias"),
    )
    .expect("link the synthetic journal");
    worker
        .handle(
            &store,
            SessionCommand::ClearHistory {
                request_id: 11,
                start: 0,
                end: i64::MAX,
            },
            &sender,
        )
        .expect("the worker reports a failed clear instead of stopping");
    // The window hears its levels, and the dialog hears the failure.
    assert_eq!(replaced_levels(&mut events), kept_level());
    assert!(matches!(
        next_any_event(&mut events),
        SessionEvent::HistoryCleared {
            request_id: 11,
            result: Err(_),
        }
    ));
    // The levels were sent once, and the refused completion left the request pending.
    assert_eq!(replaced_levels(&mut events), None);
    assert!(worker.deletions.log().pending().is_some());
    drop(worker);
    store.shutdown().expect("close synthetic store");
    fs::remove_file(fixture.1.join("journal-alias")).expect("remove the synthetic link");
    fixture.cleanup();
}

#[test]
fn a_levels_send_that_fails_does_not_cancel_the_history_clear_after_it() {
    let fixture = Fixture::new();
    let (mut worker, store, sender, events) = worker_with_pending_zoom_clear(&fixture);
    // With the window gone, telling it the levels fails; the owner's clear must still run.
    drop(events);
    let result = worker.handle(
        &store,
        SessionCommand::ClearHistory {
            request_id: 12,
            start: 0,
            end: i64::MAX,
        },
        &sender,
    );
    assert!(result.is_err(), "the closed window is still reported");
    // Both the interrupted zoom clear and the new history clear are journaled and complete.
    assert_eq!(worker.deletions.log().pending(), None);
    assert_eq!(worker.deletions.log().completed_generation(), 2);
    drop(worker);
    store.shutdown().expect("close synthetic store");
    fixture.cleanup();
}

#[test]
fn a_failed_levels_send_and_a_failed_completion_leave_the_request_pending() {
    let fixture = Fixture::new();
    let (mut worker, store, sender, events) = worker_with_pending_zoom_clear(&fixture);
    drop(events);
    fs::hard_link(
        fixture.0.join("syrinx.deletions"),
        fixture.1.join("journal-alias"),
    )
    .expect("link the synthetic journal");
    let result = worker.handle(
        &store,
        SessionCommand::ClearHistory {
            request_id: 13,
            start: 0,
            end: i64::MAX,
        },
        &sender,
    );
    // Neither failure completes the request, and the worker keeps running.
    assert!(result.is_err());
    assert!(worker.deletions.log().pending().is_some());
    drop(worker);
    store.shutdown().expect("close synthetic store");
    fs::remove_file(fixture.1.join("journal-alias")).expect("remove the synthetic link");
    fixture.cleanup();
}
