//! `syrinx-setup.exe`: installs Syrinx for this Windows user from the package it carries, and
//! upgrades that install in place when run again. See `docs/p1-37-installer-design.md`.

#![windows_subsystem = "windows"]

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

use syrinx_platform::setup::{self, Confirmation};
use syrinx_update::Version;
use syrinx_update::install::{
    Existing, InstallError, PREVIOUS_FOLDER, PROGRAM_FOLDER, SetupPackage, UpdateLayout,
    existing_install, install_package,
};

/// The package `cargo xtask package` packed into this program; empty in ordinary builds.
static PAYLOAD: &[u8] = include_bytes!(env!("SYRINX_SETUP_PAYLOAD_FILE"));

/// Installed Syrinx's profile under `%LOCALAPPDATA%`, the same as the `installed` app feature.
const DATA_FOLDER: &str = r"Syrinx\data";

/// The browser key `register-syrinx.ps1` writes; its presence means Syrinx is registered.
const BROWSER_KEY: &str = r"Software\Clients\StartMenuInternet\SyrinxBrowser";

/// How long a just-closed Syrinx may hold its folder before the swap gives up.
const RENAME_PATIENCE: Duration = Duration::from_secs(10);

/// Windows PowerShell, by its fixed system path so `PATH` cannot substitute another program.
const POWERSHELL: &str = r"System32\WindowsPowerShell\v1.0\powershell.exe";

/// Keeps the registration script's console window from flashing up.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Why setup stopped: whether Syrinx's files went in decides the dialog's heading.
enum Stop {
    NotInstalled(String),
    NotRegistered(String),
}

fn main() -> ExitCode {
    let (heading, message) = match run() {
        Ok(()) => return ExitCode::SUCCESS,
        Err(Stop::NotInstalled(message)) => ("Syrinx was not installed", message),
        Err(Stop::NotRegistered(message)) => (
            "Syrinx is installed, but Windows does not list it yet",
            message,
        ),
    };
    setup::show_error(heading, &message);
    ExitCode::FAILURE
}

fn run() -> Result<(), Stop> {
    let package = SetupPackage::read(PAYLOAD).map_err(|error| {
        Stop::NotInstalled(format!(
            "This setup file is damaged or incomplete ({error}). Download it again from the \
             Syrinx release page."
        ))
    })?;
    let layout = layout().map_err(Stop::NotInstalled)?;
    let existing = existing_install(&layout).map_err(not_installed)?;
    let Some(register) = confirm(&layout, existing, package.version()) else {
        return Ok(());
    };
    if !wait_until_closed(&layout).map_err(Stop::NotInstalled)? {
        return Ok(());
    }
    let version = install_package(&layout, &package, RENAME_PATIENCE).map_err(not_installed)?;
    register_with_windows(&layout, register).map_err(Stop::NotRegistered)?;
    finish(&layout, version);
    Ok(())
}

fn not_installed(error: InstallError) -> Stop {
    Stop::NotInstalled(describe(&error))
}

/// `%LOCALAPPDATA%\Programs` for the program and `%LOCALAPPDATA%\Syrinx\data` for the profile.
fn layout() -> Result<UpdateLayout, String> {
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or("Windows did not say where this user's local app data is (%LOCALAPPDATA%).")?;
    UpdateLayout::new(local.join("Programs"), local.join(DATA_FOLDER))
        .map_err(|error| describe(&error))
}

/// Ask before changing anything; `None` when the owner cancels.
fn confirm(layout: &UpdateLayout, existing: Existing, version: Version) -> Option<bool> {
    let heading = heading(existing, version);
    let mut details = format!(
        "Syrinx goes into {} for this Windows user only, without administrator rights. \
         Your tabs, history and sign-ins in {} are kept.",
        layout.program().display(),
        layout.data().display()
    );
    if existing != Existing::Nothing {
        details.push_str(" The version it replaces is kept as Syrinx.previous.");
    }
    let checked = existing == Existing::Nothing || setup::user_key_exists(BROWSER_KEY);
    setup::confirm(&Confirmation {
        heading: &heading,
        details: &details,
        action: if existing == Existing::Nothing {
            "Install"
        } else {
            "Update"
        },
        checkbox: "Register Syrinx as a browser, so it appears in Settings > Default apps \
                   (you choose the default browser yourself)",
        checked,
    })
}

fn heading(existing: Existing, version: Version) -> String {
    match existing {
        Existing::Nothing => format!("Install Syrinx {version}"),
        Existing::Unknown => format!("Update Syrinx to {version}"),
        Existing::Version(installed) if installed < version => {
            format!("Update Syrinx {installed} to {version}")
        }
        Existing::Version(installed) if installed == version => {
            format!("Reinstall Syrinx {version}")
        }
        Existing::Version(installed) => {
            format!("Replace Syrinx {installed} with the older {version}")
        }
    }
}

/// Wait for the owner to close every Syrinx running from the program folders; `false` on Cancel.
fn wait_until_closed(layout: &UpdateLayout) -> Result<bool, String> {
    let folders = [layout.program(), layout.staged(), layout.previous()];
    loop {
        let running = setup::running_from(&folders)
            .map_err(|error| format!("Setup could not check whether Syrinx is running: {error}"))?;
        if !running {
            return Ok(true);
        }
        let retry = setup::ask_retry(
            "Close Syrinx first",
            "Syrinx is running. Close every Syrinx window, then press Retry.",
        );
        if !retry {
            return Ok(false);
        }
    }
}

/// Add the Installed apps entry and Start menu shortcut, and the browser keys if asked for.
fn register_with_windows(layout: &UpdateLayout, register: bool) -> Result<(), String> {
    let program = layout.program();
    if register {
        return run_script(&program, "register-syrinx.ps1", &[]);
    }
    // Unticked after an earlier registration: remove it, then add back only the app entry.
    if setup::user_key_exists(BROWSER_KEY) {
        run_script(&program, "unregister-syrinx.ps1", &[])?;
    }
    run_script(&program, "register-syrinx.ps1", &["-AppOnly"])
}

/// Run one of the installed scripts on `program`, hidden, and report its output if it fails.
fn run_script(program: &Path, script: &str, arguments: &[&str]) -> Result<(), String> {
    let windows = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .ok_or("Windows did not say where it is installed (%SystemRoot%).")?;
    let mut command = Command::new(windows.join(POWERSHELL));
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "RemoteSigned",
            "-File",
        ])
        .arg(program.join(script))
        .arg("-InstallDirectory")
        .arg(program)
        .args(arguments)
        .current_dir(program)
        .creation_flags(CREATE_NO_WINDOW);
    let output = command
        .output()
        .map_err(|error| format!("Setup could not start Windows PowerShell: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{script} failed. Run it from {} to try again:\n\n{}{}",
        program.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

/// Say it is done, and start Syrinx if the owner asks.
fn finish(layout: &UpdateLayout, version: Version) {
    let open = setup::show_finished(
        &format!("Syrinx {version} is installed"),
        "Find it in the Start menu. To make it your default browser, open Settings > Apps > \
         Default apps > Syrinx.",
        "Open Syrinx",
    );
    if !open {
        return;
    }
    // Syrinx runs on its own once started; setup does not wait for it.
    if let Err(error) = Command::new(layout.executable()).spawn() {
        setup::show_error(
            "Syrinx could not be started",
            &format!("{}: {error}", layout.executable().display()),
        );
    }
}

/// A sentence for the owner, naming the folders setup would not replace.
fn describe(error: &InstallError) -> String {
    match error {
        InstallError::UnexpectedEntry { what } => format!(
            "{what} in %LOCALAPPDATA%\\Programs is not a folder Syrinx made, so setup left it \
             alone. Move it aside, then run setup again."
        ),
        InstallError::Busy => "A Syrinx update is being applied right now. Wait a minute, then \
                               run setup again."
            .to_owned(),
        InstallError::ProgramInUse => format!(
            "{PROGRAM_FOLDER} could not be replaced because something still uses it. Close \
             Syrinx and any window showing that folder, then run setup again."
        ),
        InstallError::SetAside(cause) => format!(
            "The installed version was moved to {PREVIOUS_FOLDER} and could not be put back \
             ({cause}). Rename {PREVIOUS_FOLDER} in %LOCALAPPDATA%\\Programs back to \
             {PROGRAM_FOLDER}, or run setup again."
        ),
        other => format!("Setup stopped: {other}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_heading_names_what_setup_will_do() {
        let new = Version::new(0, 3, 0);
        assert_eq!(heading(Existing::Nothing, new), "Install Syrinx 0.3.0");
        assert_eq!(heading(Existing::Unknown, new), "Update Syrinx to 0.3.0");
        let older = Existing::Version(Version::new(0, 2, 0));
        assert_eq!(heading(older, new), "Update Syrinx 0.2.0 to 0.3.0");
        assert_eq!(
            heading(Existing::Version(new), new),
            "Reinstall Syrinx 0.3.0"
        );
        let newer = Existing::Version(Version::new(1, 0, 0));
        assert_eq!(
            heading(newer, new),
            "Replace Syrinx 1.0.0 with the older 0.3.0"
        );
    }

    #[test]
    fn an_ordinary_build_carries_no_package() {
        // Only `cargo xtask package` packs a payload in; this build must refuse to install.
        if PAYLOAD.is_empty() {
            assert!(SetupPackage::read(PAYLOAD).is_err());
        }
    }
}
