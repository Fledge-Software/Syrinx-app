//! Connects Syrinx to P1-39b's installer: which copy is running, what start-up finds, and the
//! handover to `syrinx-apply.exe` once Syrinx has closed.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use syrinx_update::install::{
    ApplyMode, Handoff, InstallError, Outcome, Pending, StageRequest, Staged, UpdateLayout,
    UpdateResult, prepare_handoff, record_started, recover, stage_from_folder,
};
use syrinx_update::{KeySet, Timestamp, UpdateError, Version};

/// What this launch knows about updating itself, shared by the window and `main`.
#[derive(Clone)]
pub struct UpdateContext {
    /// The update folders, or why this copy cannot install updates.
    layout: Result<UpdateLayout, InstallError>,
    running: Option<Version>,
    /// The test profile to reopen after an update; `None` for the installed data folder.
    relaunch_data: Option<PathBuf>,
    /// Started by the helper: skip the launch activation and report a clean start.
    after_update: bool,
    /// How the window asked to hand over, read by `main` after the window closes.
    requested: Arc<Mutex<Option<ApplyMode>>>,
}

impl UpdateContext {
    /// `data` is the profile folder; `test_data` is set only by `--test-data-dir`.
    pub fn new(data: &Path, test_data: Option<&Path>, after_update: bool) -> Self {
        let layout = installed_layout(data);
        let relaunch_data = test_data.and_then(|folder| std::path::absolute(folder).ok());
        Self {
            layout,
            running: Version::parse(env!("CARGO_PKG_VERSION")),
            relaunch_data,
            after_update,
            requested: Arc::new(Mutex::new(None)),
        }
    }

    /// A launch that never installs, such as the smoke run's fresh synthetic profile.
    pub fn disabled() -> Self {
        Self {
            layout: Err(InstallError::NotInstalled),
            running: None,
            relaunch_data: None,
            after_update: false,
            requested: Arc::new(Mutex::new(None)),
        }
    }

    pub fn running(&self) -> Option<Version> {
        self.running
    }

    pub fn after_update(&self) -> bool {
        self.after_update
    }

    /// Whether this copy can install updates, or the owner's explanation why not.
    pub fn availability(&self) -> Result<(), String> {
        match (&self.layout, self.running) {
            (Ok(_), Some(_)) => Ok(()),
            (Err(error), _) => Err(refusal_message(error)),
            (Ok(_), None) => {
                Err("This build's version cannot be read, so it cannot update.".into())
            }
        }
    }

    /// Finish or clear what an earlier update left, before the window opens.
    pub fn recover(&self) -> StartupUpdates {
        let (Ok(layout), Some(running)) = (&self.layout, self.running) else {
            return StartupUpdates::default();
        };
        match recover(layout, running) {
            Ok(recovery) => StartupUpdates {
                notice: recovery.outcome.as_ref().and_then(outcome_message),
                staged: recovery.staged,
            },
            Err(error) => {
                eprintln!("update recovery: {error}");
                StartupUpdates {
                    notice: Some(format!("Syrinx couldn't tidy its update files: {error}.")),
                    staged: None,
                }
            }
        }
    }

    /// The complete staged version waiting for this Syrinx, if any; reads a small file.
    pub fn staged(&self) -> Option<Version> {
        let layout = self.layout.as_ref().ok()?;
        let pending = match Pending::read(layout) {
            Ok(pending) => pending?,
            Err(error) => {
                eprintln!("update record: {error}");
                return None;
            }
        };
        let complete = Some(pending.from) == self.running && layout.staged().is_dir();
        complete.then_some(pending.version)
    }

    /// Stage the package in `source`; blocks, so call it off the interface thread.
    pub fn stage(&self, source: &Path) -> Result<Staged, InstallError> {
        let layout = self.layout.as_ref().map_err(Clone::clone)?;
        let installed = self.running.ok_or(InstallError::NotInstalled)?;
        // Only the keys compiled into Syrinx are trusted; there is no other route (P1-38).
        let keys = KeySet::production()?;
        let request = StageRequest {
            source,
            keys: &keys,
            installed,
            now: Timestamp::now()?,
        };
        stage_from_folder(layout, &request, &syrinx_platform::disk::free_bytes)
    }

    /// A job that records this relaunched Syrinx's clean start, or `None` when not relaunched.
    pub fn clean_start_job(
        &self,
    ) -> Option<impl FnOnce() -> Result<(), InstallError> + Send + use<>> {
        if !self.after_update {
            return None;
        }
        let layout = self.layout.clone().ok()?;
        let running = self.running?;
        Some(move || record_started(&layout, running))
    }

    /// Ask `main` to start the helper in `mode` once the window has closed.
    pub fn request(&self, mode: ApplyMode) {
        match self.requested.lock() {
            Ok(mut requested) => *requested = Some(mode),
            Err(_) => eprintln!("The update request could not be recorded."),
        }
    }

    pub fn requested(&self) -> Option<ApplyMode> {
        match self.requested.lock() {
            Ok(requested) => *requested,
            Err(_) => {
                eprintln!("The update request could not be read.");
                None
            }
        }
    }

    /// Check the helper, start it, and return what must stay held until Syrinx exits.
    pub fn hand_off(&self, mode: ApplyMode) -> Result<Handoff, String> {
        let layout = self.layout.as_ref().map_err(ToString::to_string)?;
        let keys = KeySet::production().map_err(|error| error.to_string())?;
        let handoff = prepare_handoff(layout, &keys).map_err(|error| error.to_string())?;
        let log = File::create(layout.log()).map_err(|error| error.kind().to_string())?;
        let mut command = Command::new(layout.helper());
        command
            .arg("--programs")
            .arg(layout.programs())
            .arg("--data")
            .arg(layout.data())
            .arg("--mode")
            .arg(mode.as_str());
        if let Some(folder) = &self.relaunch_data {
            command.arg("--relaunch-data-dir").arg(folder);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .map_err(|error| error.kind().to_string())?;
        Ok(handoff)
    }
}

/// What start-up found: something to tell the owner once, and a staged version.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StartupUpdates {
    pub notice: Option<String>,
    pub staged: Option<Version>,
}

/// The installed copy's folders: only an `installed` build running from `Programs\Syrinx`.
fn installed_layout(data: &Path) -> Result<UpdateLayout, InstallError> {
    if !cfg!(feature = "installed") {
        return Err(InstallError::NotInstalled);
    }
    let executable = std::env::current_exe().map_err(|error| InstallError::Io {
        step: "finding the running program",
        kind: error.kind(),
    })?;
    let data = std::path::absolute(data).map_err(|error| InstallError::Io {
        step: "finding the data folder",
        kind: error.kind(),
    })?;
    UpdateLayout::for_running_program(&executable, data)
}

/// What staging's result means for the owner.
pub fn staged_message(staged: &Staged) -> String {
    match staged {
        Staged::UpToDate(version) => format!("Syrinx {version} is up to date."),
        Staged::Ready { version, .. } => format!("Syrinx {version} is ready."),
    }
}

/// Why nothing was installed, in the owner's terms; no path appears.
pub fn refusal_message(error: &InstallError) -> String {
    match error {
        InstallError::Update(error) => verifier_message(error),
        InstallError::NotInstalled | InstallError::Layout => {
            "Updates install only into a copy installed with install-syrinx.ps1.".into()
        }
        InstallError::NotAPackage => "That folder is not a signed Syrinx update package.".into(),
        InstallError::Busy => "Another update step is still running. Try again shortly.".into(),
        InstallError::NotEnoughSpace { .. } => {
            "There isn't enough free disk space to stage this update.".into()
        }
        InstallError::MissingProgramFile { .. } => {
            "That package is incomplete, so nothing was installed.".into()
        }
        other => format!("Syrinx couldn't stage the update ({other}), so nothing was installed."),
    }
}

fn verifier_message(error: &UpdateError) -> String {
    match error {
        UpdateError::UnknownKey => {
            "This update isn't signed by a key this Syrinx trusts, so nothing was installed.".into()
        }
        UpdateError::StatementExpired { .. } => {
            "Couldn't confirm the latest version: the update information has expired.".into()
        }
        UpdateError::StatementReplayed { .. } | UpdateError::StatementSerialReused { .. } => {
            "This update information is older than what Syrinx has already accepted, so \
             nothing was installed."
                .into()
        }
        UpdateError::NotNewer { installed, .. } => {
            format!("That package is not newer than the installed Syrinx {installed}.")
        }
        UpdateError::InstalledTooOld { minimum, .. } => {
            format!("This update needs Syrinx {minimum} or later installed first.")
        }
        _ => "The update failed its checks, so nothing was installed.".into(),
    }
}

/// What the helper did, told once at the next start; `None` when nothing needs saying.
pub fn outcome_message(outcome: &Outcome) -> Option<String> {
    let Outcome {
        result,
        attempted,
        current,
    } = outcome;
    Some(match result {
        UpdateResult::Installed => format!("Syrinx was updated to {attempted}."),
        UpdateResult::ApplyRolledBack => {
            format!("The update to {attempted} didn't start; you are on {current}.")
        }
        UpdateResult::VerificationFailed => format!(
            "The update to {attempted} failed its final check, so it was removed and Syrinx \
             stayed on {current}."
        ),
        UpdateResult::ApplyFailed => format!(
            "The update to {attempted} could not replace the installed copy, so Syrinx stayed \
             on {current}."
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ATTEMPTED: Version = Version::new(1, 4, 0);
    const CURRENT: Version = Version::new(1, 3, 0);

    #[test]
    fn a_development_build_never_installs_updates() {
        // `cargo syrinx` and the gate build without `installed`, so nothing beside them moves.
        let context = UpdateContext::new(Path::new(r"C:\Synthetic\data"), None, false);
        assert!(context.availability().is_err());
        assert_eq!(context.recover(), StartupUpdates::default());
        assert!(matches!(
            context.stage(Path::new(r"C:\Synthetic\package")),
            Err(InstallError::NotInstalled)
        ));
        assert!(context.hand_off(ApplyMode::Restart).is_err());
        assert!(UpdateContext::disabled().clean_start_job().is_none());
    }

    #[test]
    fn the_rollback_notice_names_both_versions() {
        let rolled_back = Outcome {
            result: UpdateResult::ApplyRolledBack,
            attempted: ATTEMPTED,
            current: CURRENT,
        };
        assert_eq!(
            outcome_message(&rolled_back).as_deref(),
            Some("The update to 1.4.0 didn't start; you are on 1.3.0.")
        );
        let staged = Outcome {
            result: UpdateResult::Staged,
            ..rolled_back
        };
        assert_eq!(outcome_message(&staged), None);
    }

    #[test]
    fn refusals_explain_without_naming_paths() {
        assert_eq!(
            refusal_message(&InstallError::Update(UpdateError::UnknownKey)),
            "This update isn't signed by a key this Syrinx trusts, so nothing was installed."
        );
        let io = InstallError::Io {
            step: "copying the update package",
            kind: std::io::ErrorKind::PermissionDenied,
        };
        let message = refusal_message(&io);
        assert!(message.contains("copying the update package"));
        assert!(!message.contains('\\'));
    }

    #[test]
    fn a_requested_handover_is_seen_by_every_copy_of_the_context() {
        let context = UpdateContext::disabled();
        let shared = context.clone();
        assert_eq!(shared.requested(), None);
        context.request(ApplyMode::Quit);
        assert_eq!(shared.requested(), Some(ApplyMode::Quit));
    }
}
