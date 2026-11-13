//! Builds the synthetic test plugins in `tests/fixtures/plugins` once per test
//! run, for `wasm32-wasip2`, and hands out their bytes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use syrinx_plugin_host::manifest::CapabilityKind;
use syrinx_plugin_host::test_host::{Declarations, DeclaredCommand, SyntheticGrants};

/// A fixture: a package of the fixture workspace, built with at most one feature.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fixture {
    pub package: &'static str,
    pub feature: Option<&'static str>,
}

pub const FULL: Fixture = Fixture {
    package: "full",
    feature: None,
};
pub const MINIMAL: Fixture = Fixture {
    package: "minimal",
    feature: None,
};
pub const PLAIN_VARIANT: Fixture = Fixture {
    package: "variant",
    feature: None,
};

pub fn variant(feature: &'static str) -> Fixture {
    Fixture {
        package: "variant",
        feature: Some(feature),
    }
}

/// The component's bytes, building it on first use.
pub fn bytes(fixture: Fixture) -> Vec<u8> {
    static BUILT: OnceLock<Mutex<HashMap<Fixture, Vec<u8>>>> = OnceLock::new();
    let built = BUILT.get_or_init(|| Mutex::new(HashMap::new()));
    let mut built = built
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    built
        .entry(fixture)
        .or_insert_with(|| build(fixture))
        .clone()
}

fn fixture_workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plugins")
}

/// Build one fixture with the workspace's pinned toolchain, offline and locked.
/// Its twin is `full_component` in crates/syrinx-app/tests/plugin_package_load.rs; change both.
fn build(fixture: Fixture) -> Vec<u8> {
    let target_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("plugin-fixtures");
    let mut command = Command::new(env!("CARGO"));
    command
        .current_dir(fixture_workspace())
        .args([
            "build",
            "--release",
            "--offline",
            "--locked",
            "--target",
            "wasm32-wasip2",
        ])
        .args(["--package", fixture.package])
        .arg("--target-dir")
        .arg(&target_dir);
    if let Some(feature) = fixture.feature {
        command.args(["--features", feature]);
    }
    let output = command.output().expect("cargo starts");
    assert!(
        output.status.success(),
        "building fixture {} {:?} failed:\n{}",
        fixture.package,
        fixture.feature,
        String::from_utf8_lossy(&output.stderr)
    );
    let wasm = target_dir.join(format!("wasm32-wasip2/release/{}.wasm", fixture.package));
    std::fs::read(&wasm).expect("the fixture's component was written")
}

/// Grants the full fixture, plugin `test`, what its `echo` uses (its settings and its feed),
/// as the owner would at install.
pub fn fixture_grants() -> SyntheticGrants {
    let grants = SyntheticGrants::new();
    grants.grant("test", CapabilityKind::Storage);
    grants.grant("test", CapabilityKind::Index);
    grants
}

/// What the full fixture's manifest would declare.
pub fn full_declarations() -> Declarations {
    let command = |id: &str, form: bool| DeclaredCommand {
        id: id.into(),
        form,
    };
    let mut commands: Vec<DeclaredCommand> = [
        "test.echo",
        "test.recall",
        "test.fail",
        "test.spin",
        "test.grow",
        "test.alloc",
        "test.open",
        "test.print",
        "test.panic",
        "test.exit",
        "test.sleep",
        "test.clock",
        "test.cancel",
        "test.views",
        "test.events",
        "test.chain",
        "test.slow-chain",
        "test.long-error",
    ]
    .into_iter()
    .map(|id| command(id, false))
    .collect();
    commands.push(command("test.new-item", true));
    commands.push(command("test.too-big", true));
    Declarations {
        plugin_id: "test".into(),
        commands,
        view_kinds: vec!["test.page".into()],
        feeds: vec!["test.items".into()],
    }
}

/// Declarations for a fixture with one plain command.
pub fn one_command(plugin_id: &str, command: &str) -> Declarations {
    Declarations {
        plugin_id: plugin_id.into(),
        commands: vec![DeclaredCommand {
            id: command.into(),
            form: false,
        }],
        view_kinds: Vec::new(),
        feeds: Vec::new(),
    }
}
