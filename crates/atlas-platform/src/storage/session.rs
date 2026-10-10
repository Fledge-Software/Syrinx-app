//! Keeps lifecycle ownership and clean-shutdown evidence outside the replaceable database.

use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use atlas_store::{
    LocationError, LocationOperation, SessionControl, ShutdownState, StorageSession,
};

use super::{GuardedEntry, check_entry, native};

pub(super) const CONTROL_NAME: &str = "atlas.lifecycle";
pub(super) const RECOVERY_INTENT_NAME: &str = "atlas.recovery-intent";
const CLEAN_RECORD: &[u8] = b"Atlas storage lifecycle 1: clean\n";
const DIRTY_RECORD: &[u8] = b"Atlas storage lifecycle 1: dirty\n";

#[derive(Clone)]
pub(super) struct LifecycleControl {
    state: Arc<LifecycleState>,
}

struct LifecycleState {
    entry: GuardedEntry,
    directory: PathBuf,
    directory_guards: Arc<Vec<GuardedEntry>>,
    active: AtomicBool,
    poisoned: AtomicBool,
}

struct SessionToken {
    state: Arc<LifecycleState>,
    previous: ShutdownState,
}

impl LifecycleControl {
    pub(super) fn acquire(
        directory: &Path,
        directory_guards: Arc<Vec<GuardedEntry>>,
    ) -> Result<Self, LocationError> {
        // Inspect existing control entries without following a link or changing bytes.
        let path = directory.join(CONTROL_NAME);
        if let Some(file) = native::optional_file(&path, LocationOperation::OpenControl)? {
            check_entry(&native::entry_info(&file)?, false)?;
        }
        let file = native::open_control(&path)?;
        let entry = GuardedEntry::regular_file(path, file)?;
        Ok(Self {
            state: Arc::new(LifecycleState {
                entry,
                directory: directory.to_owned(),
                directory_guards,
                active: AtomicBool::new(false),
                poisoned: AtomicBool::new(false),
            }),
        })
    }

    pub(super) fn revalidate(&self) -> Result<(), LocationError> {
        self.state.revalidate()
    }

    pub(super) fn revalidate_location(&self) -> Result<(), LocationError> {
        self.state.revalidate_location()
    }

    fn begin_token(&self) -> Result<Box<SessionToken>, LocationError> {
        // The owned token releases the claim on every failure without writing clean state.
        self.state
            .active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| LocationError::SessionAlreadyActive)?;
        let mut token = Box::new(SessionToken {
            state: Arc::clone(&self.state),
            previous: ShutdownState::Unclean,
        });
        if self.state.poisoned.load(Ordering::Acquire) {
            return Err(LocationError::ControlPoisoned);
        }
        self.state.revalidate()?;
        token.previous = self.state.read_previous()?;
        self.state.write_record(DIRTY_RECORD)?;
        Ok(token)
    }
}

impl SessionControl for LifecycleControl {
    fn begin(&self) -> Result<Box<dyn StorageSession>, LocationError> {
        Ok(self.begin_token()?)
    }
}

impl StorageSession for SessionToken {
    fn previous_shutdown(&self) -> ShutdownState {
        self.previous
    }

    fn complete(self: Box<Self>) -> Result<(), LocationError> {
        (*self).complete_with_flush(File::sync_all)
    }
}

impl SessionToken {
    fn complete_with_flush(
        self,
        flush: impl FnOnce(&File) -> io::Result<()>,
    ) -> Result<(), LocationError> {
        // The store calls this only after all connections close and the writer joins.
        self.state.revalidate()?;
        self.state.write_record_with_flush(CLEAN_RECORD, flush)
    }
}

impl Drop for SessionToken {
    fn drop(&mut self) {
        // Unwinding and fallback drop release ownership without fallible filesystem work.
        self.state.active.store(false, Ordering::Release);
    }
}

impl LifecycleState {
    fn revalidate(&self) -> Result<(), LocationError> {
        self.revalidate_location()?;
        refuse_recovery_intent(&self.directory)
    }

    fn revalidate_location(&self) -> Result<(), LocationError> {
        // Tokens retain the directory guards even if the caller drops its lease first.
        for directory in self.directory_guards.iter() {
            directory.revalidate(true, LocationOperation::OpenDirectory)?;
        }
        self.entry.revalidate(false, LocationOperation::OpenControl)
    }

    fn read_previous(&self) -> Result<ShutdownState, LocationError> {
        // The active claim serializes the shared file cursor; parsing is fixed-size.
        let mut file = &self.entry.file;
        let length = file
            .metadata()
            .map_err(|error| native::io_error(LocationOperation::ReadControl, error))?
            .len();
        if length != CLEAN_RECORD.len() as u64 {
            return Ok(ShutdownState::Unclean);
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|error| native::io_error(LocationOperation::ReadControl, error))?;
        let mut record = [0; CLEAN_RECORD.len()];
        match file.read_exact(&mut record) {
            Ok(()) if record == CLEAN_RECORD => Ok(ShutdownState::Clean),
            Ok(()) => Ok(ShutdownState::Unclean),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                Ok(ShutdownState::Unclean)
            }
            Err(error) => Err(native::io_error(LocationOperation::ReadControl, error)),
        }
    }

    fn write_record(&self, record: &[u8]) -> Result<(), LocationError> {
        self.write_record_with_flush(record, File::sync_all)
    }

    fn write_record_with_flush(
        &self,
        record: &[u8],
        flush: impl FnOnce(&File) -> io::Result<()>,
    ) -> Result<(), LocationError> {
        let result = persist_record(&self.entry.file, record, flush);
        if result.is_err() {
            // Failed flushes may leave cached clean bytes; this control cannot be reused.
            // A flush error supplies no guarantee about the record after a later reopen.
            self.poisoned.store(true, Ordering::Release);
        }
        result
    }
}

fn persist_record(
    mut file: &File,
    record: &[u8],
    flush: impl FnOnce(&File) -> io::Result<()>,
) -> Result<(), LocationError> {
    // Flush the complete fixed record before the caller may act on its status.
    file.seek(SeekFrom::Start(0))
        .and_then(|_| file.write_all(record))
        .and_then(|_| file.set_len(record.len() as u64))
        .map_err(|error| native::io_error(LocationOperation::WriteControl, error))?;
    flush(file).map_err(|error| native::io_error(LocationOperation::FlushControl, error))
}

fn refuse_recovery_intent(directory: &Path) -> Result<(), LocationError> {
    // Any intent needs reconciliation before a missing primary may be created.
    let path = directory.join(RECOVERY_INTENT_NAME);
    let Some(file) = native::optional_file(&path, LocationOperation::OpenRecoveryIntent)? else {
        return Ok(());
    };
    check_entry(&native::entry_info(&file)?, false)?;
    Err(LocationError::RecoveryPending)
}

#[cfg(test)]
mod tests {
    use std::fs::{self, OpenOptions};

    use super::*;
    use crate::storage::tests::Fixture;

    fn control_with_access(fixture: &Fixture, read: bool, write: bool) -> LifecycleControl {
        // Restricted test handles exercise native I/O failures without changing permissions.
        let path = fixture.root.join(CONTROL_NAME);
        fs::write(&path, CLEAN_RECORD).expect("write synthetic lifecycle record");
        let file = OpenOptions::new()
            .read(read)
            .write(write)
            .open(&path)
            .expect("open restricted synthetic handle");
        LifecycleControl {
            state: Arc::new(LifecycleState {
                entry: GuardedEntry::regular_file(path, file).expect("check synthetic control"),
                directory: fixture.root.clone(),
                directory_guards: Arc::new(vec![
                    GuardedEntry::directory(&fixture.root).expect("retain synthetic directory"),
                ]),
                active: AtomicBool::new(false),
                poisoned: AtomicBool::new(false),
            }),
        }
    }

    #[test]
    fn control_read_errors_are_not_malformed_record_successes() {
        let fixture = Fixture::new();
        let control = control_with_access(&fixture, false, true);
        for _ in 0..2 {
            assert!(matches!(
                control.begin(),
                Err(LocationError::Os {
                    operation: LocationOperation::ReadControl,
                    ..
                })
            ));
        }
        assert!(!control.state.active.load(Ordering::Acquire));
    }

    #[test]
    fn failed_dirty_writes_never_return_a_session_or_retain_its_claim() {
        let fixture = Fixture::new();
        let control = control_with_access(&fixture, true, false);
        assert!(matches!(
            control.begin(),
            Err(LocationError::Os {
                operation: LocationOperation::WriteControl,
                ..
            })
        ));
        assert!(matches!(
            control.begin(),
            Err(LocationError::ControlPoisoned)
        ));
        assert!(!control.state.active.load(Ordering::Acquire));
        assert_eq!(
            fs::read(fixture.root.join(CONTROL_NAME)).expect("read synthetic unchanged marker"),
            CLEAN_RECORD
        );
    }

    #[test]
    fn failed_completion_flush_surfaces_the_error_and_poisoned_control() {
        let fixture = Fixture::new();
        let control = control_with_access(&fixture, true, true);
        let token = control.begin_token().expect("start synthetic lifecycle");
        let result = (*token).complete_with_flush(|_| Err(io::Error::from_raw_os_error(5)));
        assert_eq!(
            result,
            Err(LocationError::Os {
                operation: LocationOperation::FlushControl,
                code: Some(5),
            })
        );
        assert!(!control.state.active.load(Ordering::Acquire));
        assert!(matches!(
            control.begin(),
            Err(LocationError::ControlPoisoned)
        ));

        // A cached clean record is not enough to permit reuse after a failed flush.
        assert_eq!(
            fs::read(fixture.root.join(CONTROL_NAME)).expect("read synthetic cached record"),
            CLEAN_RECORD
        );
    }
}
