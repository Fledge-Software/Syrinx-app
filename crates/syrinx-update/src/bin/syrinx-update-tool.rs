//! The release tool behind `cargo xtask package` and `cargo xtask sign-update`; it installs nothing.

use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use syrinx_update::{
    FileEntry, KeySet, KeyStorage, MAX_FILE_BYTES, MAX_MANIFEST_BYTES, MAX_STATEMENT_BYTES,
    MAX_STATEMENT_LIFETIME_SECONDS, Manifest, PackagePath, SECONDS_PER_DAY, SignedRelease,
    Statement, StatementTerms, StoredKey, Timestamp, Version, read_bounded, sha256, sign_release,
};

const USAGE: &str = "Usage:
  syrinx-update-tool manifest --package <folder> --version <x.y.z> --minimum-version <x.y.z> --file <path>...
  syrinx-update-tool sign --package <folder> --storage <software|smart-card> --key <name> --serial <n> --days <1-60>";

/// File names the tool reads and writes inside a package folder.
const MANIFEST: &str = "manifest";
const MANIFEST_SIGNATURE: &str = "manifest.sig";
const STATEMENT: &str = "latest";
const STATEMENT_SIGNATURE: &str = "latest.sig";

/// Longest statement the tool will sign, in days (the 60-day cap).
const MAX_DAYS: u64 = MAX_STATEMENT_LIFETIME_SECONDS / SECONDS_PER_DAY;

#[derive(Debug, PartialEq, Eq)]
enum Request {
    Manifest(ManifestRequest),
    Sign(SignRequest),
}

/// List these package files, by path, in a new canonical `manifest`.
#[derive(Debug, PartialEq, Eq)]
struct ManifestRequest {
    package: PathBuf,
    version: Version,
    minimum_version: Version,
    files: Vec<String>,
}

/// Sign the package's `manifest` and a fresh latest statement with a stored key.
#[derive(Debug, PartialEq, Eq)]
struct SignRequest {
    package: PathBuf,
    storage: KeyStorage,
    key: String,
    serial: u64,
    days: u64,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let arguments = env::args_os()
        .skip(1)
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| "Arguments must be Unicode.".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    match parse_request(&arguments)? {
        Request::Manifest(request) => {
            let path = write_manifest(&request)?;
            println!("Wrote {}.", path.display());
            Ok(())
        }
        Request::Sign(request) => sign(&request),
    }
}

fn parse_request(arguments: &[String]) -> Result<Request, String> {
    let Some((command, rest)) = arguments.split_first() else {
        return Err(USAGE.to_owned());
    };
    match command.as_str() {
        "manifest" => parse_manifest_request(rest).map(Request::Manifest),
        "sign" => parse_sign_request(rest).map(Request::Sign),
        _ => Err(USAGE.to_owned()),
    }
}

fn parse_manifest_request(arguments: &[String]) -> Result<ManifestRequest, String> {
    let mut options = Options::parse(
        arguments,
        &["--package", "--version", "--minimum-version", "--file"],
    )?;
    let files = options.take_all("--file");
    if files.is_empty() {
        return Err(format!("At least one --file is required.\n{USAGE}"));
    }
    Ok(ManifestRequest {
        package: PathBuf::from(options.take_one("--package")?),
        version: parse_version(&options.take_one("--version")?)?,
        minimum_version: parse_version(&options.take_one("--minimum-version")?)?,
        files,
    })
}

fn parse_sign_request(arguments: &[String]) -> Result<SignRequest, String> {
    let mut options = Options::parse(
        arguments,
        &["--package", "--storage", "--key", "--serial", "--days"],
    )?;
    let storage = match options.take_one("--storage")?.as_str() {
        "software" => KeyStorage::Software,
        "smart-card" => KeyStorage::SmartCard,
        other => {
            return Err(format!(
                "Unknown --storage {other}; use software or smart-card."
            ));
        }
    };
    let serial = options.take_one("--serial")?;
    let serial = serial
        .parse::<u64>()
        .map_err(|_| format!("--serial {serial} is not a whole number."))?;
    let days = options.take_one("--days")?;
    let days = days
        .parse::<u64>()
        .ok()
        .filter(|days| (1..=MAX_DAYS).contains(days))
        .ok_or(format!("--days {days} must be from 1 to {MAX_DAYS}."))?;
    Ok(SignRequest {
        package: PathBuf::from(options.take_one("--package")?),
        storage,
        key: options.take_one("--key")?,
        serial,
        days,
    })
}

fn parse_version(text: &str) -> Result<Version, String> {
    Version::parse(text).ok_or(format!("{text} is not a MAJOR.MINOR.PATCH version."))
}

/// `--name value` pairs, each name allowed once except `--file`.
struct Options {
    values: BTreeMap<String, Vec<String>>,
}

impl Options {
    fn parse(arguments: &[String], allowed: &[&str]) -> Result<Self, String> {
        let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for pair in arguments.chunks(2) {
            let [name, value] = pair else {
                return Err(format!("{} needs a value.\n{USAGE}", pair[0]));
            };
            if !allowed.contains(&name.as_str()) {
                return Err(format!("Unknown option {name}.\n{USAGE}"));
            }
            let entry = values.entry(name.clone()).or_default();
            if !entry.is_empty() && name != "--file" {
                return Err(format!("{name} is given twice."));
            }
            entry.push(value.clone());
        }
        Ok(Self { values })
    }

    fn take_one(&mut self, name: &str) -> Result<String, String> {
        self.values
            .remove(name)
            .and_then(|mut values| values.pop())
            .ok_or(format!("{name} is required.\n{USAGE}"))
    }

    fn take_all(&mut self, name: &str) -> Vec<String> {
        self.values.remove(name).unwrap_or_default()
    }
}

/// Hash each listed file and write a new canonical `manifest`, refusing to replace one.
fn write_manifest(request: &ManifestRequest) -> Result<PathBuf, String> {
    let entries = request
        .files
        .iter()
        .map(|name| file_entry(&request.package, name))
        .collect::<Result<Vec<_>, _>>()?;
    let manifest = Manifest::new(request.version, request.minimum_version, entries)
        .map_err(|error| format!("The manifest would be refused: {error}."))?;
    let path = request.package.join(MANIFEST);
    write_new(&path, &manifest.to_bytes())?;
    Ok(path)
}

/// One listed file's path, exact size and SHA-256.
fn file_entry(package: &Path, name: &str) -> Result<FileEntry, String> {
    let path = PackagePath::parse(name)
        .map_err(|problem| format!("{name} is not a safe package path ({problem:?})."))?;
    let file_path = package.join(name.replace('/', "\\"));
    let file = File::open(&file_path)
        .map_err(|error| format!("Could not open {}: {error}", file_path.display()))?;
    let bytes = read_bounded(file, MAX_FILE_BYTES as usize, "a package file")
        .map_err(|error| format!("Could not read {name}: {error}."))?;
    let digest = sha256(&bytes).map_err(|error| format!("Could not hash {name}: {error}."))?;
    Ok(FileEntry::new(path, bytes.len() as u64, digest))
}

/// Sign with the owner's stored key; only public values are printed.
fn sign(request: &SignRequest) -> Result<(), String> {
    let manifest = read_file(
        &request.package.join(MANIFEST),
        MAX_MANIFEST_BYTES,
        "manifest",
    )?;
    check_serial_advances(&request.package, request.serial)?;
    let terms = statement_terms(request, Timestamp::now().map_err(describe)?)?;
    let keys = KeySet::production().map_err(describe)?;
    let signer = StoredKey::open(request.storage, &request.key).map_err(describe)?;
    let release = sign_release(&manifest, &signer, &keys, terms).map_err(describe)?;
    write_release(&request.package, &release)?;
    println!(
        "Signed {MANIFEST} and statement serial {} with key {}; it expires at {}.",
        request.serial,
        hex(&release.key_id.0),
        terms.expires
    );
    Ok(())
}

/// Issue now and expire `days` later.
fn statement_terms(request: &SignRequest, now: Timestamp) -> Result<StatementTerms, String> {
    let expires = now
        .checked_add_seconds(request.days * SECONDS_PER_DAY)
        .ok_or("The expiry would fall after the year 9999.")?;
    Ok(StatementTerms {
        serial: request.serial,
        issued: now,
        expires,
    })
}

/// Refuse a serial that does not exceed one already written to this package folder.
fn check_serial_advances(package: &Path, serial: u64) -> Result<(), String> {
    let path = package.join(STATEMENT);
    if !path.exists() {
        return Ok(());
    }
    let bytes = read_file(&path, MAX_STATEMENT_BYTES, "latest statement")?;
    let previous = Statement::parse(&bytes)
        .map_err(|error| format!("{} is not a statement: {error}.", path.display()))?;
    if serial <= previous.serial() {
        return Err(format!(
            "--serial {serial} must be greater than {}, the serial already in {}.",
            previous.serial(),
            path.display()
        ));
    }
    Ok(())
}

/// Replace the three signed files one at a time, `latest` last.
///
/// A failure part-way never leaves a new `latest` beside an old `latest.sig`, and the
/// folder's serial check still sees the previous statement.
fn write_release(package: &Path, release: &SignedRelease) -> Result<(), String> {
    write_replacing(
        &package.join(MANIFEST_SIGNATURE),
        &release.manifest_signature,
    )?;
    write_replacing(
        &package.join(STATEMENT_SIGNATURE),
        &release.statement_signature,
    )?;
    write_replacing(&package.join(STATEMENT), &release.statement)
}

fn read_file(path: &Path, limit: usize, what: &'static str) -> Result<Vec<u8>, String> {
    let file =
        File::open(path).map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    read_bounded(file, limit, what).map_err(|error| format!("Could not read {what}: {error}."))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("Could not create {}: {error}", path.display()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("Could not write {}: {error}", path.display()))
}

/// Write a temporary file beside `path`, flush it, then rename it over `path`.
fn write_replacing(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = partial_path(path)?;
    let written = write_new(&temporary, bytes).and_then(|()| {
        fs::rename(&temporary, path).map_err(|error| {
            format!(
                "Could not move {} to {}: {error}",
                temporary.display(),
                path.display()
            )
        })
    });
    let Err(message) = written else {
        return Ok(());
    };

    // Remove the partial file, reporting a removal failure alongside the original one.
    match fs::remove_file(&temporary) {
        Ok(()) => Err(message),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(message),
        Err(error) => Err(format!(
            "{message} Also could not remove {}: {error}",
            temporary.display()
        )),
    }
}

/// `<name>.partial` in the same folder, so the final rename stays on one volume.
fn partial_path(path: &Path) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("{} has no file name.", path.display()))?;
    Ok(path.with_file_name(format!("{name}.partial")))
}

fn describe(error: syrinx_update::UpdateError) -> String {
    format!("Signing failed: {error}.")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use syrinx_update::{KeyId, SIGNATURE_BYTES};

    use super::*;

    fn strings(arguments: &[&str]) -> Vec<String> {
        arguments
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect()
    }

    /// A fresh synthetic package folder under the temporary directory.
    struct Scratch {
        root: PathBuf,
    }

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let name = format!(
                "syrinx-update-tool-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let root = env::temp_dir().join(name);
            if root.exists() {
                fs::remove_dir_all(&root).expect("clear a stale synthetic folder");
            }
            fs::create_dir(&root).expect("create a synthetic folder");
            Self { root }
        }

        fn write(&self, name: &str, bytes: &[u8]) {
            let path = self.root.join(name.replace('/', "\\"));
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create a synthetic subfolder");
            }
            fs::write(path, bytes).expect("write a synthetic file");
        }

        fn request(&self, files: &[&str]) -> ManifestRequest {
            ManifestRequest {
                package: self.root.clone(),
                version: Version::new(0, 1, 0),
                minimum_version: Version::new(0, 0, 0),
                files: strings(files),
            }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            if let Err(error) = fs::remove_dir_all(&self.root) {
                eprintln!("could not remove {}: {error}", self.root.display());
            }
        }
    }

    #[test]
    fn the_emitted_manifest_lists_each_file_canonically() {
        let scratch = Scratch::new();
        scratch.write("syrinx.exe", b"synthetic executable");
        scratch.write("syrinx-package.json", b"{}\n");
        scratch.write("install-syrinx.ps1", b"# made-up script\n");
        let request = scratch.request(&["syrinx.exe", "syrinx-package.json", "install-syrinx.ps1"]);
        let path = write_manifest(&request).expect("the manifest is written");

        let bytes = fs::read(path).expect("the manifest is readable");
        let manifest = Manifest::parse(&bytes).expect("the manifest is canonical");
        assert_eq!(manifest.version(), Version::new(0, 1, 0));
        assert_eq!(manifest.minimum_version(), Version::new(0, 0, 0));
        let listed: Vec<_> = manifest
            .files()
            .iter()
            .map(|file| (file.path().as_str().to_owned(), file.size()))
            .collect();
        assert_eq!(
            listed,
            [
                ("install-syrinx.ps1".to_owned(), 17),
                ("syrinx-package.json".to_owned(), 3),
                ("syrinx.exe".to_owned(), 20),
            ]
        );
        let executable = &manifest.files()[2];
        assert_eq!(
            *executable.sha256(),
            sha256(b"synthetic executable").expect("CNG hashes")
        );
    }

    #[test]
    fn the_manifest_writer_refuses_bad_input_and_never_replaces() {
        let scratch = Scratch::new();
        scratch.write("syrinx.exe", b"synthetic executable");
        assert!(write_manifest(&scratch.request(&["missing.exe"])).is_err());
        assert!(write_manifest(&scratch.request(&["..\\syrinx.exe"])).is_err());
        assert!(write_manifest(&scratch.request(&["syrinx.exe", "syrinx.exe"])).is_err());
        assert!(!scratch.root.join(MANIFEST).exists());

        write_manifest(&scratch.request(&["syrinx.exe"])).expect("the first manifest is written");
        assert!(write_manifest(&scratch.request(&["syrinx.exe"])).is_err());
    }

    #[test]
    fn a_minimum_above_the_version_is_refused() {
        let scratch = Scratch::new();
        scratch.write("syrinx.exe", b"synthetic executable");
        let request = ManifestRequest {
            minimum_version: Version::new(0, 2, 0),
            ..scratch.request(&["syrinx.exe"])
        };
        assert!(write_manifest(&request).is_err());
    }

    #[test]
    fn requests_parse_strictly() {
        let manifest = parse_request(&strings(&[
            "manifest",
            "--package",
            r"C:\synthetic",
            "--version",
            "0.1.0",
            "--minimum-version",
            "0.0.0",
            "--file",
            "b",
            "--file",
            "a",
        ]))
        .expect("a full manifest request parses");
        assert_eq!(
            manifest,
            Request::Manifest(ManifestRequest {
                package: PathBuf::from(r"C:\synthetic"),
                version: Version::new(0, 1, 0),
                minimum_version: Version::new(0, 0, 0),
                files: strings(&["b", "a"]),
            })
        );

        let sign = parse_request(&strings(&[
            "sign",
            "--package",
            "p",
            "--storage",
            "smart-card",
            "--key",
            "k",
            "--serial",
            "7",
            "--days",
            "60",
        ]))
        .expect("a full sign request parses");
        assert_eq!(
            sign,
            Request::Sign(SignRequest {
                package: PathBuf::from("p"),
                storage: KeyStorage::SmartCard,
                key: "k".to_owned(),
                serial: 7,
                days: 60,
            })
        );

        let base = [
            "sign",
            "--package",
            "p",
            "--storage",
            "software",
            "--key",
            "k",
            "--serial",
            "7",
        ];
        for (extra, reason) in [
            (&["--days", "61"][..], "over the cap"),
            (&["--days", "0"][..], "zero days"),
            (&["--days", "-1"][..], "negative days"),
            (&["--days"][..], "missing value"),
            (&[][..], "missing days"),
            (&["--days", "5", "--days", "6"][..], "repeated option"),
            (&["--days", "5", "--force", "yes"][..], "unknown option"),
        ] {
            let mut arguments = strings(&base);
            arguments.extend(strings(extra));
            assert!(parse_request(&arguments).is_err(), "{reason}");
        }
        assert!(parse_request(&strings(&["publish"])).is_err());
        assert!(parse_request(&[]).is_err());
        assert!(
            parse_request(&strings(&[
                "sign",
                "--package",
                "p",
                "--storage",
                "registry",
                "--key",
                "k",
                "--serial",
                "1",
                "--days",
                "1"
            ]))
            .is_err()
        );
        assert!(
            parse_request(&strings(&[
                "manifest",
                "--package",
                "p",
                "--version",
                "0.1",
                "--minimum-version",
                "0.0.0",
                "--file",
                "a"
            ]))
            .is_err()
        );
    }

    #[test]
    fn terms_issue_now_and_expire_the_given_days_later() {
        let now = Timestamp::parse("2026-11-02T10:00:00Z").expect("canonical");
        let request = SignRequest {
            package: PathBuf::from("p"),
            storage: KeyStorage::Software,
            key: "k".to_owned(),
            serial: 3,
            days: 60,
        };
        let terms = statement_terms(&request, now).expect("in range");
        assert_eq!(terms.serial, 3);
        assert_eq!(terms.issued, now);
        assert_eq!(terms.expires.to_string(), "2027-01-01T10:00:00Z");

        let last = Timestamp::parse("9999-12-31T00:00:00Z").expect("canonical");
        assert!(statement_terms(&request, last).is_err());
    }

    fn synthetic_release(fill: u8) -> SignedRelease {
        SignedRelease {
            manifest_signature: [fill; SIGNATURE_BYTES],
            statement: vec![fill; 10],
            statement_signature: [fill; SIGNATURE_BYTES],
            key_id: KeyId([fill; 8]),
        }
    }

    fn leftover_partials(scratch: &Scratch) -> Vec<String> {
        fs::read_dir(&scratch.root)
            .expect("the synthetic folder is listable")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .filter(|name| name.ends_with(".partial"))
            .collect()
    }

    #[test]
    fn a_release_replaces_each_file_and_leaves_no_partials() {
        let scratch = Scratch::new();
        write_release(&scratch.root, &synthetic_release(1)).expect("the first release");
        write_release(&scratch.root, &synthetic_release(2)).expect("the second release");
        let read = |name: &str| fs::read(scratch.root.join(name)).expect("a written file");
        assert_eq!(read(MANIFEST_SIGNATURE), [2; SIGNATURE_BYTES]);
        assert_eq!(read(STATEMENT_SIGNATURE), [2; SIGNATURE_BYTES]);
        assert_eq!(read(STATEMENT), [2; 10]);
        assert!(leftover_partials(&scratch).is_empty());
    }

    #[test]
    fn a_failed_signature_write_keeps_the_old_statement() {
        let scratch = Scratch::new();
        scratch.write(STATEMENT, b"old statement");
        // A folder where `latest.sig` belongs makes its rename fail.
        fs::create_dir(scratch.root.join(STATEMENT_SIGNATURE)).expect("a blocking folder");

        assert!(write_release(&scratch.root, &synthetic_release(3)).is_err());
        assert_eq!(
            fs::read(scratch.root.join(STATEMENT)).expect("the old statement"),
            b"old statement"
        );
        assert!(leftover_partials(&scratch).is_empty());
    }

    #[test]
    fn a_serial_must_advance_past_the_folder_statement() {
        let scratch = Scratch::new();
        check_serial_advances(&scratch.root, 1).expect("no earlier statement");
        let issued = Timestamp::parse("2026-11-02T10:00:00Z").expect("canonical");
        let expires = issued
            .checked_add_seconds(SECONDS_PER_DAY)
            .expect("in range");
        let statement = Statement::new(5, Version::new(0, 1, 0), [0; 32], issued, expires)
            .expect("a valid statement");
        scratch.write(STATEMENT, &statement.to_bytes());
        check_serial_advances(&scratch.root, 6).expect("a higher serial");
        assert!(check_serial_advances(&scratch.root, 5).is_err());
        assert!(check_serial_advances(&scratch.root, 4).is_err());

        scratch.write(STATEMENT, b"not a statement\n");
        assert!(check_serial_advances(&scratch.root, 9).is_err());
    }
}
