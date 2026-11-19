//! Runs the local development gate and checks the workspace's dependency boundaries.

mod benchmark;
mod deferred_work;
mod dependency_policy;
mod javascript_packages;
mod licences;
mod network_sources;
mod packaging;
mod palette_acceptance;
mod plugin_contract;

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use serde_json::Value;

const USAGE: &str = "Usage: cargo xtask <check | check-deps | check-licences | check-plugin-contract | check-javascript | check-deferred | check-packaging | write-registration | package | sign-update <options> | bench | palette-acceptance>";

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
        if !cfg!(windows) {
            return Err(
                "`cargo xtask sign-update` uses Windows key storage and runs only on Windows."
                    .to_owned(),
            );
        }
        return packaging::sign_update(&root, &unicode_arguments(rest)?);
    }
    if !rest.is_empty() {
        return Err(USAGE.to_owned());
    }
    refuse_windows_only_elsewhere(&command)?;
    match command.to_str() {
        Some("check") => check_workspace(&root),
        Some("check-deps") => check_dependencies(&root),
        Some("check-licences") => check_licences(&root),
        Some("check-plugin-contract") => plugin_contract::check(&root),
        Some("check-javascript") => javascript_packages::check(&root),
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

/// The registration table describes Windows registry keys; Linux packages use per-user scripts.
fn refuse_windows_only_elsewhere(command: &OsString) -> Result<(), String> {
    let windows_only = matches!(command.to_str(), Some("write-registration"));
    if windows_only && !cfg!(windows) {
        return Err(format!(
            "`cargo xtask {}` builds or drives the Windows app and runs only on Windows.",
            command.to_string_lossy()
        ));
    }
    Ok(())
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
    // The tree this executable was built from, and the tree the caller is standing in.
    let built_from = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "xtask must live under the workspace's crates directory.".to_owned())?;
    let working_directory = env::current_dir()
        .map_err(|error| format!("Cannot read the current directory: {error}."))?;
    choose_workspace_root(built_from, &working_directory)
}

/// Refuse to check a source tree other than the one the caller is in (#232).
fn choose_workspace_root(built_from: &Path, working_directory: &Path) -> Result<PathBuf, String> {
    // Outside any workspace, the tree this executable was built from is the only answer.
    let Some(calling_root) = enclosing_workspace(working_directory) else {
        return Ok(built_from.to_path_buf());
    };
    if same_directory(&calling_root, built_from) {
        return Ok(built_from.to_path_buf());
    }
    Err(format!(
        "This xtask was built from {}, but you are in the workspace at {}. Rebuild it there \
         (give each worktree its own CARGO_TARGET_DIR) instead of checking another tree.",
        built_from.display(),
        calling_root.display()
    ))
}

/// The nearest folder at or above `start` whose Cargo.toml declares a workspace.
fn enclosing_workspace(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|folder| {
            std::fs::read_to_string(folder.join("Cargo.toml"))
                .is_ok_and(|manifest| manifest.lines().any(|line| line.trim() == "[workspace]"))
        })
        .map(Path::to_path_buf)
}

/// Compare folders by identity, so symbolic links and `..` do not cause a false mismatch.
fn same_directory(first: &Path, second: &Path) -> bool {
    match (first.canonicalize(), second.canonicalize()) {
        (Ok(first), Ok(second)) => first == second,
        _ => first == second,
    }
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

/// The targets Syrinx ships for; the licence check reads the graph compiled for each.
const SHIPPED_TARGETS: [&str; 2] = ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu"];

/// Cargo's locked, offline, all-features metadata, with `extra` options such as a target filter.
fn cargo_metadata(root: &Path, extra: &[&str]) -> Result<Value, String> {
    let output = cargo_command(root)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--all-features",
            "--offline",
            "--locked",
        ])
        .args(extra)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("Could not run cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata failed ({}).", output.status));
    }

    // Decode without dumping metadata or accepting a partial document.
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Cargo metadata could not be decoded: {error}"))
}

fn check_dependencies(root: &Path) -> Result<(), String> {
    // Read declared dependencies, including inactive optional and target-specific ones, and the
    // resolved graph that the D9 network check follows from each headless crate.
    let metadata = cargo_metadata(root, &[])?;
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

fn check_licences(root: &Path) -> Result<(), String> {
    // Judge what each shipped target compiles, whichever machine runs the gate; the lockfile
    // also records packages for platforms Syrinx does not ship.
    for target in SHIPPED_TARGETS {
        let metadata = cargo_metadata(root, &["--filter-platform", target])?;
        licences::check(root, target, &metadata)?;
    }
    Ok(())
}

fn check_deferred_work(root: &Path) -> Result<(), String> {
    // Allow deferred-work markers only when docs/dev/deferred-work.md lists their issue.
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
    check_licences(root)?;
    plugin_contract::check(root)?;
    javascript_packages::check(root)?;
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
    // It signs through Windows key storage, so only the Windows gate builds it.
    if cfg!(windows) {
        run_cargo(
            root,
            &release_tool_step("clippy", &["--all-targets"], &["--", "-D", "warnings"]),
        )?;
        run_cargo(root, &release_tool_step("test", &[], &[]))?;
    } else {
        println!("The release tool signs through Windows key storage; the Windows gate checks it.");
    }
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

    // Linux proves native page lifecycles and palette behavior on the real X11 display.
    if cfg!(target_os = "linux") {
        // Every Linux entry point compiles this shared module; execute its unit tests once.
        run_cargo(root, &fixture_step("test", "page_close"))?;
        for fixture in [
            "page_close",
            "cookie_persistence",
            "navigation_history_spike",
        ] {
            run_cargo(root, &fixture_step("run", fixture))?;
        }
        palette_acceptance::run(root)?;
    }

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

/// A Linux fixture build or unit run with the same offline graph as the workspace gate.
fn fixture_step<'a>(command: &'a str, fixture: &'a str) -> Vec<&'a str> {
    let mut arguments = vec![
        command,
        "--package",
        "syrinx-platform",
        "--example",
        fixture,
    ];
    arguments.extend(GATE_OPTIONS);
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
    run_cargo_with(root, arguments, &[])
}

/// `run_cargo` with extra environment variables, such as the setup payload's path.
fn run_cargo_with(
    root: &Path,
    arguments: &[&str],
    environment: &[(&str, &Path)],
) -> Result<(), String> {
    // Inherit output so compiler and test failures remain visible to the caller.
    let mut command = cargo_command(root);
    for (name, value) in environment {
        command.env(name, value);
    }
    let status = command
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

    /// A scratch workspace folder with a nested crate folder, removed by the caller.
    fn scratch_workspace(name: &str) -> PathBuf {
        let root = env::temp_dir().join(format!("syrinx-xtask-root-{name}-{}", std::process::id()));
        std::fs::create_dir_all(root.join("crates/inner")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        root
    }

    #[test]
    fn a_gate_built_in_another_tree_refuses_to_check_this_one() {
        let built_from = scratch_workspace("built");
        let calling = scratch_workspace("calling");
        let message = choose_workspace_root(&built_from, &calling.join("crates/inner"))
            .expect_err("a mismatched tree must fail");
        assert!(message.contains("built from"), "{message}");
        assert!(
            message.contains(&calling.display().to_string()),
            "{message}"
        );
        std::fs::remove_dir_all(built_from).unwrap();
        std::fs::remove_dir_all(calling).unwrap();
    }

    #[test]
    fn the_gate_runs_from_any_folder_of_its_own_tree_or_from_outside_one() {
        let built_from = scratch_workspace("same");
        let inside = built_from.join("crates/inner");
        assert_eq!(
            choose_workspace_root(&built_from, &inside).unwrap(),
            built_from
        );
        let outside = env::temp_dir().join(format!("syrinx-xtask-outside-{}", std::process::id()));
        std::fs::create_dir_all(&outside).unwrap();
        assert_eq!(
            choose_workspace_root(&built_from, &outside).unwrap(),
            built_from
        );
        std::fs::remove_dir_all(built_from).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn fixture_units_select_the_example_test_harness_with_gate_options() {
        assert_eq!(
            fixture_step("test", "page_close"),
            [
                "test",
                "--package",
                "syrinx-platform",
                "--example",
                "page_close",
                "--profile",
                "local",
                "--offline",
                "--locked",
            ]
        );
        for fixture in [
            "page_close",
            "cookie_persistence",
            "navigation_history_spike",
        ] {
            let arguments = fixture_step("run", fixture);
            assert_eq!(
                &arguments[..5],
                ["run", "--package", "syrinx-platform", "--example", fixture]
            );
            assert!(arguments.ends_with(&GATE_OPTIONS));
        }
    }

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
