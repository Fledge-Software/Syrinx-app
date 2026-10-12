//! Serializes durable writes and keeps two read connections behind independent locks.

use std::fmt;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};

use atlas_core::TabModelSnapshot;
use rusqlite::{Connection, OpenFlags};

use crate::error::{StoreError, StoreOperation, combine_results, database_error, with_cleanup};
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
        // Alternate readers; each connection is accessed by only one caller at a time.
        let readers = self.readers.as_ref().ok_or(StoreError::Closed)?;
        let index = self.next_reader.fetch_add(1, Ordering::Relaxed) % readers.len();
        let mut reader = readers[index]
            .lock()
            .map_err(|_| StoreError::ReaderPoisoned)?;
        records::load(&mut reader.connection)
    }

    /// Commits only if `expected_revision` is current. A lost reply requires reloading the revision.
    pub fn save(
        &self,
        expected_revision: u64,
        snapshot: TabModelSnapshot,
    ) -> Result<u64, StoreError> {
        // A reply acknowledges the committed revision, never merely a queued request.
        snapshot.validate()?;
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
            eprintln!("Atlas store shutdown failed: {error}");
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
    Save(SaveRequest),
    Backup {
        now_utc_seconds: i64,
        idle: bool,
        reply: mpsc::SyncSender<Result<BackupOutcome, StoreError>>,
    },
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
            .name("atlas-store-writer".to_owned())
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
            WriterRequest::Save(request) => {
                let result = records::save(
                    &mut writer.connection,
                    request.expected_revision,
                    &request.snapshot,
                );
                request
                    .reply
                    .send(result)
                    .map_err(|_| StoreError::ReplyAbandoned)?;
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
            ConnectionRole::Reader => schema::prepare_reader(&self.connection)?,
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
