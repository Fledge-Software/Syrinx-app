//! Runs the installer in a synthetic home to check optional media warnings and successful copies.

use std::{
    env, fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt, symlink},
    path::PathBuf,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use super::tests::repository;
use crate::packaging::sha256::hex_digest;

struct Installer {
    root: PathBuf,
    package: PathBuf,
    home: PathBuf,
    tools: PathBuf,
}

impl Installer {
    fn new(probe: Option<bool>) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "syrinx-install-media-{}-{stamp}",
            std::process::id()
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for name in [
            "package", "home", "tools", "data", "cache", "config", "runtime", "cwd",
        ] {
            fs::create_dir(root.join(name)).unwrap();
        }
        let fixture = Self {
            package: root.join("package"),
            home: root.join("home"),
            tools: root.join("tools"),
            root,
        };
        // Keep every command deterministic; only ordinary file-copy tools are real.
        for tool in [
            "dirname",
            "sha256sum",
            "grep",
            "mkdir",
            "cp",
            "chmod",
            "mv",
            "rm",
            "rmdir",
        ] {
            symlink(system_tool(tool), fixture.tools.join(tool)).unwrap();
        }
        fixture.tool("id", "printf '1000\\n'");
        fixture.tool("ldconfig", "printf 'libwebkit2gtk-4.1.so\\n'");
        if let Some(available) = probe {
            fixture.tool(
                "gst-inspect-1.0",
                &format!(
                    "[ \"$#\" -eq 1 ] && [ \"$1\" = autoaudiosink ] || exit 64\nexit {}",
                    if available { 0 } else { 1 }
                ),
            );
        }
        // Package integrity remains real, including the script under test.
        fs::copy(
            repository().join("packaging/linux/install.sh"),
            fixture.package.join("install.sh"),
        )
        .unwrap();
        fs::copy(
            repository().join("packaging/linux/uninstall.sh"),
            fixture.package.join("uninstall.sh"),
        )
        .unwrap();
        fs::copy(
            repository().join("packaging/linux/syrinx.desktop"),
            fixture.package.join("syrinx.desktop"),
        )
        .unwrap();
        fs::write(
            fixture.package.join("syrinx"),
            b"synthetic program, never executed\n",
        )
        .unwrap();
        fs::write(fixture.package.join("syrinx.png"), b"synthetic icon\n").unwrap();
        let mut sums = String::new();
        for name in ["install.sh", "syrinx.desktop", "syrinx", "syrinx.png"] {
            let bytes = fs::read(fixture.package.join(name)).unwrap();
            sums.push_str(&format!("{}  {name}\n", hex_digest(&bytes)));
        }
        fs::write(fixture.package.join("SHA256SUMS"), sums).unwrap();
        fixture
    }

    fn tool(&self, name: &str, body: &str) {
        let path = self.tools.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn run(&self) -> std::process::Output {
        self.run_script("install.sh", None)
    }

    /// Runs a package script from the scratch `cwd` folder, optionally overriding XDG_DATA_HOME.
    fn run_script(&self, script: &str, data_home: Option<&str>) -> std::process::Output {
        let mut command = Command::new(system_tool("sh"));
        command
            .arg(self.package.join(script))
            .current_dir(self.root.join("cwd"))
            .env("HOME", &self.home)
            .env("PATH", &self.tools)
            .stdin(Stdio::null());
        for kind in ["DATA", "CACHE", "CONFIG", "RUNTIME"] {
            let variable = if kind == "RUNTIME" {
                "XDG_RUNTIME_DIR".to_owned()
            } else {
                format!("XDG_{kind}_HOME")
            };
            command.env(variable, self.root.join(kind.to_ascii_lowercase()));
        }
        if let Some(value) = data_home {
            command.env("XDG_DATA_HOME", value);
        }
        command.output().unwrap()
    }
}

fn system_tool(name: &str) -> PathBuf {
    env::split_paths(&env::var_os("PATH").expect("test tool path"))
        .map(|directory| directory.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("Test prerequisite missing: {name}"))
}

#[test]
fn media_check_warns_without_blocking_installation() {
    for (probe, warning) in [(Some(false), true), (Some(true), false), (None, false)] {
        let installer = Installer::new(probe);
        let output = installer.run();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(output.status.success(), "probe={probe:?}: {stderr}");
        assert_eq!(
            stderr.contains("autoaudiosink"),
            warning,
            "probe={probe:?}: {stderr}"
        );
        if warning {
            assert!(
                stderr.contains("GStreamer") && stderr.contains("INSTALL.txt"),
                "explain the missing runtime and where to find its packages: {stderr}"
            );
        }
        assert_eq!(
            fs::read(installer.home.join(".local/bin/syrinx")).unwrap(),
            b"synthetic program, never executed\n"
        );
        assert!(
            installer
                .root
                .join("data/applications/syrinx.desktop")
                .is_file()
        );
        fs::remove_dir_all(installer.root).unwrap();
    }
}

#[test]
fn a_relative_xdg_data_home_is_ignored_like_the_app_does() {
    let installer = Installer::new(None);
    let installed = installer
        .home
        .join(".local/share/applications/syrinx.desktop");
    let stray = installer.root.join("cwd/share");
    let output = installer.run_script("install.sh", Some("share"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        installed.is_file(),
        "the entry belongs under HOME's default data folder"
    );
    assert!(
        !stray.exists(),
        "nothing may be written under the current folder"
    );

    let output = installer.run_script("uninstall.sh", Some("share"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!installed.exists(), "uninstall removes what install wrote");
    assert!(!stray.exists());
    assert_eq!(fs::read_dir(installer.root.join("cwd")).unwrap().count(), 0);
    fs::remove_dir_all(installer.root).unwrap();
}
