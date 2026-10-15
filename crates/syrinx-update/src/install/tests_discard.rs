//! DW-69 on synthetic folders: a discard stopped after `pending` is gone, by a crash or a
//! locked file, is finished by the next start, while an unmarked installer copy is kept.

use std::collections::BTreeMap;
use std::fs;

use super::fault::{Step, hook};
use super::stage::{DISCARD_MARK, UNREADABLE_STATE_DISCARD_MARK};
use super::startup::open_unchangeable;
use super::tests::{
    NEW, OLD, Sandbox, assert_untouched, now, packaged, plenty, program_files, read_tree, stage,
    write_package, write_tree,
};
use super::tests_reset::damage_state;
use super::*;
use crate::Version;

/// A running version other than the one that staged, so start-up discards the staged copy.
const OTHER: Version = Version::new(1, 3, 5);

/// What a discard stopped after `pending` leaves: the staged copy, marked, with no `pending`.
fn assert_stopped_mid_discard(sandbox: &Sandbox, mark: &[u8]) {
    let layout = &sandbox.layout;
    assert!(!layout.pending().exists(), "pending already removed");
    assert_eq!(read_tree(&layout.staged()), program_files(NEW));
    assert_eq!(fs::read(layout.staging_marker()).expect("mark"), mark);
    assert_eq!(read_tree(&layout.program()), program_files(OLD));
}

/// Nothing staged or marked, and the installed program untouched.
///
/// `clear_unfinished` leaves `Updates\apply` (the small helper) for the next staging: a
/// helper that wrote a mark may still be exiting when the previous Syrinx starts.
fn assert_discard_finished(sandbox: &Sandbox) {
    assert_untouched(sandbox);
    assert!(!sandbox.layout.staging_marker().exists(), "mark cleared");
}

/// Stage `newer` over the staged `NEW`, as "Install update from file…" would.
fn restage(sandbox: &Sandbox, keys: &super::tests::Keys) -> Result<Staged, InstallError> {
    let newer = Version::new(1, 5, 0);
    let next = Sandbox::new();
    write_package(&next.source, keys, newer, 13, &program_files(newer));
    let request = StageRequest {
        source: &next.source,
        keys: &keys.pinned,
        installed: OLD,
        now: now(),
    };
    stage_from_folder(&sandbox.layout, &request, &plenty)
}

#[test]
fn a_restaging_stopped_mid_discard_is_finished_by_the_next_start() {
    let (sandbox, keys) = packaged();
    stage(&sandbox, &keys.pinned).expect("first staging");
    hook::stop_at(Some(Step::DiscardBegun));
    assert_eq!(
        restage(&sandbox, &keys),
        Err(InstallError::Interrupted(Step::DiscardBegun))
    );
    assert_stopped_mid_discard(&sandbox, DISCARD_MARK);

    let recovery = recover(&sandbox.layout, OLD).expect("next start");
    assert_eq!(recovery.staged, None);
    assert!(!recovery.discarded_for_unreadable_state);
    assert_discard_finished(&sandbox);
    assert!(matches!(restage(&sandbox, &keys), Ok(Staged::Ready { .. })));
}

#[test]
fn a_start_up_discard_stopped_part_way_is_finished_by_the_following_start() {
    let (sandbox, keys) = packaged();
    stage(&sandbox, &keys.pinned).expect("stage");
    hook::stop_at(Some(Step::DiscardBegun));
    assert_eq!(
        recover(&sandbox.layout, OTHER),
        Err(InstallError::Interrupted(Step::DiscardBegun))
    );
    assert_stopped_mid_discard(&sandbox, DISCARD_MARK);

    let recovery = recover(&sandbox.layout, OTHER).expect("following start");
    assert_eq!(recovery.staged, None);
    assert_discard_finished(&sandbox);
}

#[test]
fn an_unreadable_record_discard_stopped_part_way_still_tells_the_owner() {
    let (sandbox, keys) = packaged();
    stage(&sandbox, &keys.pinned).expect("stage");
    damage_state(&sandbox);
    hook::stop_at(Some(Step::DiscardBegun));
    assert_eq!(
        recover(&sandbox.layout, OLD),
        Err(InstallError::Interrupted(Step::DiscardBegun))
    );
    assert_stopped_mid_discard(&sandbox, UNREADABLE_STATE_DISCARD_MARK);

    // No `pending` remains, yet the mark still says why, so the owner is told once.
    let recovery = recover(&sandbox.layout, OLD).expect("following start");
    assert!(recovery.discarded_for_unreadable_state);
    assert_eq!(recovery.staged, None);
    assert_discard_finished(&sandbox);
    assert!(
        !sandbox.layout.apply_folder().exists(),
        "the whole discard ran"
    );
    assert_eq!(
        state_is_unreadable(&sandbox.layout),
        Ok(true),
        "left for the reset"
    );
    let again = recover(&sandbox.layout, OLD).expect("start after that");
    assert!(!again.discarded_for_unreadable_state, "told only once");
}

#[test]
fn a_discard_stopped_by_a_locked_file_keeps_the_copy_recognisable_until_it_finishes() {
    let (sandbox, keys) = packaged();
    stage(&sandbox, &keys.pinned).expect("stage");
    damage_state(&sandbox);
    // Held without delete sharing, as a virus scanner might hold it.
    let locked = open_unchangeable(&sandbox.layout.staged().join(PROGRAM_EXECUTABLE))
        .expect("hold the staged program");
    let stopped = recover(&sandbox.layout, OLD).expect_err("the locked file stops the discard");
    assert!(matches!(stopped, InstallError::Io { .. }), "{stopped:?}");
    assert!(!sandbox.layout.pending().exists());
    assert!(
        sandbox.layout.staged().join(PACKAGE_RECORD).exists(),
        "the package record goes last"
    );
    assert_eq!(read_tree(&sandbox.layout.program()), program_files(OLD));

    drop(locked);
    let recovery = recover(&sandbox.layout, OLD).expect("next start");
    assert!(recovery.discarded_for_unreadable_state);
    assert_discard_finished(&sandbox);
    assert!(
        !sandbox.layout.apply_folder().exists(),
        "the whole discard ran"
    );
}

#[test]
fn an_unmarked_installer_copy_is_still_kept_after_a_finished_discard() {
    let (sandbox, keys) = packaged();
    stage(&sandbox, &keys.pinned).expect("stage");
    hook::stop_at(Some(Step::DiscardBegun));
    assert_eq!(
        recover(&sandbox.layout, OTHER),
        Err(InstallError::Interrupted(Step::DiscardBegun))
    );
    recover(&sandbox.layout, OTHER).expect("following start");
    assert_discard_finished(&sandbox);

    // `install-syrinx.ps1`'s copy in progress carries the same package record but no mark.
    let installer_copy = BTreeMap::from([(PACKAGE_RECORD.to_owned(), b"package 1.4.0".to_vec())]);
    write_tree(&sandbox.layout.staged(), &installer_copy);
    let recovery = recover(&sandbox.layout, OTHER).expect("start during the installer");
    assert_eq!(recovery.staged, None);
    assert_eq!(read_tree(&sandbox.layout.staged()), installer_copy);
    assert_eq!(read_tree(&sandbox.layout.program()), program_files(OLD));
}
