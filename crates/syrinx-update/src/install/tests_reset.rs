//! "Reset update records" (DW-66) on synthetic folders: only an unreadable `state` is set
//! aside, never while an update step runs or waits, and the serial floor starts again.

use std::fs;

use super::tests::{
    NEW, OLD, Sandbox, assert_untouched, now, packaged, plenty, program_files, read_tree, stage,
    write_package, write_tree,
};
use super::*;

const DAMAGED: &[u8] = b"not a state record\n";

/// Write synthetic damage over `Updates\state`.
fn damage_state(sandbox: &Sandbox) {
    fs::create_dir_all(sandbox.layout.updates()).expect("updates");
    fs::write(sandbox.layout.state(), DAMAGED).expect("damaged state");
}

fn set_aside(sandbox: &Sandbox) -> std::path::PathBuf {
    sandbox.layout.updates().join(SET_ASIDE_STATE)
}

#[test]
fn only_a_damaged_record_counts_as_unreadable() {
    let (sandbox, keys) = packaged();
    let layout = &sandbox.layout;
    assert_eq!(state_is_unreadable(layout), Ok(false), "no record yet");
    stage(&sandbox, &keys.pinned).expect("stage");
    assert_eq!(state_is_unreadable(layout), Ok(false), "a written record");
    fs::write(layout.state(), vec![b'x'; 4096]).expect("oversized state");
    assert_eq!(state_is_unreadable(layout), Ok(true), "oversized");
    damage_state(&sandbox);
    assert_eq!(state_is_unreadable(layout), Ok(true), "malformed");
}

#[test]
fn a_readable_or_missing_record_is_never_reset() {
    let sandbox = Sandbox::new();
    let layout = &sandbox.layout;
    assert_eq!(
        reset_unreadable_state(layout),
        Ok(StateReset::AlreadyReadable)
    );
    assert!(!layout.state().exists());
    let fresh = UpdateState {
        result: Some(UpdateResult::UpToDate),
        ..UpdateState::default()
    };
    fresh.write(layout).expect("readable state");
    let before = fs::read(layout.state()).expect("state");
    assert_eq!(
        reset_unreadable_state(layout),
        Ok(StateReset::AlreadyReadable)
    );
    assert_eq!(fs::read(layout.state()).expect("state"), before);
    assert!(!set_aside(&sandbox).exists());
}

#[test]
fn a_reset_sets_the_damaged_record_aside_and_touches_nothing_else() {
    let sandbox = Sandbox::new();
    damage_state(&sandbox);
    let previous = sandbox.layout.previous();
    let kept = program_files(OLD);
    write_tree(&previous, &kept);
    assert_eq!(
        reset_unreadable_state(&sandbox.layout),
        Ok(StateReset::Reset)
    );
    assert!(!sandbox.layout.state().exists(), "a fresh record");
    assert_eq!(fs::read(set_aside(&sandbox)).expect("evidence"), DAMAGED);
    assert_eq!(state_is_unreadable(&sandbox.layout), Ok(false));
    assert_eq!(read_tree(&previous), kept, "Syrinx.previous untouched");
    assert_untouched(&sandbox);

    // A second damage replaces the older evidence rather than piling copies up.
    fs::write(sandbox.layout.state(), b"other damage\n").expect("damage again");
    assert_eq!(
        reset_unreadable_state(&sandbox.layout),
        Ok(StateReset::Reset)
    );
    assert_eq!(
        fs::read(set_aside(&sandbox)).expect("evidence"),
        b"other damage\n"
    );
}

#[test]
fn a_reset_waits_for_any_update_step_holding_the_lock() {
    let sandbox = Sandbox::new();
    damage_state(&sandbox);
    let held = ApplyLock::try_acquire(&sandbox.layout)
        .expect("lock")
        .expect("free");
    assert_eq!(
        reset_unreadable_state(&sandbox.layout),
        Err(InstallError::Busy)
    );
    assert_eq!(fs::read(sandbox.layout.state()).expect("state"), DAMAGED);
    drop(held);
    assert_eq!(
        reset_unreadable_state(&sandbox.layout),
        Ok(StateReset::Reset)
    );
}

#[test]
fn a_reset_refuses_while_a_staged_update_waits() {
    let (sandbox, keys) = packaged();
    stage(&sandbox, &keys.pinned).expect("stage");
    damage_state(&sandbox);
    let staged = read_tree(&sandbox.layout.staged());
    assert_eq!(
        reset_unreadable_state(&sandbox.layout),
        Err(InstallError::UpdateWaiting)
    );
    assert_eq!(fs::read(sandbox.layout.state()).expect("state"), DAMAGED);
    assert_eq!(
        read_tree(&sandbox.layout.staged()),
        staged,
        "Syrinx.new kept"
    );
    assert!(sandbox.layout.pending().exists());
}

#[test]
fn after_a_reset_any_valid_serial_is_accepted_as_on_a_fresh_install() {
    // Serial 12 is accepted first; once the record is lost and reset, serial 3 passes, which
    // is the floor the owner accepts losing by resetting.
    let (sandbox, keys) = packaged();
    write_package(&sandbox.source, &keys, OLD, 12, &program_files(OLD));
    assert_eq!(stage(&sandbox, &keys.pinned), Ok(Staged::UpToDate(OLD)));
    damage_state(&sandbox);
    let older = Sandbox::new();
    write_package(&older.source, &keys, NEW, 3, &program_files(NEW));
    let request = StageRequest {
        source: &older.source,
        keys: &keys.pinned,
        installed: OLD,
        now: now(),
    };
    let refused = stage_from_folder(&sandbox.layout, &request, &plenty).expect_err("refused");
    assert!(is_unreadable_state(&refused), "{refused:?}");
    assert_eq!(
        reset_unreadable_state(&sandbox.layout),
        Ok(StateReset::Reset)
    );
    let staged = stage_from_folder(&sandbox.layout, &request, &plenty).expect("stage");
    assert!(matches!(staged, Staged::Ready { version, .. } if version == NEW));
    let state = UpdateState::read(&sandbox.layout).expect("fresh state");
    assert_eq!(
        state.accepted.map(|record| record.statement.serial()),
        Some(3)
    );
}
