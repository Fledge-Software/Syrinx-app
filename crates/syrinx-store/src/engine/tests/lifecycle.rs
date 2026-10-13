//! Proves session ownership and clean-marker ordering across startup and shutdown failures.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};

use rusqlite_migration::{M, Migrations};

use super::{TestDirectory, close, one_tab};
use crate::engine::{SaveRequest, WriterRequest};
use crate::{
    DirectoryLease, LocationError, LocationOperation, SessionControl, ShutdownState,
    StorageSession, Store, StoreError, schema,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Event {
    Begin,
    Validate,
    Complete,
    TokenDropped,
    LeaseDropped,
}

struct Tracker {
    events: Mutex<Vec<Event>>,
    lease: Mutex<Weak<TrackedLease>>,
    clean: AtomicBool,
    active: AtomicBool,
    fail_begin: AtomicBool,
    fail_complete: AtomicBool,
    checks: AtomicUsize,
    fail_check: AtomicUsize,
    panic_check: AtomicUsize,
    references_at_complete: AtomicUsize,
    references_at_drop: AtomicUsize,
}

impl Tracker {
    fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            lease: Mutex::new(Weak::new()),
            clean: AtomicBool::new(true),
            active: AtomicBool::new(false),
            fail_begin: AtomicBool::new(false),
            fail_complete: AtomicBool::new(false),
            checks: AtomicUsize::new(0),
            fail_check: AtomicUsize::new(0),
            panic_check: AtomicUsize::new(0),
            references_at_complete: AtomicUsize::new(0),
            references_at_drop: AtomicUsize::new(0),
        }
    }

    fn record(&self, event: Event) {
        self.events
            .lock()
            .expect("synthetic event lock")
            .push(event);
    }

    fn events(&self) -> Vec<Event> {
        self.events.lock().expect("synthetic event lock").clone()
    }

    fn lease_references(&self) -> usize {
        self.lease
            .lock()
            .expect("synthetic weak lease lock")
            .strong_count()
    }
}

struct TrackedLease {
    path: PathBuf,
    tracker: Arc<Tracker>,
}

impl DirectoryLease for TrackedLease {
    fn database_path(&self) -> &Path {
        &self.path
    }

    fn revalidate(&self) -> Result<(), LocationError> {
        // Every SQLite-open path must already own the session before its first lease check.
        assert!(self.tracker.active.load(Ordering::SeqCst));
        self.tracker.record(Event::Validate);
        let check = self.tracker.checks.fetch_add(1, Ordering::SeqCst) + 1;
        assert_ne!(
            check,
            self.tracker.panic_check.load(Ordering::SeqCst),
            "synthetic worker panic"
        );
        if check == self.tracker.fail_check.load(Ordering::SeqCst) {
            return Err(LocationError::IdentityChanged);
        }
        Ok(())
    }

    fn session_control(&self) -> Option<&dyn SessionControl> {
        Some(self)
    }
}

impl SessionControl for TrackedLease {
    fn begin(&self) -> Result<Box<dyn StorageSession>, LocationError> {
        self.tracker.record(Event::Begin);
        if self.tracker.fail_begin.load(Ordering::SeqCst) {
            return Err(marker_failure());
        }
        self.tracker
            .active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| LocationError::SessionAlreadyActive)?;
        let previous = if self.tracker.clean.swap(false, Ordering::SeqCst) {
            ShutdownState::Clean
        } else {
            ShutdownState::Unclean
        };
        Ok(Box::new(TrackedSession {
            tracker: Arc::clone(&self.tracker),
            previous,
        }))
    }
}

impl Drop for TrackedLease {
    fn drop(&mut self) {
        self.tracker.record(Event::LeaseDropped);
    }
}

struct TrackedSession {
    tracker: Arc<Tracker>,
    previous: ShutdownState,
}

impl StorageSession for TrackedSession {
    fn previous_shutdown(&self) -> ShutdownState {
        self.previous
    }

    fn complete(self: Box<Self>) -> Result<(), LocationError> {
        self.tracker.record(Event::Complete);
        self.tracker
            .references_at_complete
            .store(self.tracker.lease_references(), Ordering::SeqCst);
        if self.tracker.fail_complete.load(Ordering::SeqCst) {
            return Err(marker_failure());
        }
        self.tracker.clean.store(true, Ordering::SeqCst);
        Ok(())
    }
}

impl Drop for TrackedSession {
    fn drop(&mut self) {
        self.tracker
            .references_at_drop
            .store(self.tracker.lease_references(), Ordering::SeqCst);
        self.tracker.record(Event::TokenDropped);
        self.tracker.active.store(false, Ordering::SeqCst);
    }
}

fn tracked_lease(directory: &TestDirectory) -> (Arc<TrackedLease>, Arc<Tracker>) {
    let tracker = Arc::new(Tracker::new());
    let lease = Arc::new(TrackedLease {
        path: directory.database_path(),
        tracker: Arc::clone(&tracker),
    });
    *tracker.lease.lock().expect("synthetic weak lease lock") = Arc::downgrade(&lease);
    (lease, tracker)
}

fn marker_failure() -> LocationError {
    LocationError::Os {
        operation: LocationOperation::FlushControl,
        code: Some(112),
    }
}

fn assert_unclean_release(tracker: &Tracker) {
    assert!(!tracker.clean.load(Ordering::SeqCst));
    assert!(!tracker.active.load(Ordering::SeqCst));
    assert!(!tracker.events().contains(&Event::Complete));
    // Only the caller and held-session lease may remain; every connection has gone.
    assert_eq!(tracker.references_at_drop.load(Ordering::SeqCst), 2);
}

#[test]
fn failed_session_begin_prevents_any_sqlite_open() {
    let directory = TestDirectory::new();
    let (lease, tracker) = tracked_lease(&directory);
    tracker.fail_begin.store(true, Ordering::SeqCst);
    assert_eq!(
        Store::open(lease.clone()).expect_err("marker failure refuses startup"),
        StoreError::Location(marker_failure())
    );
    assert!(!directory.database_path().exists());
    assert_eq!(tracker.events(), [Event::Begin]);
    assert!(!tracker.active.load(Ordering::SeqCst));
    drop(lease);
    directory.cleanup();
}

#[test]
fn explicit_shutdown_completes_after_connections_close_and_keeps_the_lease_alive() {
    let directory = TestDirectory::new();
    let (lease, tracker) = tracked_lease(&directory);
    let store = Store::open(lease.clone()).expect("tracked store opens");
    assert_eq!(store.previous_shutdown(), ShutdownState::Clean);
    assert_eq!(tracker.events().first(), Some(&Event::Begin));
    assert!(!tracker.clean.load(Ordering::SeqCst));
    store.save(0, one_tab(1)).expect("synthetic state saves");
    assert_eq!(
        Store::open(lease.clone()).expect_err("overlap is refused"),
        StoreError::Location(LocationError::SessionAlreadyActive)
    );
    store.shutdown().expect("explicit shutdown completes");
    assert!(tracker.clean.load(Ordering::SeqCst));
    assert!(!tracker.active.load(Ordering::SeqCst));
    assert_eq!(tracker.references_at_complete.load(Ordering::SeqCst), 2);
    assert_eq!(tracker.references_at_drop.load(Ordering::SeqCst), 2);
    assert!(
        tracker
            .events()
            .ends_with(&[Event::Complete, Event::TokenDropped])
    );
    drop(lease);
    assert_eq!(tracker.events().last(), Some(&Event::LeaseDropped));
    directory.cleanup();
}

#[test]
fn refused_writer_startup_releases_the_claim_without_marking_clean() {
    let directory = TestDirectory::new();
    let connection = directory.connection();
    connection
        .execute_batch("CREATE TABLE foreign_data(value TEXT)")
        .expect("create synthetic foreign schema");
    close(connection);
    let (lease, tracker) = tracked_lease(&directory);
    assert_eq!(
        Store::open(lease.clone()).expect_err("foreign schema stays refused"),
        StoreError::ForeignDatabase
    );
    assert_unclean_release(&tracker);
    drop(lease);
    directory.cleanup();
}

#[test]
fn failed_second_reader_startup_closes_first_reader_and_writer_before_releasing_claim() {
    let directory = TestDirectory::new();
    let (lease, tracker) = tracked_lease(&directory);
    // Three checks initialize each successful connection; refuse the second reader's first check.
    tracker.fail_check.store(7, Ordering::SeqCst);
    assert_eq!(
        Store::open(lease.clone()).expect_err("second reader fails"),
        StoreError::Location(LocationError::IdentityChanged)
    );
    assert_unclean_release(&tracker);
    drop(lease);
    directory.cleanup();
}

#[test]
fn fallback_drop_releases_ownership_without_a_clean_completion() {
    let directory = TestDirectory::new();
    let (lease, tracker) = tracked_lease(&directory);
    let store = Store::open(lease.clone()).expect("tracked store opens");
    drop(store);
    assert_unclean_release(&tracker);
    let reopened = Store::open(lease.clone()).expect("unclean session can be checked and reopened");
    assert_eq!(reopened.previous_shutdown(), ShutdownState::Unclean);
    reopened
        .shutdown()
        .expect("reopened session shuts down cleanly");
    drop(lease);
    directory.cleanup();
}

#[test]
fn failed_clean_completion_is_reported_after_database_connections_close() {
    let directory = TestDirectory::new();
    let (lease, tracker) = tracked_lease(&directory);
    let store = Store::open(lease.clone()).expect("tracked store opens");
    tracker.fail_complete.store(true, Ordering::SeqCst);
    assert_eq!(
        store.shutdown(),
        Err(StoreError::Location(marker_failure()))
    );
    assert!(!tracker.clean.load(Ordering::SeqCst));
    assert!(!tracker.active.load(Ordering::SeqCst));
    assert_eq!(tracker.references_at_complete.load(Ordering::SeqCst), 2);
    assert_eq!(tracker.references_at_drop.load(Ordering::SeqCst), 2);
    drop(lease);
    directory.cleanup();
}

#[test]
fn failed_writer_shutdown_does_not_complete_the_session() {
    let directory = TestDirectory::new();
    let (lease, tracker) = tracked_lease(&directory);
    let store = Store::open(lease.clone()).expect("tracked store opens");
    tracker
        .fail_check
        .store(tracker.checks.load(Ordering::SeqCst) + 1, Ordering::SeqCst);
    assert_eq!(
        store.shutdown(),
        Err(StoreError::Location(LocationError::IdentityChanged))
    );
    assert_unclean_release(&tracker);
    drop(lease);
    directory.cleanup();
}

#[test]
fn abandoned_writer_reply_prevents_clean_completion() {
    let directory = TestDirectory::new();
    let (lease, tracker) = tracked_lease(&directory);
    let store = Store::open(lease.clone()).expect("tracked store opens");
    let (reply, receiver) = mpsc::sync_channel(1);
    drop(receiver);
    assert!(
        store
            .writer
            .as_ref()
            .expect("writer exists")
            .sender
            .send(WriterRequest::Save(SaveRequest {
                expected_revision: 0,
                snapshot: one_tab(1),
                reply
            }))
            .is_ok()
    );
    assert_eq!(store.shutdown(), Err(StoreError::ReplyAbandoned));
    assert_unclean_release(&tracker);
    drop(lease);
    directory.cleanup();
}

#[test]
fn worker_panic_keeps_the_session_claimed_until_join_and_reader_cleanup() {
    let directory = TestDirectory::new();
    let (lease, tracker) = tracked_lease(&directory);
    let store = Store::open(lease.clone()).expect("tracked store opens");
    tracker
        .panic_check
        .store(tracker.checks.load(Ordering::SeqCst) + 1, Ordering::SeqCst);
    assert_eq!(store.shutdown(), Err(StoreError::WorkerPanicked));
    assert_unclean_release(&tracker);
    drop(lease);
    directory.cleanup();
}

#[test]
fn missing_session_capability_is_unclean_and_detects_corruption_outside_model_rows() {
    let directory = TestDirectory::new();
    let store = Store::open(directory.lease()).expect("untracked new store opens");
    assert_eq!(store.previous_shutdown(), ShutdownState::Unclean);
    store.shutdown().expect("untracked store closes");
    directory.cleanup();

    // A wrong freelist count leaves the model rows decodable but fails SQLite's quick_check.
    let directory = TestDirectory::new();
    let mut connection = directory.connection();
    let initial = schema::initial_schema();
    schema::apply_migrations(&mut connection, &Migrations::new(vec![M::up(&initial)]))
        .expect("initialize synthetic database without WAL");
    close(connection);
    let mut bytes = fs::read(directory.database_path()).expect("read synthetic database bytes");
    assert_eq!(&bytes[32..40], &[0; 8]);
    bytes[36..40].copy_from_slice(&1u32.to_be_bytes());
    assert!(
        fs::write(directory.database_path(), &bytes).is_ok(),
        "damage synthetic freelist count"
    );
    assert_eq!(
        Store::open(directory.lease()).expect_err("quick_check refuses freelist corruption"),
        StoreError::IntegrityCheckFailed
    );
    assert_eq!(
        fs::read(directory.database_path()).expect("read unchanged synthetic database"),
        bytes
    );
    directory.cleanup();
}
