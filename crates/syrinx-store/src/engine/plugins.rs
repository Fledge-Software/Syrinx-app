//! The store's plugin records (#136): installs, grants, folders and settings, and the bounded
//! calls a plugin's host services make, which a deadline cannot interrupt.

use std::sync::{MutexGuard, TryLockError, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::{Connection, Transaction, TransactionBehavior};

use super::{LeasedConnection, Store, WriterRequest};
use crate::error::{StoreError, StoreOperation, database_error};
use crate::plugins::{PluginWrite, PluginWritten};
use crate::schema;

/// How long a plugin's bounded write may wait to start, and to finish.
#[derive(Clone, Copy)]
pub(super) struct WriteLimit {
    start_by: Instant,
    deadline: Instant,
}

/// How often a bounded read looks for a free reader.
const READER_POLL: Duration = Duration::from_millis(1);

impl Store {
    /// Every installed plugin, by id.
    pub fn plugin_installs(&self) -> Result<Vec<crate::PluginInstall>, StoreError> {
        self.read(|connection| crate::plugins::installs(connection))
    }

    /// Every capability granted to every plugin, read at start-up.
    pub fn plugin_grants(&self) -> Result<Vec<crate::StoredGrant>, StoreError> {
        self.read(|connection| crate::plugins::grants(connection))
    }

    /// The folders granted to `plugin_id`. Their paths are owner data.
    pub fn plugin_folders(&self, plugin_id: &str) -> Result<Vec<crate::PluginFolder>, StoreError> {
        self.read(|connection| crate::plugins::folders(connection, plugin_id))
    }

    /// One key from `plugin_id`'s own settings namespace.
    pub fn plugin_setting(
        &self,
        plugin_id: &str,
        key: &str,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        self.read(|connection| crate::plugins::setting(connection, plugin_id, key))
    }

    /// Records an installed or updated plugin; its grants and settings are kept.
    pub fn save_plugin_install(&self, install: crate::PluginInstall) -> Result<(), StoreError> {
        self.write_plugin(PluginWrite::Install(install), None)
            .map(drop)
    }

    /// Grants `plugin_id` a capability, or replaces the grant's scope; returns the grant's id,
    /// which stays the same across a re-grant.
    pub fn grant_plugin(
        &self,
        plugin_id: String,
        grant: crate::PluginGrant,
    ) -> Result<i64, StoreError> {
        match self.write_plugin(PluginWrite::Grant { plugin_id, grant }, None)? {
            PluginWritten::Grant(grant_id) => Ok(grant_id),
            PluginWritten::Saved | PluginWritten::Removed(_) => Err(StoreError::ReplyAbandoned),
        }
    }

    /// Withdraws one capability and the folders granted under it; `false` if none was granted.
    pub fn revoke_plugin_grant(
        &self,
        plugin_id: String,
        capability: String,
    ) -> Result<bool, StoreError> {
        self.removed(PluginWrite::Revoke {
            plugin_id,
            capability,
        })
    }

    /// Adds a folder under one of `plugin_id`'s grants. The caller has applied the deny list.
    pub fn add_plugin_folder(
        &self,
        plugin_id: String,
        folder: crate::PluginFolder,
    ) -> Result<(), StoreError> {
        self.write_plugin(PluginWrite::Folder { plugin_id, folder }, None)
            .map(drop)
    }

    /// Saves one key in `plugin_id`'s own settings namespace.
    pub fn save_plugin_setting(
        &self,
        plugin_id: String,
        key: String,
        value: Vec<u8>,
    ) -> Result<(), StoreError> {
        let write = PluginWrite::Setting {
            plugin_id,
            key,
            value,
        };
        self.write_plugin(write, None).map(drop)
    }

    /// Removes `plugin_id` with its grants, folders and settings in one statement; `false` if
    /// it was not installed.
    pub fn remove_plugin(&self, plugin_id: String) -> Result<bool, StoreError> {
        self.removed(PluginWrite::Remove { plugin_id })
    }

    /// As `plugin_setting`, waiting at most `budget` for a reader and for SQLite; past it,
    /// `StoreError::Busy`. For a plugin's host call, which a deadline cannot interrupt.
    pub fn plugin_setting_within(
        &self,
        plugin_id: &str,
        key: &str,
        budget: Duration,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        let deadline = Instant::now() + budget;
        let reader = self.reader_by(deadline)?;
        bounded(&reader.connection, deadline, |connection| {
            crate::plugins::setting(connection, plugin_id, key)
        })
    }

    /// As `save_plugin_setting`, answering within `budget`. `Busy` is true: the writer starts
    /// the write only within the first half of the budget, and rolls it back if the budget
    /// has passed when it is ready to commit.
    pub fn save_plugin_setting_within(
        &self,
        plugin_id: String,
        key: String,
        value: Vec<u8>,
        budget: Duration,
    ) -> Result<(), StoreError> {
        let write = PluginWrite::Setting {
            plugin_id,
            key,
            value,
        };
        let now = Instant::now();
        let limit = WriteLimit {
            start_by: now + budget / 2,
            deadline: now + budget,
        };
        self.write_plugin(write, Some(limit)).map(drop)
    }

    fn removed(&self, write: PluginWrite) -> Result<bool, StoreError> {
        match self.write_plugin(write, None)? {
            PluginWritten::Removed(removed) => Ok(removed),
            PluginWritten::Saved | PluginWritten::Grant(_) => Err(StoreError::ReplyAbandoned),
        }
    }

    /// The one path for plugin writes. An unbounded write waits for the writer as every
    /// owner's write does; a bounded one never waits for a full queue or past its deadline.
    fn write_plugin(
        &self,
        write: PluginWrite,
        limit: Option<WriteLimit>,
    ) -> Result<PluginWritten, StoreError> {
        crate::plugins::validate_write(&write)?;
        let writer = self.writer.as_ref().ok_or(StoreError::Closed)?;
        let (reply, result) = mpsc::sync_channel(1);
        let request = WriterRequest::Plugin {
            write,
            limit,
            reply,
        };
        let Some(limit) = limit else {
            writer
                .sender
                .send(request)
                .map_err(|_| StoreError::WorkerUnavailable)?;
            return result.recv().map_err(|_| StoreError::WorkerUnavailable)?;
        };
        match writer.sender.try_send(request) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => return Err(StoreError::Busy),
            Err(mpsc::TrySendError::Disconnected(_)) => return Err(StoreError::WorkerUnavailable),
        }
        let left = limit.deadline.saturating_duration_since(Instant::now());
        match result.recv_timeout(left) {
            Ok(written) => written,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(StoreError::Busy),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(StoreError::WorkerUnavailable),
        }
    }

    /// The first reader free before `deadline`, trying each in turn.
    fn reader_by(&self, deadline: Instant) -> Result<MutexGuard<'_, LeasedConnection>, StoreError> {
        let readers = self.readers.as_ref().ok_or(StoreError::Closed)?;
        loop {
            for reader in readers {
                match reader.try_lock() {
                    Ok(guard) => return Ok(guard),
                    Err(TryLockError::Poisoned(_)) => return Err(StoreError::ReaderPoisoned),
                    Err(TryLockError::WouldBlock) => {}
                }
            }
            if Instant::now() >= deadline {
                return Err(StoreError::Busy);
            }
            thread::sleep(READER_POLL);
        }
    }
}

/// Run `query` with SQLite's busy wait cut to what is left before `deadline`, then restore
/// it. A failed restore is logged and never replaces the query's own result, which may be a
/// committed write.
fn bounded<T>(
    connection: &Connection,
    deadline: Instant,
    query: impl FnOnce(&Connection) -> Result<T, StoreError>,
) -> Result<T, StoreError> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(StoreError::Busy);
    }
    connection
        .busy_timeout(left)
        .map_err(|error| database_error(StoreOperation::Configure, error))?;
    let result = query(connection).map_err(busy_as_busy);
    if let Err(error) = connection.busy_timeout(schema::BUSY_TIMEOUT) {
        let error = database_error(StoreOperation::Configure, error);
        eprintln!("Syrinx could not restore the store's busy timeout: {error}");
    }
    result
}

/// SQLite's own busy answer, once the cut wait ran out, is the bounded call's `Busy`.
fn busy_as_busy(error: StoreError) -> StoreError {
    match error {
        StoreError::Database {
            code: Some(code), ..
        } if code & 0xff == SQLITE_BUSY => StoreError::Busy,
        error => error,
    }
}

/// SQLite's primary result code for a lock held by another connection.
const SQLITE_BUSY: i32 = 5;

/// Apply a plugin write on the writer connection. A bounded one is refused unstarted once
/// its start time has passed, and otherwise runs in its own transaction: only taking the
/// write lock waits, and the deadline is checked again before COMMIT, so a write that
/// overran is rolled back and its caller's `Busy` is true. Only the COMMIT's own disk write
/// can still end after the deadline.
pub(super) fn write_within(
    connection: &Connection,
    write: &PluginWrite,
    limit: Option<WriteLimit>,
) -> Result<PluginWritten, StoreError> {
    let Some(limit) = limit else {
        return crate::plugins::write(connection, write);
    };
    if Instant::now() > limit.start_by {
        return Err(StoreError::Busy);
    }
    bounded(connection, limit.deadline, |connection| {
        let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
            .map_err(|error| database_error(StoreOperation::BeginWrite, error))?;
        let written = crate::plugins::write(&transaction, write)?;
        commit_by(transaction, limit.deadline)?;
        Ok(written)
    })
}

/// Commit `transaction` only while `deadline` has not passed; otherwise roll it back and
/// answer `Busy`, so a caller told `Busy` is never wrong about its write.
pub(super) fn commit_by(transaction: Transaction<'_>, deadline: Instant) -> Result<(), StoreError> {
    if Instant::now() >= deadline {
        transaction
            .rollback()
            .map_err(|error| database_error(StoreOperation::Rollback, error))?;
        return Err(StoreError::Busy);
    }
    transaction
        .commit()
        .map_err(|error| database_error(StoreOperation::Commit, error))
}
