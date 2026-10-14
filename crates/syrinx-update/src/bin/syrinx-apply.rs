//! `syrinx-apply.exe`: swaps a staged, verified Syrinx into place once Syrinx has exited (P1-39b).
#![cfg_attr(not(test), windows_subsystem = "windows")]

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use syrinx_update::KeySet;
use syrinx_update::install::{
    Applied, ApplyMode, ApplyRequest, ApplyTiming, ProcessLauncher, UpdateLayout, apply_update,
};

const USAGE: &str = "syrinx-apply --programs <folder> --data <folder> --mode <restart|quit> \
                     [--relaunch-data-dir <folder>]";

/// Standard error goes to `Updates\apply.log`, which Syrinx opens for the helper.
fn main() -> ExitCode {
    match run(std::env::args_os().skip(1)) {
        Ok(applied) => {
            eprintln!("syrinx-apply: {}", describe(&applied));
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("syrinx-apply: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<Applied, String> {
    let options = parse(arguments)?;
    let layout = UpdateLayout::new(options.programs, options.data).map_err(|e| e.to_string())?;
    // Windows refuses to rename a folder that is any process's current one, and Syrinx may
    // have been started from `Programs\Syrinx`; work from `Updates`, which is never renamed.
    std::env::set_current_dir(layout.updates())
        .map_err(|error| format!("cannot work from the Updates folder: {}", error.kind()))?;
    // Only the keys compiled into this executable are trusted (P1-38).
    let keys = KeySet::production().map_err(|error| error.to_string())?;
    let request = ApplyRequest {
        layout: &layout,
        keys: &keys,
        mode: options.mode,
        relaunch_arguments: relaunch_arguments(options.relaunch_data),
        timing: ApplyTiming::PRODUCTION,
    };
    let mut launcher = ProcessLauncher::new(layout.updates());
    apply_update(&request, &mut launcher).map_err(|error| error.to_string())
}

/// What the helper was asked to do.
#[derive(Debug, PartialEq)]
struct Options {
    programs: PathBuf,
    data: PathBuf,
    mode: ApplyMode,
    /// The test profile Syrinx ran with, passed on when it is relaunched.
    relaunch_data: Option<PathBuf>,
}

fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Options, String> {
    let mut arguments = arguments.into_iter();
    let (mut programs, mut data, mut mode, mut relaunch_data) = (None, None, None, None);
    while let Some(name) = arguments.next() {
        let value = arguments.next().ok_or(USAGE)?;
        match name.to_str() {
            Some("--programs") if programs.is_none() => programs = Some(PathBuf::from(value)),
            Some("--data") if data.is_none() => data = Some(PathBuf::from(value)),
            Some("--mode") if mode.is_none() => {
                mode = Some(value.to_str().and_then(ApplyMode::parse).ok_or(USAGE)?);
            }
            Some("--relaunch-data-dir") if relaunch_data.is_none() => {
                relaunch_data = Some(PathBuf::from(value));
            }
            _ => return Err(USAGE.to_owned()),
        }
    }
    Ok(Options {
        programs: programs.ok_or(USAGE)?,
        data: data.ok_or(USAGE)?,
        mode: mode.ok_or(USAGE)?,
        relaunch_data,
    })
}

fn relaunch_arguments(data: Option<PathBuf>) -> Vec<OsString> {
    match data {
        Some(data) => vec![OsString::from("--test-data-dir"), data.into_os_string()],
        None => Vec::new(),
    }
}

/// A fixed category and the versions; never a path.
fn describe(applied: &Applied) -> String {
    match applied {
        Applied::Installed(version) => format!("installed {version}"),
        Applied::Discarded(version) => format!("verification_failed {version}"),
        Applied::NotSwapped(version) => format!("apply_failed {version}"),
        Applied::RolledBack { attempted, current } => {
            format!("apply_rolled_back {attempted}, running {current}")
        }
        Applied::RollbackFailed { attempted, current } => {
            format!("apply_rollback_failed {attempted}, {current} kept aside")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(text: &[&str]) -> Vec<OsString> {
        text.iter().map(OsString::from).collect()
    }

    #[test]
    fn reads_each_option_once() {
        let parsed = parse(arguments(&[
            "--programs",
            r"C:\Synthetic\Programs",
            "--data",
            r"C:\Synthetic\data",
            "--mode",
            "restart",
            "--relaunch-data-dir",
            r"C:\Synthetic\data",
        ]))
        .expect("complete options");
        assert_eq!(parsed.mode, ApplyMode::Restart);
        assert_eq!(
            parsed.relaunch_data,
            Some(PathBuf::from(r"C:\Synthetic\data"))
        );
        assert_eq!(
            relaunch_arguments(parsed.relaunch_data),
            arguments(&["--test-data-dir", r"C:\Synthetic\data"])
        );
    }

    #[test]
    fn refuses_missing_repeated_and_unknown_options() {
        for refused in [
            &["--programs", "P", "--data", "D"][..],
            &["--programs", "P", "--data", "D", "--mode", "later"][..],
            &[
                "--programs",
                "P",
                "--programs",
                "P",
                "--data",
                "D",
                "--mode",
                "quit",
            ][..],
            &[
                "--programs",
                "P",
                "--data",
                "D",
                "--mode",
                "quit",
                "--keys",
                "K",
            ][..],
            &["--programs", "P", "--data", "D", "--mode"][..],
        ] {
            assert!(parse(arguments(refused)).is_err(), "{refused:?}");
        }
    }
}
