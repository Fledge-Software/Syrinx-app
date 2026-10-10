//! Checks bounded marker parsing and native publication without claiming SQLite validation.

use super::*;
use crate::storage::{prepare_directory, tests::Fixture};

struct CountingProbe {
    calls: std::sync::atomic::AtomicUsize,
    roots: Mutex<Vec<PathBuf>>,
}

impl crate::storage::LocationProbe for CountingProbe {
    fn drive_type(&self, root: &Path) -> Result<u32, LocationError> {
        crate::storage::LocationProbe::drive_type(&native::WindowsProbe, root)
    }

    fn sync_roots(&self) -> Result<Vec<PathBuf>, LocationError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.roots
            .lock()
            .map(|roots| roots.clone())
            .map_err(|_| LocationError::SyncRootCheckPanicked)
    }
}

fn archive(fixture: &Fixture) -> WindowsBackupArchive {
    let prepared = prepare_directory(&fixture.root, Arc::new(native::WindowsProbe))
        .expect("prepare synthetic backup directory");
    WindowsBackupArchive::new(prepared)
}

fn metadata(id: BackupId) -> BackupMetadata {
    BackupMetadata {
        id,
        created_at: 42,
        revision: 7,
        deletion_generation: 0,
    }
}

#[test]
fn marker_preserves_full_integer_ranges_and_privacy_generations() {
    for created_at in [i64::MIN, 0, i64::MAX] {
        let metadata = BackupMetadata {
            id: BackupId(u64::MAX),
            created_at,
            revision: u64::MAX,
            deletion_generation: u64::MAX,
        };
        assert_eq!(
            decode_marker(&encode_marker(metadata), metadata.id),
            Some(metadata)
        );
    }
}

#[test]
fn every_single_byte_marker_change_is_ineligible_and_identity_must_match() {
    let metadata = metadata(BackupId(19));
    let bytes = encode_marker(metadata);
    for index in 0..MARKER_SIZE {
        let mut damaged = bytes;
        damaged[index] ^= 1;
        assert_eq!(decode_marker(&damaged, metadata.id), None);
    }
    assert_eq!(decode_marker(&bytes, BackupId(20)), None);
    assert_eq!(decode_marker(&[0; MARKER_SIZE], metadata.id), None);
    let mut future_format = bytes;
    future_format[7] = b'2';
    future_format[PAYLOAD_SIZE + 7] = !b'2';
    assert_eq!(decode_marker(&future_format, metadata.id), None);
}

#[test]
fn bundle_names_accept_only_the_fixed_opaque_format() {
    for id in [1, 15, 16, u64::MAX] {
        assert_eq!(
            parse_bundle_name(&bundle_name(BackupId(id))),
            Some(BackupId(id))
        );
    }
    for invalid in [
        "",
        "b-0000000000000000",
        "b-000000000000000A",
        "b-1",
        "b-0000000000000001.extra",
        "../b-0000000000000001",
        "b-😀😀😀😀",
        "b-000000000000000g",
    ] {
        assert_eq!(parse_bundle_name(invalid), None);
    }
}

#[test]
fn archive_is_lazy_and_a_reserved_incomplete_output_is_not_a_candidate() {
    let fixture = Fixture::new();
    let archive = archive(&fixture);
    assert!(
        archive
            .candidates()
            .expect("enumerate empty synthetic archive")
            .is_empty()
    );
    assert!(!fixture.root.join(ARCHIVE_NAME).exists());
    let output = archive.reserve().expect("reserve synthetic output");
    let bundle = output
        .database_path()
        .parent()
        .expect("synthetic bundle")
        .to_owned();
    assert!(
        archive
            .candidates()
            .expect("skip incomplete output")
            .is_empty()
    );
    output.remove().expect("remove this reserved empty output");
    assert!(!bundle.exists());
}

#[test]
fn publication_freezes_files_and_removal_requires_only_this_owned_bundle() {
    let fixture = Fixture::new();
    let archive = archive(&fixture);
    let output = archive.reserve().expect("reserve synthetic native output");
    fs::write(
        output.database_path(),
        b"synthetic image verified by the caller",
    )
    .expect("write test-owned native image");
    let expected = metadata(output.id());
    assert_eq!(
        output.publish(metadata(BackupId(0))),
        Err(LocationError::InvalidBackup)
    );
    let mut unsupported = expected;
    unsupported.deletion_generation = 1;
    assert_eq!(
        output.publish(unsupported),
        Err(LocationError::InvalidBackup)
    );
    output.flush().expect("flush synthetic image");
    output
        .publish(expected)
        .expect("publish synthetic native image");
    assert_eq!(output.publish(expected), Err(LocationError::InvalidBackup));
    assert!(fs::write(output.database_path(), b"replacement").is_err());

    // Enumeration also works while the original published capability is retained.
    let mut entries = archive
        .candidates()
        .expect("enumerate immutable synthetic image");
    assert_eq!(entries.len(), 1);
    let entry = entries.pop().expect("one synthetic candidate");
    assert_eq!(entry.metadata, expected);
    let bundle = entry
        .file
        .database_path()
        .parent()
        .expect("synthetic bundle")
        .to_owned();
    drop(output);
    entry
        .file
        .remove()
        .expect("remove exact published identities");
    assert!(!bundle.exists());
}

#[test]
fn malformed_and_incomplete_markers_stay_on_disk_without_becoming_candidates() {
    let fixture = Fixture::new();
    let archive = archive(&fixture);
    let output = archive.reserve().expect("reserve synthetic output");
    let bundle = output
        .database_path()
        .parent()
        .expect("synthetic bundle")
        .to_owned();
    let database = output.database_path().to_owned();
    drop(output);
    let marker = bundle.join(COMPLETE_NAME);
    for bytes in [
        vec![],
        vec![0; MARKER_SIZE - 1],
        vec![0; MARKER_SIZE],
        vec![0; MARKER_SIZE + 1],
    ] {
        fs::write(&marker, &bytes).expect("write synthetic malformed marker");
        assert!(
            archive
                .candidates()
                .expect("refuse malformed candidate")
                .is_empty()
        );
        assert_eq!(fs::read(&marker).expect("preserve malformed marker"), bytes);
        assert!(database.exists());
    }
}

#[test]
fn unexpected_sidecars_prevent_cleanup_without_deleting_known_files() {
    let fixture = Fixture::new();
    let archive = archive(&fixture);
    let output = archive.reserve().expect("reserve synthetic output");
    let database = output.database_path().to_owned();
    let sidecar = database
        .parent()
        .expect("synthetic bundle")
        .join("atlas.db-wal");
    fs::write(&sidecar, b"synthetic unexpected sidecar").expect("write synthetic sidecar");
    assert_eq!(output.remove(), Err(LocationError::InvalidBackup));
    assert!(database.exists());
    assert_eq!(
        fs::read(&sidecar).expect("preserve sidecar"),
        b"synthetic unexpected sidecar"
    );
}

#[test]
fn sync_roots_are_checked_once_per_boundary_and_new_bundle_registration_is_refused() {
    let fixture = Fixture::new();
    let probe = Arc::new(CountingProbe {
        calls: std::sync::atomic::AtomicUsize::new(0),
        roots: Mutex::new(Vec::new()),
    });
    let prepared =
        prepare_directory(&fixture.root, probe.clone()).expect("prepare synthetic counting probe");
    let archive = WindowsBackupArchive::new(prepared);
    probe.calls.store(0, Ordering::Relaxed);
    let output = archive.reserve().expect("reserve outside registered roots");
    assert_eq!(probe.calls.load(Ordering::Relaxed), 1);
    let bundle = output
        .database_path()
        .parent()
        .expect("synthetic bundle")
        .to_owned();
    output
        .revalidate()
        .expect("check retained synthetic identities");
    output.flush().expect("flush synthetic output");
    output
        .publish(metadata(output.id()))
        .expect("publish synthetic marker");
    assert_eq!(probe.calls.load(Ordering::Relaxed), 1);

    let candidates = archive
        .candidates()
        .expect("start next maintenance boundary");
    assert_eq!(candidates.len(), 1);
    candidates[0]
        .file
        .revalidate()
        .expect("revalidate candidate identities");
    assert_eq!(probe.calls.load(Ordering::Relaxed), 2);

    // A later operation takes a fresh snapshot and rejects newly registered child roots.
    *probe.roots.lock().expect("set synthetic registration") = vec![bundle];
    assert!(matches!(
        archive.candidates(),
        Err(LocationError::SyncedDirectory)
    ));
    assert_eq!(probe.calls.load(Ordering::Relaxed), 3);
    *probe
        .roots
        .lock()
        .expect("set synthetic archive registration") = vec![fixture.root.join(ARCHIVE_NAME)];
    assert!(matches!(
        archive.candidates(),
        Err(LocationError::SyncedDirectory)
    ));
    assert_eq!(probe.calls.load(Ordering::Relaxed), 4);
}
