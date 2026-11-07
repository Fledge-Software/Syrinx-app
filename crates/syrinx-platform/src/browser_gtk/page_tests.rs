//! Guards Find reports, native command ordering, media consent, GTK units and modal dialogs.

use super::*;

#[test]
fn find_events_number_matches_from_one_and_wrap_in_both_directions() {
    let (shared, mut events) = page_events();
    let find = FindState::default();
    assert!(find.begin(41, "synthetic", 7, &shared));
    find.count.set(3);
    find.report(7, &shared);
    expect_find(&mut events, 41, 3, 1);
    for (forward, expected) in [
        (true, 2),
        (true, 3),
        (true, 1),
        (false, 3),
        (false, 2),
        (false, 1),
    ] {
        assert!(find.step(forward));
        find.report(7, &shared);
        expect_find(&mut events, 41, 3, expected);
    }
    find.count.set(0);
    find.report(7, &shared);
    expect_find(&mut events, 41, 0, -1);
    assert!(!find.step(true));
    assert!(!find.step(false));
}

#[test]
fn an_empty_find_completes_its_request_and_retires_the_previous_search() {
    let (shared, mut events) = page_events();
    let find = FindState::default();
    assert!(find.begin(41, "synthetic", 7, &shared));
    find.count.set(3);
    assert!(!find.begin(42, "", 7, &shared));
    expect_find(&mut events, 42, 0, -1);
    assert!(!find.step(true));
    // A late native callback from the ended search cannot resurrect its request.
    find.count.set(3);
    find.report(7, &shared);
    assert!(matches!(
        events.try_recv(),
        Err(futures::channel::mpsc::TryRecvError::Empty)
    ));
}

/// Creates only the Rust event boundary, without a GTK thread or native page.
fn page_events() -> (
    Shared,
    futures::channel::mpsc::UnboundedReceiver<BrowserEvent>,
) {
    let (events, outgoing) = futures::channel::mpsc::unbounded();
    (
        Shared {
            parent: 0,
            events: super::super::EventSink(events, None),
            popups: Rc::default(),
            peek_generation: std::sync::Arc::default(),
            requests: Rc::default(),
        },
        outgoing,
    )
}

#[test]
fn a_rejected_tls_report_uses_the_failed_address_after_an_http_document() {
    let (shared, mut events) = page_events();
    let state = Rc::new(LoadState::default());
    let watch = LoadWatch {
        id: 7,
        shared,
        state: state.clone(),
        text: None,
    };
    // The previous document can remain active when the provisional HTTPS load fails.
    watch.connection_for_uri("http://127.0.0.1:1234/previous", ConnectionStatus::Loaded);
    let previous = events.try_recv().expect("previous document connection");
    assert!(matches!(previous, BrowserEvent::Connection { id: 7, state }
        if state.scheme == ConnectionScheme::Http && state.status == ConnectionStatus::Loaded));

    watch.certificate_rejected("https://127.0.0.1:4321/rejected");
    let rejected = events.try_recv().expect("TLS failure connection");
    assert!(matches!(rejected, BrowserEvent::Connection { id: 7, state }
        if state.scheme == ConnectionScheme::Https && state.status == ConnectionStatus::CertificateRejected));
    assert!(state.rejected.get());
    assert_eq!(state.failed.get(), Some(false));
    assert!(matches!(
        events.try_recv(),
        Err(futures::channel::mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn a_tls_callback_does_not_invent_https_for_a_missing_or_different_failing_uri() {
    for (uri, expected) in [
        ("", ConnectionScheme::Other),
        ("http://127.0.0.1:1234/synthetic", ConnectionScheme::Http),
    ] {
        let (shared, mut events) = page_events();
        let watch = LoadWatch {
            id: 7,
            shared,
            state: Rc::default(),
            text: None,
        };
        watch.certificate_rejected(uri);
        let rejected = events.try_recv().expect("TLS failure connection");
        assert!(matches!(rejected, BrowserEvent::Connection { id: 7, state }
            if state.scheme == expected && state.status == ConnectionStatus::CertificateRejected));
    }
}

fn expect_find(
    events: &mut futures::channel::mpsc::UnboundedReceiver<BrowserEvent>,
    request: u64,
    count: i32,
    active: i32,
) {
    let event = events.try_recv().expect("Find must emit a result");
    let BrowserEvent::FindResult {
        id,
        request: actual_request,
        count: actual_count,
        active: actual_active,
    } = event
    else {
        panic!("expected FindResult, got {event:?}");
    };
    assert_eq!(
        (id, actual_request, actual_count, actual_active),
        (7, request, count, active)
    );
}

#[test]
fn combined_media_cannot_use_a_camera_only_choice() {
    assert_eq!(media_permission_kind(true, true), None);
    assert_eq!(
        media_permission_kind(true, false),
        Some(PermissionKind::Camera)
    );
    assert_eq!(
        media_permission_kind(false, true),
        Some(PermissionKind::Microphone)
    );
    assert_eq!(media_permission_kind(false, false), None);
}

#[test]
fn physical_page_bounds_become_gtk_logical_allocations() {
    assert_eq!(logical_extent(1236, 2), 618);
    assert_eq!(logical_extent(1916, 2), 958);
    assert_eq!(logical_extent(619, 2), 310);
    assert_eq!(logical_extent(619, 1), 619);
    assert_eq!(logical_extent(0, 2), 1);
    assert_eq!(logical_extent(u32::MAX, 1), i32::MAX);
}

#[test]
fn a_modal_dialog_starts_after_the_host_borrow_and_skips_closed_pages() {
    let outcome = crate::gtk_thread::run(|| {
        let called = Rc::new(Cell::new(false));
        let host_available = Rc::new(Cell::new(false));
        let life = CallbackLife::new();
        let observed = called.clone();
        let available = host_available.clone();
        // A print command arrives with HOST borrowed; its nested loop must run later.
        super::super::HOST.with(|host| {
            let _borrow = host.borrow_mut();
            run_page_dialog(life.ticket(true), move || {
                observed.set(true);
                let free = super::super::HOST.with(|host| host.try_borrow_mut().is_ok());
                available.set(free);
                if free {
                    super::super::pump();
                }
            });
        });
        let before_dispatch = called.get();
        let closed = CallbackLife::new();
        let late = Rc::new(Cell::new(false));
        let observed = late.clone();
        run_page_dialog(closed.ticket(true), move || observed.set(true));
        closed.retire();
        // A lower-priority sentinel finishes after the queued dialog callbacks.
        let loop_ = glib::MainLoop::new(None, false);
        let finish = loop_.clone();
        glib::idle_add_local_full(glib::Priority::LOW, move || {
            finish.quit();
            glib::ControlFlow::Break
        });
        loop_.run();
        (
            before_dispatch,
            called.get(),
            host_available.get(),
            late.get(),
        )
    })
    .expect("GTK thread");
    assert_eq!(outcome, (false, true, true, false));
}

#[test]
fn a_forward_request_cannot_land_twice_and_erase_rows_before_replace_state() {
    use syrinx_core::NavigationList;

    for previous_finished in [false, true] {
        let state = LoadState::default();
        let mut list = NavigationList::default();
        // Commit 40 through 44, then jump back to 41 as the native fixture does.
        for page in 40..=44 {
            observe_history(
                &mut list,
                HistoryChange::NewDocument {
                    user_initiated: true,
                },
                page,
                page > 40,
                false,
            );
        }
        assert!(list.start_jump(HistoryDirection::Back, 3));
        state.delivery.borrow_mut().boundary();
        observe_history(&mut list, HistoryChange::BackOrForward, 41, true, true);
        if previous_finished {
            assert!(!state.delivery.borrow_mut().finished(true));
        }

        // WebKit exposes Forward's URI before policy/start, or while 41 finishes late.
        assert!(list.start_jump(HistoryDirection::Forward, 2));
        state.request(|| {
            let immediate = state.delivery.borrow_mut().uri_changed(!previous_finished);
            let deferred = state.delivery.borrow_mut().finished(true);
            if immediate || deferred {
                observe_history(
                    &mut list,
                    HistoryChange::SamePage { navigating: false },
                    43,
                    true,
                    true,
                );
            }
        });
        state.delivery.borrow_mut().boundary();
        observe_history(&mut list, HistoryChange::BackOrForward, 43, true, true);
        assert!(!state.delivery.borrow_mut().finished(true));

        // Commit 45, then deliver its parsing-time replaceState at finish.
        state.request(|| state.delivery.borrow_mut().boundary());
        observe_history(
            &mut list,
            HistoryChange::NewDocument {
                user_initiated: true,
            },
            45,
            true,
            false,
        );
        assert!(!state.delivery.borrow_mut().uri_changed(true));
        assert!(state.delivery.borrow_mut().finished(true));
        list.observe(&HistoryReport {
            change: HistoryChange::SamePage { navigating: false },
            address: "http://example.test/p/45r".into(),
            can_go_back: true,
            can_go_forward: false,
        });
        assert_eq!(list.current_address(), Some("http://example.test/p/45r"));
        assert_eq!(
            list.rows(HistoryDirection::Back, 15)
                .iter()
                .map(|row| row.address)
                .collect::<Vec<_>>(),
            [
                "http://example.test/p/43",
                "http://example.test/p/42",
                "http://example.test/p/41",
                "http://example.test/p/40"
            ],
            "previous document finished before Forward request: {previous_finished}"
        );
        assert!(list.rows(HistoryDirection::Forward, 15).is_empty());
    }
}

/// Feeds synthetic document addresses through the real shared list.
fn observe_history(
    list: &mut syrinx_core::NavigationList,
    change: HistoryChange,
    page: u32,
    can_go_back: bool,
    can_go_forward: bool,
) {
    list.observe(&HistoryReport {
        change,
        address: format!("http://example.test/p/{page}"),
        can_go_back,
        can_go_forward,
    });
}
