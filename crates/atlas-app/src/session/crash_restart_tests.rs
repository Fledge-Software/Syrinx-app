//! Proves session restore after the process ends without any shutdown, on disposable storage.

use std::{
    env,
    process::{Child, Command, ExitStatus, Stdio},
};

use super::*;

const CHILD_ROOT: &str = "ATLAS_SESSION_CRASH_RESTART_CHILD_ROOT";
const CHILD_TEST: &str = "session::worker_tests::crash_restart::unclean_exit_restores_committed_order_associations_and_unloaded_rows";
const CHILD_EXIT: i32 = 86;
const SAVED_A: &str = "https://crash-saved-a.example.test/";
const SAVED_B: &str = "https://crash-saved-b.example.test/";
const LIVE_B: &str = "https://crash-saved-b.example.test/later";
const ORDINARY: &str = "https://crash-ordinary.example.test/";
const UNCOMMITTED: &str = "https://crash-uncommitted.example.test/";

fn tab(value: u64) -> TabId {
    TabId::new(value).expect("synthetic tab identity is nonzero")
}

/// Two saved pages (the second left unloaded), an ordinary tab and a non-final selection.
fn committed_script() -> Vec<SessionCommand> {
    vec![
        SessionCommand::NewTab {
            address: SAVED_A.to_owned(),
        },
        SessionCommand::SaveActive,
        SessionCommand::NewTab {
            address: SAVED_B.to_owned(),
        },
        SessionCommand::SaveActive,
        SessionCommand::ObserveMetadata {
            id: tab(2),
            title: Some("Synthetic crash page after navigation".to_owned()),
            address: Some(LIVE_B.to_owned()),
        },
        SessionCommand::CloseTab(tab(2)),
        SessionCommand::NewTab {
            address: ORDINARY.to_owned(),
        },
        SessionCommand::ObserveMetadata {
            id: tab(3),
            title: Some("Synthetic ordinary crash page".to_owned()),
            address: None,
        },
        SessionCommand::FocusTab(tab(1)),
    ]
}

fn final_command() -> SessionCommand {
    SessionCommand::NewTab {
        address: UNCOMMITTED.to_owned(),
    }
}

/// Replays commands through the worker's own model rules and counts committed revisions.
fn expected_state(commands: Vec<SessionCommand>) -> (TabModelSnapshot, u64) {
    let mut model = TabModel::new();
    let mut revision = 0;
    for command in commands {
        let before = model.snapshot();
        apply_model(&mut model, command).expect("synthetic script command applies");
        if model.snapshot() != before {
            revision += 1;
        }
    }
    (model.snapshot(), revision)
}

/// Runs in the subprocess: commit the script, queue one more change, then end without shutdown.
fn run_child(root: PathBuf) -> ! {
    let fixture = Fixture(root.join("State"), root);
    let (handle, mut events) = fixture.start();
    state(&mut events);
    for command in committed_script() {
        change(&handle, &mut events, command);
    }
    send(&handle, final_command());
    // Process exit skips every destructor: no store close, WAL checkpoint or clean marker.
    std::process::exit(CHILD_EXIT);
}

#[test]
fn unclean_exit_restores_committed_order_associations_and_unloaded_rows() {
    if let Some(root) = env::var_os(CHILD_ROOT) {
        run_child(PathBuf::from(root));
    }
    let fixture = Fixture::new();
    let status = run_crashing_child(&fixture.1);
    assert_eq!(status.code(), Some(CHILD_EXIT));

    let (handle, mut events) = fixture.start();
    let restored = state(&mut events);
    assert_eq!(restored.previous_shutdown, ShutdownState::Unclean);
    assert!(restored.created.is_empty());
    assert_eq!(restored.navigate, None);

    // Every acknowledged change survives; the unacknowledged one is either whole or absent.
    let committed = expected_state(committed_script());
    let mut with_final = committed_script();
    with_final.push(final_command());
    let completed = expected_state(with_final);
    let actual = (restored.model.clone(), restored.revision);
    assert!(
        actual == committed || actual == completed,
        "restored state is neither the acknowledged nor the next complete revision"
    );
    assert_restored_layout(&restored.model);

    // The acknowledged visit (P1-20) survives too, with the title it was reported with.
    send(
        &handle,
        SessionCommand::RecentHistory {
            request_id: 5,
            limit: 10,
        },
    );
    let SessionEvent::History { request_id, result } = next_event(&mut events) else {
        panic!("expected synthetic history after the unclean exit");
    };
    assert_eq!(request_id, 5);
    let visits = result.expect("history reads after the unclean exit");
    assert_eq!(visits.len(), 1);
    assert_eq!(visits[0].address, LIVE_B);
    assert_eq!(visits[0].title, "Synthetic crash page after navigation");

    // Closed-tab history is in-session only (D42); the unloaded saved row opens its fixed target.
    let unchanged = change(&handle, &mut events, SessionCommand::ReopenClosed);
    assert_eq!(unchanged.revision, restored.revision);
    let saved_b = restored.model.saved_pages[1].id;
    let opened = change(&handle, &mut events, SessionCommand::OpenSaved(saved_b));
    let reopened = opened.model.active_id.expect("saved row opens a live page");
    assert!(reopened.get() > restored.model.last_tab_id);
    assert_eq!(opened.created, vec![reopened]);
    let live = opened
        .model
        .tabs
        .iter()
        .find(|tab| tab.id == reopened)
        .expect("opened saved page is live");
    assert_eq!(live.metadata.address, SAVED_B);
    assert_eq!(live.saved_page_id, Some(saved_b));
    stop(handle, &mut events);

    // A clean stop after recovery marks the next start clean and keeps the new state.
    let (handle, mut events) = fixture.start();
    let clean = state(&mut events);
    assert_eq!(clean.previous_shutdown, ShutdownState::Clean);
    assert_eq!(clean.model, opened.model);
    stop(handle, &mut events);
    fixture.cleanup();
}

fn assert_restored_layout(model: &TabModelSnapshot) {
    // Saved rows keep their order and fixed targets; only the first remains loaded.
    let saved: Vec<_> = model
        .saved_pages
        .iter()
        .map(|page| page.metadata.address.as_str())
        .collect();
    assert_eq!(saved, [SAVED_A, SAVED_B]);
    let first_saved = model.saved_pages[0].id;
    let second_saved = model.saved_pages[1].id;
    assert!(
        model
            .tabs
            .iter()
            .all(|tab| tab.saved_page_id != Some(second_saved))
    );
    assert_eq!(model.tabs[0].id, tab(1));
    assert_eq!(model.tabs[0].saved_page_id, Some(first_saved));
    assert_eq!(model.tabs[1].id, tab(3));
    assert_eq!(model.tabs[1].saved_page_id, None);
    assert_eq!(
        model.tabs[1].metadata.title,
        "Synthetic ordinary crash page"
    );
    assert_eq!(model.tabs[1].metadata.address, ORDINARY);
    // The interrupted new tab, when it committed, is appended and active.
    match model.tabs.get(2) {
        Some(last) => {
            assert_eq!(last.metadata.address, UNCOMMITTED);
            assert_eq!(model.active_id, Some(last.id));
        }
        None => assert_eq!(model.active_id, Some(tab(1))),
    }
}

fn run_crashing_child(root: &Path) -> ExitStatus {
    let mut child = Command::new(env::current_exe().expect("locate session test executable"))
        .args(["--exact", CHILD_TEST, "--nocapture", "--test-threads=1"])
        .env(CHILD_ROOT, root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start synthetic crashing session");
    wait_for_child(&mut child)
}

fn wait_for_child(child: &mut Child) -> ExitStatus {
    // Bound the subprocess; terminate only this test's own child if it overruns.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = child.try_wait().expect("poll synthetic crashing session") {
            return status;
        }
        if Instant::now() >= deadline {
            child
                .kill()
                .expect("terminate overrunning synthetic session");
            child.wait().expect("reap overrunning synthetic session");
            panic!("synthetic crashing session exceeded its deadline");
        }
        thread::sleep(Duration::from_millis(20));
    }
}
