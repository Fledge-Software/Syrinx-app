//! Injects failures into real synthetic controllers without terminating any process.

use super::*;
use std::time::{SystemTime, UNIX_EPOCH};
use windows::{
    Win32::{
        System::Threading::GetCurrentThreadId,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
        },
    },
    core::{Interface, w},
};

struct Fixture {
    state: Option<BrowserState>,
    parent: HWND,
    events: UnboundedReceiver<BrowserEvent>,
    native: Receiver<BrowserEvent>,
    _apartment: environment::Apartment,
    _deadline: Deadline,
}

struct Deadline {
    cancel: Sender<()>,
    worker: Option<JoinHandle<()>>,
}
impl Deadline {
    fn new() -> Self {
        let (cancel, receive) = mpsc::channel();
        let worker = thread::spawn(move || {
            if receive
                .recv_timeout(std::time::Duration::from_secs(60))
                .is_err()
            {
                eprintln!("Synthetic native recovery fixture exceeded its independent deadline.");
                std::process::exit(1);
            }
        });
        Self {
            cancel,
            worker: Some(worker),
        }
    }
}
impl Drop for Deadline {
    fn drop(&mut self) {
        if self.cancel.send(()).is_err() {
            eprintln!("Synthetic native recovery deadline could not be cancelled.");
        }
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            eprintln!("Synthetic recovery deadline worker panicked.");
        }
    }
}

impl Fixture {
    fn new() -> Self {
        let deadline = Deadline::new();
        let apartment = environment::Apartment::new().expect("synthetic COM apartment");
        let mut message = MSG::default();
        let _queued = unsafe { PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE) };
        let parent = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("Atlas synthetic process recovery"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                800,
                600,
                None,
                None,
                None,
                None,
            )
        }
        .expect("synthetic parent");
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("synthetic clock")
            .as_nanos();
        let profile = std::env::temp_dir().join(format!(
            "atlas-native-recovery-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&profile).expect("exclusive synthetic profile");
        let environment = environment::create(&profile).expect("synthetic native environment");
        let (events, receive) = unbounded();
        let (send, native) = mpsc::channel();
        Self {
            state: Some(BrowserState {
                pages: HashMap::new(),
                popups: popups::PopupRequests::default(),
                environment: Some(environment),
                profile,
                unavailable: HashMap::new(),
                active: None,
                recovery_sequence: 0,
                bounds: PageBounds {
                    x: 0,
                    y: 0,
                    width: 800,
                    height: 600,
                },
                parent: parent.0 as isize,
                events: EventSink(events, None),
                shortcuts: NativeSink {
                    send,
                    thread_id: unsafe { GetCurrentThreadId() },
                },
            }),
            parent,
            events: receive,
            native,
            _apartment: apartment,
            _deadline: deadline,
        }
    }
    fn state(&mut self) -> &mut BrowserState {
        self.state.as_mut().expect("fixture remains live")
    }
    fn create(&mut self, id: u64) {
        self.state()
            .apply(BrowserCommand::Create {
                id,
                url: "about:blank".into(),
            })
            .expect("create synthetic controller");
    }
    fn drain(&mut self) -> Vec<BrowserEvent> {
        let mut result = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(event) => {
                    if let Some(event) = event.into_current() {
                        result.push(event);
                    }
                }
                Err(futures::channel::mpsc::TryRecvError::Empty) => break,
                Err(futures::channel::mpsc::TryRecvError::Closed) => {
                    panic!("synthetic live event queue disconnected")
                }
            }
        }
        result
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.state.take());
        if let Err(error) = unsafe { DestroyWindow(self.parent) } {
            eprintln!(
                "Synthetic recovery parent cleanup failed: {}",
                error.code().0
            );
        }
    }
}

#[test]
fn failure_kind_policy_does_not_restart_the_browser_for_renderer_or_auxiliary_failures() {
    assert_eq!(failure_action(0), FailureAction::Browser);
    assert_eq!(failure_action(1), FailureAction::Renderer);
    for kind in 2..=10 {
        assert_eq!(failure_action(kind), FailureAction::Informational);
    }
    assert_eq!(failure_action(i32::MAX), FailureAction::Informational);
}

#[test]
fn native_faults_recover_once_keep_all_tabs_and_leave_other_renderers_untouched() {
    let mut fixture = Fixture::new();
    for id in 1..=3 {
        fixture.create(id);
    }
    fixture
        .state()
        .apply(BrowserCommand::ShowActive(Some(2)))
        .expect("activate synthetic sibling");
    let sibling = fixture.state().pages[&2].webview().as_raw();
    let first = fixture.state().pages[&1].synthetic_failure(1, 1);
    let duplicate = fixture.state().pages[&1].synthetic_failure(1, 1);
    fixture.state().native_event(first);
    fixture.state().native_event(duplicate);
    assert_eq!(fixture.state().pages.len(), 2);
    assert_eq!(fixture.state().unavailable.len(), 1);
    assert_eq!(fixture.state().pages[&2].webview().as_raw(), sibling);
    assert_eq!(fixture.state().active, Some(2));
    assert_eq!(
        fixture
            .drain()
            .iter()
            .filter(|event| matches!(event, BrowserEvent::PageCrashed { id: 1 }))
            .count(),
        1
    );
    fixture
        .state()
        .apply(BrowserCommand::Reload { id: 1 })
        .expect("reload only failed page");
    assert_eq!(fixture.state().pages.len(), 3);
    assert!(fixture.state().unavailable.is_empty());
    assert_eq!(fixture.state().pages[&2].webview().as_raw(), sibling);

    // All old controllers can report the same shared process loss; only the first can recover it.
    let failures: Vec<_> = (1..=3)
        .map(|id| fixture.state().pages[&id].synthetic_failure(id, 0))
        .collect();
    for event in failures {
        fixture.state().native_event(event);
    }
    assert_eq!(fixture.state().recovery_sequence, 1);
    assert_eq!(fixture.state().pages.len(), 3);
    assert_eq!(fixture.state().active, Some(2));
    assert!(fixture.state().unavailable.is_empty());
    for id in 1..=3 {
        assert_eq!(fixture.state().pages[&id].destination(), "about:blank");
    }
    let events = fixture.drain();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, BrowserEvent::RecoveryStarted { .. }))
            .count(),
        1
    );
    assert!(events.iter().any(|event| matches!(event, BrowserEvent::RecoveryFinished { sequence: 1, restored, failed } if restored == &[1,2,3] && failed.is_empty())));
    assert_eq!(
        fixture.state().apply(BrowserCommand::Create {
            id: 2,
            url: "about:blank".into()
        }),
        Err(BrowserError::DuplicateTab)
    );

    // A queued failure for a closed controller cannot target a reused numeric identity.
    let late = fixture.state().pages[&3].synthetic_failure(3, 0);
    fixture
        .state()
        .apply(BrowserCommand::Close { id: 3 })
        .expect("close synthetic page");
    fixture.create(3);
    fixture.state().native_event(late);
    assert_eq!(fixture.state().recovery_sequence, 1);
    assert_eq!(fixture.state().pages.len(), 3);
    assert!(matches!(
        fixture.native.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

#[test]
fn environment_and_controller_recovery_failures_keep_retryable_live_records() {
    let mut fixture = Fixture::new();
    fixture.create(11);
    fixture.create(12);
    fixture
        .state()
        .recover_browser_with(|_| Err(BrowserError::Environment));
    assert!(fixture.state().pages.is_empty());
    assert_eq!(fixture.state().unavailable.len(), 2);
    assert!(fixture.state().environment.is_none());
    assert!(fixture.drain().iter().any(|event| matches!(event, BrowserEvent::RecoveryFinished { restored, failed, .. } if restored.is_empty() && failed == &[11,12])));
    fixture
        .state()
        .apply(BrowserCommand::Close { id: 12 })
        .expect("close during failed recovery");
    fixture
        .state()
        .apply(BrowserCommand::Reload { id: 11 })
        .expect("retry shared environment");
    assert_eq!(fixture.state().pages.len(), 1);
    assert!(fixture.state().pages.contains_key(&11));
    assert!(!fixture.state().pages.contains_key(&12));
    assert!(fixture.state().unavailable.is_empty());

    let renderer = fixture.state().pages[&11].synthetic_failure(11, 1);
    fixture.state().native_event(renderer);
    fixture
        .state()
        .unavailable
        .insert(11, "invalid synthetic destination".into());
    assert_eq!(
        fixture.state().restore_page(11),
        Err(BrowserError::InvalidUrl)
    );
    assert!(fixture.state().unavailable.contains_key(&11));
    assert!(!fixture.state().pages.contains_key(&11));
    fixture
        .state()
        .apply(BrowserCommand::Navigate {
            id: 11,
            url: "about:blank".into(),
        })
        .expect("retry failed controller with explicit valid destination");
    assert_eq!(fixture.state().pages.len(), 1);
    assert!(fixture.state().unavailable.is_empty());
}
