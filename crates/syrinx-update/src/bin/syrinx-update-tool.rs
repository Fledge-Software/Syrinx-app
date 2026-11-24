//! The release tool behind `cargo xtask package` and `cargo xtask sign-update`; it installs nothing.

use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use syrinx_update::{
    FileEntry, HostFiles, KeyId, KeySet, KeyStorage, MAX_FILE_BYTES, MAX_MANIFEST_BYTES,
    MAX_STATEMENT_BYTES, MAX_STATEMENT_LIFETIME_SECONDS, Manifest, PRODUCTION_KEYS, PackagePath,
    PublishedRelease, SECONDS_PER_DAY, SIGNATURE_BYTES, SignedRelease, Statement, StatementTerms,
    StoredKey, Timestamp, Version, delete_software_key, import_software_key, install, read_bounded,
    resign_statement, sha256, sign_release, verify_published, write_host_layout,
};

const USAGE: &str = "Usage:
  syrinx-update-tool manifest --package <folder> --version <x.y.z> --minimum-version <x.y.z> --file <path>...
  syrinx-update-tool sign --package <folder> --storage <software|smart-card> --key <name> --serial <n> --days <1-60>
  syrinx-update-tool payload --package <folder> --output <file>
  syrinx-update-tool verify-release --package <folder> --version <x.y.z>
  syrinx-update-tool resign --package <folder> --version <x.y.z> --storage <software|smart-card> --key <name> --serial <n> --days <1-60>
  syrinx-update-tool import-key --key <name> --from-env <variable>
  syrinx-update-tool delete-key --key <name>
  syrinx-update-tool pinned-keys
  syrinx-update-tool host-layout --package <folder> --output <folder> --files <include|omit>";

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
    Payload(PayloadRequest),
    VerifyRelease(VerifyRequest),
    Resign(ResignRequest),
    ImportKey(ImportKeyRequest),
    DeleteKey(String),
    PinnedKeys,
    HostLayout(HostLayoutRequest),
}

/// Verify a downloaded release, which must be for the tag's `version`.
#[derive(Debug, PartialEq, Eq)]
struct VerifyRequest {
    package: PathBuf,
    version: Version,
}

/// Sign only a new statement for the tag's `version`, whose files are in `sign.package`.
#[derive(Debug, PartialEq, Eq)]
struct ResignRequest {
    sign: SignRequest,
    version: Version,
}

/// Store the base64 PKCS#8 key held in an environment variable as a named software key.
///
/// The key travels by environment variable so it never appears in a command line or a log.
#[derive(Debug, PartialEq, Eq)]
struct ImportKeyRequest {
    key: String,
    variable: String,
}

/// Write the host's `updates/v1/` files from a signed package folder.
#[derive(Debug, PartialEq, Eq)]
struct HostLayoutRequest {
    package: PathBuf,
    output: PathBuf,
    files: HostFiles,
}

/// Pack a package folder into the payload `syrinx-setup.exe` carries.
#[derive(Debug, PartialEq, Eq)]
struct PayloadRequest {
    package: PathBuf,
    output: PathBuf,
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
        Request::Payload(request) => {
            write_setup_payload(&request)?;
            println!("Wrote {}.", request.output.display());
            Ok(())
        }
        Request::VerifyRelease(request) => verify_release(&request),
        Request::Resign(request) => resign(&request),
        Request::ImportKey(request) => import_key(&request),
        Request::DeleteKey(key) => delete_key(&key),
        Request::PinnedKeys => print_pinned_keys(),
        Request::HostLayout(request) => {
            let root = host_layout(&request)?;
            println!("Wrote the host files under {}.", root.display());
            Ok(())
        }
    }
}

fn parse_request(arguments: &[String]) -> Result<Request, String> {
    let Some((command, rest)) = arguments.split_first() else {
        return Err(USAGE.to_owned());
    };
    match command.as_str() {
        "manifest" => parse_manifest_request(rest).map(Request::Manifest),
        "sign" => parse_sign_request(rest).map(Request::Sign),
        "payload" => parse_payload_request(rest).map(Request::Payload),
        "verify-release" => parse_verify_request(rest).map(Request::VerifyRelease),
        "resign" => parse_resign_request(rest).map(Request::Resign),
        "import-key" => parse_import_key_request(rest).map(Request::ImportKey),
        "delete-key" => Options::parse(rest, &["--key"])?
            .take_one("--key")
            .map(Request::DeleteKey),
        "pinned-keys" if rest.is_empty() => Ok(Request::PinnedKeys),
        "host-layout" => parse_host_layout_request(rest).map(Request::HostLayout),
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

fn parse_payload_request(arguments: &[String]) -> Result<PayloadRequest, String> {
    let mut options = Options::parse(arguments, &["--package", "--output"])?;
    Ok(PayloadRequest {
        package: PathBuf::from(options.take_one("--package")?),
        output: PathBuf::from(options.take_one("--output")?),
    })
}

fn parse_import_key_request(arguments: &[String]) -> Result<ImportKeyRequest, String> {
    let mut options = Options::parse(arguments, &["--key", "--from-env"])?;
    Ok(ImportKeyRequest {
        key: options.take_one("--key")?,
        variable: options.take_one("--from-env")?,
    })
}

fn parse_host_layout_request(arguments: &[String]) -> Result<HostLayoutRequest, String> {
    let mut options = Options::parse(arguments, &["--package", "--output", "--files"])?;
    let files = match options.take_one("--files")?.as_str() {
        "include" => HostFiles::Include,
        "omit" => HostFiles::Omit,
        other => return Err(format!("Unknown --files {other}; use include or omit.")),
    };
    Ok(HostLayoutRequest {
        package: PathBuf::from(options.take_one("--package")?),
        output: PathBuf::from(options.take_one("--output")?),
        files,
    })
}

/// A published release's four signed files, read from a package folder.
struct PublishedFiles {
    manifest: Vec<u8>,
    manifest_signature: Vec<u8>,
    statement: Vec<u8>,
    statement_signature: Vec<u8>,
}

impl PublishedFiles {
    fn read(package: &Path) -> Result<Self, String> {
        Ok(Self {
            manifest: read_file(&package.join(MANIFEST), MAX_MANIFEST_BYTES, "manifest")?,
            manifest_signature: read_file(
                &package.join(MANIFEST_SIGNATURE),
                SIGNATURE_BYTES,
                "manifest signature",
            )?,
            statement: read_file(
                &package.join(STATEMENT),
                MAX_STATEMENT_BYTES,
                "latest statement",
            )?,
            statement_signature: read_file(
                &package.join(STATEMENT_SIGNATURE),
                SIGNATURE_BYTES,
                "statement signature",
            )?,
        })
    }

    fn release(&self) -> PublishedRelease<'_> {
        PublishedRelease {
            manifest: &self.manifest,
            manifest_signature: &self.manifest_signature,
            statement: &self.statement,
            statement_signature: &self.statement_signature,
        }
    }

    /// Verify against the pinned keys as a release of `version`, apart from the statement's expiry.
    fn verify(&self, version: Version) -> Result<Statement, String> {
        let keys = KeySet::production().map_err(describe)?;
        verify_published(&self.release(), &keys, version)
            .map_err(|error| format!("The published release would be refused: {error}."))
    }
}

/// Check a downloaded release before any key is imported to re-sign it.
fn verify_release(request: &VerifyRequest) -> Result<(), String> {
    let statement = PublishedFiles::read(&request.package)?.verify(request.version)?;
    println!(
        "Verified {} {}: statement serial {} names its manifest, and both signatures are pinned.",
        request.package.display(),
        statement.version(),
        statement.serial()
    );
    Ok(())
}

/// Sign only a fresh statement for the verified manifest; `manifest.sig` is never rewritten.
fn resign(resign: &ResignRequest) -> Result<(), String> {
    let request = &resign.sign;
    let files = PublishedFiles::read(&request.package)?;
    // Refuse a tampered download, or another version's files, before the key is even opened.
    files.verify(resign.version)?;
    let terms = statement_terms(request, Timestamp::now().map_err(describe)?)?;
    let keys = KeySet::production().map_err(describe)?;
    let signer = StoredKey::open(request.storage, &request.key).map_err(describe)?;
    let signed = resign_statement(&files.release(), &signer, &keys, resign.version, terms)
        .map_err(describe)?;
    write_replacing(
        &request.package.join(STATEMENT_SIGNATURE),
        &signed.statement_signature,
    )?;
    write_replacing(&request.package.join(STATEMENT), &signed.statement)?;
    println!(
        "Signed statement serial {} with key {}; it expires at {}. The manifest was not re-signed.",
        request.serial,
        hex(&signed.key_id.0),
        terms.expires
    );
    Ok(())
}

/// Import the key from its variable; only the key's public identifier is printed.
fn import_key(request: &ImportKeyRequest) -> Result<(), String> {
    let text = env::var(&request.variable)
        .ok()
        .filter(|text| !text.trim().is_empty())
        .ok_or(format!("{} is not set or is empty.", request.variable))?;
    let point = import_software_key(&request.key, &text).map_err(|error| {
        format!(
            "Could not import the key from {}: {error}.",
            request.variable
        )
    })?;
    let id = KeyId::of(&point).map_err(describe)?;
    println!(
        "Stored key {} as {} in Windows software key storage.",
        hex(&id.0),
        request.key
    );
    Ok(())
}

/// Delete the named key; an absent key is reported, not an error, so cleanup can always run.
fn delete_key(key: &str) -> Result<(), String> {
    let deleted =
        delete_software_key(key).map_err(|error| format!("Could not delete {key}: {error}."))?;
    if deleted {
        println!("Deleted {key} from Windows software key storage.");
    } else {
        println!("No key named {key} was stored; nothing to delete.");
    }
    Ok(())
}

/// Print each pinned key's identifier and public point, one per line; nothing when none is pinned.
fn print_pinned_keys() -> Result<(), String> {
    for key in PRODUCTION_KEYS {
        let id = KeyId::of(key.point()).map_err(describe)?;
        println!("{} {}", hex(&id.0), hex(key.point()));
    }
    Ok(())
}

/// Verify the signed package folder against the pinned keys now, then lay it out for the host.
fn host_layout(request: &HostLayoutRequest) -> Result<PathBuf, String> {
    let keys = KeySet::production().map_err(describe)?;
    let now = Timestamp::now().map_err(describe)?;
    write_host_layout(&request.package, &request.output, &keys, now, request.files)
}

/// Pack the folder, checking every file against its manifest, then write the payload.
fn write_setup_payload(request: &PayloadRequest) -> Result<(), String> {
    let payload = install::write_payload(&request.package)
        .map_err(|error| format!("Could not pack {}: {error}.", request.package.display()))?;
    fs::write(&request.output, payload)
        .map_err(|error| format!("Could not write {}: {error}", request.output.display()))
}

/// The options `sign` takes; `resign` takes these and `--version`.
const SIGN_OPTIONS: [&str; 5] = ["--package", "--storage", "--key", "--serial", "--days"];

fn parse_sign_request(arguments: &[String]) -> Result<SignRequest, String> {
    sign_request_from(Options::parse(arguments, &SIGN_OPTIONS)?)
}

fn parse_resign_request(arguments: &[String]) -> Result<ResignRequest, String> {
    let mut allowed = SIGN_OPTIONS.to_vec();
    allowed.push("--version");
    let mut options = Options::parse(arguments, &allowed)?;
    let version = parse_version(&options.take_one("--version")?)?;
    Ok(ResignRequest {
        sign: sign_request_from(options)?,
        version,
    })
}

fn parse_verify_request(arguments: &[String]) -> Result<VerifyRequest, String> {
    let mut options = Options::parse(arguments, &["--package", "--version"])?;
    Ok(VerifyRequest {
        package: PathBuf::from(options.take_one("--package")?),
        version: parse_version(&options.take_one("--version")?)?,
    })
}

fn sign_request_from(mut options: Options) -> Result<SignRequest, String> {
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

        let payload = parse_request(&strings(&["payload", "--package", "p", "--output", "o"]))
            .expect("a full payload request parses");
        assert_eq!(
            payload,
            Request::Payload(PayloadRequest {
                package: PathBuf::from("p"),
                output: PathBuf::from("o"),
            })
        );
        assert!(parse_request(&strings(&["payload", "--package", "p"])).is_err());

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
    fn key_and_layout_requests_parse_strictly() {
        assert_eq!(
            parse_request(&strings(&["import-key", "--key", "k", "--from-env", "V"])),
            Ok(Request::ImportKey(ImportKeyRequest {
                key: "k".to_owned(),
                variable: "V".to_owned(),
            }))
        );
        // The key itself is never an option, so it cannot reach a command line.
        assert!(parse_request(&strings(&["import-key", "--key", "k", "--secret", "x"])).is_err());
        assert!(parse_request(&strings(&["import-key", "--key", "k"])).is_err());
        assert_eq!(
            parse_request(&strings(&["delete-key", "--key", "k"])),
            Ok(Request::DeleteKey("k".to_owned()))
        );
        assert!(parse_request(&strings(&["delete-key"])).is_err());
        assert_eq!(
            parse_request(&strings(&[
                "verify-release",
                "--package",
                "p",
                "--version",
                "0.7.0"
            ])),
            Ok(Request::VerifyRelease(VerifyRequest {
                package: PathBuf::from("p"),
                version: Version::new(0, 7, 0),
            }))
        );
        assert!(parse_request(&strings(&["verify-release", "--package", "p"])).is_err());
        assert_eq!(
            parse_request(&strings(&[
                "resign",
                "--package",
                "p",
                "--version",
                "0.7.0",
                "--storage",
                "software",
                "--key",
                "k",
                "--serial",
                "9",
                "--days",
                "30"
            ])),
            Ok(Request::Resign(ResignRequest {
                sign: SignRequest {
                    package: PathBuf::from("p"),
                    storage: KeyStorage::Software,
                    key: "k".to_owned(),
                    serial: 9,
                    days: 30,
                },
                version: Version::new(0, 7, 0),
            }))
        );
        // `sign` never takes a version, and `resign` never goes without one.
        assert!(
            parse_request(&strings(&[
                "sign",
                "--package",
                "p",
                "--version",
                "0.7.0",
                "--storage",
                "software",
                "--key",
                "k",
                "--serial",
                "9",
                "--days",
                "30"
            ]))
            .is_err()
        );
        assert!(parse_request(&strings(&["resign", "--package", "p"])).is_err());
        assert_eq!(
            parse_request(&strings(&["pinned-keys"])),
            Ok(Request::PinnedKeys)
        );
        assert!(parse_request(&strings(&["pinned-keys", "--all", "yes"])).is_err());

        let layout = |files: &str| {
            parse_request(&strings(&[
                "host-layout",
                "--package",
                "p",
                "--output",
                "o",
                "--files",
                files,
            ]))
        };
        assert_eq!(
            layout("omit"),
            Ok(Request::HostLayout(HostLayoutRequest {
                package: PathBuf::from("p"),
                output: PathBuf::from("o"),
                files: HostFiles::Omit,
            }))
        );
        assert!(layout("include").is_ok());
        assert!(layout("some").is_err());
    }

    #[test]
    fn a_missing_key_variable_is_refused_before_key_storage() {
        let request = ImportKeyRequest {
            key: "never-stored".to_owned(),
            variable: "SYRINX_TEST_VARIABLE_THAT_IS_NEVER_SET".to_owned(),
        };
        let message = import_key(&request).expect_err("an unset variable is refused");
        assert!(message.contains("is not set or is empty"), "{message}");
    }

    #[test]
    fn a_resign_refuses_a_folder_without_its_signed_files_before_any_key() {
        let scratch = Scratch::new();
        scratch.write(MANIFEST, b"syrinx-update-manifest 1\n");
        let request = ResignRequest {
            sign: SignRequest {
                package: scratch.root.clone(),
                storage: KeyStorage::Software,
                key: "syrinx-update-tool-test-key-that-never-existed".to_owned(),
                serial: 2,
                days: 30,
            },
            version: Version::new(0, 7, 0),
        };
        let verify = VerifyRequest {
            package: scratch.root.clone(),
            version: Version::new(0, 7, 0),
        };
        let message = resign(&request).expect_err("missing signatures are refused");
        assert!(message.contains("Could not open"), "{message}");
        assert!(verify_release(&verify).is_err());

        // With all four files present, an unverifiable release stops before the key is opened.
        scratch.write(MANIFEST_SIGNATURE, &[0; SIGNATURE_BYTES]);
        scratch.write(STATEMENT, b"syrinx-update-latest 1\n");
        scratch.write(STATEMENT_SIGNATURE, &[0; SIGNATURE_BYTES]);
        let message = resign(&request).expect_err("an unverified release is refused");
        assert!(message.contains("would be refused"), "{message}");
        assert!(verify_release(&verify).is_err());
    }

    #[test]
    fn deleting_an_absent_key_succeeds() {
        delete_key("syrinx-update-tool-test-key-that-never-existed")
            .expect("an absent key is reported, not an error");
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
