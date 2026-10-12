//! Builds Atlas's portable package and checks the owner-run scripts; it never installs or registers.

mod manifest;
mod registration;
mod sha256;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use manifest::{BuildIdentity, MANIFEST, PackagedFile};
use registration::{EXECUTABLE, GENERATED_SCRIPT};

/// Repository folder holding the owner-run PowerShell scripts.
const SCRIPT_FOLDER: &str = "packaging";

/// Output folder for packages, inside Cargo's ignored target directory.
const OUTPUT_FOLDER: &str = r"target\package";

/// Build the `local` executable and assemble a versioned folder and zip under `target\package`.
pub fn package(root: &Path) -> Result<(), String> {
    check(root)?;
    crate::run_cargo(
        root,
        &[
            "build",
            "--package",
            "atlas-app",
            "--profile",
            "local",
            "--offline",
            "--locked",
        ],
    )?;

    let identity = build_identity(root)?;
    let output = root.join(OUTPUT_FOLDER);
    let name = manifest::package_name(&identity);
    let folder = fresh_package_folder(&output.join(&name))?;
    let files = copy_package_files(root, &folder)?;
    fs::write(folder.join(MANIFEST), manifest::render(&identity, &files)?)
        .map_err(|error| format!("Could not write {MANIFEST}: {error}"))?;
    let archive = zip_package(&output, &name)?;

    println!("Package folder: {}", folder.display());
    println!("Package zip:    {}", archive.display());
    println!("Nothing was installed or registered. See docs/p1-37-installer-design.md.");
    Ok(())
}

/// Refuse script drift from the Rust registration table and any PowerShell syntax error.
pub fn check(root: &Path) -> Result<(), String> {
    check_generated_script(root)?;
    let scripts: Vec<PathBuf> = manifest::SCRIPTS
        .iter()
        .map(|script| root.join(SCRIPT_FOLDER).join(script))
        .collect();
    parse_powershell(&scripts)?;
    println!("Packaging scripts match the registration table and parse.");
    Ok(())
}

/// Rewrite the generated registration data after the Rust table changes.
pub fn write_registration(root: &Path) -> Result<(), String> {
    let path = root.join(SCRIPT_FOLDER).join(GENERATED_SCRIPT);
    fs::write(&path, registration::render_script())
        .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
    println!("Wrote {}.", path.display());
    Ok(())
}

fn check_generated_script(root: &Path) -> Result<(), String> {
    // Compare without line endings: git may check the file out with LF or CRLF.
    let path = root.join(SCRIPT_FOLDER).join(GENERATED_SCRIPT);
    let committed = fs::read_to_string(&path)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    if normalized(&committed) != normalized(&registration::render_script()) {
        return Err(format!(
            "{} differs from the registration table; run `cargo xtask write-registration`.",
            path.display()
        ));
    }
    Ok(())
}

fn normalized(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn parse_powershell(scripts: &[PathBuf]) -> Result<(), String> {
    // Parse only: the PowerShell parser reads each file without running any of it.
    let list = scripts
        .iter()
        .map(|path| format!("'{}'", path.display().to_string().replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");
    let command = format!(
        "$failed = $false; foreach ($path in @({list})) {{ $tokens = $null; $errors = $null; \
         [void][System.Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$errors); \
         foreach ($problem in $errors) {{ $failed = $true; [Console]::Error.WriteLine(('{{0}}:{{1}}: {{2}}' -f $path, $problem.Extent.StartLineNumber, $problem.Message)) }} }}; \
         if ($failed) {{ exit 1 }}"
    );
    let status = Command::new(windows_tool(r"WindowsPowerShell\v1.0\powershell.exe")?)
        .args(["-NoProfile", "-NonInteractive", "-Command", &command])
        .status()
        .map_err(|error| format!("Could not start PowerShell to parse the scripts: {error}"))?;
    if !status.success() {
        return Err(format!("PowerShell scripts failed to parse ({status})."));
    }
    Ok(())
}

fn build_identity(root: &Path) -> Result<BuildIdentity, String> {
    // atlas-app and xtask share the workspace version.
    let commit = git(root, &["rev-parse", "--short=12", "HEAD"])?;
    let status = git(root, &["status", "--porcelain"])?;
    Ok(BuildIdentity {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        commit,
        dirty: !status.is_empty(),
    })
}

fn git(root: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(arguments)
        .output()
        .map_err(|error| format!("Could not run git {}: {error}", arguments.join(" ")))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed ({}).",
            arguments.join(" "),
            output.status
        ));
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim().to_owned())
        .map_err(|_| format!("git {} printed non-UTF-8 output.", arguments.join(" ")))
}

fn fresh_package_folder(folder: &Path) -> Result<PathBuf, String> {
    // Replace only an earlier package of ours, recognised by its manifest.
    if folder.exists() {
        if !folder.join(MANIFEST).is_file() {
            return Err(format!(
                "{} exists but is not an Atlas package; move it aside first.",
                folder.display()
            ));
        }
        fs::remove_dir_all(folder)
            .map_err(|error| format!("Could not replace {}: {error}", folder.display()))?;
    }
    fs::create_dir_all(folder)
        .map_err(|error| format!("Could not create {}: {error}", folder.display()))?;
    Ok(folder.to_path_buf())
}

fn copy_package_files(root: &Path, folder: &Path) -> Result<Vec<PackagedFile>, String> {
    // Copy each layout entry from its source, then hash the copy the owner will receive.
    manifest::layout()
        .into_iter()
        .map(|name| {
            let source = package_source(root, name);
            let destination = folder.join(name);
            fs::copy(&source, &destination)
                .map_err(|error| format!("Could not copy {}: {error}", source.display()))?;
            hashed(&destination, name)
        })
        .collect()
}

fn package_source(root: &Path, name: &str) -> PathBuf {
    if name == EXECUTABLE {
        return root.join(r"target\local").join(EXECUTABLE);
    }
    root.join(SCRIPT_FOLDER).join(name)
}

fn hashed(path: &Path, name: &str) -> Result<PackagedFile, String> {
    let bytes = fs::read(path).map_err(|error| format!("Could not read {name}: {error}"))?;
    Ok(PackagedFile {
        path: name.to_owned(),
        bytes: bytes.len() as u64,
        sha256: sha256::hex_digest(&bytes),
    })
}

fn zip_package(output: &Path, name: &str) -> Result<PathBuf, String> {
    // Windows' own bsdtar writes the zip; Git's GNU tar on PATH cannot.
    let archive = output.join(format!("{name}.zip"));
    if archive.exists() {
        fs::remove_file(&archive)
            .map_err(|error| format!("Could not replace {}: {error}", archive.display()))?;
    }
    let status = Command::new(windows_tool("tar.exe")?)
        .current_dir(output)
        .args(["-a", "-c", "-f"])
        .arg(&archive)
        .arg(name)
        .status()
        .map_err(|error| format!("Could not start Windows tar: {error}"))?;
    if !status.success() {
        return Err(format!("Windows tar could not create the zip ({status})."));
    }
    Ok(archive)
}

/// Locate a tool that ships with Windows, never one found earlier on PATH.
fn windows_tool(relative: &str) -> Result<PathBuf, String> {
    let system_root = env::var_os("SystemRoot")
        .ok_or("SystemRoot is not set, so Windows tools cannot be found.")?;
    Ok(PathBuf::from(system_root).join("System32").join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(r"..\..")
            .to_path_buf()
    }

    #[test]
    fn committed_registration_script_matches_the_table() {
        check_generated_script(&repository()).unwrap();
    }

    #[test]
    fn every_packaged_script_exists_in_the_repository() {
        for script in manifest::SCRIPTS {
            assert!(
                repository().join(SCRIPT_FOLDER).join(script).is_file(),
                "{script} is missing"
            );
        }
    }

    #[test]
    fn executable_comes_from_the_local_profile() {
        assert_eq!(
            package_source(Path::new(r"C:\repo"), EXECUTABLE),
            Path::new(r"C:\repo\target\local\atlas-app.exe")
        );
        assert_eq!(
            package_source(Path::new(r"C:\repo"), "register-atlas.ps1"),
            Path::new(r"C:\repo\packaging\register-atlas.ps1")
        );
    }
}
