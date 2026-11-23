//! "Install plugin from folder…" end to end without the review sheet (#140): the real full
//! fixture signed with keys generated in this run, staged and verified by `syrinx-update`,
//! reviewed and installed by the plugin host, updated, held and uninstalled.
//!
//! `Installer` below is the order the review sheet's slice calls these steps in.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use syrinx_core::CommandRegistry;
use syrinx_plugin_host::PluginRegistry;
use syrinx_plugin_host::SignedIdentity;
use syrinx_plugin_host::grants::DenyList;
use syrinx_plugin_host::manifest::{CapabilityKind, MANIFEST_FILE};
use syrinx_plugin_host::owner::{CacheSetup, InstallRefusal, Offer, OwnerRuntime, Plan, Reviewed};
use syrinx_store::{DirectoryLease, LocationError, Store};
use syrinx_update::{
    DetachedSignature, FileEntry, Installed, KeyId, PACKAGE_FILE, PACKAGE_SIGNATURE_FILE,
    PackagePath, PinnedKey, PluginFolders, PluginInstallError, PluginOffer, PluginPackageList,
    Publisher, SignatureKind, StagedPlugin, TestKeyPair, Version, sha256, stage_plugin,
};

#[path = "support/plugin_fixture.rs"]
mod plugin_fixture;

use plugin_fixture::fixture_workspace;

const WORLD: &str = "syrinx:plugin/plugin@0.1.0";
const MODULE_FILE: &str = "plugin.wasm";
const READ_LIMIT: usize = 64 * 1024 * 1024;
/// Every test package asks for storage, which the echo command writes a setting to.
const STORAGE: &str = "[[capabilities]]\nkind = \"storage\"\nreason = \"Keeps synthetic settings.\"\nrequired = true\n";
const FOLDER: &str = "[[capabilities]]\nkind = \"folder\"\naccess = \"read\"\nreason = \"Reads a synthetic vault.\"\nrequired = false\n";

static NEXT: AtomicU64 = AtomicU64::new(0);

fn full_component() -> &'static [u8] {
    plugin_fixture::full_component(&Path::new(env!("CARGO_TARGET_TMPDIR")).join("plugin-fixtures"))
}

/// A fresh folder for one test, with the data folder Syrinx always has, removed afterwards.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "syrinx-plugin-install-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("data").join("PluginCache")).expect("a synthetic folder");
        Self(root)
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

struct Lease(PathBuf);

impl DirectoryLease for Lease {
    fn database_path(&self) -> &Path {
        &self.0
    }

    fn revalidate(&self) -> Result<(), LocationError> {
        Ok(())
    }
}

/// Write the full fixture as `version` of plugin `test`, asking for storage and
/// `capabilities`, into a new folder, signed by `signer`.
fn package(
    scratch: &Scratch,
    version: Version,
    capabilities: &str,
    signer: &TestKeyPair,
) -> PathBuf {
    let folder = scratch
        .0
        .join(format!("chosen-{}", NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&folder).expect("a package folder");
    let manifest = fs::read_to_string(fixture_workspace().join("full").join(MANIFEST_FILE))
        .expect("the fixture's manifest")
        .replace(
            "\nversion = \"0.1.0\"",
            &format!("\nversion = \"{version}\""),
        )
        + STORAGE
        + capabilities;
    let files = [
        (MODULE_FILE, full_component().to_vec()),
        (MANIFEST_FILE, manifest.into_bytes()),
    ];
    let mut entries = Vec::new();
    for (path, bytes) in &files {
        fs::write(folder.join(path), bytes).expect("a package file");
        let path = PackagePath::parse(path).expect("a plain name");
        let digest = sha256(bytes).expect("hashed");
        entries.push(FileEntry::new(path, bytes.len() as u64, digest));
    }
    let list = PluginPackageList::new("test", version, WORLD, entries)
        .expect("a list")
        .to_bytes();
    let point = signer.public_point().expect("a point");
    let value = signer.sign_message(&list).expect("signed");
    let id = KeyId::of(&point).expect("an id");
    let signature = DetachedSignature::new(SignatureKind::PluginPackage, id, value);
    fs::write(folder.join(PACKAGE_FILE), &list).expect("package");
    fs::write(folder.join(PACKAGE_SIGNATURE_FILE), signature.to_bytes()).expect("package.sig");
    folder
}

fn compiled_sha256(bytes: &[u8]) -> Result<[u8; 32], String> {
    sha256(bytes).map_err(|error| error.to_string())
}

/// What the application holds: the store, the runtime, the registry and the palette's
/// commands, and the data folder's plugin folders.
struct Installer {
    store: Arc<Store>,
    runtime: OwnerRuntime,
    plugins: PluginRegistry,
    commands: CommandRegistry,
    folders: PluginFolders,
    cache: PathBuf,
}

impl Installer {
    fn start(scratch: &Scratch) -> Self {
        let lease = Arc::new(Lease(scratch.data().join("syrinx.db")));
        let store = Arc::new(Store::open(lease).expect("a synthetic store"));
        let cache = scratch.data().join("PluginCache");
        let setup = CacheSetup {
            folder: cache.clone(),
            sha256: compiled_sha256,
        };
        let runtime =
            OwnerRuntime::start(Arc::clone(&store), "0.1.0", DenyList::new(), Some(setup))
                .expect("starts");
        Self {
            store,
            runtime,
            plugins: PluginRegistry::new(),
            commands: CommandRegistry::browser(),
            folders: PluginFolders::new(&scratch.data()),
            cache,
        }
    }

    /// Stage, review, place, install; any failure leaves no folder, record or registration.
    fn install(
        &mut self,
        folder: &Path,
        trusted: PinnedKey,
        granted: &[CapabilityKind],
    ) -> Result<(), String> {
        let offer = PluginOffer {
            folder,
            installed: None,
            first_use: Some(trusted),
            developer_mode: false,
        };
        let staged = stage_plugin(&self.folders, &offer).map_err(|error| error.to_string())?;
        let reviewed = self
            .runtime
            .review(host_offer(&staged)?, None)
            .map_err(|error| error.to_string())?;
        let placed = staged
            .place(&self.folders)
            .map_err(|error| error.to_string())?;
        self.runtime
            .install_reviewed(reviewed, granted, &mut self.plugins, &mut self.commands, 20)
            .map_err(|error| error.to_string())?;
        placed.keep();
        Ok(())
    }

    /// Stage and review an update of `plugin` from its install record; a held one keeps its
    /// placed folder beside the running version until the owner answers.
    fn offer_update(&mut self, plugin: &str, folder: &Path) -> Result<Reviewed, String> {
        let record = self
            .store
            .plugin_installs()
            .expect("read")
            .into_iter()
            .find(|record| record.plugin_id == plugin)
            .ok_or("not installed")?;
        let installed = Installed {
            id: &record.plugin_id,
            version: Version::parse(&record.version).ok_or("a damaged version")?,
            publisher: Publisher::from_record(&record.signer_key)
                .map_err(|error| error.to_string())?,
        };
        let offer = PluginOffer {
            folder,
            installed: Some(installed),
            first_use: None,
            developer_mode: false,
        };
        let staged = stage_plugin(&self.folders, &offer).map_err(|error| error.to_string())?;
        let reviewed = self
            .runtime
            .review(host_offer(&staged)?, Some(plugin))
            .map_err(|error| error.to_string())?;
        staged
            .place(&self.folders)
            .map_err(|error| error.to_string())?
            .keep();
        Ok(reviewed)
    }

    fn update(&mut self, reviewed: Reviewed, granted: &[CapabilityKind]) -> Result<(), String> {
        self.runtime
            .update_reviewed(reviewed, granted, &mut self.plugins, &mut self.commands, 30)
            .map_err(|error| error.to_string())
    }

    /// Withdraw, stop and forget the plugin, then delete its package folders.
    fn uninstall(&mut self, plugin: &str) {
        self.runtime
            .uninstall(plugin, &mut self.plugins, &mut self.commands)
            .expect("uninstalled");
        self.folders.remove_plugin(plugin).expect("folders removed");
    }

    fn echo(&mut self) {
        let pending = self
            .runtime
            .call("test", |session| session.run_command("test.echo"))
            .expect("queued");
        let answer = pending.wait(Duration::from_secs(30)).expect("answered");
        assert_eq!(answer, Ok(Ok(())));
    }

    /// The compiled-code record for `plugin` once the background compile has written it.
    fn wait_for_compiled(&self, plugin: &str) -> [u8; 32] {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            if let Some(compiled) = self.store.plugin_compiled(plugin).expect("read") {
                return compiled.module_sha256;
            }
            assert!(
                Instant::now() < deadline,
                "the background compile never finished"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn close(self) {
        self.runtime.shutdown();
        let Ok(store) = Arc::try_unwrap(self.store) else {
            panic!("something else still holds the store");
        };
        store.shutdown().expect("the store closes");
    }
}

/// The host's view of a staged package, read from its checked files.
fn host_offer(staged: &StagedPlugin) -> Result<Offer, String> {
    let list = staged.list();
    let manifest = staged
        .read_listed(MANIFEST_FILE, READ_LIMIT)
        .map_err(|error| error.to_string())?;
    let module = staged
        .read_listed(MODULE_FILE, READ_LIMIT)
        .map_err(|error| error.to_string())?;
    let publisher = staged.publisher();
    Ok(Offer {
        signed: SignedIdentity::from_signed_lines(
            list.id(),
            &list.version().to_string(),
            list.world(),
        ),
        manifest: manifest.bytes,
        manifest_sha256: manifest.sha256,
        component: Arc::from(module.bytes),
        module_sha256: module.sha256,
        publisher: publisher.to_record(),
        unsigned: publisher == Publisher::Unsigned,
    })
}

fn v(minor: u32) -> Version {
    Version::new(0, minor, 0)
}

#[test]
fn the_test_plugin_installs_from_a_signed_folder_and_records_what_loads() {
    let scratch = Scratch::new("install");
    let signer = TestKeyPair::generate().expect("a key");
    let key = PinnedKey::new(signer.public_point().expect("a point"));
    let mut installer = Installer::start(&scratch);
    let folder = package(&scratch, v(1), "", &signer);
    installer
        .install(&folder, key, &[CapabilityKind::Storage])
        .expect("installed");

    assert!(installer.commands.find("test.echo").is_some());
    installer.echo();
    let record = &installer.store.plugin_installs().expect("read")[0];
    let module = sha256(full_component()).expect("hashed");
    assert_eq!(record.module_sha256, module, "the bytes that load");
    assert_eq!(record.signer_key, Publisher::Key(key).to_record());
    assert_eq!(installer.wait_for_compiled("test"), module);
    let placed = installer.folders.version_folder("test", v(1));
    assert!(placed.join(MODULE_FILE).is_file());
    assert_eq!(installer.folders.versions("test").expect("listed"), [v(1)]);
    installer.close();
}

#[test]
fn a_widened_update_is_held_while_the_old_version_runs_and_declining_keeps_it() {
    let scratch = Scratch::new("held");
    let signer = TestKeyPair::generate().expect("a key");
    let key = PinnedKey::new(signer.public_point().expect("a point"));
    let mut installer = Installer::start(&scratch);
    let first = package(&scratch, v(1), "", &signer);
    installer
        .install(&first, key, &[CapabilityKind::Storage])
        .expect("installed");

    let wider = package(&scratch, v(2), FOLDER, &signer);
    let held = installer.offer_update("test", &wider).expect("reviewed");
    assert!(
        matches!(held.plan(), Plan::Held(changes) if changes.added == [CapabilityKind::Folder])
    );
    assert_eq!(
        installer.folders.versions("test").expect("listed"),
        [v(1), v(2)]
    );
    assert_eq!(
        installer.store.plugin_installs().expect("read")[0].version,
        "0.1.0"
    );
    installer.echo();

    // Declining keeps the old version and deletes the held one's folder.
    drop(held);
    installer
        .folders
        .remove_version("test", v(2))
        .expect("declined");
    assert_eq!(installer.folders.versions("test").expect("listed"), [v(1)]);

    // Accepting replaces it; once it has run, the old version's folder goes.
    let held = installer.offer_update("test", &wider).expect("reviewed");
    installer
        .update(held.accept(), &[CapabilityKind::Folder])
        .expect("updated");
    installer.echo();
    let record = &installer.store.plugin_installs().expect("read")[0];
    assert_eq!(record.version, "0.2.0");
    installer
        .folders
        .remove_older_versions("test", v(2))
        .expect("pruned");
    assert_eq!(installer.folders.versions("test").expect("listed"), [v(2)]);
    installer.close();
}

#[test]
fn an_update_signed_by_a_different_publisher_is_refused_and_leaves_nothing() {
    let scratch = Scratch::new("publisher");
    let signer = TestKeyPair::generate().expect("a key");
    let stranger = TestKeyPair::generate().expect("a key");
    let key = PinnedKey::new(signer.public_point().expect("a point"));
    let mut installer = Installer::start(&scratch);
    let first = package(&scratch, v(1), "", &signer);
    installer
        .install(&first, key, &[CapabilityKind::Storage])
        .expect("installed");

    let foreign = package(&scratch, v(2), "", &stranger);
    let refusal = installer.offer_update("test", &foreign).err();
    assert_eq!(
        refusal.as_deref(),
        Some("This update is signed by a different publisher")
    );
    assert_eq!(
        PluginInstallError::DifferentPublisher.to_string(),
        InstallRefusal::DifferentPublisher.to_string(),
        "both checks speak with one voice"
    );
    assert_eq!(installer.folders.versions("test").expect("listed"), [v(1)]);
    installer.echo();
    installer.close();
}

#[test]
fn uninstall_leaves_no_install_row_grant_setting_cache_entry_or_folder() {
    let scratch = Scratch::new("uninstall");
    let signer = TestKeyPair::generate().expect("a key");
    let key = PinnedKey::new(signer.public_point().expect("a point"));
    let mut installer = Installer::start(&scratch);
    let folder = package(&scratch, v(1), "", &signer);
    installer
        .install(&folder, key, &[CapabilityKind::Storage])
        .expect("installed");
    installer.echo();
    installer.wait_for_compiled("test");
    assert!(
        installer
            .store
            .plugin_setting("test", "test.greeting")
            .expect("read")
            .is_some()
    );

    installer.uninstall("test");
    assert_eq!(installer.store.plugin_installs(), Ok(Vec::new()));
    assert_eq!(installer.store.plugin_grants(), Ok(Vec::new()));
    assert_eq!(
        installer.store.plugin_setting("test", "test.greeting"),
        Ok(None)
    );
    assert_eq!(installer.store.plugin_compiled("test"), Ok(None));
    let cached: Vec<_> = fs::read_dir(&installer.cache)
        .expect("the cache folder")
        .collect();
    assert!(cached.is_empty(), "no compiled code is left");
    assert!(
        installer
            .folders
            .versions("test")
            .expect("listed")
            .is_empty()
    );
    assert!(installer.commands.find("test.echo").is_none());
    installer.close();
}
