//! Setup payload and install tests on synthetic packages in fresh folders under `target`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use super::tests::{NEW, OLD, Sandbox, entries, program_files, read_tree, write_tree};
use super::*;
use crate::{Manifest, Version};

const PATIENCE: Duration = Duration::from_millis(200);

/// Write an unsigned package for `version` into `folder`, as `cargo xtask package` does.
fn write_unsigned_package(folder: &Path, version: Version) -> BTreeMap<String, Vec<u8>> {
    let files = program_files(version);
    let manifest = Manifest::new(version, Version::new(0, 0, 0), entries(&files))
        .expect("synthetic manifest")
        .to_bytes();
    write_tree(folder, &files);
    fs::write(folder.join(MANIFEST_FILE), manifest).expect("manifest");
    files
}

/// The program files plus the manifest, as setup leaves them installed.
fn installed_tree(folder: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = read_tree(folder);
    files.remove(MANIFEST_FILE).expect("installed manifest");
    files
}

#[test]
fn a_payload_round_trips_every_file() {
    let sandbox = Sandbox::new();
    let files = write_unsigned_package(&sandbox.source, NEW);
    let payload = write_payload(&sandbox.source).expect("payload");
    let package = SetupPackage::read(&payload).expect("read back");
    assert_eq!(package.version(), NEW);
    let total: usize = files.values().map(Vec::len).sum();
    assert!(payload.len() > total, "payload holds every file");
}

#[test]
fn a_damaged_payload_is_refused() {
    let sandbox = Sandbox::new();
    write_unsigned_package(&sandbox.source, NEW);
    let payload = write_payload(&sandbox.source).expect("payload");
    // Flip a byte in the last file, then cut one byte off, then append one.
    let mut flipped = payload.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 0xFF;
    let shortened = &payload[..payload.len() - 1];
    let mut lengthened = payload.clone();
    lengthened.push(0);
    for damaged in [&flipped[..], shortened, &lengthened[..], b"", b"SYRINX"] {
        assert!(matches!(
            SetupPackage::read(damaged),
            Err(InstallError::NotAPackage | InstallError::Update(_))
        ));
    }
}

#[test]
fn a_package_without_the_helper_is_refused() {
    let sandbox = Sandbox::new();
    write_unsigned_package(&sandbox.source, NEW);
    let mut files = program_files(NEW);
    files.remove(APPLY_HELPER);
    let manifest = Manifest::new(NEW, Version::new(0, 0, 0), entries(&files))
        .expect("manifest")
        .to_bytes();
    fs::write(sandbox.source.join(MANIFEST_FILE), manifest).expect("manifest");
    assert!(matches!(
        write_payload(&sandbox.source),
        Err(InstallError::MissingProgramFile { .. })
    ));
}

#[test]
fn a_first_install_creates_the_program_folder() {
    let sandbox = Sandbox::new();
    let layout = &sandbox.layout;
    fs::remove_dir_all(layout.program()).expect("start without an install");
    let files = write_unsigned_package(&sandbox.source, NEW);
    let payload = write_payload(&sandbox.source).expect("payload");
    let package = SetupPackage::read(&payload).expect("package");
    assert_eq!(existing_install(layout), Ok(Existing::Nothing));

    let installed = install_package(layout, &package, PATIENCE).expect("install");

    assert_eq!(installed, NEW);
    assert_eq!(installed_tree(&layout.program()), files);
    assert_eq!(existing_install(layout), Ok(Existing::Version(NEW)));
    assert!(!layout.staged().exists() && !layout.previous().exists());
}

#[test]
fn rerunning_setup_upgrades_and_keeps_the_previous_version() {
    let sandbox = Sandbox::new();
    let layout = &sandbox.layout;
    // The sandbox's installed copy predates P1-39a's manifest.
    assert_eq!(existing_install(layout), Ok(Existing::Unknown));
    let data_file = layout.data().join("State").join("syrinx.db");
    fs::create_dir_all(data_file.parent().expect("State")).expect("State");
    fs::write(&data_file, b"made-up profile").expect("profile");
    let files = write_unsigned_package(&sandbox.source, NEW);
    let payload = write_payload(&sandbox.source).expect("payload");
    let package = SetupPackage::read(&payload).expect("package");

    install_package(layout, &package, PATIENCE).expect("upgrade");

    assert_eq!(installed_tree(&layout.program()), files);
    assert_eq!(read_tree(&layout.previous()), program_files(OLD));
    assert_eq!(fs::read(&data_file).expect("profile"), b"made-up profile");
    assert!(!layout.staged().exists());

    // Running the same setup again replaces the copy and keeps the one it replaced.
    install_package(layout, &package, PATIENCE).expect("reinstall");
    assert_eq!(installed_tree(&layout.program()), files);
    assert_eq!(installed_tree(&layout.previous()), files);
}

#[test]
fn a_folder_setup_did_not_make_is_never_replaced() {
    let sandbox = Sandbox::new();
    let layout = &sandbox.layout;
    fs::remove_file(layout.program().join(PACKAGE_RECORD)).expect("make it foreign");
    write_unsigned_package(&sandbox.source, NEW);
    let payload = write_payload(&sandbox.source).expect("payload");
    let package = SetupPackage::read(&payload).expect("package");

    let refused = install_package(layout, &package, PATIENCE);

    assert_eq!(
        refused,
        Err(InstallError::UnexpectedEntry { what: "Syrinx" })
    );
    assert!(!layout.staged().exists());
    assert!(layout.program().join(PROGRAM_EXECUTABLE).exists());
}

#[test]
fn setup_waits_for_no_one_while_an_update_step_holds_the_lock() {
    let sandbox = Sandbox::new();
    let layout = &sandbox.layout;
    write_unsigned_package(&sandbox.source, NEW);
    let payload = write_payload(&sandbox.source).expect("payload");
    let package = SetupPackage::read(&payload).expect("package");
    let _held = ApplyLock::try_acquire(layout).expect("lock").expect("free");

    assert_eq!(
        install_package(layout, &package, PATIENCE),
        Err(InstallError::Busy)
    );
    assert_eq!(read_tree(&layout.program()), program_files(OLD));
}

/// A package for `NEW` read from the sandbox's source folder.
fn new_payload(sandbox: &Sandbox) -> Vec<u8> {
    write_unsigned_package(&sandbox.source, NEW);
    write_payload(&sandbox.source).expect("payload")
}

#[test]
fn an_in_app_update_waiting_beside_the_program_is_discarded_with_its_records() {
    let sandbox = Sandbox::new();
    let layout = &sandbox.layout;
    let payload = new_payload(&sandbox);
    let package = SetupPackage::read(&payload).expect("package");
    // A staged in-app update: its copy, its pending record and its apply folder.
    write_tree(&layout.staged(), &program_files(Version::new(1, 3, 5)));
    fs::create_dir_all(layout.apply_folder()).expect("apply folder");
    fs::write(layout.pending(), b"made-up pending record").expect("pending");

    install_package(layout, &package, PATIENCE).expect("install");

    assert!(!layout.pending().exists(), "pending record removed");
    assert!(!layout.apply_folder().exists(), "apply folder removed");
    assert!(!layout.staged().exists());
    assert_eq!(existing_install(layout), Ok(Existing::Version(NEW)));
}

#[test]
fn a_foreign_previous_folder_is_refused_before_anything_is_written() {
    let sandbox = Sandbox::new();
    let layout = &sandbox.layout;
    let payload = new_payload(&sandbox);
    let package = SetupPackage::read(&payload).expect("package");
    fs::create_dir(layout.previous()).expect("previous");
    fs::write(layout.previous().join("someone-else.txt"), b"made-up").expect("foreign file");

    assert_eq!(
        install_package(layout, &package, PATIENCE),
        Err(InstallError::UnexpectedEntry {
            what: "Syrinx.previous"
        })
    );
    assert!(!layout.staged().exists());
    assert_eq!(read_tree(&layout.program()), program_files(OLD));
}

#[test]
fn a_program_folder_in_use_is_left_in_place() {
    use std::os::windows::fs::OpenOptionsExt;

    let sandbox = Sandbox::new();
    let layout = &sandbox.layout;
    let payload = new_payload(&sandbox);
    let package = SetupPackage::read(&payload).expect("package");
    // A file opened without delete sharing stops Windows renaming its folder, as a running exe does.
    let _held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(layout.program().join(PROGRAM_EXECUTABLE))
        .expect("hold the program");

    assert_eq!(
        install_package(layout, &package, PATIENCE),
        Err(InstallError::ProgramInUse)
    );
    assert!(!layout.staged().exists(), "the new copy is removed");
    assert!(layout.program().join(PACKAGE_RECORD).exists());
}
