//! Builds the plugin host's full test fixture for syrinx-app's tests: the package test here
//! and the session's unit tests include this one file. Its twin is `build` in
//! crates/syrinx-plugin-host/tests/support/mod.rs, which another crate's tests cannot share.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The fixture workspace that `syrinx-plugin-host`'s contract tests also build.
pub fn fixture_workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../syrinx-plugin-host/tests/fixtures/plugins")
}

/// The full fixture's component, built once per test binary, offline and locked, into
/// `target_dir`.
pub fn full_component(target_dir: &Path) -> &'static [u8] {
    static BUILT: OnceLock<Vec<u8>> = OnceLock::new();
    BUILT.get_or_init(|| {
        let output = Command::new(env!("CARGO"))
            .current_dir(fixture_workspace())
            .args(["build", "--release", "--offline", "--locked"])
            .args(["--target", "wasm32-wasip2", "--package", "full"])
            .arg("--target-dir")
            .arg(target_dir)
            .output()
            .expect("cargo starts");
        assert!(
            output.status.success(),
            "building the full fixture failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        fs::read(target_dir.join("wasm32-wasip2/release/full.wasm")).expect("the component")
    })
}
