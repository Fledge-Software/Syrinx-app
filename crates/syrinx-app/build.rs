//! Compiles Syrinx's icon and version details into `syrinx.exe` with the Windows SDK's `rc.exe`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// The committed icon, generated once from the logo (`design/brand/README.md`).
const ICON: &str = "../../design/brand/icon/syrinx.ico";

/// The publisher shown in the file's properties. The owner has not chosen one yet; when they
/// do, set it here and the resource gains a CompanyName.
const COMPANY_NAME: Option<&str> = None;

/// Where the Windows SDK installs its tools when `ProgramFiles(x86)` is unset.
const DEFAULT_KITS_BIN: &str = r"C:\Program Files (x86)\Windows Kits\10\bin";

fn main() -> ExitCode {
    // A failed step stops the build with its reason rather than shipping an icon-less exe.
    match compile_resources() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("syrinx-app build script: {message}");
            ExitCode::FAILURE
        }
    }
}

fn compile_resources() -> Result<(), String> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={ICON}");
    println!("cargo:rerun-if-env-changed=RC");

    // Syrinx is Windows 11 only (D2); the linker must also take a compiled .res.
    let target_os = required_env("CARGO_CFG_TARGET_OS")?;
    let target_env = required_env("CARGO_CFG_TARGET_ENV")?;
    if target_os != "windows" || target_env != "msvc" {
        return Err(format!(
            "Syrinx builds only for Windows with the MSVC toolchain, not {target_os}/{target_env}."
        ));
    }

    let manifest_dir = PathBuf::from(required_env("CARGO_MANIFEST_DIR")?);
    let out_dir = PathBuf::from(required_env("OUT_DIR")?);
    let icon = manifest_dir.join(ICON);
    if !icon.is_file() {
        return Err(format!("the icon {} is missing.", icon.display()));
    }

    let script = out_dir.join("syrinx.rc");
    let compiled = out_dir.join("syrinx.res");
    fs::write(
        &script,
        resource_script(&icon, &required_env("CARGO_PKG_VERSION")?)?,
    )
    .map_err(|error| format!("could not write {}: {error}", script.display()))?;
    run_rc(&find_rc()?, &script, &compiled)?;

    // Only the executable takes the resource; tests and examples keep their own.
    println!("cargo:rustc-link-arg-bins={}", compiled.display());
    Ok(())
}

fn required_env(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("Cargo did not set {name}."))
}

/// The resource script: icon 1, which GPUI and the registry's `syrinx.exe,0` load, and VERSIONINFO.
fn resource_script(icon: &Path, version: &str) -> Result<String, String> {
    let numbers = version_numbers(version)?;
    let [major, minor, patch] = numbers;
    let icon_path = icon.display().to_string().replace('\\', "\\\\");
    let company = match COMPANY_NAME {
        Some(name) => format!("VALUE \"CompanyName\", \"{name}\"\n            "),
        None => String::new(),
    };
    // Numeric constants stand in for winver.h's names, so rc.exe needs no include path.
    Ok(format!(
        r#"1 ICON "{icon_path}"

1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3F
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904B0"
        BEGIN
            {company}VALUE "FileDescription", "Syrinx"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "syrinx"
            VALUE "OriginalFilename", "syrinx.exe"
            VALUE "ProductName", "Syrinx"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    ))
}

/// Cargo's `major.minor.patch`, ignoring any pre-release or build suffix.
fn version_numbers(version: &str) -> Result<[u16; 3], String> {
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let parts: Vec<u16> = core
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .map_err(|_| format!("the package version {version} is not major.minor.patch."))?;
    parts
        .try_into()
        .map_err(|_| format!("the package version {version} is not major.minor.patch."))
}

/// `RC`, then the MSVC environment, then `PATH`, then the newest installed Windows 10/11 SDK.
fn find_rc() -> Result<PathBuf, String> {
    if let Some(explicit) = env::var_os("RC") {
        let explicit = PathBuf::from(explicit);
        if explicit.is_file() {
            return Ok(explicit);
        }
        return Err(format!(
            "RC is set to {}, which is not a file.",
            explicit.display()
        ));
    }
    let candidates = msvc_environment_rc()
        .into_iter()
        .chain(path_rc())
        .chain(newest_sdk_rc());
    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(missing_rc_message())
}

fn msvc_environment_rc() -> Option<PathBuf> {
    // A Developer Command Prompt sets the SDK's versioned bin folder.
    let bin = env::var_os("WindowsSdkVerBinPath")?;
    Some(PathBuf::from(bin).join("x64").join("rc.exe"))
}

fn path_rc() -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join("rc.exe"))
        .find(|candidate| candidate.is_file())
}

fn newest_sdk_rc() -> Option<PathBuf> {
    // Version folders such as 10.0.26100.0; pick the highest that has an x64 rc.exe.
    let kits_bin = match env::var_os("ProgramFiles(x86)") {
        Some(program_files) => PathBuf::from(program_files).join(r"Windows Kits\10\bin"),
        None => PathBuf::from(DEFAULT_KITS_BIN),
    };
    let entries = fs::read_dir(kits_bin).ok()?;
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let version = sdk_version(&entry.file_name().to_string_lossy())?;
            let rc = entry.path().join("x64").join("rc.exe");
            rc.is_file().then_some((version, rc))
        })
        .max()
        .map(|(_, rc)| rc)
}

fn sdk_version(name: &str) -> Option<Vec<u32>> {
    name.split('.').map(|part| part.parse().ok()).collect()
}

fn missing_rc_message() -> String {
    "rc.exe, the Windows SDK resource compiler, was not found. Install the Windows 11 SDK \
     (Visual Studio Installer, \"Desktop development with C++\"), build from a Developer \
     Command Prompt, or set RC to rc.exe's full path. See docs/LOCAL-DEV.md."
        .to_owned()
}

fn run_rc(rc: &Path, script: &Path, compiled: &Path) -> Result<(), String> {
    // /nologo keeps the build quiet; /fo names the compiled resource file.
    let output = Command::new(rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(compiled)
        .arg(script)
        .output()
        .map_err(|error| format!("could not run {}: {error}", rc.display()))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{} failed ({}):\n{}{}",
        rc.display(),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}
