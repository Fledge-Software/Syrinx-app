//! DW-69 on synthetic folders: a discard stopped part way, by a crash or a locked file, is
//! finished by a later start, which still tells the owner what it would have told.

use std::fs;

use super::fault::{Step, hook};
use super::stage::{DISCARD_MARK, UNREADABLE_STATE_DISCARD_MARK};
#[cfg(windows)]
use super::startup::open_unchangeable;
use super::tests::{
    NEW, OLD, Sandbox, assert_untouched, now, packaged, plenty, program_files, read_tree, stage,
    write_package,
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
fn an_unreadable_record_discard_stopped_before_pending_goes_is_redone_and_told() {
    let (sandbox, keys) = packaged();
    stage(&sandbox, &keys.pinned).expect("stage");
    damage_state(&sandbox);
    hook::stop_at(Some(Step::DiscardMarked));
    assert_eq!(
        recover(&sandbox.layout, OLD),
        Err(InstallError::Interrupted(Step::DiscardMarked))
    );
    assert!(sandbox.layout.pending().exists(), "pending not yet removed");
    assert_eq!(read_tree(&sandbox.layout.staged()), program_files(NEW));

    // `pending` beside an unreadable record is discarded again, for the same reason.
    let recovery = recover(&sandbox.layout, OLD).expect("following start");
    assert!(recovery.discarded_for_unreadable_state);
    assert_eq!(recovery.staged, None);
    assert_discard_finished(&sandbox);
    assert!(
        !sandbox.layout.apply_folder().exists(),
        "the whole discard ran"
    );
}

#[test]
fn a_damaged_mark_beside_a_staged_update_never_hides_it() {
    let (sandbox, keys) = packaged();
    stage(&sandbox, &keys.pinned).expect("stage");
    // Too long to be any mark Syrinx writes, so reading it would be refused.
    fs::write(sandbox.layout.staging_marker(), vec![b'x'; 4096]).expect("damaged mark");
    let recovery = recover(&sandbox.layout, OLD).expect("start");
    assert_eq!(recovery.staged, Some(NEW));
    assert!(!recovery.discarded_for_unreadable_state);
    assert_eq!(read_tree(&sandbox.layout.staged()), program_files(NEW));
}

#[test]
fn an_over_long_mark_without_pending_is_cleared_rather_than_stopping_every_start() {
    let (sandbox, keys) = packaged();
    stage(&sandbox, &keys.pinned).expect("stage");
    fs::remove_file(sandbox.layout.pending()).expect("simulate a finished pending");
    // Too long to be any mark Syrinx writes; it still marks `Syrinx.new` as Syrinx's own.
    fs::write(sandbox.layout.staging_marker(), vec![b'x'; 4096]).expect("damaged mark");
    let waiting = Outcome {
        result: UpdateResult::ApplyFailed,
        attempted: NEW,
        current: OLD,
    };
    waiting.write(&sandbox.layout).expect("outcome");
    let recovery = recover(&sandbox.layout, OLD).expect("start");
    assert_eq!(recovery.outcome, Some(waiting), "not held back");
    assert!(!recovery.discarded_for_unreadable_state);
    assert_discard_finished(&sandbox);
}

// Needs Windows refusing to delete a file another handle holds.
#[cfg(windows)]
#[test]
fn a_discard_stopped_by_a_locked_file_keeps_the_copy_recognisable_until_it_finishes() {
    let (sandbox, keys) = packaged();
    stage(&sandbox, &keys.pinned).expect("stage");
    damage_state(&sandbox);
    // A helper outcome waiting to be told must survive the failed start too.
    let waiting = Outcome {
        result: UpdateResult::ApplyFailed,
        attempted: NEW,
        current: OLD,
    };
    waiting.write(&sandbox.layout).expect("outcome");
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

    assert!(sandbox.layout.outcome().exists(), "outcome kept");

    drop(locked);
    let recovery = recover(&sandbox.layout, OLD).expect("next start");
    assert!(recovery.discarded_for_unreadable_state);
    assert_eq!(recovery.outcome, Some(waiting));
    assert_discard_finished(&sandbox);
    assert!(
        !sandbox.layout.apply_folder().exists(),
        "the whole discard ran"
    );
    let again = recover(&sandbox.layout, OLD).expect("start after that");
    assert_eq!(again.outcome, None, "told once");
}
