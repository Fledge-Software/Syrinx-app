//! Exercises the Unix spelling and link rules on disposable synthetic directories.

use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use syrinx_store::{DirectoryLease, LocationError};

use super::{
    LOCAL_DRIVE, LocationProbe, WindowsDirectoryLease, native, normal_absolute_path,
    sqlite_database_path,
};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        // A fresh name confines creation and cleanup to this test's own directory.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "syrinx-unix-path-{}-{time}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create disposable synthetic directory");
        // Resolve the temporary folder's own links, so only links a test makes are refused.
        let root = fs::canonicalize(&root).expect("resolve the synthetic directory");
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
fn keeps_one_absolute_spelling_and_refuses_relative_or_dotted_paths() {
    assert_eq!(
        normal_absolute_path(Path::new("/synthetic/database")),
        Ok(PathBuf::from("/synthetic/database"))
    );
    // `Path` already drops a `.` component, so only relative and `..` spellings remain to refuse.
    for path in ["synthetic/database", "/synthetic/../database", ""] {
        assert_eq!(
            normal_absolute_path(Path::new(path)),
            Err(LocationError::InvalidPath),
            "{path}"
        );
    }
    let database = Path::new("/synthetic/syrinx.db");
    assert_eq!(sqlite_database_path(database), database);
}

#[test]
fn creates_the_database_and_refuses_a_second_lease_on_the_same_folder() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("data");
    let lease = WindowsDirectoryLease::prepare(&destination).expect("prepare a fresh folder");
    assert!(lease.database_path().is_file());
    assert_eq!(
        WindowsDirectoryLease::prepare(&destination).map(|_| ()),
        Err(LocationError::StorageBusy)
    );
    drop(lease);
    WindowsDirectoryLease::prepare(&destination).expect("the folder is free again");
}

#[test]
fn a_linked_data_folder_is_refused() {
    let fixture = Fixture::new();
    let real = fixture.root.join("real");
    fs::create_dir(&real).expect("create the link's target");
    let linked = fixture.root.join("linked");
    symlink(&real, &linked).expect("create a synthetic folder link");
    assert_eq!(
        WindowsDirectoryLease::prepare(&linked.join("data")).map(|_| ()),
        Err(LocationError::ReparsePoint)
    );
}

#[test]
fn a_hard_linked_database_is_refused() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("data");
    drop(WindowsDirectoryLease::prepare(&destination).expect("prepare a fresh folder"));
    fs::hard_link(destination.join("syrinx.db"), fixture.root.join("copy.db"))
        .expect("hard-link the synthetic database");
    assert_eq!(
        WindowsDirectoryLease::prepare(&destination).map(|_| ()),
        Err(LocationError::HardLinkedFile)
    );
}

#[test]
fn a_renamed_folder_fails_revalidation() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("data");
    let lease = WindowsDirectoryLease::prepare(&destination).expect("prepare a fresh folder");
    fs::rename(&destination, fixture.root.join("moved")).expect("move the synthetic folder");
    fs::create_dir(&destination).expect("put a stand-in at the old name");
    assert!(lease.revalidate().is_err());
}

/// Reports one synthetic folder as a mounted sync client (FUSE, as pCloud's `~/pCloudDrive`
/// is) and everything else as the real filesystem the test runs on.
struct MountedSyncRoot {
    root: PathBuf,
}

impl LocationProbe for MountedSyncRoot {
    fn drive_type(&self, root: &Path) -> Result<u32, LocationError> {
        if root.starts_with(&self.root) {
            // Any answer but `LOCAL_DRIVE` is a network or FUSE filesystem.
            return Ok(LOCAL_DRIVE + 1);
        }
        native::NativeProbe.drive_type(root)
    }

    fn sync_roots(&self) -> Result<Vec<PathBuf>, LocationError> {
        Ok(vec![self.root.clone()])
    }
}

#[test]
fn a_mounted_sync_folder_elsewhere_does_not_stop_a_local_database() {
    let fixture = Fixture::new();
    let mounted = fixture.root.join("pCloudDrive");
    fs::create_dir(&mounted).expect("create the synthetic sync folder");
    let probe = Arc::new(MountedSyncRoot {
        root: mounted.clone(),
    });
    // The database sits beside the sync folder, never inside it.
    let destination = fixture.root.join("data");
    let lease = WindowsDirectoryLease::prepare_with_probe(&destination, probe.clone())
        .expect("a sync folder that cannot hold the database is not a reason to refuse");
    lease.revalidate().expect("revalidation keeps accepting it");
    drop(lease);
    // Inside the mounted folder the database is still refused.
    assert_eq!(
        WindowsDirectoryLease::prepare_with_probe(&mounted.join("data"), probe).map(|_| ()),
        Err(LocationError::NonLocalDrive)
    );
}
