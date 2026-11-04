//! Builds Syrinx's portable Linux tarball: the program, its desktop entry and icon, and the
//! per-user install scripts. Like the Windows package, it installs nothing itself.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::manifest::{self, PackagedFile};
use super::{build_identity, fresh_package_folder, hashed};

/// Output folder for packages and the separate `installed` build, as on Windows.
const OUTPUT_FOLDER: &str = "target/package";
const PACKAGE_TARGET_DIR: &str = "target/package-build";

/// Cargo's override of the `local` profile's `strip` setting, for the package build only.
const STRIP_VARIABLE: &str = "CARGO_PROFILE_LOCAL_STRIP";

/// The program Cargo builds and the package ships.
const EXECUTABLE: &str = "syrinx";

/// The checksum list `install.sh` verifies; its presence marks a folder as our package.
pub const SUMS: &str = "SHA256SUMS";

/// The shell scripts, which the gate parses with `sh -n` and never runs.
pub const SCRIPTS: [&str; 2] = ["install.sh", "uninstall.sh"];

/// Where one packaged file comes from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Source {
    /// The `installed` build of `syrinx-app`.
    Build,
    /// A file in the repository, relative to its root.
    Repository(&'static str),
}

/// One file of the tarball: its name, source and whether it is executable.
#[derive(Clone, Copy, Debug)]
struct Entry {
    name: &'static str,
    source: Source,
    executable: bool,
}

/// Every file in the package except `SHA256SUMS`, sorted by name.
const LAYOUT: [Entry; 7] = [
    entry(
        "INSTALL.txt",
        Source::Repository("packaging/linux/INSTALL.txt"),
        false,
    ),
    // The Windows package ships no licence file; a Linux binary download carries Syrinx's.
    entry("LICENSE", Source::Repository("LICENSE"), false),
    entry(
        "install.sh",
        Source::Repository("packaging/linux/install.sh"),
        true,
    ),
    entry(EXECUTABLE, Source::Build, true),
    entry(
        "syrinx.desktop",
        Source::Repository("packaging/linux/syrinx.desktop"),
        false,
    ),
    // The brand's 256 px raster, the size install.sh puts in the hicolor theme.
    entry(
        "syrinx.png",
        Source::Repository("design/brand/icon/syrinx-icon-256.png"),
        false,
    ),
    entry(
        "uninstall.sh",
        Source::Repository("packaging/linux/uninstall.sh"),
        true,
    ),
];

const fn entry(name: &'static str, source: Source, executable: bool) -> Entry {
    Entry {
        name,
        source,
        executable,
    }
}

/// Build the `local` program and write `target/package/<name>/` and `<name>.tar.gz`.
pub fn package(root: &Path) -> Result<(), String> {
    // Windows keeps debug information in a separate .pdb; on Linux it would add ~300 MB to the binary.
    crate::run_cargo_with(
        root,
        &[
            "build",
            "--package",
            "syrinx-app",
            "--bin",
            EXECUTABLE,
            "--features",
            "installed",
            "--profile",
            "local",
            "--target-dir",
            PACKAGE_TARGET_DIR,
            "--offline",
            "--locked",
        ],
        &[(STRIP_VARIABLE, Path::new("debuginfo"))],
    )?;

    let identity = build_identity(root)?;
    let name = manifest::linux_package_name(&identity)?;
    let output = root.join(OUTPUT_FOLDER);
    let folder = fresh_package_folder(&output.join(&name), SUMS)?;
    let files = copy_package_files(root, &folder)?;
    fs::write(folder.join(SUMS), render_sums(&files))
        .map_err(|error| format!("Could not write {SUMS}: {error}"))?;
    let archive = tar_package(&output, &name)?;

    println!("Package folder: {}", folder.display());
    println!("Package tarball: {}", archive.display());
    println!("Nothing was installed. See docs/installing.md, \"Linux\".");
    Ok(())
}

/// Refuse a missing package source or, where `sh` exists, a shell script that does not parse.
pub fn check(root: &Path) -> Result<(), String> {
    for path in repository_sources(root) {
        if !path.is_file() {
            return Err(format!("{} is missing.", path.display()));
        }
    }
    if !cfg!(unix) {
        return Ok(());
    }
    for script in SCRIPTS {
        parse_shell(&root.join("packaging/linux").join(script))?;
    }
    Ok(())
}

fn repository_sources(root: &Path) -> Vec<PathBuf> {
    LAYOUT
        .iter()
        .filter_map(|entry| match entry.source {
            Source::Repository(path) => Some(root.join(path)),
            Source::Build => None,
        })
        .collect()
}

/// Parse a script with `sh -n`, which reads it without running any command.
fn parse_shell(script: &Path) -> Result<(), String> {
    let status = Command::new("sh")
        .arg("-n")
        .arg(script)
        .status()
        .map_err(|error| format!("Could not start sh to parse {}: {error}", script.display()))?;
    if !status.success() {
        return Err(format!("{} failed to parse ({status}).", script.display()));
    }
    Ok(())
}

fn source_path(root: &Path, entry: &Entry) -> PathBuf {
    match entry.source {
        Source::Build => root.join(PACKAGE_TARGET_DIR).join("local").join(EXECUTABLE),
        Source::Repository(path) => root.join(path),
    }
}

fn copy_package_files(root: &Path, folder: &Path) -> Result<Vec<PackagedFile>, String> {
    // Copy each entry, fix its mode, then hash the copy the owner will receive.
    LAYOUT
        .iter()
        .map(|entry| {
            let source = source_path(root, entry);
            let destination = folder.join(entry.name);
            fs::copy(&source, &destination)
                .map_err(|error| format!("Could not copy {}: {error}", source.display()))?;
            set_mode(&destination, entry.executable)?;
            hashed(&destination, entry.name)
        })
        .collect()
}

/// Give programs 0755 and data 0644, whatever the checkout's modes were.
#[cfg(unix)]
fn set_mode(path: &Path, executable: bool) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o755 } else { 0o644 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| format!("Could not set the mode of {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _executable: bool) -> Result<(), String> {
    Ok(())
}

/// The checksum list in `sha256sum`'s format, so `sha256sum -c` checks it.
fn render_sums(files: &[PackagedFile]) -> String {
    files
        .iter()
        .map(|file| format!("{}  {}\n", file.sha256, file.path))
        .collect()
}

fn tar_package(output: &Path, name: &str) -> Result<PathBuf, String> {
    // Root-owned, name-sorted entries, so the archive does not record who built it.
    let archive = output.join(format!("{name}.tar.gz"));
    if archive.exists() {
        fs::remove_file(&archive)
            .map_err(|error| format!("Could not replace {}: {error}", archive.display()))?;
    }
    let status = Command::new("tar")
        .current_dir(output)
        .args([
            "--create",
            "--gzip",
            "--sort=name",
            "--owner=0",
            "--group=0",
            "--numeric-owner",
            "--file",
        ])
        .arg(&archive)
        .arg(name)
        .status()
        .map_err(|error| format!("Could not start tar: {error}"))?;
    if !status.success() {
        return Err(format!("tar could not create the tarball ({status})."));
    }
    Ok(archive)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
    }

    fn read(relative: &str) -> String {
        fs::read_to_string(repository().join(relative))
            .unwrap_or_else(|error| panic!("{relative}: {error}"))
            .replace("\r\n", "\n")
    }

    fn names() -> Vec<&'static str> {
        LAYOUT.iter().map(|entry| entry.name).collect()
    }

    #[test]
    fn layout_holds_the_program_its_desktop_files_scripts_and_licence_once() {
        assert_eq!(
            names(),
            [
                "INSTALL.txt",
                "LICENSE",
                "install.sh",
                "syrinx",
                "syrinx.desktop",
                "syrinx.png",
                "uninstall.sh",
            ]
        );
        let mut sorted = names();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, names(), "sorted and unique");
        assert!(!names().contains(&SUMS));
        let executables: Vec<_> = LAYOUT
            .iter()
            .filter(|entry| entry.executable)
            .map(|entry| entry.name)
            .collect();
        assert_eq!(executables, ["install.sh", "syrinx", "uninstall.sh"]);
        for script in SCRIPTS {
            assert!(names().contains(&script), "{script} is packaged");
        }
    }

    #[test]
    fn every_repository_source_exists() {
        for path in repository_sources(&repository()) {
            assert!(path.is_file(), "{} is missing", path.display());
        }
    }

    #[test]
    fn the_program_comes_from_the_separate_package_build() {
        let program = LAYOUT.iter().find(|entry| entry.source == Source::Build);
        let program = program.expect("one built file");
        assert_eq!(
            source_path(Path::new("/repo"), program),
            Path::new("/repo/target/package-build/local/syrinx")
        );
        assert!(!source_path(Path::new("/repo"), program).starts_with("/repo/target/local"));
    }

    #[test]
    fn sums_use_sha256sums_format() {
        let files = [
            PackagedFile {
                path: "a.txt".to_owned(),
                bytes: 1,
                sha256: "0".repeat(64),
            },
            PackagedFile {
                path: "syrinx".to_owned(),
                bytes: 2,
                sha256: "f".repeat(64),
            },
        ];
        assert_eq!(
            render_sums(&files),
            format!("{}  a.txt\n{}  syrinx\n", "0".repeat(64), "f".repeat(64))
        );
    }

    #[test]
    fn desktop_entry_launches_syrinx_and_carries_the_ownership_marker() {
        let desktop = read("packaging/linux/syrinx.desktop");
        let lines: Vec<&str> = desktop.lines().collect();
        assert_eq!(lines[0], "[Desktop Entry]");
        for expected in [
            "Type=Application",
            "Name=Syrinx",
            "Exec=syrinx",
            "Icon=syrinx",
            "Terminal=false",
            "Categories=Network;WebBrowser;",
            "StartupWMClass=syrinx",
            "X-Syrinx-Packaging=syrinx-packaging-v1",
        ] {
            assert!(lines.contains(&expected), "missing {expected}");
        }
        // Handling links is a later choice: `syrinx --` without a link is refused today.
        assert!(!desktop.contains("MimeType="));
        assert!(!desktop.contains('%'));
    }

    #[test]
    fn the_window_class_matches_the_desktop_entry() {
        // The window manager pairs window and entry by WM_CLASS, which GPUI sets from app_id.
        let main = read("crates/syrinx-app/src/main.rs");
        assert!(main.contains("app_id: Some(\"syrinx\".into())"));
    }

    #[test]
    fn scripts_stay_per_user_and_never_choose_the_default_browser() {
        for script in SCRIPTS {
            let text = read(&format!("packaging/linux/{script}"));
            assert!(text.starts_with("#!/bin/sh\n"), "{script} is POSIX sh");
            assert!(text.contains("\nset -eu\n"), "{script} stops on errors");
            assert!(
                text.contains("[ \"$(id -u)\" -ne 0 ]"),
                "{script} refuses root"
            );
            for forbidden in [
                "sudo ",
                "/usr/",
                "/etc/",
                "xdg-settings",
                "xdg-mime default",
            ] {
                assert!(!text.contains(forbidden), "{script} uses {forbidden}");
            }
        }
    }

    #[test]
    fn uninstall_removes_exactly_what_install_writes() {
        let install = read("packaging/linux/install.sh");
        let uninstall = read("packaging/linux/uninstall.sh");
        for path in [
            "bin_dir=\"$HOME/.local/bin\"",
            "data_home=\"${XDG_DATA_HOME:-$HOME/.local/share}\"",
            "icons/hicolor/256x256/apps",
            "program=\"$bin_dir/syrinx\"",
        ] {
            assert!(install.contains(path), "install.sh lacks {path}");
            assert!(uninstall.contains(path), "uninstall.sh lacks {path}");
        }
        assert!(install.contains("syrinx.desktop") && uninstall.contains("syrinx.desktop"));
        let marker = "X-Syrinx-Packaging=syrinx-packaging-v1";
        assert!(
            uninstall.contains(marker) && read("packaging/linux/syrinx.desktop").contains(marker)
        );
        // Data is never deleted by the script.
        assert!(!uninstall.contains("rm -r"));
        assert!(install.contains("sha256sum -c SHA256SUMS"));
    }

    #[test]
    fn the_icon_is_a_256_pixel_png() {
        let bytes = fs::read(repository().join("design/brand/icon/syrinx-icon-256.png")).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        // IHDR's width and height, big-endian, follow the signature and chunk header.
        assert_eq!(&bytes[16..24], [0, 0, 1, 0, 0, 0, 1, 0]);
    }

    #[cfg(unix)]
    #[test]
    fn scripts_parse() {
        check(&repository()).unwrap();
    }
}
