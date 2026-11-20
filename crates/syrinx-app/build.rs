//! Compiles the icon and version details of `syrinx.exe` and `syrinx-setup.exe` with the Windows
//! SDK's `rc.exe`, and hands the setup program the package it carries.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// The committed icon, generated once from the logo (`design/brand/README.md`).
const ICON: &str = "../../design/brand/icon/syrinx.ico";

/// The publisher shown in the file's properties. The owner has not chosen one yet; when they
/// do, set it here and the resource gains a CompanyName.
const COMPANY_NAME: Option<&str> = None;

/// Set by `cargo xtask package` to the payload `syrinx-setup.exe` carries; unset, it carries none.
const PAYLOAD_VARIABLE: &str = "SYRINX_SETUP_PAYLOAD";

/// The setup program's manifest: run as the owner, draw with Common Controls 6 (task dialogs),
/// and stay sharp on high-DPI screens.
const SETUP_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}" />
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" />
    </dependentAssembly>
  </dependency>
</assembly>
"#;

/// What one executable's resources say about it.
struct Program {
    /// The Cargo binary name, which also names its resource files.
    binary: &'static str,
    description: &'static str,
    /// Only the setup program carries its own manifest; GPUI supplies `syrinx.exe`'s.
    manifest: Option<&'static str>,
}

const PROGRAMS: [Program; 2] = [
    Program {
        binary: "syrinx",
        description: "Syrinx",
        manifest: None,
    },
    Program {
        binary: "syrinx-setup",
        description: "Syrinx Setup",
        manifest: Some(SETUP_MANIFEST),
    },
];

/// Where the Windows SDK installs its tools when `ProgramFiles(x86)` is unset.
const DEFAULT_KITS_BIN: &str = r"C:\Program Files (x86)\Windows Kits\10\bin";

fn main() -> ExitCode {
    // A failed step stops the build with its reason rather than shipping an icon-less exe.
    match compile_resources().and_then(|()| provide_payload()) {
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

    // Only a Windows executable carries a compiled .res; elsewhere the desktop entry names the icon.
    let target_os = required_env("CARGO_CFG_TARGET_OS")?;
    if target_os != "windows" {
        return Ok(());
    }
    // The Windows linker must take that .res, which only the MSVC toolchain's link.exe does here.
    let target_env = required_env("CARGO_CFG_TARGET_ENV")?;
    if target_env != "msvc" {
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

    let version = required_env("CARGO_PKG_VERSION")?;
    let rc = find_rc()?;
    for program in &PROGRAMS {
        let compiled = compile_program(&rc, &out_dir, &icon, &version, program)?;
        // Each binary and its own unit-test executable link only their own resources.
        println!(
            "cargo:rustc-link-arg-bin={}={}",
            program.binary,
            compiled.display()
        );
    }
    Ok(())
}

fn compile_program(
    rc: &Path,
    out_dir: &Path,
    icon: &Path,
    version: &str,
    program: &Program,
) -> Result<PathBuf, String> {
    let manifest = match program.manifest {
        Some(text) => {
            let path = out_dir.join(format!("{}.manifest", program.binary));
            fs::write(&path, text)
                .map_err(|error| format!("could not write {}: {error}", path.display()))?;
            Some(path)
        }
        None => None,
    };
    let script = out_dir.join(format!("{}.rc", program.binary));
    let compiled = out_dir.join(format!("{}.res", program.binary));
    let text = resource_script(icon, version, program, manifest.as_deref())?;
    fs::write(&script, utf16_with_bom(&text))
        .map_err(|error| format!("could not write {}: {error}", script.display()))?;
    run_rc(rc, &script, &compiled)?;
    Ok(compiled)
}

/// Point `syrinx-setup.exe` at its payload, or at an empty one that it refuses to install.
fn provide_payload() -> Result<(), String> {
    println!("cargo:rerun-if-env-changed={PAYLOAD_VARIABLE}");
    let payload = match env::var_os(PAYLOAD_VARIABLE) {
        Some(path) => {
            let path = PathBuf::from(path);
            if !path.is_file() {
                return Err(format!(
                    "{PAYLOAD_VARIABLE} is set to {}, which is not a file.",
                    path.display()
                ));
            }
            println!("cargo:rerun-if-changed={}", path.display());
            path
        }
        None => {
            let empty = PathBuf::from(required_env("OUT_DIR")?).join("no-setup-payload.bin");
            fs::write(&empty, b"")
                .map_err(|error| format!("could not write {}: {error}", empty.display()))?;
            empty
        }
    };
    println!(
        "cargo:rustc-env=SYRINX_SETUP_PAYLOAD_FILE={}",
        payload.display()
    );
    Ok(())
}

fn required_env(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("Cargo did not set {name}."))
}

/// rc.exe reads a script with a byte-order mark as UTF-16, so a checkout path outside the
/// ANSI code page still names the icon correctly; plain UTF-8 failed with RC2135.
fn utf16_with_bom(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

/// Quotes text as a resource-script string, where a `"` is written `""`.
fn rc_string(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

/// The resource script: icon 1, which GPUI and the registry's `syrinx.exe,0` load, VERSIONINFO
/// and, for the setup program, its manifest (resource type 24).
fn resource_script(
    icon: &Path,
    version: &str,
    program: &Program,
    manifest: Option<&Path>,
) -> Result<String, String> {
    let numbers = version_numbers(version)?;
    let [major, minor, patch] = numbers;
    let icon_path = rc_path(icon);
    let manifest = manifest
        .map(|path| format!("1 24 {}\n\n", rc_path(path)))
        .unwrap_or_default();
    let Program {
        binary,
        description,
        ..
    } = program;
    let company = match COMPANY_NAME {
        Some(name) => format!("VALUE \"CompanyName\", {}\n            ", rc_string(name)),
        None => String::new(),
    };
    // Numeric constants stand in for winver.h's names, so rc.exe needs no include path.
    Ok(format!(
        r#"1 ICON {icon_path}

{manifest}1 VERSIONINFO
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
            {company}VALUE "FileDescription", "{description}"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "{binary}"
            VALUE "OriginalFilename", "{binary}.exe"
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

/// A path as a resource-script string, with backslashes doubled.
fn rc_path(path: &Path) -> String {
    rc_string(&path.display().to_string().replace('\\', "\\\\"))
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
    if let Some(candidate) = msvc_environment_rc().filter(|candidate| candidate.is_file()) {
        return Ok(candidate);
    }
    if let Some(candidate) = path_rc() {
        return Ok(candidate);
    }
    newest_sdk_rc()?.ok_or_else(missing_rc_message)
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

/// The highest version folder (such as 10.0.26100.0) with an x64 rc.exe. A missing SDK
/// folder means none is installed; any other I/O error is reported.
fn newest_sdk_rc() -> Result<Option<PathBuf>, String> {
    let kits_bin = match env::var_os("ProgramFiles(x86)") {
        Some(program_files) => PathBuf::from(program_files).join(r"Windows Kits\10\bin"),
        None => PathBuf::from(DEFAULT_KITS_BIN),
    };
    let unreadable = |error: io::Error| format!("could not read {}: {error}", kits_bin.display());
    let entries = match fs::read_dir(&kits_bin) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(unreadable(error)),
    };
    let mut newest: Option<(Vec<u32>, PathBuf)> = None;
    for entry in entries {
        let entry = entry.map_err(unreadable)?;
        let Some(version) = sdk_version(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        let rc = entry.path().join("x64").join("rc.exe");
        if rc.is_file() && newest.as_ref().is_none_or(|(best, _)| version > *best) {
            newest = Some((version, rc));
        }
    }
    Ok(newest.map(|(_, rc)| rc))
}

fn sdk_version(name: &str) -> Option<Vec<u32>> {
    name.split('.').map(|part| part.parse().ok()).collect()
}

fn missing_rc_message() -> String {
    "rc.exe, the Windows SDK resource compiler, was not found. Install the Windows 11 SDK \
     (Visual Studio Installer, \"Desktop development with C++\"), build from a Developer \
     Command Prompt, or set RC to rc.exe's full path. See docs/dev/LOCAL-DEV.md."
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
