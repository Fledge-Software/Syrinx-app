//! Runs the local development gate and checks the workspace's dependency boundaries.

mod benchmark;
mod deferred_work;
mod dependency_policy;
mod network_sources;
mod packaging;

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

#[cfg(not(target_os = "windows"))]
compile_error!("Atlas's development gate supports Windows 11 only.");

const USAGE: &str = "Usage: cargo xtask <check | check-deps | check-deferred | check-packaging | write-registration | package | bench>";

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
    if arguments.next().is_some() {
        return Err(USAGE.to_owned());
    }

    let root = workspace_root()?;
    match command.to_str() {
        Some("check") => check_workspace(&root),
        Some("check-deps") => check_dependencies(&root),
        Some("check-deferred") => check_deferred_work(&root),
        Some("check-packaging") => packaging::check(&root),
        Some("write-registration") => packaging::write_registration(&root),
        Some("package") => packaging::package(&root),
        Some("bench") => benchmark::run(),
        Some("--help" | "-h") => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(USAGE.to_owned()),
    }
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
    // Standard-library sockets need no dependency, so the typing path's source is read too.
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
    run_cargo(
        root,
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--profile",
            "local",
            "--offline",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run_cargo(
        root,
        &[
            "test",
            "--workspace",
            "--profile",
            "local",
            "--offline",
            "--locked",
        ],
    )?;
    run_cargo(
        root,
        &[
            "run",
            "--package",
            "atlas-app",
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
