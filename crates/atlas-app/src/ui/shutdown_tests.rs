//! Checks that shutdown stops user intent while preserving final worker messages.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use atlas_core::{Activation, TabId, TabKind, TabMetadata, TabModel, TabModelSnapshot};
use atlas_platform::browser::{BrowserCommand, PageBounds};
use atlas_search::Scope;

use super::{HomePaletteIntent, page_command_allowed, queue_session_command};
use crate::session::{HomeEntryAction, SessionCommand};

fn home_snapshot() -> (TabModelSnapshot, TabId) {
    let mut model = TabModel::new();
    let id = model
        .open(
            TabKind::Web,
            TabMetadata {
                address: "about:blank".into(),
                ..TabMetadata::default()
            },
            Activation::Foreground,
        )
        .expect("create synthetic Home");
    (model.snapshot(), id)
}

fn new_home() -> SessionCommand {
    SessionCommand::NewTab {
        address: "about:blank".into(),
    }
}

fn query() -> SessionCommand {
    SessionCommand::Query {
        request_id: 17,
        text: "synthetic".into(),
        scope: Scope::Everything,
        policy: super::search_policy("synthetic"),
    }
}

fn queue_home(
    home: &mut HomePaletteIntent,
    sender: &Sender<SessionCommand>,
    receiver: &Receiver<SessionCommand>,
) -> u64 {
    assert!(queue_session_command(false, true, home, sender, new_home()).expect("queue Home"));
    match receiver.try_recv().expect("receive Home intent") {
        SessionCommand::HomeEntry {
            request_id,
            action: HomeEntryAction::NewTab,
        } => request_id,
        _ => panic!("new Home must retain its acknowledgment identity"),
    }
}

#[test]
fn closing_rejects_late_user_actions_but_keeps_final_metadata_before_shutdown() {
    let (sender, receiver) = mpsc::channel();
    let mut home = HomePaletteIntent::default();
    let (model, id) = home_snapshot();
    let request = queue_home(&mut home, &sender, &receiver);

    // None of these late inputs may reach the worker or replace its pending Home intent.
    for command in [
        new_home(),
        SessionCommand::CloseActive,
        SessionCommand::CloseTab(id),
        SessionCommand::FocusTab(id),
        SessionCommand::CycleActive { backwards: false },
        SessionCommand::NavigateActive {
            address: "https://late.example.test/".into(),
        },
        query(),
        SessionCommand::ConfirmRecovery { offer_id: 23 },
        SessionCommand::HomeEntry {
            request_id: 99,
            action: HomeEntryAction::NewTab,
        },
    ] {
        assert!(
            !queue_session_command(true, true, &mut home, &sender, command)
                .expect("reject late input")
        );
    }
    assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));

    for command in [
        SessionCommand::ObserveMetadata {
            id,
            title: Some("Synthetic final title".into()),
            address: None,
        },
        SessionCommand::ObserveMetadata {
            id,
            title: None,
            address: Some("https://final.example.test/".into()),
        },
        SessionCommand::Shutdown,
    ] {
        assert!(
            queue_session_command(true, true, &mut home, &sender, command)
                .expect("queue final worker message")
        );
    }
    assert!(matches!(
        receiver.try_recv().expect("final title"),
        SessionCommand::ObserveMetadata { id: received, title: Some(title), address: None }
            if received == id && title == "Synthetic final title"
    ));
    assert!(matches!(
        receiver.try_recv().expect("final address"),
        SessionCommand::ObserveMetadata { id: received, title: None, address: Some(address) }
            if received == id && address == "https://final.example.test/"
    ));
    assert!(matches!(
        receiver.try_recv().expect("ordered shutdown"),
        SessionCommand::Shutdown
    ));
    assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    assert!(!home.acknowledge(&model, &[id], Some(request)));
}

#[test]
fn rejected_close_preserves_home_identity_and_open_actions_still_supersede_it() {
    let (sender, receiver) = mpsc::channel();
    let mut home = HomePaletteIntent::default();
    let (model, id) = home_snapshot();
    let request = queue_home(&mut home, &sender, &receiver);
    assert!(
        !queue_session_command(true, true, &mut home, &sender, SessionCommand::CloseActive)
            .expect("reject closing input")
    );
    assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    assert!(home.acknowledge(&model, &[id], Some(request)));

    let next = queue_home(&mut home, &sender, &receiver);
    assert!(next > request);
    assert!(queue_session_command(false, true, &mut home, &sender, query()).expect("query"));
    assert!(matches!(
        receiver.try_recv().expect("local query"),
        SessionCommand::Query { request_id: 17, .. }
    ));
    assert!(home.acknowledge(&model, &[id], Some(next)));

    let superseded = queue_home(&mut home, &sender, &receiver);
    assert!(
        queue_session_command(false, true, &mut home, &sender, SessionCommand::CloseActive)
            .expect("ordinary close")
    );
    assert!(matches!(
        receiver.try_recv().expect("ordinary close reaches worker"),
        SessionCommand::CloseActive
    ));
    assert!(!home.acknowledge(&model, &[id], Some(superseded)));
}

#[test]
fn startup_admits_confirmation_but_closing_startup_admits_only_shutdown() {
    let (sender, receiver) = mpsc::channel();
    let mut home = HomePaletteIntent::default();
    let (_, id) = home_snapshot();
    for command in [new_home(), query(), SessionCommand::CloseActive] {
        assert!(
            !queue_session_command(false, false, &mut home, &sender, command)
                .expect("reject pre-startup input")
        );
    }
    for closing in [false, true] {
        assert!(
            !queue_session_command(
                closing,
                false,
                &mut home,
                &sender,
                SessionCommand::ObserveMetadata {
                    id,
                    title: Some("Synthetic premature title".into()),
                    address: None,
                },
            )
            .expect("startup cannot accept metadata before restoring its model")
        );
    }
    assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    assert!(
        queue_session_command(
            false,
            false,
            &mut home,
            &sender,
            SessionCommand::ConfirmRecovery { offer_id: 23 },
        )
        .expect("explicit startup confirmation")
    );
    assert!(matches!(
        receiver.try_recv().expect("confirmation identity"),
        SessionCommand::ConfirmRecovery { offer_id: 23 }
    ));
    assert!(
        !queue_session_command(
            true,
            false,
            &mut home,
            &sender,
            SessionCommand::ConfirmRecovery { offer_id: 23 },
        )
        .expect("cancelled startup cannot restore")
    );
    assert!(
        queue_session_command(true, false, &mut home, &sender, SessionCommand::Shutdown)
            .expect("cancel startup")
    );
    assert!(matches!(
        receiver.try_recv().expect("startup cancellation"),
        SessionCommand::Shutdown
    ));
    assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
}

#[test]
fn disconnected_worker_errors_are_preserved_without_cancelling_accepted_home() {
    let (sender, receiver) = mpsc::channel();
    let mut home = HomePaletteIntent::default();
    let (model, id) = home_snapshot();
    let request = queue_home(&mut home, &sender, &receiver);
    drop(receiver);

    let error = queue_session_command(false, true, &mut home, &sender, SessionCommand::CloseActive)
        .expect_err("accepted action must report a disconnected worker");
    assert!(matches!(error.0, SessionCommand::CloseActive));
    let error = queue_session_command(true, true, &mut home, &sender, SessionCommand::Shutdown)
        .expect_err("shutdown must report a disconnected worker");
    assert!(matches!(error.0, SessionCommand::Shutdown));
    assert!(home.acknowledge(&model, &[id], Some(request)));
    assert!(
        !queue_session_command(true, true, &mut home, &sender, new_home())
            .expect("rejected input must not attempt a send")
    );
}

#[test]
fn closing_page_host_accepts_teardown_but_no_late_focus_or_navigation() {
    // Existing page operations stop at the boundary; host shutdown remains available.
    let commands = [
        BrowserCommand::Create {
            id: 1,
            url: "https://created.example.test/".into(),
        },
        BrowserCommand::Navigate {
            id: 1,
            url: "https://navigated.example.test/".into(),
        },
        BrowserCommand::Close { id: 1 },
        BrowserCommand::ShowActive(Some(1)),
        BrowserCommand::ShowActive(None),
        BrowserCommand::Resize(PageBounds::default()),
        BrowserCommand::Back { id: 1 },
        BrowserCommand::Forward { id: 1 },
        BrowserCommand::Reload { id: 1 },
        BrowserCommand::Focus { id: 1 },
    ];
    for command in commands {
        assert!(page_command_allowed(false, &command));
        assert!(!page_command_allowed(true, &command));
    }
    assert!(page_command_allowed(false, &BrowserCommand::Shutdown));
    assert!(page_command_allowed(true, &BrowserCommand::Shutdown));
}
