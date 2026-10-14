//! Staging and recovery tests on synthetic packages in fresh folders under the workspace's
//! `target`, signed by keys generated in-process.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::fault::{Step, hook};
use super::*;
use crate::cng::test_keys::TestKeyPair;
use crate::error::UpdateError;
use crate::{
    FileEntry, KeySet, Manifest, PackagePath, PinnedKey, SECONDS_PER_DAY, StatementTerms,
    Timestamp, Version, cng, sign_release,
};

pub(super) const OLD: Version = Version::new(1, 3, 0);
pub(super) const NEW: Version = Version::new(1, 4, 0);

static NEXT_SANDBOX: AtomicU32 = AtomicU32::new(0);

/// A fresh `Programs` and data folder under `target`, removed when the test ends.
pub(super) struct Sandbox {
    root: PathBuf,
    pub(super) layout: UpdateLayout,
    pub(super) source: PathBuf,
}

impl Sandbox {
    pub(super) fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        let name = format!(
            "{}-{}-{nanos}",
            std::process::id(),
            NEXT_SANDBOX.fetch_add(1, Ordering::Relaxed)
        );
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(r"..\..\target\syrinx-update-tests")
            .join(name);
        // A fresh name only: the test owns this folder and nothing outside it.
        fs::create_dir_all(root.parent().expect("sandbox parent")).expect("sandbox parent");
        fs::create_dir(&root).expect("fresh sandbox");
        let programs = root.join("Programs");
        let data = root.join("data");
        let source = root.join("package");
        for folder in [&programs, &data, &source] {
            fs::create_dir(folder).expect("sandbox folder");
        }
        let layout = UpdateLayout::new(programs, data).expect("synthetic layout");
        let sandbox = Self {
            root,
            layout,
            source,
        };
        write_tree(&sandbox.layout.program(), &program_files(OLD));
        sandbox
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root)
            && !std::thread::panicking()
        {
            panic!("remove the test's own sandbox: {error}");
        }
    }
}

/// The files of a synthetic Syrinx `version`; contents name the version.
pub(super) fn program_files(version: Version) -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([
        (
            PACKAGE_RECORD.to_owned(),
            format!("package {version}").into_bytes(),
        ),
        (
            PROGRAM_EXECUTABLE.to_owned(),
            format!("synthetic syrinx {version}")
                .into_bytes()
                .repeat(40),
        ),
        (
            APPLY_HELPER.to_owned(),
            format!("synthetic helper {version}").into_bytes(),
        ),
        ("resources/notes.txt".to_owned(), b"made-up text\n".to_vec()),
    ])
}

pub(super) fn write_tree(root: &Path, files: &BTreeMap<String, Vec<u8>>) {
    for (path, bytes) in files {
        let target = path
            .split('/')
            .fold(root.to_path_buf(), |joined, part| joined.join(part));
        fs::create_dir_all(target.parent().expect("parent")).expect("synthetic folders");
        fs::write(target, bytes).expect("synthetic file");
    }
}

/// Every file under `root`, by `/`-separated path.
pub(super) fn read_tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((folder, prefix)) = pending.pop() {
        for entry in fs::read_dir(&folder).expect("list synthetic folder") {
            let entry = entry.expect("synthetic entry");
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.file_type().expect("entry type").is_dir() {
                pending.push((entry.path(), format!("{name}/")));
            } else {
                files.insert(name, fs::read(entry.path()).expect("read synthetic file"));
            }
        }
    }
    files
}

pub(super) fn issued() -> Timestamp {
    Timestamp::parse("2026-11-02T10:00:00Z").expect("canonical")
}

pub(super) fn now() -> Timestamp {
    issued().checked_add_seconds(3600).expect("in range")
}

/// An in-memory signing key and the key set that pins it.
pub(super) struct Keys {
    pub(super) signer: TestKeyPair,
    pub(super) pinned: KeySet,
}

pub(super) fn keys() -> Keys {
    let signer = TestKeyPair::generate().expect("test key");
    let pinned = KeySet::new(&[PinnedKey::new(signer.public_point().expect("point"))])
        .expect("test key set");
    Keys { signer, pinned }
}

pub(super) fn entries(files: &BTreeMap<String, Vec<u8>>) -> Vec<FileEntry> {
    files
        .iter()
        .map(|(path, bytes)| {
            FileEntry::new(
                PackagePath::parse(path).expect("synthetic path"),
                bytes.len() as u64,
                cng::sha256(bytes).expect("hash"),
            )
        })
        .collect()
}

/// Write a signed package for `files` into `folder`, as `cargo xtask package` and `sign-update` do.
pub(super) fn write_package(
    folder: &Path,
    keys: &Keys,
    version: Version,
    serial: u64,
    files: &BTreeMap<String, Vec<u8>>,
) {
    let manifest = Manifest::new(version, Version::new(1, 0, 0), entries(files))
        .expect("synthetic manifest")
        .to_bytes();
    let terms = StatementTerms {
        serial,
        issued: issued(),
        expires: issued()
            .checked_add_seconds(30 * SECONDS_PER_DAY)
            .expect("in range"),
    };
    let signed = sign_release(&manifest, &keys.signer, &keys.pinned, terms).expect("sign");
    write_tree(folder, files);
    fs::write(folder.join("manifest"), &manifest).expect("manifest");
    fs::write(folder.join("manifest.sig"), signed.manifest_signature).expect("signature");
    fs::write(folder.join("latest"), &signed.statement).expect("statement");
    fs::write(folder.join("latest.sig"), signed.statement_signature).expect("signature");
}

pub(super) fn plenty(_: &Path) -> io::Result<u64> {
    Ok(u64::MAX)
}

pub(super) fn stage(sandbox: &Sandbox, keys: &KeySet) -> Result<Staged, InstallError> {
    let request = StageRequest {
        source: &sandbox.source,
        keys,
        installed: OLD,
        now: now(),
    };
    stage_from_folder(&sandbox.layout, &request, &plenty)
}

/// A sandbox holding a signed package for `NEW`.
pub(super) fn packaged() -> (Sandbox, Keys) {
    let sandbox = Sandbox::new();
    let keys = keys();
    write_package(&sandbox.source, &keys, NEW, 12, &program_files(NEW));
    (sandbox, keys)
}

/// The installed version untouched, and nothing staged or left in `incoming`.
pub(super) fn assert_untouched(sandbox: &Sandbox) {
    let layout = &sandbox.layout;
    assert_eq!(read_tree(&layout.program()), program_files(OLD));
    assert!(!layout.staged().exists(), "no staged copy");
    assert!(!layout.pending().exists(), "no pending record");
    assert!(!layout.incoming().exists(), "incoming cleaned up");
}

fn refusal(result: Result<Staged, InstallError>) -> UpdateError {
    match result {
        Err(InstallError::Update(error)) => error,
        other => panic!("expected a verifier refusal, got {other:?}"),
    }
}

#[test]
fn stages_a_verified_package_beside_the_program() {
    let (sandbox, keys) = packaged();
    let staged = stage(&sandbox, &keys.pinned).expect("stage");
    let layout = &sandbox.layout;
    let expected = program_files(NEW);
    let bytes = expected.values().map(|bytes| bytes.len() as u64).sum();
    assert_eq!(
        staged,
        Staged::Ready {
            version: NEW,
            bytes
        }
    );
    assert_eq!(read_tree(&layout.staged()), expected);
    assert_eq!(read_tree(&layout.program()), program_files(OLD));
    assert_eq!(
        fs::read(layout.helper()).expect("helper"),
        expected[APPLY_HELPER]
    );
    assert_eq!(
        fs::read(layout.apply_manifest()).expect("kept manifest"),
        fs::read(sandbox.source.join("manifest")).expect("source manifest")
    );
    assert!(!layout.incoming().exists());
    let pending = Pending::read(layout).expect("read").expect("pending");
    assert_eq!((pending.version, pending.from), (NEW, OLD));
    let state = UpdateState::read(layout).expect("state");
    assert_eq!(
        state.accepted.map(|record| record.statement.serial()),
        Some(12)
    );
    assert_eq!(state.result, Some(UpdateResult::Staged));
    assert_eq!(state.version, Some(NEW));
}

#[test]
fn production_keys_refuse_every_package_today() {
    // PRODUCTION_KEYS is empty until DW-30, so installing from a file fails closed.
    let (sandbox, _keys) = packaged();
    let production = KeySet::production().expect("empty production set");
    assert_eq!(
        refusal(stage(&sandbox, &production)),
        UpdateError::UnknownKey
    );
    assert_untouched(&sandbox);
    let state = UpdateState::read(&sandbox.layout).expect("state");
    assert_eq!(state.accepted, None);
    assert_eq!(state.result, Some(UpdateResult::VerificationFailed));
}

#[test]
fn a_statement_naming_the_installed_version_is_up_to_date() {
    let sandbox = Sandbox::new();
    let keys = keys();
    write_package(&sandbox.source, &keys, OLD, 4, &program_files(OLD));
    assert_eq!(stage(&sandbox, &keys.pinned), Ok(Staged::UpToDate(OLD)));
    assert_untouched(&sandbox);
    let state = UpdateState::read(&sandbox.layout).expect("state");
    assert_eq!(
        state.accepted.map(|record| record.statement.serial()),
        Some(4)
    );
    assert_eq!(state.result, Some(UpdateResult::UpToDate));
}

/// Change one byte of a file in the package folder.
fn flip(path: &Path, index: usize) {
    let mut bytes = fs::read(path).expect("read");
    bytes[index] ^= 0x01;
    fs::write(path, bytes).expect("write");
}

#[test]
fn tampered_packages_leave_the_installed_version_and_clean_up() {
    type Tamper = fn(&Sandbox);
    type Expected = fn(&UpdateError) -> bool;
    let cases: [(&str, Tamper, Expected); 7] = [
        (
            "statement signature",
            |sandbox| flip(&sandbox.source.join("latest.sig"), 40),
            |error| *error == UpdateError::BadSignature,
        ),
        (
            "manifest signature",
            |sandbox| flip(&sandbox.source.join("manifest.sig"), 40),
            |error| *error == UpdateError::ManifestNotStated || *error == UpdateError::BadSignature,
        ),
        (
            "manifest bytes",
            |sandbox| flip(&sandbox.source.join("manifest"), 30),
            |error| *error == UpdateError::ManifestNotStated,
        ),
        (
            "payload hash",
            |sandbox| flip(&sandbox.source.join(PROGRAM_EXECUTABLE), 3),
            |error| matches!(error, UpdateError::HashMismatch { .. }),
        ),
        (
            "truncated payload",
            |sandbox| fs::write(sandbox.source.join(PROGRAM_EXECUTABLE), b"short").expect("write"),
            |error| matches!(error, UpdateError::SizeMismatch { .. }),
        ),
        (
            "oversized payload",
            |sandbox| {
                let path = sandbox.source.join("resources").join("notes.txt");
                fs::write(path, b"made-up text\nand more\n").expect("write");
            },
            |error| matches!(error, UpdateError::SizeMismatch { .. }),
        ),
        (
            "missing payload",
            |sandbox| fs::remove_file(sandbox.source.join(APPLY_HELPER)).expect("remove"),
            |error| matches!(error, UpdateError::MissingFile { .. }),
        ),
    ];
    for (name, tamper, expected) in cases {
        let (sandbox, keys) = packaged();
        tamper(&sandbox);
        let error = refusal(stage(&sandbox, &keys.pinned));
        assert!(expected(&error), "{name}: {error:?}");
        assert_untouched(&sandbox);
        let state = UpdateState::read(&sandbox.layout).expect("state");
        assert_eq!(state.accepted, None, "{name} stores no serial");
    }
}

#[test]
fn an_unknown_key_an_expired_statement_and_a_missing_package_are_refused() {
    let (sandbox, keys) = packaged();
    let stranger = self::keys();
    write_package(&sandbox.source, &stranger, NEW, 12, &program_files(NEW));
    assert_eq!(
        refusal(stage(&sandbox, &keys.pinned)),
        UpdateError::UnknownKey
    );
    assert_untouched(&sandbox);

    write_package(&sandbox.source, &keys, NEW, 12, &program_files(NEW));
    let request = StageRequest {
        source: &sandbox.source,
        keys: &keys.pinned,
        installed: OLD,
        now: issued()
            .checked_add_seconds(30 * SECONDS_PER_DAY)
            .expect("in range"),
    };
    let expired = stage_from_folder(&sandbox.layout, &request, &plenty);
    assert!(matches!(
        refusal(expired),
        UpdateError::StatementExpired { .. }
    ));
    let state = UpdateState::read(&sandbox.layout).expect("state");
    assert_eq!(state.result, Some(UpdateResult::StatementExpired));
    assert_untouched(&sandbox);

    fs::remove_file(sandbox.source.join("latest")).expect("remove");
    assert_eq!(
        stage(&sandbox, &keys.pinned),
        Err(InstallError::NotAPackage)
    );
    assert_untouched(&sandbox);
}

#[test]
fn a_lower_or_reused_serial_is_refused_after_one_was_accepted() {
    let (sandbox, keys) = packaged();
    stage(&sandbox, &keys.pinned).expect("serial 12 staged");

    let older = Sandbox::new();
    write_package(&older.source, &keys, NEW, 11, &program_files(NEW));
    let request = StageRequest {
        source: &older.source,
        keys: &keys.pinned,
        installed: OLD,
        now: now(),
    };
    assert!(matches!(
        refusal(stage_from_folder(&sandbox.layout, &request, &plenty)),
        UpdateError::StatementReplayed {
            accepted: 12,
            offered: 11
        }
    ));

    // Serial 12 again, but naming a different manifest.
    let mut other_files = program_files(NEW);
    other_files.insert("resources/extra.txt".to_owned(), b"extra".to_vec());
    let reused = Sandbox::new();
    write_package(&reused.source, &keys, NEW, 12, &other_files);
    let request = StageRequest {
        source: &reused.source,
        ..request
    };
    assert_eq!(
        refusal(stage_from_folder(&sandbox.layout, &request, &plenty)),
        UpdateError::StatementSerialReused { serial: 12 }
    );
    // The refused attempts discarded nothing they did not replace: the earlier staging stays.
    assert_eq!(read_tree(&sandbox.layout.staged()), program_files(NEW));
}

#[test]
fn a_full_disk_and_an_incomplete_package_are_refused_before_writing() {
    let (sandbox, keys) = packaged();
    let request = StageRequest {
        source: &sandbox.source,
        keys: &keys.pinned,
        installed: OLD,
        now: now(),
    };
    let full = stage_from_folder(&sandbox.layout, &request, &|_| Ok(SPACE_MARGIN_BYTES));
    assert!(matches!(full, Err(InstallError::NotEnoughSpace { .. })));
    assert_untouched(&sandbox);

    let incomplete = Sandbox::new();
    let mut files = program_files(NEW);
    files.remove(APPLY_HELPER);
    write_package(&incomplete.source, &keys, NEW, 13, &files);
    assert_eq!(
        stage(&incomplete, &keys.pinned),
        Err(InstallError::MissingProgramFile { name: APPLY_HELPER })
    );
    assert_untouched(&incomplete);
}

#[test]
fn an_unreadable_state_refuses_staging_rather_than_forgetting_the_serial() {
    let (sandbox, keys) = packaged();
    fs::create_dir_all(sandbox.layout.updates()).expect("updates");
    fs::write(sandbox.layout.state(), b"not a state record\n").expect("corrupt state");
    assert_eq!(
        stage(&sandbox, &keys.pinned),
        Err(InstallError::Record {
            what: "update state"
        })
    );
    assert_untouched(&sandbox);
}

#[test]
fn staging_again_replaces_the_earlier_staged_copy() {
    let (sandbox, keys) = packaged();
    stage(&sandbox, &keys.pinned).expect("first staging");
    let newer = Version::new(1, 5, 0);
    let next = Sandbox::new();
    write_package(&next.source, &keys, newer, 13, &program_files(newer));
    let request = StageRequest {
        source: &next.source,
        keys: &keys.pinned,
        installed: OLD,
        now: now(),
    };
    let staged = stage_from_folder(&sandbox.layout, &request, &plenty).expect("restage");
    assert!(matches!(staged, Staged::Ready { version, .. } if version == newer));
    assert_eq!(read_tree(&sandbox.layout.staged()), program_files(newer));
}

#[test]
fn a_staging_stopped_at_any_step_is_cleared_by_the_next_start() {
    for step in [
        Step::StagingCleared,
        Step::CopiedToIncoming,
        Step::FirstStagedFile,
        Step::StagedCopied,
        Step::HelperCopied,
        Step::IncomingRemoved,
    ] {
        let (sandbox, keys) = packaged();
        hook::stop_at(Some(step));
        assert_eq!(
            stage(&sandbox, &keys.pinned),
            Err(InstallError::Interrupted(step))
        );
        let recovery = recover(&sandbox.layout, OLD).expect("next start");
        assert_eq!(recovery.staged, None, "{step:?}");
        assert_untouched(&sandbox);
        // A clean retry still works afterwards.
        assert!(matches!(
            stage(&sandbox, &keys.pinned),
            Ok(Staged::Ready { .. })
        ));
    }
}

#[test]
fn a_start_keeps_a_complete_staged_copy_and_refuses_strangers() {
    let (sandbox, keys) = packaged();
    stage(&sandbox, &keys.pinned).expect("stage");
    let recovery = recover(&sandbox.layout, OLD).expect("start");
    assert_eq!(recovery.staged, Some(NEW));
    assert!(sandbox.layout.staged().exists());

    // A different running version means the staged copy is stale.
    let recovery = recover(&sandbox.layout, Version::new(1, 3, 5)).expect("start");
    assert_eq!(recovery.staged, None);
    assert!(!sandbox.layout.staged().exists());

    // A `Syrinx.new` without Syrinx's staging mark is left alone: it may be the owner's own
    // folder, or `install-syrinx.ps1`'s copy in progress, which holds the same package record.
    write_tree(&sandbox.layout.staged(), &program_files(NEW));
    write_tree(
        &sandbox.layout.staged(),
        &BTreeMap::from([("owner-file.txt".to_owned(), b"keep".to_vec())]),
    );
    let recovery = recover(&sandbox.layout, OLD).expect("start");
    assert_eq!(recovery.staged, None);
    assert!(sandbox.layout.staged().join("owner-file.txt").exists());
    assert!(sandbox.layout.staged().join(PACKAGE_RECORD).exists());
}

#[test]
fn a_staged_copy_without_its_record_or_the_staging_mark_is_left_alone() {
    // Staging leaves its mark only until `pending` is written.
    let (sandbox, keys) = packaged();
    stage(&sandbox, &keys.pinned).expect("stage");
    assert!(!sandbox.layout.staging_marker().exists());
    fs::remove_file(sandbox.layout.pending()).expect("simulate a lost record");
    let recovery = recover(&sandbox.layout, OLD).expect("start");
    assert_eq!(recovery.staged, None);
    assert!(sandbox.layout.staged().exists(), "unmarked, so left alone");
}

#[test]
fn staging_refuses_while_the_helper_holds_the_lock() {
    let (sandbox, keys) = packaged();
    let lock = ApplyLock::try_acquire(&sandbox.layout)
        .expect("lock")
        .expect("free");
    assert_eq!(stage(&sandbox, &keys.pinned), Err(InstallError::Busy));
    let recovery = recover(&sandbox.layout, OLD).expect("start");
    assert!(recovery.helper_running);
    drop(lock);
    assert!(stage(&sandbox, &keys.pinned).is_ok());
}

#[test]
fn layouts_keep_program_and_data_folders_apart() {
    let programs = PathBuf::from(r"C:\Synthetic\Programs");
    let data = PathBuf::from(r"C:\Synthetic\Syrinx\data");
    assert!(UpdateLayout::new(programs.clone(), data).is_ok());
    for overlapping in [
        r"C:\Synthetic\Programs\Syrinx\data",
        r"C:\Synthetic\Programs\syrinx.NEW",
        r"C:\Synthetic\programs\Syrinx.previous\x",
        r"C:\Synthetic",
    ] {
        assert_eq!(
            UpdateLayout::new(programs.clone(), PathBuf::from(overlapping)),
            Err(InstallError::Layout),
            "{overlapping}"
        );
    }
    assert_eq!(
        UpdateLayout::new(PathBuf::from("Programs"), PathBuf::from(r"C:\data")),
        Err(InstallError::Layout)
    );
}

#[test]
fn only_an_installed_program_folder_has_an_update_layout() {
    let data = PathBuf::from(r"C:\Synthetic\data");
    let installed = UpdateLayout::for_running_program(
        Path::new(r"C:\Synthetic\Programs\Syrinx\syrinx.exe"),
        data.clone(),
    )
    .expect("installed copy");
    assert_eq!(installed.programs(), Path::new(r"C:\Synthetic\Programs"));
    for elsewhere in [
        r"C:\repo\target\local\syrinx.exe",
        r"C:\Synthetic\Programs\Syrinx.new\syrinx.exe",
        r"C:\Synthetic\Programs\Syrinx\other.exe",
    ] {
        assert_eq!(
            UpdateLayout::for_running_program(Path::new(elsewhere), data.clone()),
            Err(InstallError::NotInstalled),
            "{elsewhere}"
        );
    }
}

#[test]
fn records_round_trip_and_refuse_anything_else() {
    let sandbox = Sandbox::new();
    let layout = &sandbox.layout;
    fs::create_dir_all(layout.updates()).expect("updates");
    let pending = Pending {
        version: NEW,
        from: OLD,
        manifest_sha256: [7; 32],
    };
    pending.write(layout).expect("write");
    assert_eq!(Pending::read(layout), Ok(Some(pending)));
    let outcome = Outcome {
        result: UpdateResult::ApplyRolledBack,
        attempted: NEW,
        current: OLD,
    };
    outcome.write(layout).expect("write");
    assert_eq!(Outcome::take(layout), Ok(Some(outcome)));
    assert_eq!(Outcome::take(layout), Ok(None), "told once");
    assert_eq!(UpdateState::read(layout), Ok(UpdateState::default()));

    for malformed in [
        &b"syrinx-update-pending 1\nversion 1.4.0\nfrom 1.3.0\n"[..],
        b"syrinx-update-pending 1\r\nversion 1.4.0\nfrom 1.3.0\nmanifest-sha256 00\n",
        b"syrinx-update-pending 2\nversion 1.4.0\nfrom 1.3.0\nmanifest-sha256 none\n",
    ] {
        fs::write(layout.pending(), malformed).expect("write");
        assert!(Pending::read(layout).is_err());
    }
}
