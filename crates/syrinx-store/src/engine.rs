//! Serializes durable writes and keeps two read connections behind independent locks.

use std::collections::BTreeMap;
use std::fmt;
use std::num::NonZeroU64;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};

use rusqlite::{Connection, OpenFlags};
use syrinx_core::{Destination, DestinationSurface, TabModelSnapshot};

use crate::deletion::DeletionRequest;
use crate::error::{
    EncodingField, StoreError, StoreOperation, combine_results, database_error, with_cleanup,
};
use crate::history::{
    self, HistoryEntry, HistoryOutcome, HistoryRequest, HistoryVisit, HistoryWrite, RecordedVisit,
};
use crate::preferences::{self, PreferenceWrite, StoredPreferences};
use crate::replay::{self, AppliedDeletions};
use crate::{BackupOutcome, DirectoryLease, ShutdownState};
use crate::{backup, records, schema};

mod session;

use session::HeldSession;

/// A durable snapshot and its compare-and-save revision; debug output redacts metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredSnapshot {
    pub revision: u64,
    pub snapshot: TabModelSnapshot,
}

/// Blocking persistence for host worker threads, never the GUI event thread.
/// Call `shutdown` to receive close errors; dropping also closes and reports structural failures.
#[must_use = "call shutdown to observe database close failures"]
pub struct Store {
    readers: Option<[Mutex<LeasedConnection>; 2]>,
    writer: Option<WriterHandle>,
    session: Option<HeldSession>,
    previous_shutdown: ShutdownState,
    next_reader: AtomicUsize,
}

impl fmt::Debug for Store {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Connection paths and queued snapshots never enter diagnostics.
        formatter
            .debug_struct("Store")
            .field("open", &self.writer.is_some())
            .finish_non_exhaustive()
    }
}

impl Store {
    /// Opens only a supplied checked lease; this does not discover or open the owner's data.
    pub fn open(lease: Arc<dyn DirectoryLease>) -> Result<Self, StoreError> {
        // Claim the session before any thread can open SQLite, then initialize the writer first.
        let session = HeldSession::begin(lease)?;
        let previous_shutdown = session.previous_shutdown();
        let writer = WriterHandle::start(Arc::clone(session.lease()), previous_shutdown)?;
        let first =
            match LeasedConnection::open(Arc::clone(session.lease()), ConnectionRole::Reader) {
                Ok(connection) => connection,
                Err(error) => return Err(with_cleanup(error, writer.stop())),
            };
        let second =
            match LeasedConnection::open(Arc::clone(session.lease()), ConnectionRole::Reader) {
                Ok(connection) => connection,
                Err(error) => {
                    let cleanup = combine_results(first.close(false), writer.stop());
                    return Err(with_cleanup(error, cleanup));
                }
            };
        Ok(Self {
            readers: Some([Mutex::new(first), Mutex::new(second)]),
            writer: Some(writer),
            session: Some(session),
            previous_shutdown,
            next_reader: AtomicUsize::new(0),
        })
    }

    /// Reports the previous session's evidence; an untracked lease is conservatively unclean.
    pub fn previous_shutdown(&self) -> ShutdownState {
        self.previous_shutdown
    }

    /// Reads one committed revision. This blocks on a reader lock and SQLite's busy timeout.
    pub fn load(&self) -> Result<StoredSnapshot, StoreError> {
        // Reading here and then telling the writer means load now depends on the writer: it
        // fails with WorkerUnavailable if the writer stopped, waits behind queued saves or a
        // backup, and the writer keeps one more full snapshot. Load runs once, at start-up.
        let stored =
            self.read(|connection| records::load(connection, schema::CURRENT_GENERATION))?;
        // A revision's rows never change, so the writer may compare the next save with it.
        let writer = self.writer.as_ref().ok_or(StoreError::Closed)?;
        writer
            .sender
            .send(WriterRequest::Remember(stored.clone()))
            .map_err(|_| StoreError::WorkerUnavailable)?;
        Ok(stored)
    }

    /// Reads the owner's settings and tiles from one committed snapshot. This blocks.
    pub fn load_preferences(&self) -> Result<StoredPreferences, StoreError> {
        self.read(preferences::load)
    }

    /// Inserts or replaces settings keys in one transaction. This blocks on the writer queue.
    pub fn save_settings(&self, entries: Vec<(String, String)>) -> Result<(), StoreError> {
        self.write_preferences(PreferenceWrite::Settings(entries))
    }

    /// Replaces one tile surface in order. This blocks on the writer queue.
    pub fn save_destinations(
        &self,
        surface: DestinationSurface,
        tiles: Vec<Destination>,
    ) -> Result<(), StoreError> {
        self.write_preferences(PreferenceWrite::Destinations { surface, tiles })
    }

    fn write_preferences(&self, change: PreferenceWrite) -> Result<(), StoreError> {
        let writer = self.writer.as_ref().ok_or(StoreError::Closed)?;
        let (reply, result) = mpsc::sync_channel(1);
        writer
            .sender
            .send(WriterRequest::Preferences { change, reply })
            .map_err(|_| StoreError::WorkerUnavailable)?;
        result.recv().map_err(|_| StoreError::WorkerUnavailable)?
    }

    /// Commits only if `expected_revision` is current. A lost reply requires reloading the revision.
    pub fn save(
        &self,
        expected_revision: u64,
        snapshot: TabModelSnapshot,
    ) -> Result<u64, StoreError> {
        // A reply acknowledges the committed revision, never merely a queued request.
        // The writer validates the snapshot once before writing (`records::save`).
        let writer = self.writer.as_ref().ok_or(StoreError::Closed)?;
        let (reply, result) = mpsc::sync_channel(1);
        writer
            .sender
            .send(WriterRequest::Save(SaveRequest {
                expected_revision,
                snapshot,
                reply,
            }))
            .map_err(|_| StoreError::WorkerUnavailable)?;
        result.recv().map_err(|_| StoreError::WorkerUnavailable)?
    }

    /// Reads the explicit choice for a canonical HTTP(S) origin on a reader connection.
    pub fn permission(
        &self,
        origin: &str,
        kind: crate::PermissionKind,
    ) -> Result<Option<bool>, StoreError> {
        let readers = self.readers.as_ref().ok_or(StoreError::Closed)?;
        let reader = readers[0].lock().map_err(|_| StoreError::ReaderPoisoned)?;
        crate::permissions::load(&reader.connection, origin, kind)
    }

    /// Serializes an explicit remembered choice with other durable writes.
    pub fn remember_permission(
        &self,
        origin: String,
        kind: crate::PermissionKind,
        allowed: bool,
    ) -> Result<(), StoreError> {
        crate::permissions::validate_origin(&origin)?;
        let writer = self.writer.as_ref().ok_or(StoreError::Closed)?;
        let (reply, result) = mpsc::sync_channel(1);
        writer
            .sender
            .send(WriterRequest::Permission {
                origin,
                kind,
                allowed,
                reply,
            })
            .map_err(|_| StoreError::WorkerUnavailable)?;
        result.recv().map_err(|_| StoreError::WorkerUnavailable)?
    }

    /// Forgets one remembered origin/kind choice so the next request asks again. Returns
    /// `false` when no choice was saved, which callers may report as already forgotten.
    pub fn forget_permission(
        &self,
        origin: String,
        kind: crate::PermissionKind,
    ) -> Result<bool, StoreError> {
        self.forget(origin, kind, None)
    }

    /// Forgets a choice the owner saw listed, only while it is still saved as `allowed`, so a
    /// stale click never removes a choice that changed since. `false` when nothing matched.
    pub fn forget_listed_permission(
        &self,
        listed: crate::RememberedPermission,
    ) -> Result<bool, StoreError> {
        self.forget(listed.origin, listed.kind, Some(listed.allowed))
    }

    fn forget(
        &self,
        origin: String,
        kind: crate::PermissionKind,
        expected: Option<bool>,
    ) -> Result<bool, StoreError> {
        crate::permissions::validate_origin(&origin)?;
        let writer = self.writer.as_ref().ok_or(StoreError::Closed)?;
        let (reply, result) = mpsc::sync_channel(1);
        writer
            .sender
            .send(WriterRequest::ForgetPermission {
                origin,
                kind,
                expected,
                reply,
            })
            .map_err(|_| StoreError::WorkerUnavailable)?;
        result.recv().map_err(|_| StoreError::WorkerUnavailable)?
    }

    /// Every remembered site permission, ordered by origin and kind, for the settings list.
    pub fn remembered_permissions(&self) -> Result<Vec<crate::RememberedPermission>, StoreError> {
        self.read(|connection| crate::permissions::list(connection))
    }

    /// Every saved per-site zoom level by host (#90), read once at start-up.
    pub fn site_zooms(&self) -> Result<BTreeMap<String, f64>, StoreError> {
        self.read(|connection| crate::site_zoom::load(connection))
    }

    /// Saves `host`'s zoom level as changed at `modified_at` Unix seconds, or forgets it with
    /// `None` so the site opens at 100%. A default (100%) factor is refused: pass `None`.
    pub fn save_site_zoom(
        &self,
        host: String,
        factor: Option<f64>,
        modified_at: i64,
    ) -> Result<(), StoreError> {
        crate::site_zoom::validate_host(&host)?;
        let writer = self.writer.as_ref().ok_or(StoreError::Closed)?;
        let (reply, result) = mpsc::sync_channel(1);
        writer
            .sender
            .send(WriterRequest::SiteZoom {
                host,
                factor,
                modified_at,
                reply,
            })
            .map_err(|_| StoreError::WorkerUnavailable)?;
        result.recv().map_err(|_| StoreError::WorkerUnavailable)?
    }

    /// Forgets the zoom levels changed in `[start, end)` Unix seconds; returns how many.
    pub fn clear_site_zooms(&self, start: i64, end: i64) -> Result<usize, StoreError> {
        let writer = self.writer.as_ref().ok_or(StoreError::Closed)?;
        let (reply, result) = mpsc::sync_channel(1);
        writer
            .sender
            .send(WriterRequest::ClearSiteZoom { start, end, reply })
            .map_err(|_| StoreError::WorkerUnavailable)?;
        result.recv().map_err(|_| StoreError::WorkerUnavailable)?
    }

    /// Creates a daily validated backup only while idle. This blocks on the existing writer queue.
    pub fn backup_if_due(
        &self,
        now_utc_seconds: i64,
        idle: bool,
    ) -> Result<BackupOutcome, StoreError> {
        let writer = self.writer.as_ref().ok_or(StoreError::Closed)?;
        let (reply, result) = mpsc::sync_channel(1);
        writer
            .sender
            .send(WriterRequest::Backup {
                now_utc_seconds,
                idle,
                reply,
            })
            .map_err(|_| StoreError::WorkerUnavailable)?;
        result.recv().map_err(|_| StoreError::WorkerUnavailable)?
    }

    /// Takes one validated backup before an update installs (P1-39b), outside the daily
    /// schedule; `NotDue` when a valid copy is under an hour old. Blocks on the writer queue.
    pub fn backup_before_update(&self, now_utc_seconds: i64) -> Result<BackupOutcome, StoreError> {
        let writer = self.writer.as_ref().ok_or(StoreError::Closed)?;
        let (reply, result) = mpsc::sync_channel(1);
        writer
            .sender
            .send(WriterRequest::BackupNow {
                now_utc_seconds,
                reply,
            })
            .map_err(|_| StoreError::WorkerUnavailable)?;
        result.recv().map_err(|_| StoreError::WorkerUnavailable)?
    }

    /// Records one visit (P1-20) at `visited_at` Unix seconds. An empty title keeps a known one.
    pub fn record_visit(
        &self,
        address: String,
        title: String,
        visited_at: i64,
    ) -> Result<RecordedVisit, StoreError> {
        match self.write_history(HistoryWrite::Record {
            address,
            title,
            visited_at,
        })? {
            HistoryOutcome::Recorded(recorded) => Ok(recorded),
            _ => Err(StoreError::InvalidEncoding(EncodingField::History)),
        }
    }

    /// Updates a remembered address's title; `None` means the entry no longer exists.
    pub fn retitle_history(
        &self,
        id: NonZeroU64,
        title: String,
    ) -> Result<Option<HistoryEntry>, StoreError> {
        self.write_entry(HistoryWrite::Retitle { id, title })
    }

    /// Retracts one visit, such as a failed navigation. Returns the address's remaining
    /// totals, or `None` when no visit remains for it.
    pub fn remove_visit(&self, visit: NonZeroU64) -> Result<Option<HistoryEntry>, StoreError> {
        self.write_entry(HistoryWrite::RemoveVisit(visit))
    }

    /// Applies D47 clears of history and site zoom levels and records the newest generation
    /// applied, in one transaction. Reports what changed so a running catalogue can follow.
    pub fn apply_deletions(
        &self,
        requests: &[DeletionRequest],
    ) -> Result<AppliedDeletions, StoreError> {
        let writer = self.writer.as_ref().ok_or(StoreError::Closed)?;
        let (reply, result) = mpsc::sync_channel(1);
        writer
            .sender
            .send(WriterRequest::Deletions {
                requests: requests.to_vec(),
                reply,
            })
            .map_err(|_| StoreError::WorkerUnavailable)?;
        result.recv().map_err(|_| StoreError::WorkerUnavailable)?
    }

    /// The newest D47 clear this database has applied, of any category; the journal must
    /// reach at least it.
    pub fn deletion_generation(&self) -> Result<u64, StoreError> {
        self.read(|connection| history::applied_generation(connection))
    }

    /// Where the next bounded history expiry `[0, end)` stops: at `cutoff`, or earlier so
    /// that it removes at most `limit` visits. `None` when no visit is older than `cutoff`.
    pub fn history_expiry_end(&self, cutoff: i64, limit: usize) -> Result<Option<i64>, StoreError> {
        self.read(|connection| history::expiry_end(connection, cutoff, limit))
    }

    /// Unix seconds of the newest stored visit, or `None` without history.
    pub fn newest_visit_time(&self) -> Result<Option<i64>, StoreError> {
        self.read(|connection| history::newest_visit_time(connection))
    }

    /// Every remembered address, for rebuilding the search catalogue at startup.
    pub fn history_entries(&self) -> Result<Vec<HistoryEntry>, StoreError> {
        self.read(|connection| history::entries(connection))
    }

    /// At most `limit` visits, newest first.
    pub fn recent_visits(&self, limit: usize) -> Result<Vec<HistoryVisit>, StoreError> {
        self.read(|connection| history::recent(connection, limit))
    }

    fn read<T>(
        &self,
        query: impl FnOnce(&mut Connection) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        // Alternate readers; each connection is accessed by only one caller at a time.
        let readers = self.readers.as_ref().ok_or(StoreError::Closed)?;
        let index = self.next_reader.fetch_add(1, Ordering::Relaxed) % readers.len();
        let mut reader = readers[index]
            .lock()
            .map_err(|_| StoreError::ReaderPoisoned)?;
        query(&mut reader.connection)
    }

    fn write_entry(&self, write: HistoryWrite) -> Result<Option<HistoryEntry>, StoreError> {
        match self.write_history(write)? {
            HistoryOutcome::Entry(entry) => Ok(entry),
            _ => Err(StoreError::InvalidEncoding(EncodingField::History)),
        }
    }

    fn write_history(&self, write: HistoryWrite) -> Result<HistoryOutcome, StoreError> {
        let writer = self.writer.as_ref().ok_or(StoreError::Closed)?;
        let (reply, result) = mpsc::sync_channel(1);
        writer
            .sender
            .send(WriterRequest::History(HistoryRequest { write, reply }))
            .map_err(|_| StoreError::WorkerUnavailable)?;
        result.recv().map_err(|_| StoreError::WorkerUnavailable)?
    }

    /// Closes readers, drains the writer and reports all shutdown failures. This blocks.
    pub fn shutdown(mut self) -> Result<(), StoreError> {
        self.close_all(true)
    }

    fn close_all(&mut self, complete_session: bool) -> Result<(), StoreError> {
        // Close readers first so the writer can finish the final WAL cleanup while leased.
        let mut result = Ok(());
        if let Some(readers) = self.readers.take() {
            for reader in readers {
                result = combine_results(result, close_reader(reader));
            }
        }
        if let Some(writer) = self.writer.take() {
            result = combine_results(result, writer.stop());
        }
        // Only explicit, fully successful shutdown can turn the external marker clean.
        if let Some(session) = self.session.take() {
            if complete_session && result.is_ok() {
                result = session.complete();
            } else {
                drop(session);
            }
        }
        result
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        // Explicit shutdown returns errors; this fallback still surfaces structural failures.
        if let Err(error) = self.close_all(false) {
            eprintln!("Syrinx store shutdown failed: {error}");
        }
    }
}

struct SaveRequest {
    expected_revision: u64,
    snapshot: TabModelSnapshot,
    reply: mpsc::SyncSender<Result<u64, StoreError>>,
}

enum WriterRequest {
    Permission {
        origin: String,
        kind: crate::PermissionKind,
        allowed: bool,
        reply: mpsc::SyncSender<Result<(), StoreError>>,
    },
    ForgetPermission {
        origin: String,
        kind: crate::PermissionKind,
        /// Removes the choice only while it is still saved as this value.
        expected: Option<bool>,
        reply: mpsc::SyncSender<Result<bool, StoreError>>,
    },
    SiteZoom {
        host: String,
        factor: Option<f64>,
        modified_at: i64,
        reply: mpsc::SyncSender<Result<(), StoreError>>,
    },
    ClearSiteZoom {
        start: i64,
        end: i64,
        reply: mpsc::SyncSender<Result<usize, StoreError>>,
    },
    /// D47 journal requests to apply in one transaction, whatever their categories.
    Deletions {
        requests: Vec<DeletionRequest>,
        reply: mpsc::SyncSender<Result<AppliedDeletions, StoreError>>,
    },
    Save(SaveRequest),
    /// A committed revision a reader loaded; no reply.
    Remember(StoredSnapshot),
    Backup {
        now_utc_seconds: i64,
        idle: bool,
        reply: mpsc::SyncSender<Result<BackupOutcome, StoreError>>,
    },
    BackupNow {
        now_utc_seconds: i64,
        reply: mpsc::SyncSender<Result<BackupOutcome, StoreError>>,
    },
    Preferences {
        change: PreferenceWrite,
        reply: mpsc::SyncSender<Result<(), StoreError>>,
    },
    History(HistoryRequest),
}

struct WriterHandle {
    sender: mpsc::SyncSender<WriterRequest>,
    thread: JoinHandle<Result<(), StoreError>>,
}

impl WriterHandle {
    fn start(
        lease: Arc<dyn DirectoryLease>,
        previous_shutdown: ShutdownState,
    ) -> Result<Self, StoreError> {
        // Bound retained snapshots; callers already run off the GUI thread and may wait.
        let (sender, requests) = mpsc::sync_channel(8);
        let (ready, initialized) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("syrinx-store-writer".to_owned())
            .spawn(move || writer_main(lease, previous_shutdown, requests, ready))
            .map_err(|error| StoreError::WorkerStart {
                code: error.raw_os_error(),
            })?;
        let writer = Self { sender, thread };
        match initialized.recv() {
            Ok(Ok(())) => Ok(writer),
            Ok(Err(error)) => Err(with_cleanup(error, writer.stop())),
            Err(_) => Err(with_cleanup(StoreError::WorkerUnavailable, writer.stop())),
        }
    }

    fn stop(self) -> Result<(), StoreError> {
        // Closing the sole sender lets the worker drain queued requests and close its connection.
        drop(self.sender);
        self.thread.join().map_err(|_| StoreError::WorkerPanicked)?
    }
}

fn writer_main(
    lease: Arc<dyn DirectoryLease>,
    previous_shutdown: ShutdownState,
    requests: mpsc::Receiver<WriterRequest>,
    ready: mpsc::SyncSender<Result<(), StoreError>>,
) -> Result<(), StoreError> {
    let mut writer = match LeasedConnection::open(lease, ConnectionRole::Writer(previous_shutdown))
    {
        Ok(connection) => connection,
        Err(error) => {
            return ready
                .send(Err(error))
                .map_err(|_| StoreError::ReplyAbandoned);
        }
    };
    if ready.send(Ok(())).is_err() {
        return Err(with_cleanup(
            StoreError::ReplyAbandoned,
            writer.close(false),
        ));
    }
    let work = serve_writes(&mut writer, requests);
    combine_results(work, writer.close(true))
}

fn serve_writes(
    writer: &mut LeasedConnection,
    requests: mpsc::Receiver<WriterRequest>,
) -> Result<(), StoreError> {
    let mut schedule = backup::BackupSchedule::default();
    // The last snapshot this writer committed, so an unchanged saved tree is not rewritten.
    let mut committed: Option<StoredSnapshot> = None;
    loop {
        // Sender disconnection is the shutdown signal, not an ignored receive failure.
        let request = match requests.recv() {
            Ok(request) => request,
            Err(mpsc::RecvError) => return Ok(()),
        };
        match request {
            WriterRequest::Permission {
                origin,
                kind,
                allowed,
                reply,
            } => {
                reply
                    .send(crate::permissions::save(
                        &writer.connection,
                        &origin,
                        kind,
                        allowed,
                    ))
                    .map_err(|_| StoreError::ReplyAbandoned)?;
            }
            WriterRequest::ForgetPermission {
                origin,
                kind,
                expected,
                reply,
            } => {
                let result =
                    crate::permissions::forget(&writer.connection, &origin, kind, expected);
                reply.send(result).map_err(|_| StoreError::ReplyAbandoned)?;
            }
            WriterRequest::SiteZoom {
                host,
                factor,
                modified_at,
                reply,
            } => {
                let result =
                    crate::site_zoom::write(&writer.connection, &host, factor, modified_at);
                reply.send(result).map_err(|_| StoreError::ReplyAbandoned)?;
            }
            WriterRequest::ClearSiteZoom { start, end, reply } => {
                let result = crate::site_zoom::clear(&writer.connection, start, end);
                reply.send(result).map_err(|_| StoreError::ReplyAbandoned)?;
            }
            WriterRequest::Deletions { requests, reply } => {
                let result = replay::apply_live(&mut writer.connection, &requests);
                reply.send(result).map_err(|_| StoreError::ReplyAbandoned)?;
            }
            WriterRequest::Save(request) => {
                let saved_rows = committed
                    .as_ref()
                    .filter(|stored| stored.revision == request.expected_revision)
                    .map_or(records::SavedRows::Replace, |stored| {
                        records::SavedRows::between(&stored.snapshot, &request.snapshot)
                    });
                let result = records::save(
                    &mut writer.connection,
                    request.expected_revision,
                    &request.snapshot,
                    saved_rows,
                );
                // Any failure forgets the cache, so the next save rewrites everything.
                committed = match &result {
                    Ok(revision) => Some(StoredSnapshot {
                        revision: *revision,
                        snapshot: request.snapshot,
                    }),
                    Err(_) => None,
                };
                request
                    .reply
                    .send(result)
                    .map_err(|_| StoreError::ReplyAbandoned)?;
            }
            WriterRequest::Remember(stored) => {
                if committed
                    .as_ref()
                    .is_none_or(|current| current.revision < stored.revision)
                {
                    committed = Some(stored);
                }
            }
            WriterRequest::Backup {
                now_utc_seconds,
                idle,
                reply,
            } => {
                let result = if !idle {
                    Ok(BackupOutcome::NotIdle)
                } else {
                    writer
                        .lease
                        .backup_archive()
                        .ok_or(StoreError::BackupUnavailable)
                        .and_then(|archive| {
                            schedule.create_if_due(
                                &mut writer.connection,
                                archive.as_ref(),
                                now_utc_seconds,
                                idle,
                            )
                        })
                };
                reply.send(result).map_err(|_| StoreError::ReplyAbandoned)?;
            }
            WriterRequest::BackupNow {
                now_utc_seconds,
                reply,
            } => {
                let result = writer
                    .lease
                    .backup_archive()
                    .ok_or(StoreError::BackupUnavailable)
                    .and_then(|archive| {
                        schedule.create_now(
                            &mut writer.connection,
                            archive.as_ref(),
                            now_utc_seconds,
                        )
                    });
                reply.send(result).map_err(|_| StoreError::ReplyAbandoned)?;
            }
            WriterRequest::Preferences { change, reply } => {
                let result = preferences::write(&mut writer.connection, &change);
                reply.send(result).map_err(|_| StoreError::ReplyAbandoned)?;
            }
            WriterRequest::History(HistoryRequest { write, reply }) => {
                let result = history::write(&mut writer.connection, write);
                reply.send(result).map_err(|_| StoreError::ReplyAbandoned)?;
            }
        }
    }
}

struct LeasedConnection {
    // Declaration order keeps the lease alive through connection cleanup, including unwinding.
    connection: Connection,
    lease: Arc<dyn DirectoryLease>,
}

#[derive(Clone, Copy)]
enum ConnectionRole {
    Writer(ShutdownState),
    Reader,
}

impl LeasedConnection {
    fn open(lease: Arc<dyn DirectoryLease>, role: ConnectionRole) -> Result<Self, StoreError> {
        lease.revalidate()?;
        let access = if matches!(role, ConnectionRole::Writer(_)) {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        };
        // SQLite's MSVC default VFS limits Windows paths; this built-in VFS keeps normal locking.
        let connection = Connection::open_with_flags_and_vfs(
            lease.database_path(),
            access | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            "win32-longpath",
        )
        .map_err(|error| database_error(StoreOperation::Open, error))?;
        let mut leased = Self { connection, lease };
        let prepared = leased.prepare(role);
        match prepared {
            Ok(()) => Ok(leased),
            Err(error) => Err(with_cleanup(error, leased.close(false))),
        }
    }

    fn prepare(&mut self, role: ConnectionRole) -> Result<(), StoreError> {
        self.lease.revalidate()?;
        schema::configure_connection(&self.connection)?;
        match role {
            ConnectionRole::Writer(previous_shutdown) => {
                schema::prepare_writer(&mut self.connection, previous_shutdown)?;
            }
            ConnectionRole::Reader => {
                // The writer migrates first, so a reader only ever sees the current schema.
                if schema::prepare_reader(&self.connection)? != schema::CURRENT_GENERATION {
                    return Err(StoreError::SchemaMismatch);
                }
            }
        }
        self.lease.revalidate()?;
        Ok(())
    }

    fn close(self, optimize: bool) -> Result<(), StoreError> {
        let maintenance = if optimize {
            self.lease
                .revalidate()
                .map_err(StoreError::from)
                .and_then(|()| schema::optimize(&self.connection))
        } else {
            Ok(())
        };
        let Self { connection, lease } = self;
        let closed = connection.close().map_err(|(connection, error)| {
            let error = database_error(StoreOperation::Close, error);
            drop(connection);
            error
        });
        drop(lease);
        combine_results(maintenance, closed)
    }
}

fn close_reader(reader: Mutex<LeasedConnection>) -> Result<(), StoreError> {
    match reader.into_inner() {
        Ok(reader) => reader.close(false),
        Err(poisoned) => Err(with_cleanup(
            StoreError::ReaderPoisoned,
            poisoned.into_inner().close(false),
        )),
    }
}

#[cfg(test)]
mod tests;
