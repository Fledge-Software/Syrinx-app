//! The package's fixed file layout and its `atlas-package.json` manifest.

use serde_json::{Value, json};

use super::registration::{ACCEPTS_OPEN_URL, DATA_FOLDER, EXECUTABLE, GENERATED_SCRIPT, PRODUCT};

/// Manifest file name at the package root; the install script and P1-38 read it.
pub const MANIFEST: &str = "atlas-package.json";

/// Manifest format; raise it whenever a reader would misread the new shape.
pub const FORMAT: u64 = 1;

/// Rust target every package is built for (D2: Windows 11 x64 only).
pub const TARGET: &str = "x86_64-pc-windows-msvc";

/// Owner-run scripts copied from `packaging/` into every package.
pub const SCRIPTS: [&str; 5] = [
    "atlas-common.ps1",
    GENERATED_SCRIPT,
    "install-atlas.ps1",
    "register-atlas.ps1",
    "unregister-atlas.ps1",
];

/// Which build a package holds. No timestamp, so one commit yields one manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildIdentity {
    pub version: String,
    pub commit: String,
    pub dirty: bool,
}

/// One packaged file and the integrity facts the install script checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagedFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

/// Every file in a package except the manifest, in the order the manifest lists them.
pub fn layout() -> Vec<&'static str> {
    let mut files = vec![EXECUTABLE];
    files.extend(SCRIPTS);
    files.sort_unstable();
    files
}

/// Folder and zip name, such as `Atlas-0.1.0-1a2b3c4d5e6f-x64`; uncommitted builds say so.
pub fn package_name(identity: &BuildIdentity) -> String {
    let dirty = if identity.dirty { "-dirty" } else { "" };
    format!(
        "{PRODUCT}-{}-{}{dirty}-x64",
        identity.version, identity.commit
    )
}

/// Render the manifest, refusing a file list that differs from the fixed layout.
pub fn render(identity: &BuildIdentity, files: &[PackagedFile]) -> Result<String, String> {
    check_identity(identity)?;
    let listed: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
    if listed != layout() {
        return Err(format!(
            "Package files {listed:?} do not match the layout {:?}.",
            layout()
        ));
    }
    if let Some(file) = files.iter().find(|file| !is_sha256(&file.sha256)) {
        return Err(format!("{} has a malformed SHA-256 digest.", file.path));
    }

    let manifest = json!({
        "format": FORMAT,
        "product": PRODUCT,
        "version": identity.version,
        "commit": identity.commit,
        "dirty": identity.dirty,
        "target": TARGET,
        "profile": "local",
        "signed": false,
        "executable": EXECUTABLE,
        "accepts_open_url": ACCEPTS_OPEN_URL,
        "data_folder": DATA_FOLDER,
        "files": files.iter().map(file_entry).collect::<Vec<Value>>(),
    });
    serde_json::to_string_pretty(&manifest)
        .map(|text| text + "\n")
        .map_err(|error| format!("Could not encode the package manifest: {error}"))
}

fn file_entry(file: &PackagedFile) -> Value {
    json!({ "path": file.path, "bytes": file.bytes, "sha256": file.sha256 })
}

/// Keep names safe to use as one folder or zip name.
fn check_identity(identity: &BuildIdentity) -> Result<(), String> {
    let safe = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || ".-+".contains(character))
    };
    if !safe(&identity.version) || !safe(&identity.commit) {
        return Err("The package version and commit must be plain ASCII names.".to_owned());
    }
    Ok(())
}

fn is_sha256(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .chars()
            .all(|character| matches!(character, '0'..='9' | 'a'..='f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> BuildIdentity {
        BuildIdentity {
            version: "0.1.0".to_owned(),
            commit: "0123456789ab".to_owned(),
            dirty: false,
        }
    }

    fn files() -> Vec<PackagedFile> {
        layout()
            .into_iter()
            .enumerate()
            .map(|(index, path)| PackagedFile {
                path: path.to_owned(),
                bytes: index as u64 * 10,
                sha256: format!("{index:064x}"),
            })
            .collect()
    }

    #[test]
    fn layout_holds_the_executable_and_every_owner_script_once() {
        assert_eq!(
            layout(),
            [
                "atlas-app.exe",
                "atlas-common.ps1",
                "atlas-registration.ps1",
                "install-atlas.ps1",
                "register-atlas.ps1",
                "unregister-atlas.ps1",
            ]
        );
        assert!(!layout().contains(&MANIFEST));
    }

    #[test]
    fn name_identifies_version_commit_and_uncommitted_builds() {
        assert_eq!(package_name(&identity()), "Atlas-0.1.0-0123456789ab-x64");
        let dirty = BuildIdentity {
            dirty: true,
            ..identity()
        };
        assert_eq!(package_name(&dirty), "Atlas-0.1.0-0123456789ab-dirty-x64");
    }

    #[test]
    fn manifest_records_build_and_every_file() {
        let text = render(&identity(), &files()).unwrap();
        let manifest: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(manifest["format"], 1);
        assert_eq!(manifest["product"], "Atlas");
        assert_eq!(manifest["version"], "0.1.0");
        assert_eq!(manifest["commit"], "0123456789ab");
        assert_eq!(manifest["dirty"], false);
        assert_eq!(manifest["target"], TARGET);
        assert_eq!(manifest["signed"], false);
        assert_eq!(manifest["executable"], "atlas-app.exe");
        assert_eq!(manifest["accepts_open_url"], ACCEPTS_OPEN_URL);
        assert_eq!(manifest["data_folder"], DATA_FOLDER);
        let listed: Vec<_> = manifest["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["path"].as_str().unwrap())
            .collect();
        assert_eq!(listed, layout());
        assert_eq!(manifest["files"][1]["bytes"], 10);
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn manifest_is_reproducible_for_one_build() {
        assert_eq!(
            render(&identity(), &files()).unwrap(),
            render(&identity(), &files()).unwrap()
        );
    }

    #[test]
    fn manifest_refuses_missing_extra_or_reordered_files() {
        let mut missing = files();
        missing.pop();
        assert!(render(&identity(), &missing).is_err());

        let mut extra = files();
        extra.push(PackagedFile {
            path: "notes.txt".to_owned(),
            bytes: 1,
            sha256: "0".repeat(64),
        });
        assert!(render(&identity(), &extra).is_err());

        let mut reordered = files();
        reordered.swap(0, 1);
        assert!(render(&identity(), &reordered).is_err());
    }

    #[test]
    fn manifest_refuses_bad_digests_and_unsafe_names() {
        let mut bad_digest = files();
        bad_digest[0].sha256 = "ABC".to_owned();
        assert!(render(&identity(), &bad_digest).is_err());

        let unsafe_version = BuildIdentity {
            version: r"..\0.1.0".to_owned(),
            ..identity()
        };
        assert!(render(&unsafe_version, &files()).is_err());
    }
}
