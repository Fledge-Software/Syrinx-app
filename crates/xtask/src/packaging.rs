//! Builds Syrinx's portable package and checks the owner-run scripts; it never installs or registers.

mod manifest;
mod registration;
mod sha256;
mod update;

pub use update::{RELEASE_TOOL_FEATURE, UPDATE_PACKAGE};

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use manifest::{APPLY_HELPER, BuildIdentity, MANIFEST, PackagedFile};
use registration::{EXECUTABLE, GENERATED_SCRIPT};

/// Repository folder holding the owner-run PowerShell scripts.
const SCRIPT_FOLDER: &str = "packaging";

/// Output folder for packages, inside Cargo's ignored target directory.
const OUTPUT_FOLDER: &str = r"target\package";

/// Cargo target directory for the `installed` build, so it never replaces the development exe.
const PACKAGE_TARGET_DIR: &str = r"target\package-build";

/// The setup program's binary, and the variable its build script reads the payload from.
const SETUP_BINARY: &str = "syrinx-setup";
const SETUP_PAYLOAD_VARIABLE: &str = "SYRINX_SETUP_PAYLOAD";

/// Build the `local` executable and assemble a versioned folder and zip under `target\package`.
pub fn package(root: &Path) -> Result<(), String> {
    check(root)?;
    crate::run_cargo(
        root,
        &[
            "build",
            "--package",
            "syrinx-app",
            "--features",
            "installed",
            "--profile",
            "local",
            "--target-dir",
            PACKAGE_TARGET_DIR,
            "--offline",
            "--locked",
        ],
    )?;
    // The update helper ships in every package; the release tool's feature stays off.
    crate::run_cargo(
        root,
        &[
            "build",
            "--package",
            UPDATE_PACKAGE,
            "--bin",
            "syrinx-apply",
            "--profile",
            "local",
            "--target-dir",
            PACKAGE_TARGET_DIR,
            "--offline",
            "--locked",
        ],
    )?;

    let identity = build_identity(root)?;
    let output = root.join(OUTPUT_FOLDER);
    let name = manifest::package_name(&identity);
    let folder = fresh_package_folder(&output.join(&name))?;
    let files = copy_package_files(root, &folder)?;
    for program in [EXECUTABLE, APPLY_HELPER] {
        require_static_runtime(&folder.join(program))?;
    }
    fs::write(folder.join(MANIFEST), manifest::render(&identity, &files)?)
        .map_err(|error| format!("Could not write {MANIFEST}: {error}"))?;
    write_update_manifest(root, &folder, &identity)?;
    let archive = zip_package(&output, &name)?;
    let setup = build_setup(root, &output, &folder, &name)?;

    println!("Package folder: {}", folder.display());
    println!(
        "Update manifest: {} (unsigned; see cargo xtask sign-update)",
        folder.join(update::UPDATE_MANIFEST).display()
    );
    println!("Package zip:    {}", archive.display());
    println!("Setup program:  {}", setup.display());
    println!("Nothing was installed or registered. See docs/installing.md.");
    Ok(())
}

/// Write P1-38's `manifest` over the program files, through the update crate's release tool.
fn write_update_manifest(
    root: &Path,
    folder: &Path,
    identity: &BuildIdentity,
) -> Result<(), String> {
    run_tool(
        root,
        &update::manifest_command(text(folder)?, &identity.version),
    )
}

/// Pack the finished package folder into `syrinx-setup.exe` and copy it beside the zip.
fn build_setup(root: &Path, output: &Path, folder: &Path, name: &str) -> Result<PathBuf, String> {
    let payload = output.join(format!("{name}.payload"));
    run_tool(
        root,
        &update::payload_command(text(folder)?, text(&payload)?),
    )?;
    crate::run_cargo_with(
        root,
        &[
            "build",
            "--package",
            "syrinx-app",
            "--bin",
            SETUP_BINARY,
            "--features",
            "installed",
            "--profile",
            "local",
            "--target-dir",
            PACKAGE_TARGET_DIR,
            "--offline",
            "--locked",
        ],
        &[(SETUP_PAYLOAD_VARIABLE, &payload)],
    )?;
    let built = root
        .join(PACKAGE_TARGET_DIR)
        .join("local")
        .join(format!("{SETUP_BINARY}.exe"));
    let setup = output.join(setup_name(name));
    fs::copy(&built, &setup)
        .map_err(|error| format!("Could not copy {}: {error}", built.display()))?;
    require_static_runtime(&setup)?;
    fs::remove_file(&payload)
        .map_err(|error| format!("Could not remove {}: {error}", payload.display()))?;
    Ok(setup)
}

/// The Visual C++ runtime DLLs a dynamically linked executable imports.
const DYNAMIC_RUNTIME_DLLS: [&[u8]; 2] = [b"vcruntime140.dll", b"msvcp140.dll"];

/// Refuse an executable that needs the Visual C++ redistributable, which a clean Windows lacks.
///
/// `.cargo/config.toml` links the runtime statically, but a `RUSTFLAGS` variable silently
/// replaces that setting, so the built files are checked rather than the configuration.
fn require_static_runtime(executable: &Path) -> Result<(), String> {
    let bytes = fs::read(executable)
        .map_err(|error| format!("Could not read {}: {error}", executable.display()))?
        .to_ascii_lowercase();
    if let Some(dll) = DYNAMIC_RUNTIME_DLLS
        .iter()
        .find(|dll| imports_name(&bytes, dll))
    {
        return Err(format!(
            "{} imports {}, so it would not start without the Visual C++ redistributable. \
             Build without RUSTFLAGS overriding .cargo/config.toml's crt-static setting.",
            executable.display(),
            String::from_utf8_lossy(dll)
        ));
    }
    Ok(())
}

fn imports_name(bytes: &[u8], name: &[u8]) -> bool {
    bytes.windows(name.len()).any(|window| window == name)
}

/// The setup program's published name, beside the zip of the same package.
pub fn setup_name(package: &str) -> String {
    format!("{package}-setup.exe")
}

fn text(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("{} is not a Unicode path.", path.display()))
}

/// Run a release-tool command (`sign-update` and the workflow's key and host-layout steps).
///
/// Returns `None` when `command` is not one, so the caller reports its usage.
pub fn run_forwarded(root: &Path, command: &str, options: &[String]) -> Option<Result<(), String>> {
    update::forwarded_command(command, options).map(|arguments| run_tool(root, &arguments))
}

fn run_tool(root: &Path, command: &[String]) -> Result<(), String> {
    let arguments: Vec<&str> = command.iter().map(String::as_str).collect();
    crate::run_cargo(root, &arguments)
}

/// Design doc whose registry table is rendered from the same Rust table.
const DESIGN_DOC: &str = r"docs\installing.md";
const TABLE_START: &str = "<!-- BEGIN GENERATED REGISTRY TABLE -->";
const TABLE_END: &str = "<!-- END GENERATED REGISTRY TABLE -->";

/// Text a packaging script must never contain, with the reason shown on failure.
const FORBIDDEN_SCRIPT_TEXT: [(&str, &str); 3] = [
    // `using module` loads a module while parsing, so the parse check would no longer be inert.
    ("using module", "would load code while the gate parses it"),
    (
        "ExecutionPolicy Bypass",
        "skips the Mark-of-the-Web check; use RemoteSigned",
    ),
    ("HKLM:\\SOFTWARE\\Clients", "would register machine-wide"),
];

/// Refuse drift from the Rust registration table, forbidden script text and PowerShell syntax errors.
pub fn check(root: &Path) -> Result<(), String> {
    check_generated_script(root)?;
    check_design_doc_table(root)?;
    let scripts = script_paths(root);
    check_script_rules(&scripts)?;
    parse_powershell(&scripts)?;
    println!("Packaging scripts match the registration table and parse.");
    Ok(())
}

/// Rewrite the generated registration data and the design doc's table after the Rust table changes.
pub fn write_registration(root: &Path) -> Result<(), String> {
    let path = root.join(SCRIPT_FOLDER).join(GENERATED_SCRIPT);
    fs::write(&path, rendered_script())
        .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
    let doc_path = root.join(DESIGN_DOC);
    let doc = read_text(&doc_path)?;
    fs::write(&doc_path, with_doc_table(&doc)?)
        .map_err(|error| format!("Could not write {}: {error}", doc_path.display()))?;
    println!("Wrote {} and {}.", path.display(), doc_path.display());
    Ok(())
}

fn rendered_script() -> String {
    registration::render_script(&manifest::layout())
}

fn script_paths(root: &Path) -> Vec<PathBuf> {
    manifest::SCRIPTS
        .iter()
        .map(|script| root.join(SCRIPT_FOLDER).join(script))
        .collect()
}

fn read_text(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("Could not read {}: {error}", path.display()))
}

fn check_generated_script(root: &Path) -> Result<(), String> {
    // Compare without line endings: git may check the file out with LF or CRLF.
    let path = root.join(SCRIPT_FOLDER).join(GENERATED_SCRIPT);
    if normalized(&read_text(&path)?) != normalized(&rendered_script()) {
        return Err(format!(
            "{} differs from the registration table; run `cargo xtask write-registration`.",
            path.display()
        ));
    }
    Ok(())
}

fn check_design_doc_table(root: &Path) -> Result<(), String> {
    let path = root.join(DESIGN_DOC);
    let doc = normalized(&read_text(&path)?);
    if doc != normalized(&with_doc_table(&doc)?) {
        return Err(format!(
            "{}'s registry table differs from the registration table; run `cargo xtask write-registration`.",
            path.display()
        ));
    }
    Ok(())
}

/// Replace the text between the doc's table markers with the rendered table.
fn with_doc_table(doc: &str) -> Result<String, String> {
    let doc = normalized(doc);
    let (Some(start), Some(end)) = (doc.find(TABLE_START), doc.find(TABLE_END)) else {
        return Err(format!("{DESIGN_DOC} lacks its generated-table markers."));
    };
    let body_start = start + TABLE_START.len();
    if end < body_start {
        return Err(format!("{DESIGN_DOC}'s table markers are out of order."));
    }
    Ok(format!(
        "{}\n{}\n{}",
        &doc[..body_start],
        registration::render_doc_table(),
        &doc[end..]
    ))
}

fn check_script_rules(scripts: &[PathBuf]) -> Result<(), String> {
    let mut violations = Vec::new();
    for path in scripts {
        let text = read_text(path)?;
        for (forbidden, reason) in FORBIDDEN_SCRIPT_TEXT {
            if text.contains(forbidden) {
                violations.push(format!("{}: `{forbidden}` {reason}.", path.display()));
            }
        }
    }
    if !violations.is_empty() {
        return Err(violations.join("\n"));
    }
    Ok(())
}

fn normalized(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn parse_powershell(scripts: &[PathBuf]) -> Result<(), String> {
    // Parse only: ParseFile reads each file without running it, and the scripts avoid
    // `using module`, the one construct that loads code at parse time.
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
    // syrinx-app and xtask share the workspace version.
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
                "{} exists but is not a Syrinx package; move it aside first.",
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
    if name == EXECUTABLE || name == APPLY_HELPER {
        return root.join(PACKAGE_TARGET_DIR).join("local").join(name);
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
    fn design_doc_table_matches_the_table() {
        check_design_doc_table(&repository()).unwrap();
    }

    #[test]
    fn scripts_avoid_forbidden_text() {
        check_script_rules(&script_paths(&repository())).unwrap();
    }

    #[test]
    fn entry_scripts_normalize_folders_before_using_them() {
        // The scripts cannot run in tests; check they route folders through the shared guard.
        for script in [
            "install-syrinx.ps1",
            "register-syrinx.ps1",
            "unregister-syrinx.ps1",
        ] {
            let text = read_text(&repository().join(SCRIPT_FOLDER).join(script)).unwrap();
            assert!(
                text.contains("= Resolve-SyrinxFolder $InstallDirectory"),
                "{script} must normalize its install folder"
            );
        }
        let common =
            read_text(&repository().join(SCRIPT_FOLDER).join("syrinx-common.ps1")).unwrap();
        assert!(common.contains("TrimEnd('\\')") && common.contains("GetPathRoot"));
        // Relative paths must follow PowerShell's location, not the process folder.
        assert!(common.contains("GetUnresolvedProviderPathFromPSPath($Folder)"));
    }

    #[test]
    fn entry_scripts_default_their_folder_outside_the_param_block() {
        // Windows PowerShell 5.1 leaves $PSScriptRoot empty in a param default under -File.
        for script in [
            "install-syrinx.ps1",
            "register-syrinx.ps1",
            "unregister-syrinx.ps1",
        ] {
            let text =
                normalized(&read_text(&repository().join(SCRIPT_FOLDER).join(script)).unwrap());
            let start = text.find("param(").expect("entry script has a param block");
            let end = start + text[start..].find("\n)").expect("param block closes");
            assert!(
                !text[start..end].contains("$PSScriptRoot"),
                "{script} defaults a parameter to $PSScriptRoot"
            );
        }
    }

    #[test]
    fn register_script_finds_its_own_folder_when_run_as_setup_runs_it() -> Result<(), String> {
        // A synthetic folder without syrinx.exe: the script must stop before writing anything.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| format!("clock before 1970: {error}"))?
            .as_nanos();
        let folder = env::temp_dir().join(format!(
            "syrinx-register-probe-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&folder)
            .map_err(|error| format!("Could not create {}: {error}", folder.display()))?;
        let result = run_register_probe(&folder);
        fs::remove_dir_all(&folder)
            .map_err(|error| format!("Could not remove {}: {error}", folder.display()))?;
        let (succeeded, output) = result?;
        assert!(
            !succeeded,
            "the probe has no syrinx.exe, so the script must refuse"
        );
        assert!(
            output.contains("No syrinx.exe in"),
            "the script did not resolve its own folder:\n{output}"
        );
        Ok(())
    }

    /// Copy the registration scripts into `folder` and run them with setup's arguments.
    fn run_register_probe(folder: &Path) -> Result<(bool, String), String> {
        for script in ["register-syrinx.ps1", GENERATED_SCRIPT, "syrinx-common.ps1"] {
            let source = repository().join(SCRIPT_FOLDER).join(script);
            fs::copy(&source, folder.join(script))
                .map_err(|error| format!("Could not copy {}: {error}", source.display()))?;
        }
        let output = Command::new(windows_tool(r"WindowsPowerShell\v1.0\powershell.exe")?)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "RemoteSigned",
                "-File",
            ])
            .arg(folder.join("register-syrinx.ps1"))
            .current_dir(folder)
            .output()
            .map_err(|error| format!("Could not start PowerShell: {error}"))?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok((output.status.success(), text))
    }

    #[test]
    fn doc_table_replacement_keeps_surrounding_text() {
        let doc = format!("intro\n{TABLE_START}\nold\n{TABLE_END}\noutro\n");
        let updated = with_doc_table(&doc).unwrap();
        assert!(updated.starts_with("intro\n"));
        assert!(updated.ends_with(&format!("{TABLE_END}\noutro\n")));
        assert!(!updated.contains("\nold\n"));
        assert!(with_doc_table("no markers").is_err());
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
    fn a_dynamic_runtime_import_is_found_in_any_case() {
        let imports = b"kernel32.dll\0VCRUNTIME140.dll\0".to_ascii_lowercase();
        assert!(imports_name(&imports, DYNAMIC_RUNTIME_DLLS[0]));
        assert!(!imports_name(
            b"kernel32.dll\0user32.dll",
            DYNAMIC_RUNTIME_DLLS[0]
        ));
        assert_eq!(
            setup_name("Syrinx-0.3.0-abc-x64"),
            "Syrinx-0.3.0-abc-x64-setup.exe"
        );
    }

    #[test]
    fn executable_comes_from_the_separate_package_build() {
        // Development builds' executable folder (`cargo syrinx`, the gate and launch checks).
        const DEVELOPMENT_EXE_FOLDER: &str = r"target\local";
        assert_eq!(
            package_source(Path::new(r"C:\repo"), EXECUTABLE),
            Path::new(r"C:\repo\target\package-build\local\syrinx.exe")
        );
        // The installed-profile exe must never land where development launches from.
        let development = Path::new(r"C:\repo").join(DEVELOPMENT_EXE_FOLDER);
        let packaged = package_source(Path::new(r"C:\repo"), EXECUTABLE);
        assert!(!packaged.starts_with(&development));
        assert!(!development.starts_with(Path::new(r"C:\repo").join(PACKAGE_TARGET_DIR)));
        assert_ne!(PACKAGE_TARGET_DIR, "target");
        assert_eq!(
            package_source(Path::new(r"C:\repo"), APPLY_HELPER),
            Path::new(r"C:\repo\target\package-build\local\syrinx-apply.exe")
        );
        assert_eq!(
            package_source(Path::new(r"C:\repo"), "register-syrinx.ps1"),
            Path::new(r"C:\repo\packaging\register-syrinx.ps1")
        );
    }
}
