//! `syrinx-setup.exe`: installs Syrinx for this Windows user from the package it carries, and
//! upgrades that install in place when run again. See `docs/installing.md`.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
#[path = "syrinx-setup/windows.rs"]
mod windows;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    windows::main()
}

/// Outside Windows Syrinx is run from its build or a distribution package, not this installer.
#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("syrinx-setup installs Syrinx on Windows; on Linux, run `syrinx` itself.");
    std::process::ExitCode::FAILURE
}
