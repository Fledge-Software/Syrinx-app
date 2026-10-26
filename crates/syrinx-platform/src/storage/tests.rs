//! Exercises location policy on synthetic metadata and disposable filesystem entries.

use std::{
    fs,
    os::windows::ffi::OsStrExt,
    path::{Component, Path, PathBuf, Prefix},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    EntryInfo, FileIdentity, LocationProbe, SIDECAR_NAMES, WindowsDirectoryLease, check_entry,
    native, normal_absolute_path, sqlite_database_path,
};
use syrinx_store::{DirectoryLease, LocationError, LocationOperation};

#[test]
fn accepts_dos_spelling_and_removes_only_the_extended_dos_prefix() {
    let expected = PathBuf::from(r"C:\synthetic\database");
    assert_eq!(
        normal_absolute_path(Path::new(r"c:\synthetic\database")),
        Ok(expected.clone())
    );
    assert_eq!(
        normal_absolute_path(Path::new(r"\\?\C:\synthetic\database")),
        Ok(expected)
    );
}

#[test]
fn sqlite_path_reserves_space_for_the_longest_sidecar_and_terminator() {
    for length in [251, 252, 259, 260, 261] {
        let path = PathBuf::from(format!(r"C:\{}\syrinx.db", "a".repeat(length - 13)));
        assert_eq!(path.as_os_str().encode_wide().count(), length);
        let sqlite_path = sqlite_database_path(&path);
        let uses_extended = matches!(
            sqlite_path.components().next(),
            Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::VerbatimDisk(_))
        );
        assert_eq!(uses_extended, length > 251);
        assert_eq!(normal_absolute_path(&sqlite_path), Ok(path));
    }
}

#[test]
fn sqlite_path_limit_counts_utf16_units_instead_of_utf8_bytes() {
    let component = "😀".repeat(119);
    let path = PathBuf::from(format!(r"C:\{component}\syrinx.db"));
    assert_eq!(path.as_os_str().encode_wide().count(), 251);
    assert_eq!(sqlite_database_path(&path), path);

    let longer_path = PathBuf::from(format!(r"C:\{component}a\syrinx.db"));
    assert_eq!(longer_path.as_os_str().encode_wide().count(), 252);
    let sqlite_path = sqlite_database_path(&longer_path);
    assert_ne!(sqlite_path, longer_path);
    assert_eq!(normal_absolute_path(&sqlite_path), Ok(longer_path));
}

#[test]
fn rejects_relative_network_device_stream_and_ambiguous_names() {
    for value in [
        "",
        r"data",
        r"C:data",
        r"\data",
        r"\\example.invalid\share\data",
        r"\\?\UNC\example.invalid\share\data",
        r"\\.\C:\data",
        r"C:\parent\..\data",
        r"C:\data:stream",
        r"C:\data.",
        "C:\\data ",
        r"C:\CON.txt",
        r"C:\lpt1",
        r"C:\COM¹",
        r"C:\COM²",
        r"C:\COM³",
        r"C:\LPT¹",
        r"C:\LPT²",
        r"C:\LPT³",
        r"C:\com¹.txt",
        r"C:\com².tar.gz",
        r"C:\com³.txt",
        r"C:\lpt¹.txt",
        r"C:\lpt².tar.gz",
        r"C:\lpt³.txt",
        "C:\\invalid\0name",
    ] {
        assert_eq!(
            normal_absolute_path(Path::new(value)),
            Err(LocationError::InvalidPath)
        );
    }
}

#[test]
fn refuses_every_nonfixed_drive_before_creating_the_destination() {
    let fixture = Fixture::new();
    for drive in [0, 1, 2, 4, 5, 6, u32::MAX] {
        let destination = fixture.root.join(format!("drive-{drive}"));
        let probe = Arc::new(StubProbe::with_drive(drive));
        let error = WindowsDirectoryLease::prepare_with_probe(&destination, probe)
            .expect_err("a nonfixed or unknown drive must be refused");
        assert_eq!(error, LocationError::NonLocalDrive);
        assert!(!destination.exists());
    }
}

#[test]
fn enumeration_failure_is_not_treated_as_an_empty_list() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("absent");
    let error = LocationError::Os {
        operation: LocationOperation::EnumerateSyncRoots,
        code: Some(5),
    };
    let probe = Arc::new(StubProbe {
        drive: None,
        roots: Mutex::new(Err(error)),
    });
    assert_eq!(
        WindowsDirectoryLease::prepare_with_probe(&destination, probe).unwrap_err(),
        error
    );
    assert!(!destination.exists());
}

#[test]
fn a_registered_ancestor_refuses_creation() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("absent").join("data");
    let probe = Arc::new(StubProbe::with_roots(vec![fixture.root.clone()]));
    assert_eq!(
        WindowsDirectoryLease::prepare_with_probe(&destination, probe).unwrap_err(),
        LocationError::SyncedDirectory,
    );
    assert!(!fixture.root.join("absent").exists());
}

#[test]
fn an_unresolvable_registered_root_fails_closed() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("absent");
    let probe = Arc::new(StubProbe::with_roots(vec![
        fixture.root.join("missing-root"),
    ]));
    assert!(WindowsDirectoryLease::prepare_with_probe(&destination, probe).is_err());
    assert!(!destination.exists());
}

#[test]
fn a_similarly_named_registered_sibling_is_not_an_ancestor() {
    let fixture = Fixture::new();
    let registered = fixture.root.join("cloud");
    fs::create_dir(&registered).expect("create synthetic registered root");
    let probe = Arc::new(StubProbe::with_roots(vec![registered]));
    let lease = WindowsDirectoryLease::prepare_with_probe(&fixture.root.join("cloud-other"), probe)
        .expect("a sibling directory is outside the synthetic root");
    assert!(lease.database_path().is_file());
    lease.revalidate().expect("revalidate synthetic database");
}

#[test]
fn creates_missing_children_and_keeps_existing_database_bytes() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("first").join("second");
    let lease = prepare(&destination);
    assert_eq!(
        lease.database_path().file_name(),
        Some(std::ffi::OsStr::new("syrinx.db"))
    );
    assert!(
        !lease
            .database_path()
            .as_os_str()
            .to_string_lossy()
            .starts_with(r"\\?\")
    );
    fs::write(lease.database_path(), b"synthetic database bytes").expect("write synthetic bytes");
    lease.revalidate().expect("revalidate unchanged identity");
    drop(lease);

    let reopened = prepare(&destination);
    assert_eq!(
        fs::read(reopened.database_path()).expect("read synthetic bytes"),
        b"synthetic database bytes"
    );
}

#[test]
fn native_checks_support_a_disposable_path_beyond_the_legacy_length() {
    let fixture = Fixture::new();
    let mut destination = fixture.root.clone();
    for index in 0..8 {
        destination.push(format!(
            "synthetic-component-{index}-with-enough-characters"
        ));
    }
    let lease = prepare(&destination);
    assert!(lease.database_path().as_os_str().encode_wide().count() > 260);
    assert!(matches!(
        lease.database_path().components().next(),
        Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::VerbatimDisk(_))
    ));
    assert_eq!(
        normal_absolute_path(lease.database_path()),
        Ok(lease.database.path.clone())
    );
    fs::write(lease.database_path(), b"synthetic long-path bytes").expect("write synthetic bytes");
    lease.revalidate().expect("revalidate long-path identity");
    assert_eq!(
        fs::read(lease.database_path()).expect("read synthetic bytes"),
        b"synthetic long-path bytes"
    );
}

#[test]
fn new_sync_registration_is_detected_on_revalidation() {
    let fixture = Fixture::new();
    let probe = Arc::new(StubProbe::with_roots(Vec::new()));
    let lease =
        WindowsDirectoryLease::prepare_with_probe(&fixture.root.join("data"), probe.clone())
            .expect("prepare a synthetic database");
    *probe.roots.lock().expect("lock synthetic root list") = Ok(vec![fixture.root.clone()]);
    assert_eq!(lease.revalidate(), Err(LocationError::SyncedDirectory));
}

#[test]
fn directory_and_database_names_remain_locked_until_the_lease_drops() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("data");
    let lease = prepare(&destination);
    assert!(fs::rename(&destination, fixture.root.join("renamed")).is_err());
    assert!(fs::rename(lease.database_path(), destination.join("moved.db")).is_err());
    drop(lease);

    fs::rename(&destination, fixture.root.join("renamed"))
        .expect("release directory name after lease");
    fs::rename(
        fixture.root.join("renamed").join("syrinx.db"),
        fixture.root.join("moved.db"),
    )
    .expect("release database name after lease");
}

#[test]
fn ordinary_sidecars_can_be_deleted_while_the_lease_is_alive() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("data");
    fs::create_dir(&destination).expect("create synthetic data directory");
    for filename in SIDECAR_NAMES {
        fs::write(destination.join(filename), b"synthetic sidecar")
            .expect("create synthetic sidecar");
    }
    let lease = prepare(&destination);
    for filename in SIDECAR_NAMES {
        fs::remove_file(destination.join(filename)).expect("release sidecar inspection handle");
    }
    lease.revalidate().expect("missing sidecars are permitted");
}

#[test]
fn directories_cannot_stand_in_for_database_or_sidecar_files() {
    let fixture = Fixture::new();
    for (index, filename) in [
        "syrinx.db",
        "syrinx.db-wal",
        "syrinx.db-shm",
        "syrinx.db-journal",
    ]
    .into_iter()
    .enumerate()
    {
        let destination = fixture.root.join(format!("data-{index}"));
        fs::create_dir(&destination).expect("create synthetic data directory");
        fs::create_dir(destination.join(filename)).expect("create synthetic wrong-kind entry");
        assert_eq!(
            prepare_result(&destination).unwrap_err(),
            LocationError::UnexpectedEntryKind
        );
    }
}

#[test]
fn metadata_checks_refuse_reparse_pending_and_multiple_link_entries() {
    let mut information = regular_file_info();
    information.reparse = true;
    assert_eq!(
        check_entry(&information, false),
        Err(LocationError::ReparsePoint)
    );
    information.reparse = false;
    information.delete_pending = true;
    assert_eq!(
        check_entry(&information, false),
        Err(LocationError::DeletePending)
    );
    information.delete_pending = false;
    information.links = 2;
    assert_eq!(
        check_entry(&information, false),
        Err(LocationError::HardLinkedFile)
    );
    information.links = 0;
    assert_eq!(
        check_entry(&information, false),
        Err(LocationError::HardLinkedFile)
    );
}

#[test]
fn retained_identity_mismatch_is_an_explicit_error() {
    let fixture = Fixture::new();
    let mut lease = prepare(&fixture.root.join("data"));
    lease.database.identity.id[0] ^= 1;
    assert_eq!(lease.revalidate(), Err(LocationError::IdentityChanged));
}

#[test]
fn lease_diagnostics_redact_location_and_lease_is_send_and_sync() {
    fn require_send_sync<Value: Send + Sync>() {}
    require_send_sync::<WindowsDirectoryLease>();
    let fixture = Fixture::new();
    let lease = prepare(&fixture.root.join("private-synthetic-sentinel"));
    assert_eq!(
        format!("{lease:?}"),
        "WindowsDirectoryLease { location: [redacted] }"
    );
}

#[test]
fn a_refused_begin_releases_the_session_claim_without_marking_clean() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("data");
    let lease = prepare(&destination);
    let control = lease.session_control().expect("native session control");
    let intent = destination.join(super::session::RECOVERY_INTENT_NAME);
    fs::write(&intent, b"synthetic recovery intent").expect("write synthetic intent");

    assert!(matches!(
        control.begin(),
        Err(LocationError::RecoveryPending)
    ));
    fs::remove_file(&intent).expect("remove synthetic intent");
    let token = control.begin().expect("release claim after refused begin");
    assert_eq!(
        token.previous_shutdown(),
        syrinx_store::ShutdownState::Unclean
    );
}

#[test]
fn a_refused_completion_releases_the_claim_and_preserves_dirty_state() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("data");
    let lease = prepare(&destination);
    let control = lease.session_control().expect("native session control");
    let token = control.begin().expect("begin synthetic session");
    let intent = destination.join(super::session::RECOVERY_INTENT_NAME);
    fs::write(&intent, b"synthetic recovery intent").expect("write synthetic intent");

    assert_eq!(token.complete(), Err(LocationError::RecoveryPending));
    fs::remove_file(&intent).expect("remove synthetic intent");
    let reopened = control
        .begin()
        .expect("release claim after refused completion");
    assert_eq!(
        reopened.previous_shutdown(),
        syrinx_store::ShutdownState::Unclean
    );
}

fn regular_file_info() -> EntryInfo {
    EntryInfo {
        identity: FileIdentity {
            volume: 1,
            id: [0; 16],
        },
        directory: false,
        reparse: false,
        delete_pending: false,
        links: 1,
    }
}

fn prepare(path: &Path) -> WindowsDirectoryLease {
    prepare_result(path).expect("prepare a synthetic database")
}

fn prepare_result(path: &Path) -> Result<WindowsDirectoryLease, LocationError> {
    WindowsDirectoryLease::prepare_with_probe(path, Arc::new(StubProbe::with_roots(Vec::new())))
}

struct StubProbe {
    drive: Option<u32>,
    roots: Mutex<Result<Vec<PathBuf>, LocationError>>,
}

impl StubProbe {
    fn with_roots(roots: Vec<PathBuf>) -> Self {
        Self {
            drive: None,
            roots: Mutex::new(Ok(roots)),
        }
    }

    fn with_drive(drive: u32) -> Self {
        Self {
            drive: Some(drive),
            roots: Mutex::new(Ok(Vec::new())),
        }
    }
}

impl LocationProbe for StubProbe {
    fn drive_type(&self, root: &Path) -> Result<u32, LocationError> {
        match self.drive {
            Some(value) => Ok(value),
            None => native::WindowsProbe.drive_type(root),
        }
    }

    fn sync_roots(&self) -> Result<Vec<PathBuf>, LocationError> {
        self.roots
            .lock()
            .map_err(|_| LocationError::SyncRootCheckPanicked)?
            .clone()
    }
}

pub(super) struct Fixture {
    pub(super) root: PathBuf,
}

impl Fixture {
    pub(super) fn new() -> Self {
        // A fresh name confines creation and cleanup to this test's own directory.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "syrinx-path-{}-{time}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create disposable synthetic directory");
        Self { root }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Every entry below this fresh root was created by this test.
        fs::remove_dir_all(&self.root).expect("remove disposable synthetic directory");
    }
}

#[test]
fn a_cache_folder_is_created_beside_the_database_and_held_in_place() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("data");
    let lease = prepare(&destination);
    let folder = lease
        .cache_folder("synthetic-cache")
        .expect("create a synthetic cache folder");
    assert_eq!(folder.path(), destination.join("synthetic-cache"));
    assert!(folder.path().is_dir());

    // Files inside stay the cache's own to create and delete; the folder itself cannot move.
    let inside = folder.path().join("segment.bin");
    fs::write(&inside, b"synthetic cache bytes").expect("write inside the cache folder");
    fs::remove_file(&inside).expect("delete inside the cache folder");
    assert!(fs::rename(folder.path(), destination.join("moved")).is_err());
    let path = folder.path().to_owned();
    drop(folder);
    fs::rename(&path, destination.join("moved")).expect("release the cache folder's name");

    // An existing folder is opened again rather than replaced.
    fs::rename(destination.join("moved"), &path).expect("restore the synthetic folder");
    fs::write(path.join("kept.bin"), b"kept").expect("write a synthetic kept file");
    let reopened = lease
        .cache_folder("synthetic-cache")
        .expect("reopen the synthetic cache folder");
    assert_eq!(
        fs::read(reopened.path().join("kept.bin")).expect("read the kept file"),
        b"kept"
    );
}

#[test]
fn a_cache_folder_in_a_synced_location_is_refused_before_it_is_created() {
    let fixture = Fixture::new();
    let probe = Arc::new(StubProbe::with_roots(Vec::new()));
    let destination = fixture.root.join("data");
    let lease = WindowsDirectoryLease::prepare_with_probe(&destination, probe.clone())
        .expect("prepare a synthetic database");
    *probe.roots.lock().expect("lock synthetic root list") = Ok(vec![fixture.root.clone()]);
    assert_eq!(
        lease.cache_folder("synthetic-cache").err(),
        Some(LocationError::SyncedDirectory)
    );
    assert!(!destination.join("synthetic-cache").exists());
}

#[test]
fn a_cache_folder_must_be_a_plain_folder_with_a_plain_name() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("data");
    let lease = prepare(&destination);
    fs::write(destination.join("not-a-folder"), b"synthetic file").expect("write synthetic file");
    assert_eq!(
        lease.cache_folder("not-a-folder").err(),
        Some(LocationError::UnexpectedEntryKind)
    );
    for name in ["", "..", "nested\name", "CON", "trailing."] {
        assert_eq!(
            lease.cache_folder(name).err(),
            Some(LocationError::InvalidPath),
            "a cache folder name must be one plain component"
        );
    }
}

#[test]
fn cache_folder_diagnostics_redact_the_location() {
    let fixture = Fixture::new();
    let lease = prepare(&fixture.root.join("private-synthetic-sentinel"));
    let folder = lease
        .cache_folder("synthetic-cache")
        .expect("create a synthetic cache folder");
    assert_eq!(
        format!("{folder:?}"),
        "CacheFolder { location: [redacted] }"
    );
}

#[test]
fn emptying_a_cache_folder_removes_its_files_and_refuses_anything_else() {
    let fixture = Fixture::new();
    let lease = prepare(&fixture.root.join("data"));
    let folder = lease
        .cache_folder("synthetic-cache")
        .expect("create a synthetic cache folder");
    fs::write(folder.path().join("segment.bin"), b"synthetic").expect("write synthetic file");
    fs::write(folder.path().join(".synthetic-lock"), b"synthetic").expect("write synthetic file");
    folder.empty().expect("empty the synthetic cache folder");
    assert!(
        fs::read_dir(folder.path())
            .expect("list the synthetic cache folder")
            .next()
            .is_none()
    );

    // A subfolder is refused before anything is deleted, so a link inside cannot be followed.
    fs::create_dir(folder.path().join("nested")).expect("create synthetic subfolder");
    fs::write(folder.path().join("kept.bin"), b"kept").expect("write synthetic file");
    assert_eq!(
        folder.empty().err(),
        Some(LocationError::UnexpectedEntryKind)
    );
    assert!(folder.path().join("nested").is_dir());
    assert!(folder.path().join("kept.bin").is_file());
}
