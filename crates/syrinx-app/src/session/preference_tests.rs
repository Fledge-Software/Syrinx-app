//! Proves that appearance settings and tile edits survive a real worker restart.

use syrinx_core::{BuiltInPlugin, Destination, DestinationTarget};

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
            SessionEvent::Preferences { site_zooms, .. } => return site_zooms,
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
