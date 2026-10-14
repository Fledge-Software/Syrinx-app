//! Runs the local development gate and checks the workspace's dependency boundaries.

mod benchmark;
mod deferred_work;
mod dependency_policy;
mod network_sources;
mod packaging;
mod palette_acceptance;

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

#[cfg(not(target_os = "windows"))]
compile_error!("Syrinx's development gate supports Windows 11 only.");

const USAGE: &str = "Usage: cargo xtask <check | check-deps | check-deferred | check-packaging | write-registration | package | sign-update <options> | bench | palette-acceptance>";

fn main() -> ExitCode {
    // Return a failing process status for every failed gate or invalid invocation.
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    // Keep command parsing closed so misspelled checks cannot silently pass.
    let mut arguments = env::args_os().skip(1);
    let Some(command) = arguments.next() else {
        return Err(USAGE.to_owned());
    };
    let rest: Vec<_> = arguments.collect();
    let root = workspace_root()?;

    // Only `sign-update` takes options, which the release tool itself checks.
    if command == "sign-update" {
        return packaging::sign_update(&root, &unicode_arguments(rest)?);
    }
    if !rest.is_empty() {
        return Err(USAGE.to_owned());
    }
    match command.to_str() {
        Some("check") => check_workspace(&root),
        Some("check-deps") => check_dependencies(&root),
        Some("check-deferred") => check_deferred_work(&root),
        Some("check-packaging") => packaging::check(&root),
        Some("write-registration") => packaging::write_registration(&root),
        Some("package") => packaging::package(&root),
        Some("bench") => benchmark::run(),
        Some("palette-acceptance") => palette_acceptance::run(&root),
        Some("--help" | "-h") => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(USAGE.to_owned()),
    }
}

fn unicode_arguments(arguments: Vec<OsString>) -> Result<Vec<String>, String> {
    arguments
        .into_iter()
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| "Arguments must be Unicode.".to_owned())
        })
        .collect()
}

fn workspace_root() -> Result<PathBuf, String> {
    // Resolve from this crate, independent of the caller's current directory.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask must live under the workspace's crates directory.".to_owned())
}

fn cargo_command(root: &Path) -> Command {
    // Reuse Cargo's executable and prevent child rustup proxies from installing tools.
    let executable = match env::var_os("CARGO") {
        Some(executable) => executable,
        None => "cargo".into(),
    };
    let mut command = Command::new(executable);
    command.current_dir(root).env("RUSTUP_AUTO_INSTALL", "0");
    command
}

fn check_dependencies(root: &Path) -> Result<(), String> {
    // Read declared dependencies, including inactive optional and target-specific ones, and the
    // resolved graph that the D9 network check follows from each headless crate.
    let output = cargo_command(root)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--all-features",
            "--offline",
            "--locked",
        ])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("Could not run cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata failed ({}).", output.status));
    }

    // Report policy violations without dumping metadata or accepting a partial document.
    let metadata = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Cargo metadata could not be decoded: {error}"))?;
    let mut violations = dependency_policy::check_metadata(&metadata)?;
    // Standard-library sockets need no dependency and features unify, so source is read too:
    // the typing path for any network name, every other crate for WinHTTP.
    violations.extend(network_sources::check(root)?);
    if !violations.is_empty() {
        return Err(violations.join("\n"));
    }

    println!("Dependency boundaries passed.");
    Ok(())
}

fn check_deferred_work(root: &Path) -> Result<(), String> {
    // Allow deferred-work markers only when the plan's register tracks them.
    let violations = deferred_work::check(root)?;
    if !violations.is_empty() {
        return Err(violations.join(
            "
",
        ));
    }

    println!("Deferred-work markers are all registered.");
    Ok(())
}

fn check_workspace(root: &Path) -> Result<(), String> {
    // Stop at the first failed check, then prove the application itself can start.
    check_dependencies(root)?;
    check_deferred_work(root)?;
    packaging::check(root)?;
    run_cargo(root, &["fmt", "--all", "--", "--check"])?;
    // The workspace builds as Syrinx ships: without the release tool's key-storage signer.
    run_cargo(
        root,
        &workspace_step("clippy", &["--all-targets"], &["--", "-D", "warnings"]),
    )?;
    run_cargo(root, &workspace_step("test", &[], &[]))?;
    // The release tool is linted and tested on its own, so its feature never reaches the app.
    run_cargo(
        root,
        &release_tool_step("clippy", &["--all-targets"], &["--", "-D", "warnings"]),
    )?;
    run_cargo(root, &release_tool_step("test", &[], &[]))?;
    run_cargo(
        root,
        &[
            "run",
            "--package",
            "syrinx-app",
            "--profile",
            "local",
            "--offline",
            "--locked",
            "--",
            "--smoke",
        ],
    )?;

    println!("Local gate passed.");
    Ok(())
}

/// The common gate options, D38's `local` profile and the locked offline graph.
const GATE_OPTIONS: [&str; 4] = ["--profile", "local", "--offline", "--locked"];

/// A featureless Cargo step over the whole workspace.
fn workspace_step<'a>(command: &'a str, extra: &[&'a str], tail: &[&'a str]) -> Vec<&'a str> {
    let mut arguments = vec![command, "--workspace"];
    arguments.extend(extra);
    arguments.extend(GATE_OPTIONS);
    arguments.extend(tail);
    arguments
}

/// A Cargo step over `syrinx-update` alone with its release-tool feature.
fn release_tool_step<'a>(command: &'a str, extra: &[&'a str], tail: &[&'a str]) -> Vec<&'a str> {
    let mut arguments = vec![
        command,
        "--package",
        packaging::UPDATE_PACKAGE,
        "--features",
        packaging::RELEASE_TOOL_FEATURE,
    ];
    arguments.extend(extra);
    arguments.extend(GATE_OPTIONS);
    arguments.extend(tail);
    arguments
}

fn run_cargo(root: &Path, arguments: &[&str]) -> Result<(), String> {
    // Inherit output so compiler and test failures remain visible to the caller.
    let status = cargo_command(root)
        .args(arguments)
        .status()
        .map_err(|error| format!("Could not run cargo {}: {error}", arguments.join(" ")))?;
    if !status.success() {
        return Err(format!("cargo {} failed ({status}).", arguments.join(" ")));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_workspace_steps_build_without_the_release_tool() {
        // P1-39b: once the app links the verifier, a workspace-wide feature would reach it.
        for step in [
            workspace_step("clippy", &["--all-targets"], &["--", "-D", "warnings"]),
            workspace_step("test", &[], &[]),
        ] {
            assert!(!step.contains(&"--features"), "{step:?}");
            assert!(
                !step
                    .iter()
                    .any(|argument| argument.contains("release-tool"))
            );
            assert!(step.contains(&"--workspace"));
        }
        let lint = release_tool_step("clippy", &["--all-targets"], &["--", "-D", "warnings"]);
        assert_eq!(
            &lint[..5],
            [
                "clippy",
                "--package",
                "syrinx-update",
                "--features",
                "release-tool"
            ]
        );
        assert!(!lint.contains(&"--workspace"));
        assert_eq!(&lint[lint.len() - 3..], ["--", "-D", "warnings"]);
        assert!(release_tool_step("test", &[], &[]).ends_with(&GATE_OPTIONS));
    }
}
