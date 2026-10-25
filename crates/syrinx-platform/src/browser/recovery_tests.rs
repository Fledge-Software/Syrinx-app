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
                w!("Syrinx synthetic process recovery"),
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
            "syrinx-native-recovery-{}-{nonce}",
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
                downloads: super::downloads::DownloadOperations::default(),
                environment: Some(environment),
                engine: None,
                profile,
                unavailable: HashMap::new(),
                active: None,
                recovery_sequence: 0,
                recovery: RecoveryBudget::default(),
                bounds: PageBounds {
                    x: 0,
                    y: 0,
                    width: 800,
                    height: 600,
                },
                parent: parent.0 as isize,
                page_text: page_text::TextContext::default(),
                events: EventSink(events, None),
                shortcuts: NativeSink {
                    send,
                    thread_id: unsafe { GetCurrentThreadId() },
                    peek_generation: Arc::new(AtomicU64::new(0)),
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
    /// Exhausts the automatic budget through `failing`, leaving every tab crashed and the engine stopped.
    fn stop_engine(&mut self, failing: u64) {
        for _ in 0..ATTEMPT_LIMIT {
            let failure = self.state().pages[&failing].synthetic_failure(failing, 0);
            self.state().native_event(failure);
            let due = self.state().recovery.due.expect("scheduled rebuild");
            self.state().recovery_tick(due);
        }
        let failure = self.state().pages[&failing].synthetic_failure(failing, 0);
        self.state().native_event(failure);
        assert!(self.state().recovery.stopped);
        assert!(self.state().environment.is_none());
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
    assert!(
        fixture.state().pages.is_empty(),
        "backoff releases every failed controller"
    );
    let due = fixture.state().recovery.due.expect("scheduled rebuild");
    assert!(
        !fixture
            .state()
            .recovery_tick(due - Duration::from_millis(1))
    );
    assert!(
        fixture.state().pages.is_empty(),
        "no rebuild before backoff"
    );
    assert!(
        fixture.state().recovery_tick(due),
        "a rebuild must return the worker to its queues before waiting"
    );
    assert!(
        !fixture.state().recovery_tick(due),
        "an idle worker must not spin"
    );
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
    assert!(events.iter().any(|event| matches!(event, BrowserEvent::RecoveryFinished { sequence: 1, restored, failed, stopped: false } if restored == &[1,2,3] && failed.is_empty())));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, BrowserEvent::ProcessFailed { kind: 0, .. })),
        "smoke must still observe the retired controller's process failure"
    );
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
    assert!(fixture.state().begin_recovery());
    fixture
        .state()
        .rebuild_environment(|_| Err(BrowserError::Environment));
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
    fixture.state().unavailable.insert(
        11,
        Unavailable::chosen("invalid synthetic destination".into()),
    );
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

#[test]
fn recovery_budget_caps_three_attempts_and_requires_explicit_retry_after_stop() {
    let start = Instant::now();
    let mut now = start;
    let mut budget = RecoveryBudget::default();
    for delay in [1, 2, 4] {
        assert!(budget.schedule(now));
        now += Duration::from_secs(delay);
        assert_eq!(budget.due, Some(now));
        assert!(!budget.take_due(now - Duration::from_millis(1)));
        assert!(budget.take_due(now));
        assert!(!budget.take_due(now), "one attempt per wake");
    }
    assert!(!budget.schedule(now));
    assert!(budget.stopped);
    assert!(
        !budget.schedule(start + ATTEMPT_WINDOW * 2),
        "elapsed time must not restart a stopped engine"
    );
    budget.owner_retry(start + ATTEMPT_WINDOW * 2);
    assert!(!budget.stopped);
    assert_eq!(budget.attempts.len(), 1);
    assert!(budget.schedule(start + ATTEMPT_WINDOW * 2));
}

#[test]
fn isolated_failures_expire_from_the_rolling_window() {
    let start = Instant::now();
    let mut budget = RecoveryBudget::default();
    for index in 0..5 {
        let now = start + ATTEMPT_WINDOW * index;
        assert!(budget.schedule(now));
        let due = budget.due.unwrap();
        assert!(budget.take_due(due));
        assert!(budget.attempts.len() <= 2);
    }
    assert!(!budget.stopped);
}

#[test]
fn repeated_native_failures_stop_and_owner_reload_recovers_retained_tabs() {
    let mut fixture = Fixture::new();
    fixture.create(21);
    fixture.create(22);
    fixture
        .state()
        .apply(BrowserCommand::ShowActive(Some(21)))
        .unwrap();
    for _ in 0..ATTEMPT_LIMIT {
        let failure = fixture.state().pages[&21].synthetic_failure(21, 0);
        fixture.state().native_event(failure);
        let due = fixture.state().recovery.due.unwrap();
        fixture.state().recovery_tick(due);
        assert_eq!(fixture.state().pages.len(), 2);
    }
    let failure = fixture.state().pages[&21].synthetic_failure(21, 0);
    fixture.state().native_event(failure);
    assert!(fixture.state().pages.is_empty());
    assert!(fixture.state().environment.is_none());
    assert_eq!(fixture.state().unavailable_tabs(), [21, 22]);
    assert!(fixture.state().recovery.stopped);
    assert!(fixture.state().recovery.due.is_none());
    fixture
        .state()
        .recovery_tick(Instant::now() + ATTEMPT_WINDOW * 2);
    assert!(fixture.state().pages.is_empty());
    assert!(
        fixture.state().environment.is_none(),
        "elapsed time must not restart a stopped engine"
    );
    assert!(fixture.drain().iter().any(|event| matches!(event, BrowserEvent::RecoveryFinished { stopped: true, failed, .. } if failed == &[21, 22])));
    fixture
        .state()
        .apply(BrowserCommand::Close { id: 22 })
        .unwrap();
    fixture
        .state()
        .apply(BrowserCommand::Reload { id: 21 })
        .unwrap();
    assert!(!fixture.state().recovery.stopped);
    assert_eq!(fixture.state().active, Some(21));
    assert_eq!(fixture.state().pages.len(), 1);
    assert_eq!(fixture.state().pages[&21].destination(), "about:blank");
    assert!(fixture.state().unavailable.is_empty());
    assert!(fixture.drain().iter().any(|event| matches!(event, BrowserEvent::RecoveryFinished { stopped: false, restored, failed, .. } if restored == &[21] && failed.is_empty())));
}

#[test]
fn closing_every_crashed_tab_after_the_cap_lets_a_new_tab_start_the_engine() {
    let mut fixture = Fixture::new();
    fixture.create(31);
    fixture.create(32);
    fixture.stop_engine(31);
    assert_eq!(fixture.state().unavailable_tabs(), [31, 32]);
    fixture
        .state()
        .apply(BrowserCommand::Close { id: 31 })
        .expect("close first crashed tab");
    assert!(
        fixture.state().recovery.stopped,
        "a remaining crashed tab keeps the engine stopped"
    );
    fixture
        .state()
        .apply(BrowserCommand::Close { id: 32 })
        .expect("close last crashed tab");
    assert!(!fixture.state().recovery.stopped);
    assert!(fixture.state().recovery.attempts.is_empty());
    fixture.drain();

    fixture.create(33);
    assert!(fixture.state().environment.is_some());
    assert_eq!(fixture.state().pages.len(), 1);
    assert_eq!(fixture.state().pages[&33].destination(), "about:blank");
    assert!(fixture.state().unavailable.is_empty());
    assert!(
        !fixture
            .drain()
            .iter()
            .any(|event| matches!(event, BrowserEvent::RecoveryStarted { .. })),
        "nothing remained to restore"
    );
}

#[test]
fn new_tab_after_the_cap_is_an_owner_retry_that_restores_crashed_tabs() {
    let mut fixture = Fixture::new();
    fixture.create(41);
    fixture.create(42);
    fixture.stop_engine(41);
    fixture.drain();

    fixture.create(43);
    assert!(!fixture.state().recovery.stopped);
    assert_eq!(
        fixture.state().recovery.attempts.len(),
        1,
        "the owner retry starts a fresh budget"
    );
    assert!(fixture.state().environment.is_some());
    assert!(fixture.state().unavailable.is_empty());
    let mut live: Vec<_> = fixture.state().pages.keys().copied().collect();
    live.sort_unstable();
    assert_eq!(live, [41, 42, 43]);
    assert!(fixture.drain().iter().any(|event| matches!(event, BrowserEvent::RecoveryFinished { stopped: false, restored, failed, .. } if restored == &[41, 42] && failed.is_empty())));

    // The fresh budget allows automatic recovery again.
    let failure = fixture.state().pages[&43].synthetic_failure(43, 0);
    fixture.state().native_event(failure);
    assert!(fixture.state().recovery.due.is_some());
}

#[test]
fn closing_a_crashed_tab_leaves_a_running_budget_untouched() {
    let mut fixture = Fixture::new();
    fixture.create(51);
    fixture.create(52);
    let failure = fixture.state().pages[&51].synthetic_failure(51, 0);
    fixture.state().native_event(failure);
    let due = fixture.state().recovery.due.expect("scheduled rebuild");
    fixture.state().recovery_tick(due);
    let renderer = fixture.state().pages[&52].synthetic_failure(52, 1);
    fixture.state().native_event(renderer);
    fixture
        .state()
        .apply(BrowserCommand::Close { id: 52 })
        .expect("close crashed renderer tab");
    assert_eq!(
        fixture.state().recovery.attempts.len(),
        1,
        "only a stopped engine forgets its attempts"
    );
}
