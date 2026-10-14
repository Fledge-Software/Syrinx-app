//! Where an update's folders live: beside the installed program, and in the data folder.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use super::InstallError;

/// The installed program folder under `%LOCALAPPDATA%\Programs` (P1-37).
pub const PROGRAM_FOLDER: &str = "Syrinx";
/// The verified copy waiting to replace the program folder.
pub const STAGED_FOLDER: &str = "Syrinx.new";
/// The version an update replaced, kept for rollback.
pub const PREVIOUS_FOLDER: &str = "Syrinx.previous";
/// Update files inside the data folder; local, never roaming (D13).
pub const UPDATES_FOLDER: &str = "Updates";
/// The program every package must hold.
pub const PROGRAM_EXECUTABLE: &str = "syrinx.exe";
/// The helper that swaps folders once Syrinx has exited.
pub const APPLY_HELPER: &str = "syrinx-apply.exe";
/// P1-37's package record; its presence marks a program folder Syrinx's tooling made.
pub const PACKAGE_RECORD: &str = "syrinx-package.json";

/// The program files a package must list before Syrinx stages it.
pub const REQUIRED_PROGRAM_FILES: [&str; 3] = [PACKAGE_RECORD, PROGRAM_EXECUTABLE, APPLY_HELPER];

/// The folders one installation's updates touch, and nothing else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateLayout {
    programs: PathBuf,
    data: PathBuf,
}

impl UpdateLayout {
    /// `programs` holds the `Syrinx` folder; `data` is the profile folder holding `Updates`.
    ///
    /// Both must be absolute, and the data folder may not sit inside a folder an update
    /// renames, nor the program folders inside the data folder.
    pub fn new(programs: PathBuf, data: PathBuf) -> Result<Self, InstallError> {
        if !programs.is_absolute() || !data.is_absolute() {
            return Err(InstallError::Layout);
        }
        let layout = Self { programs, data };
        let renamed = [layout.program(), layout.staged(), layout.previous()];
        let data_inside_program = renamed.iter().any(|folder| contains(folder, &layout.data));
        if data_inside_program || contains(&layout.data, &layout.programs) {
            return Err(InstallError::Layout);
        }
        Ok(layout)
    }

    /// The layout of the running installed copy: `executable` must be `Syrinx\syrinx.exe`.
    pub fn for_running_program(executable: &Path, data: PathBuf) -> Result<Self, InstallError> {
        let named = |path: Option<&OsStr>, expected: &str| {
            path.and_then(OsStr::to_str)
                .is_some_and(|name| name.eq_ignore_ascii_case(expected))
        };
        let folder = executable.parent().ok_or(InstallError::NotInstalled)?;
        if !named(executable.file_name(), PROGRAM_EXECUTABLE)
            || !named(folder.file_name(), PROGRAM_FOLDER)
        {
            return Err(InstallError::NotInstalled);
        }
        let programs = folder.parent().ok_or(InstallError::NotInstalled)?;
        Self::new(programs.to_path_buf(), data)
    }

    pub fn programs(&self) -> &Path {
        &self.programs
    }

    pub fn data(&self) -> &Path {
        &self.data
    }

    pub fn program(&self) -> PathBuf {
        self.programs.join(PROGRAM_FOLDER)
    }

    pub fn staged(&self) -> PathBuf {
        self.programs.join(STAGED_FOLDER)
    }

    pub fn previous(&self) -> PathBuf {
        self.programs.join(PREVIOUS_FOLDER)
    }

    /// The installed program, which the helper starts after a swap.
    pub fn executable(&self) -> PathBuf {
        self.program().join(PROGRAM_EXECUTABLE)
    }

    pub fn updates(&self) -> PathBuf {
        self.data.join(UPDATES_FOLDER)
    }

    /// Where package files are copied before verification.
    pub fn incoming(&self) -> PathBuf {
        self.updates().join("incoming")
    }

    /// The helper and the signed manifest it rechecks against.
    pub fn apply_folder(&self) -> PathBuf {
        self.updates().join("apply")
    }

    pub fn helper(&self) -> PathBuf {
        self.apply_folder().join(APPLY_HELPER)
    }

    pub fn apply_manifest(&self) -> PathBuf {
        self.apply_folder().join("manifest")
    }

    pub fn apply_signature(&self) -> PathBuf {
        self.apply_folder().join("manifest.sig")
    }

    /// Written last when staging succeeds; a staged copy without it is unfinished.
    pub fn pending(&self) -> PathBuf {
        self.updates().join("pending")
    }

    /// The accepted statement serial and the last result.
    pub fn state(&self) -> PathBuf {
        self.updates().join("state")
    }

    /// What the helper did, for the next Syrinx to tell the owner once.
    pub fn outcome(&self) -> PathBuf {
        self.updates().join("outcome")
    }

    /// Present while `Syrinx.new` is Syrinx's own unfinished work; without it and without
    /// `pending`, a `Syrinx.new` may be `install-syrinx.ps1`'s copy and is left alone.
    pub fn staging_marker(&self) -> PathBuf {
        self.updates().join("staging")
    }

    /// Written by a relaunched Syrinx once it has started cleanly.
    pub fn started(&self) -> PathBuf {
        self.updates().join("started")
    }

    /// Held by whichever of Syrinx and the helper is changing update files.
    pub fn lock(&self) -> PathBuf {
        self.updates().join("apply.lock")
    }

    /// The helper's diagnostics: fixed categories only, never paths.
    pub fn log(&self) -> PathBuf {
        self.updates().join("apply.log")
    }
}

/// Whether `inner` is `outer` or inside it, comparing components as Windows does, without case.
fn contains(outer: &Path, inner: &Path) -> bool {
    let folded = |path: &Path| -> Vec<String> {
        path.components()
            .filter(|component| !matches!(component, Component::CurDir))
            .map(|component| component.as_os_str().to_string_lossy().to_lowercase())
            .collect()
    };
    let outer = folded(outer);
    let inner = folded(inner);
    inner.len() >= outer.len() && inner[..outer.len()] == outer[..]
}
