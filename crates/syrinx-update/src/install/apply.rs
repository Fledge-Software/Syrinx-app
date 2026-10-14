//! The helper's work once Syrinx has exited: recheck, swap the folders, and roll back a failed start.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::fault::{Step, checkpoint};
use super::files::{
    read_small, remove_file_if_present, remove_program_folder, rename_patiently, write_atomically,
};
use super::lock::ApplyLock;
use super::records::{Outcome, Pending, UpdateResult, UpdateState};
use super::{InstallError, UpdateLayout};
use crate::directory::DirectoryPayload;
use crate::keys::KeySet;
use crate::manifest::MAX_MANIFEST_BYTES;
use crate::signature::SIGNATURE_BYTES;
use crate::version::Version;
use crate::{cng, verify_manifest};

/// Passed to the relaunched Syrinx: do not take the foreground, and report a clean start.
pub const AFTER_UPDATE_FLAG: &str = "--after-update";

/// Whether the helper starts the new Syrinx (Restart now) or leaves it for the owner (Later).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyMode {
    Restart,
    Quit,
}

impl ApplyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Restart => "restart",
            Self::Quit => "quit",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "restart" => Some(Self::Restart),
            "quit" => Some(Self::Quit),
            _ => None,
        }
    }
}

/// How long the helper waits at each step.
#[derive(Clone, Copy, Debug)]
pub struct ApplyTiming {
    /// For the old Syrinx to exit and release the lock.
    pub exit_wait: Duration,
    /// For the new Syrinx to report a clean start (the proposal's 60 s).
    pub start_wait: Duration,
    /// For a just-closed program to let go of its folder before a rename.
    pub rename_patience: Duration,
    pub poll: Duration,
}

impl ApplyTiming {
    pub const PRODUCTION: Self = Self {
        exit_wait: Duration::from_secs(60),
        start_wait: Duration::from_secs(60),
        rename_patience: Duration::from_secs(30),
        poll: Duration::from_millis(250),
    };
}

/// Starts a program; tests substitute one that pretends to start cleanly or hang.
pub trait Launcher {
    fn launch(
        &mut self,
        executable: &Path,
        arguments: &[OsString],
    ) -> io::Result<Box<dyn LaunchedProgram>>;
}

/// A started program the helper may have to stop.
pub trait LaunchedProgram {
    fn has_exited(&mut self) -> io::Result<bool>;
    fn stop(&mut self) -> io::Result<()>;
}

/// Starts real processes, detached from the helper's standard streams and in a working folder
/// that no update renames: Windows refuses to rename a folder that is a process's current one.
pub struct ProcessLauncher {
    working_directory: PathBuf,
}

impl ProcessLauncher {
    /// `working_directory` must be a folder no update renames, such as `Updates`.
    pub fn new(working_directory: PathBuf) -> Self {
        Self { working_directory }
    }

    /// The command `launch` runs.
    pub fn command(&self, executable: &Path, arguments: &[OsString]) -> Command {
        let mut command = Command::new(executable);
        command
            .args(arguments)
            .current_dir(&self.working_directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }
}

impl Launcher for ProcessLauncher {
    fn launch(
        &mut self,
        executable: &Path,
        arguments: &[OsString],
    ) -> io::Result<Box<dyn LaunchedProgram>> {
        let child = self.command(executable, arguments).spawn()?;
        Ok(Box::new(ChildProgram(child)))
    }
}

struct ChildProgram(Child);

impl LaunchedProgram for ChildProgram {
    fn has_exited(&mut self) -> io::Result<bool> {
        Ok(self.0.try_wait()?.is_some())
    }

    fn stop(&mut self) -> io::Result<()> {
        if self.has_exited()? {
            return Ok(());
        }
        self.0.kill()?;
        self.0.wait().map(drop)
    }
}

/// What the helper needs: the folders, the trusted keys and how to relaunch.
pub struct ApplyRequest<'a> {
    pub layout: &'a UpdateLayout,
    /// Always `KeySet::production()` in the helper; tests pass in-memory keys.
    pub keys: &'a KeySet,
    pub mode: ApplyMode,
    /// Added after `--after-update` when starting Syrinx, such as a test data folder.
    pub relaunch_arguments: Vec<OsString>,
    pub timing: ApplyTiming,
}

/// How the helper finished.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Applied {
    /// The new version is in place (and, on Restart now, started cleanly).
    Installed(Version),
    /// The staged copy failed its second check and was removed; nothing changed.
    Discarded(Version),
    /// The folders could not be swapped, so the installed version stayed.
    NotSwapped(Version),
    /// The new version did not start; the previous one is back and was started.
    RolledBack {
        attempted: Version,
        current: Version,
    },
    /// The new version did not start and the previous one could not be put back; it waits in
    /// `Syrinx.previous` for the owner, who is told at the next start.
    RollbackFailed {
        attempted: Version,
        current: Version,
    },
}

/// Apply the staged update after the old Syrinx exits. On Restart now, whichever version is
/// installed at the end is started, on every path.
pub fn apply_update(
    request: &ApplyRequest<'_>,
    launcher: &mut dyn Launcher,
) -> Result<Applied, InstallError> {
    let applied = apply_locked(request, launcher);
    #[cfg(test)]
    if matches!(applied, Err(InstallError::Interrupted(_))) {
        // A fault hook stands in for a crash, which records and starts nothing.
        return applied;
    }
    // After the swap the new version's own records apply; recording "stayed on the old
    // version" or relaunching unwatched would both be wrong.
    if matches!(applied, Err(InstallError::AfterSwap(_))) {
        return applied;
    }
    let applied = applied.map_err(|error| record_unfinished(request.layout, error));
    let started = matches!(
        applied,
        Ok(Applied::Installed(_) | Applied::RolledBack { .. } | Applied::RollbackFailed { .. })
    );
    if request.mode == ApplyMode::Restart && !started {
        start_installed(request, launcher);
    }
    applied
}

/// Every step that needs the lock, which is released when this returns.
fn apply_locked(
    request: &ApplyRequest<'_>,
    launcher: &mut dyn Launcher,
) -> Result<Applied, InstallError> {
    let layout = request.layout;
    let timing = request.timing;
    let lock = ApplyLock::wait(layout, timing.exit_wait, timing.poll)?;
    checkpoint(Step::ExitObserved)?;
    let pending = Pending::read(layout)?.ok_or(InstallError::NothingStaged)?;
    if let Err(refusal) = reverify(layout, request.keys, &pending) {
        discard_refused(layout)?;
        let result = UpdateResult::VerificationFailed;
        finish(layout, &pending, result, pending.from, true)?;
        eprintln!("reverification refused the staged update: {refusal}");
        return Ok(Applied::Discarded(pending.version));
    }
    checkpoint(Step::Reverified)?;
    if !swap(layout, timing)? {
        finish(
            layout,
            &pending,
            UpdateResult::ApplyFailed,
            pending.from,
            true,
        )?;
        return Ok(Applied::NotSwapped(pending.version));
    }
    checkpoint(Step::Swapped)?;
    let applied = match request.mode {
        ApplyMode::Quit => finish(
            layout,
            &pending,
            UpdateResult::Installed,
            pending.version,
            true,
        )
        .map(|()| Applied::Installed(pending.version)),
        ApplyMode::Restart => restart(request, launcher, &pending, lock),
    };
    applied.map_err(after_swap)
}

/// Mark an error as happening once the new version is in place.
fn after_swap(error: InstallError) -> InstallError {
    #[cfg(test)]
    if matches!(error, InstallError::Interrupted(_)) {
        return error;
    }
    InstallError::AfterSwap(Box::new(error))
}

/// Record an unexpected stop, so the next start tells the owner; the update stays staged.
fn record_unfinished(layout: &UpdateLayout, error: InstallError) -> InstallError {
    eprintln!("the update stopped: {error}");
    let recorded = Pending::read(layout).and_then(|pending| {
        let Some(pending) = pending else {
            return Ok(());
        };
        let mut state = UpdateState::read(layout)?;
        state.result = Some(UpdateResult::ApplyFailed);
        state.version = Some(pending.version);
        state.write(layout)?;
        let outcome = Outcome {
            result: UpdateResult::ApplyFailed,
            attempted: pending.version,
            current: pending.from,
        };
        outcome.write(layout)
    });
    match recorded {
        Ok(()) => error,
        Err(cleanup) => InstallError::Incomplete {
            primary: Box::new(error),
            cleanup: Box::new(cleanup),
        },
    }
}

/// Start whatever is installed, so Restart now never leaves the owner without Syrinx.
fn start_installed(request: &ApplyRequest<'_>, launcher: &mut dyn Launcher) {
    let executable = request.layout.executable();
    if let Err(error) = launcher.launch(&executable, &relaunch(request)) {
        eprintln!(
            "the installed version could not be started: {}",
            error.kind()
        );
    }
}

/// Recheck `Syrinx.new` against the kept, signed manifest, as staging checked `incoming`.
fn reverify(layout: &UpdateLayout, keys: &KeySet, pending: &Pending) -> Result<(), InstallError> {
    const WHAT: &str = "reading the kept manifest";
    let manifest = read_small(&layout.apply_manifest(), MAX_MANIFEST_BYTES, WHAT)?
        .ok_or(InstallError::NothingStaged)?;
    let signature = read_small(&layout.apply_signature(), SIGNATURE_BYTES, WHAT)?
        .ok_or(InstallError::NothingStaged)?;
    if cng::sha256(&manifest)? != pending.manifest_sha256 {
        return Err(InstallError::StagedManifestChanged);
    }
    let verified = verify_manifest(&manifest, &signature, keys, pending.from)?;
    let version = verified.manifest().version();
    if version != pending.version {
        return Err(InstallError::StagedVersionMismatch {
            pending: pending.version,
            manifest: version,
        });
    }
    // The handles are released before the swap: Windows cannot rename a folder held open.
    verified
        .verify_payload(DirectoryPayload::open(&layout.staged())?)
        .map(drop)?;
    Ok(())
}

/// `Syrinx` becomes `Syrinx.previous`, then `Syrinx.new` becomes `Syrinx`. A failed first
/// rename changes nothing, and a failed second one is undone; either way `false` means the
/// installed version stayed.
fn swap(layout: &UpdateLayout, timing: ApplyTiming) -> Result<bool, InstallError> {
    remove_program_folder(&layout.previous(), "Syrinx.previous")?;
    checkpoint(Step::PreviousRemoved)?;
    let aside = rename_patiently(
        &layout.program(),
        &layout.previous(),
        timing.rename_patience,
        "moving the installed version aside",
    );
    if let Err(error) = aside {
        eprintln!("the installed version could not be moved aside: {error}");
        return Ok(false);
    }
    checkpoint(Step::ProgramMovedAside)?;
    let installed = rename_patiently(
        &layout.staged(),
        &layout.program(),
        timing.rename_patience,
        "moving the new version into place",
    );
    if let Err(error) = installed {
        rename_patiently(
            &layout.previous(),
            &layout.program(),
            timing.rename_patience,
            "restoring the installed version",
        )?;
        eprintln!("the new version could not be moved into place: {error}");
        return Ok(false);
    }
    Ok(true)
}

/// Start the new Syrinx and wait for its clean-start record; roll back if it never comes.
fn restart(
    request: &ApplyRequest<'_>,
    launcher: &mut dyn Launcher,
    pending: &Pending,
    lock: ApplyLock,
) -> Result<Applied, InstallError> {
    let layout = request.layout;
    let started = match start_and_watch(request, launcher, pending) {
        Ok(started) => started,
        #[cfg(test)]
        Err(InstallError::Interrupted(step)) => return Err(InstallError::Interrupted(step)),
        // An unconfirmed new version is never left running unwatched: roll back instead.
        Err(error) => {
            eprintln!("watching the new version failed: {error}");
            false
        }
    };
    if started {
        // The owner pressed Restart now and is looking at the new version: nothing to tell.
        finish(
            layout,
            pending,
            UpdateResult::Installed,
            pending.version,
            false,
        )?;
        remove_file_if_present(&layout.started(), "clearing the start record")?;
        return Ok(Applied::Installed(pending.version));
    }
    roll_back(request, launcher, pending, lock)
}

/// Start the new Syrinx and report whether it recorded a clean start.
fn start_and_watch(
    request: &ApplyRequest<'_>,
    launcher: &mut dyn Launcher,
    pending: &Pending,
) -> Result<bool, InstallError> {
    let layout = request.layout;
    remove_file_if_present(&layout.started(), "clearing the start record")?;
    let mut program = match launcher.launch(&layout.executable(), &relaunch(request)) {
        Ok(program) => program,
        Err(error) => {
            eprintln!("the new version could not be started: {}", error.kind());
            return Ok(false);
        }
    };
    checkpoint(Step::Launched)?;
    let watched = wait_for_clean_start(layout, program.as_mut(), pending.version, request.timing);
    if matches!(watched, Ok(true)) {
        return watched;
    }
    // Not confirmed, or watching failed: stop it before rolling back.
    program
        .stop()
        .map_err(super::error::io_error("stopping the new version"))?;
    watched
}

/// `true` once the new Syrinx records a clean start for `version`, `false` when it exits
/// first or the wait runs out.
fn wait_for_clean_start(
    layout: &UpdateLayout,
    program: &mut dyn LaunchedProgram,
    version: Version,
    timing: ApplyTiming,
) -> Result<bool, InstallError> {
    let deadline = Instant::now() + timing.start_wait;
    loop {
        if super::startup::started_version(layout)? == Some(version) {
            return Ok(true);
        }
        let exited = program
            .has_exited()
            .map_err(super::error::io_error("watching the new version"))?;
        if exited || Instant::now() >= deadline {
            return Ok(false);
        }
        thread::sleep(timing.poll);
    }
}

/// Put `Syrinx.previous` back, tell the owner once, and start the previous version.
fn roll_back(
    request: &ApplyRequest<'_>,
    launcher: &mut dyn Launcher,
    pending: &Pending,
    lock: ApplyLock,
) -> Result<Applied, InstallError> {
    let layout = request.layout;
    // Without `pending` but with the staging mark, a crash part-way leaves the failed copy for
    // the next start to remove.
    write_atomically(
        &layout.staging_marker(),
        b"rollback\n",
        "marking the rollback",
    )?;
    remove_file_if_present(&layout.pending(), "removing the pending record")?;
    if let Err(error) = restore_previous(layout, request.timing.rename_patience) {
        #[cfg(test)]
        if matches!(error, InstallError::Interrupted(_)) {
            return Err(error);
        }
        // The owner is told at the next start; nothing is started, as the failed build may
        // still be the installed one.
        eprintln!("the previous version could not be restored: {error}");
        let result = UpdateResult::RollbackFailed;
        finish(layout, pending, result, pending.from, true)?;
        return Ok(Applied::RollbackFailed {
            attempted: pending.version,
            current: pending.from,
        });
    }
    finish(
        layout,
        pending,
        UpdateResult::ApplyRolledBack,
        pending.from,
        true,
    )?;
    // The failed copy keeps the staging mark, so the next start removes it if this cannot.
    let removed = remove_program_folder(&layout.staged(), "Syrinx.new").and_then(|()| {
        remove_file_if_present(&layout.staging_marker(), "clearing the staging mark")
    });
    if let Err(error) = removed {
        eprintln!("the failed version stays for the next start to remove: {error}");
    }
    // The previous Syrinx must find the lock free to read its outcome.
    drop(lock);
    start_installed(request, launcher);
    Ok(Applied::RolledBack {
        attempted: pending.version,
        current: pending.from,
    })
}

/// Move the failed `Syrinx` aside to `Syrinx.new`, then `Syrinx.previous` back into place.
fn restore_previous(layout: &UpdateLayout, patience: Duration) -> Result<(), InstallError> {
    rename_patiently(
        &layout.program(),
        &layout.staged(),
        patience,
        "moving the failed version aside",
    )?;
    checkpoint(Step::FailedMovedAside)?;
    rename_patiently(
        &layout.previous(),
        &layout.program(),
        patience,
        "restoring the previous version",
    )
}

fn relaunch(request: &ApplyRequest<'_>) -> Vec<OsString> {
    let mut arguments = vec![OsString::from(AFTER_UPDATE_FLAG)];
    arguments.extend(request.relaunch_arguments.iter().cloned());
    arguments
}

/// Remove a staged copy that failed its second check, keeping the running helper itself.
fn discard_refused(layout: &UpdateLayout) -> Result<(), InstallError> {
    remove_file_if_present(&layout.pending(), "removing the pending record")?;
    remove_program_folder(&layout.staged(), "Syrinx.new")?;
    remove_file_if_present(&layout.apply_manifest(), "removing the kept manifest")?;
    remove_file_if_present(&layout.apply_signature(), "removing the kept manifest")
}

/// Clear `pending` and record the result; with `tell_owner`, leave the outcome for the next
/// Syrinx to show once.
fn finish(
    layout: &UpdateLayout,
    pending: &Pending,
    result: UpdateResult,
    current: Version,
    tell_owner: bool,
) -> Result<(), InstallError> {
    remove_file_if_present(&layout.pending(), "removing the pending record")?;
    let mut state = UpdateState::read(layout)?;
    state.result = Some(result);
    state.version = Some(pending.version);
    state.write(layout)?;
    eprintln!("update {}: {result}", pending.version);
    if !tell_owner {
        return Ok(());
    }
    let outcome = Outcome {
        result,
        attempted: pending.version,
        current,
    };
    outcome.write(layout)
}
