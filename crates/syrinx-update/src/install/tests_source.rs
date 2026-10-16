//! "Check for updates" and staging from a download, against an in-process fake host that can
//! fail, stall, truncate or never end (P1-39c). No test here touches a network.

use std::cell::RefCell;
use std::io::{self, Read};

use super::tests::{
    NEW, OLD, Sandbox, assert_untouched, keys, now, packaged, plenty, program_files, read_tree,
    write_package,
};
use super::*;
use crate::error::UpdateError;
use crate::{FileEntry, KeySet, Version};

/// How the fake host misbehaves for one file.
#[derive(Clone, Copy)]
enum Fault {
    /// The request fails before any byte arrives.
    Refused(TransferProblem),
    /// The body stops after `after` bytes with `problem`.
    Breaks {
        after: usize,
        problem: TransferProblem,
    },
    /// The body never ends.
    Endless,
}

/// Serves a signed package folder file by file, recording each request.
struct FakeHost {
    folder: FolderSource,
    /// The request name that misbehaves, and how.
    fault: Option<(&'static str, Fault)>,
    requests: RefCell<Vec<String>>,
}

impl FakeHost {
    fn new(sandbox: &Sandbox) -> Self {
        Self {
            folder: FolderSource::new(&sandbox.source),
            fault: None,
            requests: RefCell::new(Vec::new()),
        }
    }

    fn failing(sandbox: &Sandbox, name: &'static str, fault: Fault) -> Self {
        Self {
            fault: Some((name, fault)),
            ..Self::new(sandbox)
        }
    }

    fn requested(&self, name: String) -> Option<Fault> {
        let fault = self
            .fault
            .filter(|(failing, _)| *failing == name)
            .map(|(_, fault)| fault);
        self.requests.borrow_mut().push(name);
        fault
    }
}

impl PackageSource for FakeHost {
    fn read_signed(&self, file: SignedFile, limit: usize) -> Result<Vec<u8>, InstallError> {
        match self.requested(file.name().to_owned()) {
            Some(Fault::Refused(problem)) => Err(InstallError::Transfer(problem)),
            Some(_) => Err(InstallError::Transfer(TransferProblem::Truncated)),
            None => self.folder.read_signed(file, limit),
        }
    }

    fn open_file(
        &self,
        version: Version,
        entry: &FileEntry,
    ) -> Result<Box<dyn Read + '_>, InstallError> {
        let fault = self.requested(entry.path().as_str().to_owned());
        match fault {
            Some(Fault::Refused(problem)) => Err(InstallError::Transfer(problem)),
            Some(Fault::Breaks { after, problem }) => {
                let reader = self.folder.open_file(version, entry)?;
                Ok(Box::new(Breaking {
                    inner: reader.take(after as u64),
                    problem,
                }))
            }
            Some(Fault::Endless) => Ok(Box::new(io::repeat(0x5a))),
            None => self.folder.open_file(version, entry),
        }
    }
}

/// A body that delivers its first bytes and then fails as a download would.
struct Breaking<R> {
    inner: R,
    problem: TransferProblem,
}

impl<R: Read> Read for Breaking<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self.inner.read(buffer)? {
            0 => Err(self.problem.into_io()),
            read => Ok(read),
        }
    }
}

fn terms(keys: &KeySet) -> StageTerms<'_> {
    StageTerms {
        keys,
        installed: OLD,
        now: now(),
    }
}

fn package_size() -> u64 {
    program_files(NEW)
        .values()
        .map(|bytes| bytes.len() as u64)
        .sum()
}

fn state(sandbox: &Sandbox) -> UpdateState {
    UpdateState::read(&sandbox.layout).expect("state")
}

#[test]
fn a_check_reads_only_the_statement_and_manifest() {
    let (sandbox, keys) = packaged();
    let host = FakeHost::new(&sandbox);
    let checked = check_source(&sandbox.layout, &host, &terms(&keys.pinned));
    assert_eq!(
        checked,
        Ok(Checked::Available {
            version: NEW,
            bytes: package_size()
        })
    );
    // Nothing larger than the manifest is fetched before the owner says yes.
    assert_eq!(
        *host.requests.borrow(),
        ["latest", "latest.sig", "manifest", "manifest.sig"]
    );
    assert_untouched(&sandbox);
    let state = state(&sandbox);
    assert_eq!(state.result, Some(UpdateResult::Available));
    assert_eq!(state.version, Some(NEW));
    assert_eq!(state.accepted, None, "the serial waits for staging");
}

#[test]
fn a_check_naming_the_installed_version_stores_its_serial() {
    let sandbox = Sandbox::new();
    let keys = keys();
    write_package(&sandbox.source, &keys, OLD, 7, &program_files(OLD));
    let host = FakeHost::new(&sandbox);
    assert_eq!(
        check_source(&sandbox.layout, &host, &terms(&keys.pinned)),
        Ok(Checked::UpToDate(OLD))
    );
    assert_eq!(*host.requests.borrow(), ["latest", "latest.sig"]);
    let state = state(&sandbox);
    assert_eq!(state.result, Some(UpdateResult::UpToDate));
    assert_eq!(
        state.accepted.map(|record| record.statement.serial()),
        Some(7)
    );
}

#[test]
fn production_keys_refuse_every_checked_statement_today() {
    // PRODUCTION_KEYS is empty until #55, so a real check fails closed.
    let (sandbox, _keys) = packaged();
    let production = KeySet::production().expect("empty production set");
    let host = FakeHost::new(&sandbox);
    assert_eq!(
        check_source(&sandbox.layout, &host, &terms(&production)),
        Err(InstallError::Update(UpdateError::UnknownKey))
    );
    assert_eq!(*host.requests.borrow(), ["latest", "latest.sig"]);
    assert_eq!(
        state(&sandbox).result,
        Some(UpdateResult::VerificationFailed)
    );
    assert_untouched(&sandbox);
}

#[test]
fn a_failed_request_during_a_check_is_a_network_failure() {
    let problems = [
        TransferProblem::Unreachable,
        TransferProblem::Certificate,
        TransferProblem::TimedOut,
        TransferProblem::Redirected,
        TransferProblem::Status(404),
        TransferProblem::TooLong,
    ];
    for (name, problem) in ["latest", "latest.sig", "manifest", "manifest.sig"]
        .into_iter()
        .zip(problems)
    {
        let (sandbox, keys) = packaged();
        let host = FakeHost::failing(&sandbox, name, Fault::Refused(problem));
        assert_eq!(
            check_source(&sandbox.layout, &host, &terms(&keys.pinned)),
            Err(InstallError::Transfer(problem)),
            "{name}"
        );
        assert_eq!(state(&sandbox).result, Some(UpdateResult::NetworkFailed));
        assert_untouched(&sandbox);
    }
}

#[test]
fn a_checked_download_stages_beside_the_program() {
    let (sandbox, keys) = packaged();
    let host = FakeHost::new(&sandbox);
    let terms = terms(&keys.pinned);
    check_source(&sandbox.layout, &host, &terms).expect("check");
    let staged = stage_from_source(&sandbox.layout, &host, &terms, &plenty);
    assert_eq!(
        staged,
        Ok(Staged::Ready {
            version: NEW,
            bytes: package_size()
        })
    );
    assert_eq!(read_tree(&sandbox.layout.staged()), program_files(NEW));
    assert!(!sandbox.layout.incoming().exists());
    let state = state(&sandbox);
    assert_eq!(state.result, Some(UpdateResult::Staged));
    assert_eq!(
        state.accepted.map(|record| record.statement.serial()),
        Some(12)
    );
}

#[test]
fn a_broken_download_leaves_the_installed_version_and_cleans_up() {
    let cases = [
        (TransferProblem::Truncated, UpdateResult::NetworkFailed),
        (TransferProblem::TimedOut, UpdateResult::NetworkFailed),
        (TransferProblem::Unreachable, UpdateResult::NetworkFailed),
        (TransferProblem::TooLong, UpdateResult::NetworkFailed),
        (TransferProblem::Cancelled, UpdateResult::Cancelled),
    ];
    for (problem, category) in cases {
        let (sandbox, keys) = packaged();
        let fault = Fault::Breaks { after: 5, problem };
        let host = FakeHost::failing(&sandbox, PROGRAM_EXECUTABLE, fault);
        let staged = stage_from_source(&sandbox.layout, &host, &terms(&keys.pinned), &plenty);
        assert_eq!(staged, Err(InstallError::Transfer(problem)), "{problem:?}");
        assert_untouched(&sandbox);
        let state = state(&sandbox);
        assert_eq!(state.result, Some(category), "{problem:?}");
        assert_eq!(state.accepted, None, "{problem:?} stores no serial");
    }
}

#[test]
fn a_refused_or_endless_payload_request_is_refused() {
    let (sandbox, keys) = packaged();
    let refused = Fault::Refused(TransferProblem::Status(503));
    let host = FakeHost::failing(&sandbox, APPLY_HELPER, refused);
    let staged = stage_from_source(&sandbox.layout, &host, &terms(&keys.pinned), &plenty);
    assert_eq!(
        staged,
        Err(InstallError::Transfer(TransferProblem::Status(503)))
    );
    assert_untouched(&sandbox);

    // Reading stops one byte past the listed size, so an endless body cannot fill the disk.
    let host = FakeHost::failing(&sandbox, PROGRAM_EXECUTABLE, Fault::Endless);
    let staged = stage_from_source(&sandbox.layout, &host, &terms(&keys.pinned), &plenty);
    assert!(
        matches!(
            staged,
            Err(InstallError::Update(UpdateError::SizeMismatch { .. }))
        ),
        "{staged:?}"
    );
    assert_untouched(&sandbox);
}

#[test]
fn a_full_disk_is_refused_before_any_payload_is_requested() {
    let (sandbox, keys) = packaged();
    let host = FakeHost::new(&sandbox);
    let full = stage_from_source(&sandbox.layout, &host, &terms(&keys.pinned), &|_| {
        Ok(SPACE_MARGIN_BYTES)
    });
    assert!(matches!(full, Err(InstallError::NotEnoughSpace { .. })));
    assert_eq!(
        *host.requests.borrow(),
        ["latest", "latest.sig", "manifest", "manifest.sig"]
    );
    assert_untouched(&sandbox);
}

#[test]
fn a_replayed_statement_is_refused_when_checking() {
    let (sandbox, keys) = packaged();
    let host = FakeHost::new(&sandbox);
    let terms = terms(&keys.pinned);
    stage_from_source(&sandbox.layout, &host, &terms, &plenty).expect("serial 12 staged");

    let older = Sandbox::new();
    write_package(&older.source, &keys, NEW, 11, &program_files(NEW));
    let replayed = check_source(&sandbox.layout, &FakeHost::new(&older), &terms);
    assert!(matches!(
        replayed,
        Err(InstallError::Update(UpdateError::StatementReplayed {
            accepted: 12,
            offered: 11
        }))
    ));
}
