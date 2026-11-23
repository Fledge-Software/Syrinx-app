//! Staging and placing synthetic plugin packages (#140), signed by keys generated in each run.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::cng;
use crate::cng::test_keys::TestKeyPair;
use crate::error::UpdateError;
use crate::{
    Basis, DetachedSignature, FileEntry, Installed, KeyId, PACKAGE_FILE, PACKAGE_SIGNATURE_FILE,
    PLUGIN_STAGING_FOLDER, PackagePath, PinnedKey, PluginFolders, PluginInstallError, PluginOffer,
    PluginPackageList, Publisher, SignatureKind, StagedPlugin, Version, open_installed,
    settle_pending_update, stage_plugin,
};

const WORLD: &str = "syrinx:plugin/plugin@0.1.0";
const MODULE: &str = "plugin.wasm";
const MANIFEST: &str = "syrinx-plugin.toml";
const LIMIT: usize = 1024 * 1024;

/// A fresh folder for one test, removed afterwards.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "syrinx-plugin-install-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let root = std::env::temp_dir().join(name);
        // The data folder exists before any plugin is installed, as Syrinx's always does.
        fs::create_dir_all(root.join("data")).expect("a fresh synthetic folder");
        Self(root)
    }

    fn folders(&self) -> PluginFolders {
        PluginFolders::new(&self.0.join("data"))
    }

    fn data(&self) -> PathBuf {
        self.0.join("data")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Only the folder this test created is removed.
        if fs::remove_dir_all(&self.0).is_err() {
            eprintln!("a synthetic plugin folder was left behind");
        }
    }
}

/// The synthetic files of `version`, which differ between versions.
fn files(version: Version) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (MODULE, format!("\0asm synthetic {version}").into_bytes()),
        (MANIFEST, format!("# synthetic {version}\n").into_bytes()),
        ("ui/index.html", b"<p>synthetic</p>\n".to_vec()),
    ]
}

/// Write plugin `id` at `version` into `folder`, signed by `signer` unless it is `None`.
fn write_package(
    folder: &Path,
    id: &str,
    version: Version,
    signer: Option<&TestKeyPair>,
) -> Result<(), UpdateError> {
    fs::create_dir_all(folder.join("ui")).expect("a package folder");
    let mut entries = Vec::new();
    for (path, bytes) in files(version) {
        fs::write(folder.join(path), &bytes).expect("a package file");
        let path = PackagePath::parse(path).expect("a plain path");
        entries.push(FileEntry::new(
            path,
            bytes.len() as u64,
            cng::sha256(&bytes)?,
        ));
    }
    let list = PluginPackageList::new(id, version, WORLD, entries)?.to_bytes();
    fs::write(folder.join(PACKAGE_FILE), &list).expect("package");
    if let Some(signer) = signer {
        let point = signer.public_point()?;
        let value = signer.sign_message(&list)?;
        let signature =
            DetachedSignature::new(SignatureKind::PluginPackage, KeyId::of(&point)?, value);
        fs::write(folder.join(PACKAGE_SIGNATURE_FILE), signature.to_bytes()).expect("sig");
    }
    Ok(())
}

fn key(signer: &TestKeyPair) -> Result<PinnedKey, UpdateError> {
    Ok(PinnedKey::new(signer.public_point()?))
}

fn first_install(folder: &Path, trusted: PinnedKey) -> PluginOffer<'_> {
    PluginOffer {
        folder,
        basis: Basis::New {
            trusted: Some(trusted),
        },
        developer_mode: false,
    }
}

fn update<'a>(folder: &'a Path, installed: Installed<'a>) -> PluginOffer<'a> {
    PluginOffer {
        folder,
        basis: Basis::Update(installed),
        developer_mode: false,
    }
}

fn v(minor: u32) -> Version {
    Version::new(0, minor, 0)
}

fn staged_folders(scratch: &Scratch) -> Vec<String> {
    match fs::read_dir(scratch.data().join(PLUGIN_STAGING_FOLDER)) {
        Ok(entries) => entries
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into()
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[test]
fn a_signed_folder_is_staged_placed_and_read_back_exactly() -> Result<(), UpdateError> {
    let scratch = Scratch::new();
    let signer = TestKeyPair::generate()?;
    let source = scratch.0.join("chosen");
    write_package(&source, "sample", v(1), Some(&signer))?;
    let folders = scratch.folders();

    let staged = stage_plugin(&folders, &first_install(&source, key(&signer)?)).expect("staged");
    assert_eq!(staged.list().id(), "sample");
    assert_eq!(staged.publisher(), Publisher::Key(key(&signer)?));
    let placed = staged.place(&folders).expect("placed");
    let module = placed.read_listed(MODULE, LIMIT).expect("read");
    assert_eq!(module.bytes, files(v(1))[0].1);
    assert_eq!(module.sha256, cng::sha256(&module.bytes)?);
    placed.keep();

    // The version folder holds the listed files and both signed files; staging is empty.
    let installed = folders.version_folder("sample", v(1));
    for name in [
        MODULE,
        MANIFEST,
        "ui/index.html",
        PACKAGE_FILE,
        PACKAGE_SIGNATURE_FILE,
    ] {
        assert!(installed.join(name).is_file(), "{name}");
    }
    assert!(staged_folders(&scratch).is_empty());
    Ok(())
}

#[test]
fn a_failed_step_leaves_no_staged_or_placed_folder() -> Result<(), UpdateError> {
    let scratch = Scratch::new();
    let signer = TestKeyPair::generate()?;
    let source = scratch.0.join("chosen");
    write_package(&source, "sample", v(1), Some(&signer))?;
    let folders = scratch.folders();
    let offer = first_install(&source, key(&signer)?);

    // The owner says no: dropping the staged copy removes it.
    drop(stage_plugin(&folders, &offer).expect("staged"));
    assert!(staged_folders(&scratch).is_empty());

    // A later step fails: dropping the placed copy without `keep` rolls it back.
    let placed = stage_plugin(&folders, &offer)
        .expect("staged")
        .place(&folders)
        .expect("placed");
    drop(placed);
    assert!(!folders.version_folder("sample", v(1)).exists());

    // A file changed after signing is refused, and nothing is left in staging.
    fs::write(source.join(MODULE), b"\0asm tampered").expect("tampers");
    let refused = stage_plugin(&folders, &offer).err();
    assert!(
        matches!(refused, Some(PluginInstallError::Package(_))),
        "{refused:?}"
    );
    assert!(staged_folders(&scratch).is_empty());

    // The refusal ended its flow: the untampered package stages again.
    fs::write(source.join(MODULE), &files(v(1))[0].1).expect("restores");
    stage_plugin(&folders, &offer).expect("staged again");
    Ok(())
}

#[test]
fn an_update_must_come_from_the_recorded_key_for_the_recorded_slot() -> Result<(), UpdateError> {
    let scratch = Scratch::new();
    let publisher = TestKeyPair::generate()?;
    let stranger = TestKeyPair::generate()?;
    let folders = scratch.folders();
    let installed = Installed {
        id: "sample",
        version: v(1),
        publisher: Publisher::Key(key(&publisher)?),
    };

    let foreign = scratch.0.join("foreign");
    write_package(&foreign, "sample", v(2), Some(&stranger))?;
    let refusal = stage_plugin(&folders, &update(&foreign, installed)).err();
    assert_eq!(refusal, Some(PluginInstallError::DifferentPublisher));
    assert_eq!(
        PluginInstallError::DifferentPublisher.to_string(),
        "This update is signed by a different publisher"
    );

    // The slot comes from the record: a package the right key signed as another id is refused.
    let other_slot = scratch.0.join("other-slot");
    write_package(&other_slot, "other", v(2), Some(&publisher))?;
    let refusal = stage_plugin(&folders, &update(&other_slot, installed)).err();
    assert!(
        matches!(
            refusal,
            Some(PluginInstallError::Package(UpdateError::WrongPlugin { .. }))
        ),
        "{refusal:?}"
    );

    // An equal version is a replay, not an update.
    let same = scratch.0.join("same");
    write_package(&same, "sample", v(1), Some(&publisher))?;
    let refusal = stage_plugin(&folders, &update(&same, installed)).err();
    assert!(
        matches!(
            refusal,
            Some(PluginInstallError::Package(UpdateError::NotNewer { .. }))
        ),
        "{refusal:?}"
    );

    let newer = scratch.0.join("newer");
    write_package(&newer, "sample", v(2), Some(&publisher))?;
    let staged: StagedPlugin = stage_plugin(&folders, &update(&newer, installed)).expect("staged");
    assert_eq!(staged.list().version(), v(2));
    Ok(())
}

#[test]
fn an_unsigned_folder_needs_developer_mode_and_never_replaces_a_signed_plugin()
-> Result<(), UpdateError> {
    let scratch = Scratch::new();
    let folders = scratch.folders();
    let source = scratch.0.join("dev");
    write_package(&source, "sample", v(2), None)?;

    let mut offer = PluginOffer {
        folder: &source,
        basis: Basis::New { trusted: None },
        developer_mode: false,
    };
    assert_eq!(
        stage_plugin(&folders, &offer).err(),
        Some(PluginInstallError::Unsigned)
    );
    offer.developer_mode = true;
    let staged = stage_plugin(&folders, &offer).expect("a development plugin");
    assert_eq!(staged.publisher(), Publisher::Unsigned);
    drop(staged);

    // A signed plugin is never replaced by an unsigned folder, even in developer mode.
    let signer = TestKeyPair::generate()?;
    offer.basis = Basis::Update(Installed {
        id: "sample",
        version: v(1),
        publisher: Publisher::Key(key(&signer)?),
    });
    assert_eq!(
        stage_plugin(&folders, &offer).err(),
        Some(PluginInstallError::DifferentPublisher)
    );
    Ok(())
}

#[test]
fn developer_mode_never_accepts_a_folder_signed_by_another_key() -> Result<(), UpdateError> {
    let scratch = Scratch::new();
    let folders = scratch.folders();
    let trusted = TestKeyPair::generate()?;
    let other = TestKeyPair::generate()?;
    let source = scratch.0.join("signed-elsewhere");
    write_package(&source, "sample", v(1), Some(&other))?;

    let mut offer = first_install(&source, key(&trusted)?);
    offer.developer_mode = true;
    assert_eq!(
        stage_plugin(&folders, &offer).err(),
        Some(PluginInstallError::Package(UpdateError::UnknownKey))
    );

    // A signed update to an unsigned development plugin is a different publisher too.
    offer.basis = Basis::Update(Installed {
        id: "sample",
        version: Version::new(0, 0, 1),
        publisher: Publisher::Unsigned,
    });
    assert_eq!(
        stage_plugin(&folders, &offer).err(),
        Some(PluginInstallError::DifferentPublisher)
    );
    Ok(())
}

#[test]
fn a_signed_first_install_needs_the_key_the_owner_trusts() -> Result<(), UpdateError> {
    let scratch = Scratch::new();
    let signer = TestKeyPair::generate()?;
    let source = scratch.0.join("chosen");
    write_package(&source, "sample", v(1), Some(&signer))?;
    let offer = PluginOffer {
        folder: &source,
        basis: Basis::New { trusted: None },
        developer_mode: true,
    };
    assert_eq!(
        stage_plugin(&scratch.folders(), &offer).err(),
        Some(PluginInstallError::NoPublisherKey)
    );
    Ok(())
}

#[test]
fn older_versions_go_once_the_new_one_runs_and_uninstall_removes_every_version()
-> Result<(), UpdateError> {
    let scratch = Scratch::new();
    let signer = TestKeyPair::generate()?;
    let folders = scratch.folders();
    place_new(&scratch, &folders, v(1), &signer)?;
    for minor in [2, 3] {
        place_update(&scratch, &folders, v(minor), &signer)?;
    }
    assert_eq!(
        folders.versions("sample").expect("listed"),
        [v(1), v(2), v(3)]
    );

    // Version 2 activated once; version 3 is a held update, so it stays.
    folders
        .remove_older_versions("sample", v(2))
        .expect("pruned");
    assert_eq!(folders.versions("sample").expect("listed"), [v(2), v(3)]);

    folders.remove_plugin("sample").expect("removed");
    assert!(folders.versions("sample").expect("listed").is_empty());
    Ok(())
}

#[test]
fn a_recorded_publisher_round_trips_and_damage_is_refused() -> Result<(), UpdateError> {
    let signer = TestKeyPair::generate()?;
    let publisher = Publisher::Key(key(&signer)?);
    let text = publisher.to_record();
    assert!(text.len() <= 256, "fits the install record's column");
    assert_eq!(Publisher::from_record(&text), Ok(publisher));
    assert_eq!(
        Publisher::from_record(&Publisher::Unsigned.to_record()),
        Ok(Publisher::Unsigned)
    );
    let upper_hex = format!("p256:{}", text["p256:".len()..].to_uppercase());
    for damaged in ["", "synthetic-test-key", "p256:00", &upper_hex] {
        assert_eq!(
            Publisher::from_record(damaged),
            Err(PluginInstallError::RecordedPublisher),
            "{damaged}"
        );
    }
    Ok(())
}

/// Install `id` at `version` signed by `signer` as a first install, keeping its folder.
fn place_new(
    scratch: &Scratch,
    folders: &PluginFolders,
    version: Version,
    signer: &TestKeyPair,
) -> Result<(), UpdateError> {
    let source = scratch.0.join(format!("chosen-{version}"));
    write_package(&source, "sample", version, Some(signer))?;
    stage_plugin(folders, &first_install(&source, key(signer)?))
        .expect("staged")
        .place(folders)
        .expect("placed")
        .keep();
    Ok(())
}

/// Place `version` of `sample` as an update of version 1, keeping its folder.
fn place_update(
    scratch: &Scratch,
    folders: &PluginFolders,
    version: Version,
    signer: &TestKeyPair,
) -> Result<(), UpdateError> {
    let source = scratch.0.join(format!("chosen-{version}"));
    write_package(&source, "sample", version, Some(signer))?;
    let installed = Installed {
        id: "sample",
        version: v(1),
        publisher: Publisher::Key(key(signer)?),
    };
    stage_plugin(folders, &update(&source, installed))
        .expect("staged")
        .place(folders)
        .expect("placed")
        .keep();
    Ok(())
}

#[test]
fn a_new_plugin_never_replaces_an_installed_one() -> Result<(), UpdateError> {
    let scratch = Scratch::new();
    let signer = TestKeyPair::generate()?;
    let folders = scratch.folders();
    place_new(&scratch, &folders, v(1), &signer)?;

    // Offered again as new, at the same version or another, it is refused before anything moves.
    for version in [v(1), v(2)] {
        let source = scratch.0.join(format!("again-{version}"));
        write_package(&source, "sample", version, Some(&signer))?;
        let staged =
            stage_plugin(&folders, &first_install(&source, key(&signer)?)).expect("staged");
        assert_eq!(
            staged.place(&folders).err(),
            Some(PluginInstallError::AlreadyInstalled)
        );
    }
    assert_eq!(folders.versions("sample").expect("listed"), [v(1)]);
    assert!(
        folders
            .version_folder("sample", v(1))
            .join(MODULE)
            .is_file()
    );
    Ok(())
}

#[test]
fn a_folder_left_without_a_record_never_blocks_a_new_install() -> Result<(), UpdateError> {
    let scratch = Scratch::new();
    let signer = TestKeyPair::generate()?;
    let folders = scratch.folders();
    // A crash between placing and recording leaves the version folder and no record.
    place_new(&scratch, &folders, v(1), &signer)?;

    let source = scratch.0.join("again");
    write_package(&source, "sample", v(2), Some(&signer))?;
    stage_plugin(&folders, &first_install(&source, key(&signer)?))
        .expect("staged")
        .place_unrecorded(&folders)
        .expect("placed over the leftover")
        .keep();
    assert_eq!(folders.versions("sample").expect("listed"), [v(2)]);
    Ok(())
}

#[test]
fn one_install_flow_per_plugin_at_a_time() -> Result<(), UpdateError> {
    let scratch = Scratch::new();
    let signer = TestKeyPair::generate()?;
    let folders = scratch.folders();
    let first_source = scratch.0.join("first");
    let second_source = scratch.0.join("second");
    write_package(&first_source, "sample", v(1), Some(&signer))?;
    write_package(&second_source, "sample", v(2), Some(&signer))?;
    let first_offer = first_install(&first_source, key(&signer)?);
    let second_offer = first_install(&second_source, key(&signer)?);

    // While the first flow holds the id, a second is refused and the first goes on untouched.
    let first = stage_plugin(&folders, &first_offer).expect("staged");
    assert_eq!(
        stage_plugin(&folders, &second_offer).err(),
        Some(PluginInstallError::InstallInProgress)
    );
    let placed = first.place_unrecorded(&folders).expect("placed");
    assert_eq!(
        stage_plugin(&folders, &second_offer).err(),
        Some(PluginInstallError::InstallInProgress),
        "still held once placed"
    );
    placed.keep();
    assert_eq!(folders.versions("sample").expect("listed"), [v(1)]);

    // Once a flow ends, kept or dropped, the next may start.
    drop(stage_plugin(&folders, &second_offer).expect("staged after a kept flow"));
    stage_plugin(&folders, &second_offer).expect("staged after a dropped flow");

    // An update's copy is never placed as a new plugin.
    let update_source = scratch.0.join("update");
    write_package(&update_source, "other", v(3), Some(&signer))?;
    let installed = Installed {
        id: "other",
        version: v(1),
        publisher: Publisher::Key(key(&signer)?),
    };
    let staged = stage_plugin(&folders, &update(&update_source, installed)).expect("staged");
    assert_eq!(
        staged.place_unrecorded(&folders).err(),
        Some(PluginInstallError::NotANewPlugin)
    );
    Ok(())
}

#[test]
fn a_pending_update_and_an_uninstall_are_held_like_any_install_flow() -> Result<(), UpdateError> {
    let scratch = Scratch::new();
    let signer = TestKeyPair::generate()?;
    let folders = scratch.folders();
    place_new(&scratch, &folders, v(1), &signer)?;
    place_update(&scratch, &folders, v(2), &signer)?;
    let installed = Installed {
        id: "sample",
        version: v(1),
        publisher: Publisher::Key(key(&signer)?),
    };

    // While the owner looks at the pending update, no other flow touches the plugin.
    let pending = settle_pending_update(&folders, installed, false)
        .expect("settled")
        .expect("pending");
    let source = scratch.0.join("another");
    write_package(&source, "sample", v(3), Some(&signer))?;
    assert_eq!(
        stage_plugin(&folders, &update(&source, installed)).err(),
        Some(PluginInstallError::InstallInProgress)
    );
    assert_eq!(
        folders.remove_plugin("sample").err(),
        Some(PluginInstallError::InstallInProgress)
    );
    assert_eq!(folders.versions("sample").expect("listed"), [v(1), v(2)]);

    // Declining removes the pending folder and ends the flow.
    pending.decline().expect("declined");
    assert_eq!(folders.versions("sample").expect("listed"), [v(1)]);

    // An uninstall while an update is staged is refused and leaves every folder alone.
    let staged = stage_plugin(&folders, &update(&source, installed)).expect("staged");
    assert_eq!(
        folders.remove_plugin("sample").err(),
        Some(PluginInstallError::InstallInProgress)
    );
    assert_eq!(folders.versions("sample").expect("listed"), [v(1)]);
    drop(staged);
    folders
        .remove_plugin("sample")
        .expect("uninstalled once the flow ended");
    assert!(folders.versions("sample").expect("listed").is_empty());
    Ok(())
}

#[test]
fn of_two_waiting_updates_the_newer_is_offered_and_the_older_removed() -> Result<(), UpdateError> {
    let scratch = Scratch::new();
    let signer = TestKeyPair::generate()?;
    let folders = scratch.folders();
    place_new(&scratch, &folders, v(1), &signer)?;
    place_update(&scratch, &folders, v(2), &signer)?;
    place_update(&scratch, &folders, v(3), &signer)?;
    let installed = Installed {
        id: "sample",
        version: v(1),
        publisher: Publisher::Key(key(&signer)?),
    };
    let pending = settle_pending_update(&folders, installed, false).expect("settled");
    assert_eq!(pending.map(|placed| placed.list().version()), Some(v(3)));
    assert_eq!(folders.versions("sample").expect("listed"), [v(1), v(3)]);
    Ok(())
}

#[test]
fn an_unsigned_waiting_update_is_kept_but_not_offered_without_developer_mode()
-> Result<(), UpdateError> {
    let scratch = Scratch::new();
    let folders = scratch.folders();
    let installed = Installed {
        id: "sample",
        version: v(1),
        publisher: Publisher::Unsigned,
    };
    for (version, basis) in [
        (v(1), Basis::New { trusted: None }),
        (v(2), Basis::Update(installed)),
    ] {
        let source = scratch.0.join(format!("dev-{version}"));
        write_package(&source, "sample", version, None)?;
        let offer = PluginOffer {
            folder: &source,
            basis,
            developer_mode: true,
        };
        stage_plugin(&folders, &offer)
            .expect("staged")
            .place(&folders)
            .expect("placed")
            .keep();
    }

    let off = settle_pending_update(&folders, installed, false).expect("settled");
    assert!(off.is_none(), "not offered while developer mode is off");
    assert_eq!(folders.versions("sample").expect("listed"), [v(1), v(2)]);
    let on = settle_pending_update(&folders, installed, true).expect("settled");
    assert_eq!(on.map(|placed| placed.list().version()), Some(v(2)));
    Ok(())
}

#[test]
fn the_installed_version_is_found_again_and_a_changed_file_is_refused() -> Result<(), UpdateError> {
    let scratch = Scratch::new();
    let signer = TestKeyPair::generate()?;
    let folders = scratch.folders();
    place_new(&scratch, &folders, v(1), &signer)?;
    let installed = Installed {
        id: "sample",
        version: v(1),
        publisher: Publisher::Key(key(&signer)?),
    };

    let found = open_installed(&folders, installed, false).expect("found again");
    assert_eq!(found.list().version(), v(1));
    drop(found);
    assert!(
        folders.version_folder("sample", v(1)).is_dir(),
        "found again, kept"
    );

    // A record naming another plugin, or another publisher, does not match the folder.
    let other_slot = Installed {
        id: "other",
        ..installed
    };
    assert!(open_installed(&folders, other_slot, false).is_err());
    let stranger = TestKeyPair::generate()?;
    let wrong_key = Installed {
        publisher: Publisher::Key(key(&stranger)?),
        ..installed
    };
    assert_eq!(
        open_installed(&folders, wrong_key, false).err(),
        Some(PluginInstallError::DifferentPublisher)
    );

    // A component changed after placing is refused, by `read_listed` and when found again.
    let placed = open_installed(&folders, installed, false).expect("found again");
    let module = folders.version_folder("sample", v(1)).join(MODULE);
    fs::write(&module, b"\0asm changed after placing").expect("tampers");
    assert_eq!(
        placed.read_listed(MODULE, LIMIT).err(),
        Some(PluginInstallError::Package(UpdateError::HashMismatch {
            path: MODULE.to_owned()
        }))
    );
    assert!(open_installed(&folders, installed, false).is_err());
    Ok(())
}

#[test]
fn a_held_update_is_found_again_offered_again_and_a_broken_one_removed() -> Result<(), UpdateError>
{
    let scratch = Scratch::new();
    let signer = TestKeyPair::generate()?;
    let folders = scratch.folders();
    place_new(&scratch, &folders, v(1), &signer)?;
    let installed = Installed {
        id: "sample",
        version: v(1),
        publisher: Publisher::Key(key(&signer)?),
    };
    assert!(
        settle_pending_update(&folders, installed, false)
            .expect("looked")
            .is_none()
    );

    // An update held for the owner is its own kept folder, found again as after a restart.
    let source = scratch.0.join("update");
    write_package(&source, "sample", v(2), Some(&signer))?;
    let offer = update(&source, installed);
    stage_plugin(&folders, &offer)
        .expect("staged")
        .place(&folders)
        .expect("placed")
        .keep();
    // The installed version's own folder is never an update's destination.
    let replay = scratch.0.join("replay");
    write_package(&replay, "sample", v(1), Some(&signer))?;
    assert!(matches!(
        stage_plugin(&folders, &update(&replay, installed)).err(),
        Some(PluginInstallError::Package(UpdateError::NotNewer { .. }))
    ));
    let pending = settle_pending_update(&folders, installed, false).expect("looked");
    assert_eq!(pending.map(|placed| placed.list().version()), Some(v(2)));

    // Offering the same version again replaces it instead of getting stuck.
    stage_plugin(&folders, &offer)
        .expect("staged again")
        .place(&folders)
        .expect("placed again")
        .keep();
    assert_eq!(folders.versions("sample").expect("listed"), [v(1), v(2)]);

    // A waiting folder that no longer verifies is removed, never offered.
    let module = folders.version_folder("sample", v(2)).join(MODULE);
    fs::write(&module, b"\0asm changed while waiting").expect("tampers");
    assert!(
        settle_pending_update(&folders, installed, false)
            .expect("looked")
            .is_none()
    );
    assert_eq!(folders.versions("sample").expect("listed"), [v(1)]);
    Ok(())
}
