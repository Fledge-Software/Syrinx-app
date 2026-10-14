//! The helper's swap, restart and rollback, stopped at every step, in synthetic folders.

use std::cell::RefCell;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

use super::fault::{Step, hook};
use super::startup::open_unchangeable;
use super::tests::{
    NEW, OLD, Sandbox, assert_untouched, packaged, program_files, read_tree, stage, write_tree,
};
use super::*;
use crate::{KeySet, Version};

const TIMING: ApplyTiming = ApplyTiming {
    exit_wait: Duration::from_millis(500),
    start_wait: Duration::from_millis(300),
    rename_patience: Duration::from_millis(500),
    poll: Duration::from_millis(10),
};

/// How the synthetic Syrinx behaves when the helper starts it.
#[derive(Clone, Copy, PartialEq)]
enum Start {
    Cleanly,
    Hangs,
    ExitsAtOnce,
    CannotLaunch,
}

/// Records what was started; a clean start writes the start record as Syrinx would.
struct FakeLauncher {
    layout: UpdateLayout,
    start: Start,
    launched: Vec<(Vec<u8>, Vec<OsString>)>,
    stopped: Rc<RefCell<bool>>,
}

impl FakeLauncher {
    fn new(layout: &UpdateLayout, start: Start) -> Self {
        Self {
            layout: layout.clone(),
            start,
            launched: Vec::new(),
            stopped: Rc::new(RefCell::new(false)),
        }
    }

    /// The package record of each started program: which version ran.
    fn versions(&self) -> Vec<Vec<u8>> {
        self.launched
            .iter()
            .map(|(record, _)| record.clone())
            .collect()
    }
}

struct FakeProgram {
    exited: bool,
    stopped: Rc<RefCell<bool>>,
}

impl LaunchedProgram for FakeProgram {
    fn has_exited(&mut self) -> io::Result<bool> {
        Ok(self.exited)
    }

    fn stop(&mut self) -> io::Result<()> {
        *self.stopped.borrow_mut() = true;
        Ok(())
    }
}

impl Launcher for FakeLauncher {
    fn launch(
        &mut self,
        executable: &Path,
        arguments: &[OsString],
    ) -> io::Result<Box<dyn LaunchedProgram>> {
        if self.start == Start::CannotLaunch {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        let folder = executable.parent().expect("program folder");
        let record = fs::read(folder.join(PACKAGE_RECORD))?;
        self.launched.push((record.clone(), arguments.to_vec()));
        // Only the first launch is the update under test; a relaunched previous version is fine.
        if self.start == Start::Cleanly || self.launched.len() > 1 {
            let version = String::from_utf8(record).expect("synthetic record");
            let version = version
                .strip_prefix("package ")
                .expect("record names version");
            let version = Version::parse(version).expect("synthetic version");
            if let Err(error) = record_started(&self.layout, version) {
                return Err(io::Error::other(error.to_string()));
            }
        }
        Ok(Box::new(FakeProgram {
            exited: self.start == Start::ExitsAtOnce,
            stopped: Rc::clone(&self.stopped),
        }))
    }
}

fn record(version: Version) -> Vec<u8> {
    format!("package {version}").into_bytes()
}

fn request<'a>(layout: &'a UpdateLayout, keys: &'a KeySet, mode: ApplyMode) -> ApplyRequest<'a> {
    ApplyRequest {
        layout,
        keys,
        mode,
        relaunch_arguments: vec![OsString::from("--test-data-dir"), OsString::from("D")],
        timing: TIMING,
    }
}

fn staged_sandbox() -> (Sandbox, super::tests::Keys) {
    let (sandbox, keys) = packaged();
    stage(&sandbox, &keys.pinned).expect("stage");
    (sandbox, keys)
}

fn assert_new_installed(sandbox: &Sandbox) {
    let layout = &sandbox.layout;
    assert_eq!(read_tree(&layout.program()), program_files(NEW));
    assert_eq!(read_tree(&layout.previous()), program_files(OLD));
    assert!(!layout.staged().exists());
    assert!(!layout.pending().exists());
}

#[test]
fn installing_on_quit_swaps_without_starting_anything() {
    let (sandbox, keys) = staged_sandbox();
    let mut launcher = FakeLauncher::new(&sandbox.layout, Start::Cleanly);
    let applied = apply_update(
        &request(&sandbox.layout, &keys.pinned, ApplyMode::Quit),
        &mut launcher,
    );
    assert_eq!(applied, Ok(Applied::Installed(NEW)));
    assert!(launcher.launched.is_empty());
    assert_new_installed(&sandbox);
    let recovery = recover(&sandbox.layout, NEW).expect("next start");
    assert_eq!(
        recovery.outcome.map(|outcome| outcome.result),
        Some(UpdateResult::Installed)
    );
    assert_eq!(recovery.staged, None);
}

#[test]
fn restart_now_starts_the_new_version_without_taking_the_foreground() {
    let (sandbox, keys) = staged_sandbox();
    let mut launcher = FakeLauncher::new(&sandbox.layout, Start::Cleanly);
    let applied = apply_update(
        &request(&sandbox.layout, &keys.pinned, ApplyMode::Restart),
        &mut launcher,
    );
    assert_eq!(applied, Ok(Applied::Installed(NEW)));
    assert_eq!(launcher.versions(), [record(NEW)]);
    assert_eq!(
        launcher.launched[0].1,
        [AFTER_UPDATE_FLAG, "--test-data-dir", "D"].map(OsString::from)
    );
    assert_new_installed(&sandbox);
    // The owner is looking at the new version, so nothing waits to be told.
    assert!(!sandbox.layout.outcome().exists());
    let state = UpdateState::read(&sandbox.layout).expect("state");
    assert_eq!(state.result, Some(UpdateResult::Installed));
}

#[test]
fn a_new_version_that_never_starts_cleanly_is_rolled_back() {
    for start in [Start::Hangs, Start::ExitsAtOnce, Start::CannotLaunch] {
        let (sandbox, keys) = staged_sandbox();
        let mut launcher = FakeLauncher::new(&sandbox.layout, start);
        let applied = apply_update(
            &request(&sandbox.layout, &keys.pinned, ApplyMode::Restart),
            &mut launcher,
        );
        assert_eq!(
            applied,
            Ok(Applied::RolledBack {
                attempted: NEW,
                current: OLD
            })
        );
        if start == Start::Hangs {
            assert!(*launcher.stopped.borrow(), "the hung version was stopped");
        }
        if start != Start::CannotLaunch {
            // The previous version is started again after the failed one.
            assert_eq!(launcher.versions(), [record(NEW), record(OLD)]);
        }
        assert_untouched(&sandbox);
        let recovery = recover(&sandbox.layout, OLD).expect("next start");
        let outcome = recovery.outcome.expect("told once");
        assert_eq!(
            (outcome.result, outcome.attempted, outcome.current),
            (UpdateResult::ApplyRolledBack, NEW, OLD)
        );
    }
}

#[test]
fn a_staged_copy_changed_after_staging_fails_its_second_check() {
    type Tamper = fn(&UpdateLayout);
    let cases: [(&str, Tamper); 3] = [
        ("changed file", |layout| {
            fs::write(layout.staged().join(PROGRAM_EXECUTABLE), b"replaced").expect("write")
        }),
        ("added file", |layout| {
            fs::write(layout.staged().join("added.dll"), b"extra").expect("write")
        }),
        ("changed manifest", |layout| {
            fs::write(layout.apply_manifest(), b"syrinx-update-manifest 1\n").expect("write")
        }),
    ];
    for (name, tamper) in cases {
        let (sandbox, keys) = staged_sandbox();
        tamper(&sandbox.layout);
        let mut launcher = FakeLauncher::new(&sandbox.layout, Start::Cleanly);
        let applied = apply_update(
            &request(&sandbox.layout, &keys.pinned, ApplyMode::Restart),
            &mut launcher,
        );
        assert_eq!(applied, Ok(Applied::Discarded(NEW)), "{name}");
        assert!(launcher.launched.is_empty());
        assert_untouched(&sandbox);
        assert!(
            !sandbox.layout.previous().exists(),
            "{name}: nothing was swapped"
        );
        let outcome = Outcome::take(&sandbox.layout).expect("read").expect("told");
        assert_eq!(outcome.result, UpdateResult::VerificationFailed);
    }
}

#[test]
fn the_helper_waits_for_syrinx_to_exit() {
    let (sandbox, keys) = staged_sandbox();
    let held = ApplyLock::try_acquire(&sandbox.layout)
        .expect("lock")
        .expect("free");
    let mut launcher = FakeLauncher::new(&sandbox.layout, Start::Cleanly);
    assert_eq!(
        apply_update(
            &request(&sandbox.layout, &keys.pinned, ApplyMode::Quit),
            &mut launcher
        ),
        Err(InstallError::TimedOut {
            what: "Syrinx to exit"
        })
    );
    assert_eq!(read_tree(&sandbox.layout.program()), program_files(OLD));
    assert_eq!(
        recover(&sandbox.layout, OLD).map(|r| r.helper_running),
        Ok(true)
    );

    // Released part-way through the wait, as Syrinx exiting would.
    let release = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        drop(held);
    });
    let applied = apply_update(
        &request(&sandbox.layout, &keys.pinned, ApplyMode::Quit),
        &mut launcher,
    );
    release.join().expect("release thread");
    assert_eq!(applied, Ok(Applied::Installed(NEW)));
}

#[test]
fn a_previous_folder_syrinx_did_not_make_stops_the_swap() {
    let (sandbox, keys) = staged_sandbox();
    let stranger = std::collections::BTreeMap::from([("owner.txt".to_owned(), b"keep".to_vec())]);
    write_tree(&sandbox.layout.previous(), &stranger);
    let mut launcher = FakeLauncher::new(&sandbox.layout, Start::Cleanly);
    assert_eq!(
        apply_update(
            &request(&sandbox.layout, &keys.pinned, ApplyMode::Quit),
            &mut launcher
        ),
        Err(InstallError::UnexpectedEntry {
            what: "Syrinx.previous"
        })
    );
    assert_eq!(read_tree(&sandbox.layout.program()), program_files(OLD));
    assert_eq!(read_tree(&sandbox.layout.previous()), stranger);
    assert_eq!(read_tree(&sandbox.layout.staged()), program_files(NEW));
}

/// Stop the helper at `step` in `mode`, as a crash or power cut would.
fn interrupted(step: Step, mode: ApplyMode, start: Start) -> (Sandbox, super::tests::Keys) {
    let (sandbox, keys) = staged_sandbox();
    let mut launcher = FakeLauncher::new(&sandbox.layout, start);
    hook::stop_at(Some(step));
    assert_eq!(
        apply_update(&request(&sandbox.layout, &keys.pinned, mode), &mut launcher),
        Err(InstallError::Interrupted(step))
    );
    hook::stop_at(None);
    (sandbox, keys)
}

#[test]
fn a_helper_stopped_before_the_swap_leaves_the_update_staged() {
    for step in [Step::ExitObserved, Step::Reverified, Step::PreviousRemoved] {
        let (sandbox, keys) = interrupted(step, ApplyMode::Quit, Start::Cleanly);
        assert_eq!(read_tree(&sandbox.layout.program()), program_files(OLD));
        let recovery = recover(&sandbox.layout, OLD).expect("next start");
        assert_eq!(recovery.staged, Some(NEW), "{step:?}");
        // Applying again from the following start succeeds.
        let mut launcher = FakeLauncher::new(&sandbox.layout, Start::Cleanly);
        let applied = apply_update(
            &request(&sandbox.layout, &keys.pinned, ApplyMode::Quit),
            &mut launcher,
        );
        assert_eq!(applied, Ok(Applied::Installed(NEW)), "{step:?}");
    }
}

#[test]
fn a_helper_stopped_between_the_renames_needs_the_documented_owner_step() {
    // The one case the proposal cannot automate: no `Programs\Syrinx` until the owner renames
    // `Syrinx.previous` back, as the checklist says. Both copies are still on disk.
    let (sandbox, keys) = interrupted(Step::ProgramMovedAside, ApplyMode::Quit, Start::Cleanly);
    let layout = &sandbox.layout;
    assert!(!layout.program().exists());
    assert_eq!(read_tree(&layout.previous()), program_files(OLD));
    assert_eq!(read_tree(&layout.staged()), program_files(NEW));

    fs::rename(layout.previous(), layout.program()).expect("the owner's rename");
    let recovery = recover(layout, OLD).expect("next start");
    assert_eq!(recovery.staged, Some(NEW));
    let mut launcher = FakeLauncher::new(layout, Start::Cleanly);
    let applied = apply_update(
        &request(layout, &keys.pinned, ApplyMode::Quit),
        &mut launcher,
    );
    assert_eq!(applied, Ok(Applied::Installed(NEW)));
}

#[test]
fn a_helper_stopped_after_the_swap_leaves_the_new_version_working() {
    for (step, mode) in [
        (Step::Swapped, ApplyMode::Quit),
        (Step::Swapped, ApplyMode::Restart),
        (Step::Launched, ApplyMode::Restart),
    ] {
        let (sandbox, _keys) = interrupted(step, mode, Start::Cleanly);
        let layout = &sandbox.layout;
        assert_eq!(read_tree(&layout.program()), program_files(NEW), "{step:?}");
        // The new version's first start finishes the records the helper left.
        let recovery = recover(layout, NEW).expect("next start");
        assert_eq!(recovery.staged, None);
        assert!(!layout.pending().exists());
        assert_eq!(
            UpdateState::read(layout).expect("state").result,
            Some(UpdateResult::Installed)
        );
    }
}

#[test]
fn a_rollback_stopped_part_way_keeps_both_versions_and_stages_nothing() {
    let (sandbox, _keys) = interrupted(Step::FailedMovedAside, ApplyMode::Restart, Start::Hangs);
    let layout = &sandbox.layout;
    assert!(!layout.program().exists());
    assert_eq!(read_tree(&layout.previous()), program_files(OLD));
    assert!(
        !layout.pending().exists(),
        "the failed version is no longer offered"
    );

    fs::rename(layout.previous(), layout.program()).expect("the owner's rename");
    let recovery = recover(layout, OLD).expect("next start");
    assert_eq!(recovery.staged, None);
    assert_untouched(&sandbox);
}

#[test]
fn the_handoff_checks_the_helper_and_holds_the_lock() {
    let (sandbox, keys) = staged_sandbox();
    let handoff = prepare_handoff(&sandbox.layout, &keys.pinned).expect("handoff");
    assert_eq!(
        ApplyLock::try_acquire(&sandbox.layout).map(|lock| lock.is_some()),
        Ok(false)
    );
    // While held, the checked helper cannot be replaced.
    assert!(fs::write(sandbox.layout.helper(), b"replacement").is_err());
    drop(handoff);

    fs::write(sandbox.layout.helper(), b"replacement").expect("write");
    assert!(matches!(
        prepare_handoff(&sandbox.layout, &keys.pinned),
        Err(InstallError::Update(
            crate::error::UpdateError::HashMismatch { .. }
        ))
    ));
    let production = KeySet::production().expect("empty production set");
    assert!(prepare_handoff(&sandbox.layout, &production).is_err());
}

#[test]
fn windows_starts_a_helper_held_against_changes() {
    // The test binary stands in for the helper: copy it, hold it as the handoff does, start it.
    let sandbox = Sandbox::new();
    let copy = sandbox.layout.updates().join("held-helper.exe");
    fs::create_dir_all(sandbox.layout.updates()).expect("updates");
    fs::copy(std::env::current_exe().expect("test binary"), &copy).expect("copy");
    let held = open_unchangeable(&copy).expect("hold");
    let status = Command::new(&copy)
        .args(["--list", "--format", "terse"])
        .stdout(std::process::Stdio::null())
        .status()
        .expect("start the held copy");
    assert!(status.success());
    drop(held);
}
