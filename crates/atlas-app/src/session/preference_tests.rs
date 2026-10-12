//! Proves that appearance settings and tile edits survive a real worker restart.

use atlas_core::{BuiltInPlugin, Destination, DestinationTarget};

use super::*;

fn preferences(
    events: &mut UnboundedReceiver<SessionEvent>,
) -> (BTreeMap<String, String>, QuickDestinations) {
    match next_any_event(events) {
        SessionEvent::Preferences {
            settings,
            destinations,
        } => (settings, destinations),
        event => panic!("expected synthetic preferences, got {event:?}"),
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
            DestinationEdit::Move { from: 1, to: 0 },
        ),
    );
    let latest = edited(&mut events);
    send(
        &handle,
        SessionCommand::SaveSettings(vec![("appearance.palette".to_owned(), "haze".to_owned())]),
    );
    stop(handle, &mut events);

    let (handle, mut events) = fixture.start();
    let (settings, destinations) = preferences(&mut events);
    assert_eq!(
        settings.get("appearance.palette").map(String::as_str),
        Some("haze")
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
            DestinationEdit::Remove { index: 0 },
        ),
    );
    assert!(matches!(
        next_any_event(&mut events),
        SessionEvent::Failure {
            error: SessionError::Destination(atlas_core::DestinationError::UnknownPosition),
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
