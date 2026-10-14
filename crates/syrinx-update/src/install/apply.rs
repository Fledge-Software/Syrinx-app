//! The helper's work once Syrinx has exited: recheck, swap the folders, and roll back a failed start.

use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::fault::{Step, checkpoint};
use super::files::{read_small, remove_file_if_present, remove_program_folder, rename_patiently};
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

/// Starts real processes, detached from the helper's own standard streams.
pub struct ProcessLauncher;

impl Launcher for ProcessLauncher {
    fn launch(
        &mut self,
        executable: &Path,
        arguments: &[OsString],
    ) -> io::Result<Box<dyn LaunchedProgram>> {
        let child = Command::new(executable)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
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
}

/// Apply the staged update after the old Syrinx exits.
pub fn apply_update(
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
        finish(
            layout,
            &pending,
            UpdateResult::VerificationFailed,
            pending.from,
            true,
        )?;
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
    match request.mode {
        ApplyMode::Quit => {
            finish(
                layout,
                &pending,
                UpdateResult::Installed,
                pending.version,
                true,
            )?;
            Ok(Applied::Installed(pending.version))
        }
        ApplyMode::Restart => restart(request, launcher, &pending, lock),
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

/// `Syrinx` becomes `Syrinx.previous`, then `Syrinx.new` becomes `Syrinx`; on a failed second
/// rename the first is undone. `false` means the installed version stayed.
fn swap(layout: &UpdateLayout, timing: ApplyTiming) -> Result<bool, InstallError> {
    remove_program_folder(&layout.previous(), "Syrinx.previous")?;
    checkpoint(Step::PreviousRemoved)?;
    rename_patiently(
        &layout.program(),
        &layout.previous(),
        timing.rename_patience,
        "moving the installed version aside",
    )?;
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
    remove_file_if_present(&layout.started(), "clearing the start record")?;
    let started = match launcher.launch(&layout.executable(), &relaunch(request)) {
        Ok(program) => {
            checkpoint(Step::Launched)?;
            wait_for_clean_start(layout, program, pending.version, request.timing)?
        }
        Err(error) => {
            eprintln!("the new version could not be started: {}", error.kind());
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

/// `true` once the new Syrinx records a clean start for `version`; stops it otherwise.
fn wait_for_clean_start(
    layout: &UpdateLayout,
    mut program: Box<dyn LaunchedProgram>,
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
            break;
        }
        thread::sleep(timing.poll);
    }
    program
        .stop()
        .map_err(super::error::io_error("stopping the new version"))?;
    Ok(false)
}

/// Put `Syrinx.previous` back, tell the owner once, and start the previous version.
fn roll_back(
    request: &ApplyRequest<'_>,
    launcher: &mut dyn Launcher,
    pending: &Pending,
    lock: ApplyLock,
) -> Result<Applied, InstallError> {
    let layout = request.layout;
    let patience = request.timing.rename_patience;
    // Without `pending`, a crash part-way leaves the failed copy for the next start to remove.
    remove_file_if_present(&layout.pending(), "removing the pending record")?;
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
    )?;
    finish(
        layout,
        pending,
        UpdateResult::ApplyRolledBack,
        pending.from,
        true,
    )?;
    // The failed copy has no `pending`, so the next start removes it if this cannot.
    if let Err(error) = remove_program_folder(&layout.staged(), "Syrinx.new") {
        eprintln!("the failed version stays for the next start to remove: {error}");
    }
    // The previous Syrinx must find the lock free to read its outcome.
    drop(lock);
    if let Err(error) = launcher.launch(&layout.executable(), &relaunch(request)) {
        eprintln!(
            "the previous version could not be restarted: {}",
            error.kind()
        );
    }
    Ok(Applied::RolledBack {
        attempted: pending.version,
        current: pending.from,
    })
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
