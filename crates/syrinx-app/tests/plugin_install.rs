//! "Install plugin from folder…" end to end without the review sheet (#140): the real full
//! fixture signed with keys generated in this run, staged and verified by `syrinx-update`,
//! reviewed and installed by the plugin host, updated, held across a restart and uninstalled.
//!
//! `Installer` below is the order the review sheet's slice calls these steps in. It lives here
//! because `syrinx-app` is a binary: code nothing calls yet would fail Clippy, and the menu
//! item that calls it belongs with the sheet.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use syrinx_core::CommandRegistry;
use syrinx_plugin_host::grants::DenyList;
use syrinx_plugin_host::manifest::{
    CapabilityKind, DiscoveryOptions, MANIFEST_FILE, PluginManifest,
};
use syrinx_plugin_host::owner::{
    CacheSetup, Hasher, InstalledOffer, Offer, OwnerRuntime, Plan, Reviewed, UNSIGNED_PUBLISHER,
};
use syrinx_plugin_host::{PluginRegistry, SignedIdentity};
use syrinx_store::{
    DirectoryLease, FolderAccess, LocationError, PluginGrant, PluginInstall, Store,
};
use syrinx_update::{
    Basis, DetachedSignature, FileEntry, Installed, KeyId, PACKAGE_FILE, PACKAGE_SIGNATURE_FILE,
    PackagePath, PinnedKey, PlacedPlugin, PluginFolders, PluginInstallError, PluginOffer,
    PluginPackageList, Publisher, SignatureKind, StagedPlugin, TestKeyPair, Version,
    open_installed, settle_pending_update, sha256, stage_plugin,
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

/// The full fixture's component with a custom section naming `version`: other bytes, the same
/// interfaces, so each version has its own module hash.
fn component(version: Version) -> Vec<u8> {
    let name = b"syrinx-test-version";
    let payload = version.to_string().into_bytes();
    let mut body = vec![name.len() as u8];
    body.extend_from_slice(name);
    body.extend_from_slice(&payload);
    let mut bytes = full_component().to_vec();
    // Custom section id 0, then its size as one LEB128 byte (the body is well under 128).
    bytes.push(0);
    bytes.push(body.len() as u8);
    bytes.extend_from_slice(&body);
    bytes
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

    fn database(&self) -> PathBuf {
        self.data().join("syrinx.db")
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

/// Write `version` of plugin `test`, asking for storage and `capabilities`, into a new folder,
/// signed by `signer`, or unsigned for developer mode.
fn package(
    scratch: &Scratch,
    version: Version,
    capabilities: &str,
    signer: Option<&TestKeyPair>,
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
        (MODULE_FILE, component(version)),
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
    fs::write(folder.join(PACKAGE_FILE), &list).expect("package");
    let Some(signer) = signer else {
        return folder;
    };
    let point = signer.public_point().expect("a point");
    let value = signer.sign_message(&list).expect("signed");
    let id = KeyId::of(&point).expect("an id");
    let signature = DetachedSignature::new(SignatureKind::PluginPackage, id, value);
    fs::write(folder.join(PACKAGE_SIGNATURE_FILE), signature.to_bytes()).expect("package.sig");
    folder
}

fn app_sha256(bytes: &[u8]) -> Result<[u8; 32], String> {
    sha256(bytes).map_err(|error| error.to_string())
}

/// The host's view of a staged or placed package: its manifest, then the module the manifest
/// names, each read from its checked file.
fn host_offer(
    read: impl Fn(&str) -> Result<Vec<u8>, PluginInstallError>,
    list: &PluginPackageList,
    publisher: Publisher,
) -> Result<Offer, String> {
    let text = |error: PluginInstallError| error.to_string();
    let manifest = read(MANIFEST_FILE).map_err(text)?;
    let options = DiscoveryOptions {
        developer_mode: publisher == Publisher::Unsigned,
    };
    let parsed = PluginManifest::parse(&manifest, options).map_err(|error| error.to_string())?;
    let component = read(&parsed.entry().module).map_err(text)?;
    let signed =
        SignedIdentity::from_signed_lines(list.id(), &list.version().to_string(), list.world());
    Offer::new(
        signed,
        manifest,
        Arc::from(component),
        publisher.to_record(),
        Hasher::new(app_sha256).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn staged_offer(staged: &StagedPlugin) -> Result<Offer, String> {
    let read = |path: &str| staged.read_listed(path, READ_LIMIT).map(|file| file.bytes);
    host_offer(read, staged.list(), staged.publisher())
}

fn placed_offer(placed: &PlacedPlugin) -> Result<Offer, String> {
    let read = |path: &str| placed.read_listed(path, READ_LIMIT).map(|file| file.bytes);
    host_offer(read, placed.list(), placed.publisher())
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
    developer_mode: bool,
}

impl Installer {
    /// Open the store and load every installed plugin from its version folder, verified again
    /// against its record, as Syrinx does at start-up.
    fn start(scratch: &Scratch) -> Self {
        let lease = Arc::new(Lease(scratch.database()));
        let store = Arc::new(Store::open(lease).expect("a synthetic store"));
        let cache = scratch.data().join("PluginCache");
        let setup = CacheSetup {
            folder: cache.clone(),
            sha256: app_sha256,
        };
        let runtime =
            OwnerRuntime::start(Arc::clone(&store), "0.1.0", DenyList::new(), Some(setup))
                .expect("starts");
        let mut installer = Self {
            store,
            runtime,
            plugins: PluginRegistry::new(),
            commands: CommandRegistry::browser(),
            folders: PluginFolders::new(&scratch.data()),
            cache,
            developer_mode: false,
        };
        for record in installer.store.plugin_installs().expect("read") {
            installer.load_installed(&record).expect("loaded");
        }
        installer
    }

    fn load_installed(&mut self, record: &PluginInstall) -> Result<(), String> {
        let placed = open_installed(&self.folders, installed(record)?, self.developer_mode)
            .map_err(|error| error.to_string())?;
        let offer = placed_offer(&placed)?;
        self.runtime
            .load_installed(offer, &mut self.plugins, &mut self.commands)
            .map_err(|error| error.to_string())
    }

    /// Stage, review, place, install; any failure leaves no folder, record or registration.
    fn install(
        &mut self,
        folder: &Path,
        trusted: Option<PinnedKey>,
        granted: &[CapabilityKind],
    ) -> Result<(), String> {
        let offer = PluginOffer {
            folder,
            basis: Basis::New { trusted },
            developer_mode: self.developer_mode,
        };
        let staged = stage_plugin(&self.folders, &offer).map_err(|error| error.to_string())?;
        let reviewed = self
            .runtime
            .review_new(staged_offer(&staged)?)
            .map_err(|error| error.to_string())?;
        // The review found no record for the id, so a folder a crash left is cleared.
        let placed = staged
            .place_unrecorded(&self.folders)
            .map_err(|error| error.to_string())?;
        self.runtime
            .install_reviewed(reviewed, granted, &mut self.plugins, &mut self.commands, 20)
            .map_err(|error| error.to_string())?;
        placed.keep();
        Ok(())
    }

    /// The installed version read again from its own folder, as an update is reviewed against.
    fn base_offer(&self) -> Result<InstalledOffer, String> {
        let record = self.record();
        let placed = open_installed(&self.folders, installed(&record)?, self.developer_mode)
            .map_err(|error| error.to_string())?;
        placed_offer(&placed).map(InstalledOffer::new)
    }

    /// Stage, review and place an update of `test` from its install record. The placed
    /// folder is kept: a held update waits there, beside the running version, for the owner.
    fn offer_update(&mut self, folder: &Path) -> Result<Reviewed, String> {
        let record = self.record();
        let offer = PluginOffer {
            folder,
            basis: Basis::Update(installed(&record)?),
            developer_mode: self.developer_mode,
        };
        let staged = stage_plugin(&self.folders, &offer).map_err(|error| error.to_string())?;
        let reviewed = self
            .runtime
            .review_update(staged_offer(&staged)?, self.base_offer()?)
            .map_err(|error| error.to_string())?;
        staged
            .place(&self.folders)
            .map_err(|error| error.to_string())?
            .keep();
        Ok(reviewed)
    }

    /// The update waiting for the owner, found again from its folder, reviewed afresh.
    fn pending_update(&mut self) -> Option<Reviewed> {
        let record = self.record();
        let basis = installed(&record).expect("a readable record");
        let placed =
            settle_pending_update(&self.folders, basis, self.developer_mode).expect("settled")?;
        let offer = placed_offer(&placed).expect("an offer");
        let base = self.base_offer().expect("the installed version");
        Some(self.runtime.review_update(offer, base).expect("reviewed"))
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

    fn record(&self) -> PluginInstall {
        self.store.plugin_installs().expect("read").remove(0)
    }

    fn echo(&mut self) {
        let pending = self
            .runtime
            .call("test", |session| session.run_command("test.echo"))
            .expect("queued");
        let answer = pending.wait(Duration::from_secs(30)).expect("answered");
        assert_eq!(answer, Ok(Ok(())));
    }

    /// The compiled-code record for `plugin` once the background compile has written one
    /// naming `module`.
    fn wait_for_compiled(&self, plugin: &str, module: [u8; 32]) {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let compiled = self.store.plugin_compiled(plugin).expect("read");
            if compiled.is_some_and(|compiled| compiled.module_sha256 == module) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the background compile never finished"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn cache_entries(&self) -> usize {
        fs::read_dir(&self.cache).expect("the cache folder").count()
    }

    fn close(self) {
        self.runtime.shutdown();
        let Ok(store) = Arc::try_unwrap(self.store) else {
            panic!("something else still holds the store");
        };
        store.shutdown().expect("the store closes");
    }
}

/// The record's installed version, as `syrinx-update` checks against it.
fn installed(record: &PluginInstall) -> Result<Installed<'_>, String> {
    Ok(Installed {
        id: &record.plugin_id,
        version: Version::parse(&record.version).ok_or("a damaged version")?,
        publisher: Publisher::from_record(&record.signer_key).map_err(|error| error.to_string())?,
    })
}

fn v(minor: u32) -> Version {
    Version::new(0, minor, 0)
}

fn key(signer: &TestKeyPair) -> PinnedKey {
    PinnedKey::new(signer.public_point().expect("a point"))
}

fn module_hash(version: Version) -> [u8; 32] {
    sha256(&component(version)).expect("hashed")
}

#[test]
fn the_test_plugin_installs_from_a_signed_folder_and_records_what_loads() {
    let scratch = Scratch::new("install");
    let signer = TestKeyPair::generate().expect("a key");
    let mut installer = Installer::start(&scratch);
    let folder = package(&scratch, v(1), "", Some(&signer));
    installer
        .install(&folder, Some(key(&signer)), &[CapabilityKind::Storage])
        .expect("installed");

    assert!(installer.commands.find("test.echo").is_some());
    installer.echo();
    let record = installer.record();
    assert_eq!(
        record.module_sha256,
        module_hash(v(1)),
        "the bytes that load"
    );
    assert_eq!(record.signer_key, Publisher::Key(key(&signer)).to_record());
    installer.wait_for_compiled("test", module_hash(v(1)));
    assert_eq!(installer.folders.versions("test").expect("listed"), [v(1)]);
    installer.close();
}

#[test]
fn a_folder_left_by_a_crash_before_recording_never_blocks_installing_again() {
    let scratch = Scratch::new("crash");
    let signer = TestKeyPair::generate().expect("a key");
    let mut installer = Installer::start(&scratch);
    let folder = package(&scratch, v(1), "", Some(&signer));

    // Placed and kept, then nothing recorded, as if Syrinx had stopped in between.
    let offer = PluginOffer {
        folder: &folder,
        basis: Basis::New {
            trusted: Some(key(&signer)),
        },
        developer_mode: false,
    };
    stage_plugin(&installer.folders, &offer)
        .expect("staged")
        .place(&installer.folders)
        .expect("placed")
        .keep();
    assert_eq!(installer.store.plugin_installs(), Ok(Vec::new()));

    installer
        .install(&folder, Some(key(&signer)), &[CapabilityKind::Storage])
        .expect("installed again");
    installer.echo();
    assert_eq!(installer.folders.versions("test").expect("listed"), [v(1)]);
    installer.close();
}

#[test]
fn an_unsigned_folder_installs_only_in_developer_mode_with_its_label() {
    assert_eq!(Publisher::Unsigned.to_record(), UNSIGNED_PUBLISHER);
    let scratch = Scratch::new("developer");
    let mut installer = Installer::start(&scratch);
    let folder = package(&scratch, v(1), "", None);
    let refused = installer.install(&folder, None, &[CapabilityKind::Storage]);
    assert_eq!(refused, Err(PluginInstallError::Unsigned.to_string()));
    assert_eq!(installer.store.plugin_installs(), Ok(Vec::new()));

    installer.developer_mode = true;
    installer
        .install(&folder, None, &[CapabilityKind::Storage])
        .expect("installed as a development plugin");
    assert_eq!(installer.record().signer_key, UNSIGNED_PUBLISHER);
    installer.echo();
    installer.close();
}

#[test]
fn a_held_update_survives_a_restart_while_the_old_version_runs_and_can_be_accepted() {
    let scratch = Scratch::new("held");
    let signer = TestKeyPair::generate().expect("a key");
    let mut installer = Installer::start(&scratch);
    let first = package(&scratch, v(1), "", Some(&signer));
    installer
        .install(&first, Some(key(&signer)), &[CapabilityKind::Storage])
        .expect("installed");
    installer.wait_for_compiled("test", module_hash(v(1)));

    let wider = package(&scratch, v(2), FOLDER, Some(&signer));
    let held = installer.offer_update(&wider).expect("reviewed");
    assert!(
        matches!(held.plan(), Plan::Held(changes) if changes.added == [CapabilityKind::Folder])
    );
    drop(held);
    // Offering it again before answering finds no stuck folder.
    drop(installer.offer_update(&wider).expect("offered again"));
    installer.close();

    // After a restart the old version runs, untouched, and the held one is found again.
    let mut installer = Installer::start(&scratch);
    installer.echo();
    assert_eq!(installer.record().version, "0.1.0");
    assert_eq!(
        installer.folders.versions("test").expect("listed"),
        [v(1), v(2)]
    );
    let held = installer.pending_update().expect("still held");
    assert!(matches!(held.plan(), Plan::Held(_)));

    // Accepted: the new version replaces it, and only its compiled code stays warm.
    installer
        .update(held.accept(), &[CapabilityKind::Folder])
        .expect("updated");
    // The old version's compiled code is gone the moment the update returns.
    let compiled = installer.store.plugin_compiled("test").expect("read");
    assert!(compiled.is_none_or(|compiled| compiled.module_sha256 == module_hash(v(2))));
    installer.echo();
    let record = installer.record();
    assert_eq!(record.version, "0.2.0");
    assert_eq!(record.module_sha256, module_hash(v(2)));
    installer.wait_for_compiled("test", module_hash(v(2)));
    assert_eq!(installer.cache_entries(), 1, "one version warm");
    installer
        .folders
        .remove_older_versions("test", v(2))
        .expect("pruned");
    assert_eq!(installer.folders.versions("test").expect("listed"), [v(2)]);
    installer.close();
}

#[test]
fn a_held_update_declined_after_a_restart_leaves_the_old_version_alone() {
    let scratch = Scratch::new("declined");
    let signer = TestKeyPair::generate().expect("a key");
    let mut installer = Installer::start(&scratch);
    let first = package(&scratch, v(1), "", Some(&signer));
    installer
        .install(&first, Some(key(&signer)), &[CapabilityKind::Storage])
        .expect("installed");
    let wider = package(&scratch, v(2), FOLDER, Some(&signer));
    drop(installer.offer_update(&wider).expect("held"));
    installer.close();

    let mut installer = Installer::start(&scratch);
    drop(installer.pending_update().expect("still held"));
    installer
        .folders
        .remove_version("test", v(2))
        .expect("declined");
    assert!(installer.pending_update().is_none());
    assert_eq!(installer.folders.versions("test").expect("listed"), [v(1)]);
    installer.echo();
    installer.close();
}

#[test]
fn an_update_signed_by_a_different_publisher_is_refused_and_leaves_nothing() {
    let scratch = Scratch::new("publisher");
    let signer = TestKeyPair::generate().expect("a key");
    let stranger = TestKeyPair::generate().expect("a key");
    let mut installer = Installer::start(&scratch);
    let first = package(&scratch, v(1), "", Some(&signer));
    installer
        .install(&first, Some(key(&signer)), &[CapabilityKind::Storage])
        .expect("installed");

    let foreign = package(&scratch, v(2), "", Some(&stranger));
    let refusal = installer.offer_update(&foreign).err();
    assert_eq!(
        refusal.as_deref(),
        Some("This update is signed by a different publisher")
    );
    assert_eq!(installer.folders.versions("test").expect("listed"), [v(1)]);
    installer.echo();
    installer.close();
}

/// A folder request for two folders with write access, and its narrower successor.
const TWO_FOLDERS: &str = "[[capabilities]]\nkind = \"folder\"\naccess = \"read-write\"\ncount = 2\nreason = \"Reads two synthetic vaults.\"\nrequired = false\n";
const ONE_READ_FOLDER: &str = "[[capabilities]]\nkind = \"folder\"\naccess = \"read\"\ncount = 1\nreason = \"Reads one synthetic vault.\"\nrequired = false\n";
const OPEN_EXTERNAL: &str = "[[capabilities]]\nkind = \"open-external\"\nreason = \"Opens synthetic links.\"\nrequired = false\n";

/// The installed version as an update must leave it after failing: its record, every grant in
/// full, and each folder with its access.
struct Snapshot {
    record: PluginInstall,
    grants: Vec<PluginGrant>,
    folders: Vec<(PathBuf, FolderAccess)>,
}

impl Snapshot {
    /// Read the store, checking each folder hangs under the folder grant.
    fn of(installer: &Installer) -> Self {
        let stored = installer.store.plugin_grants().expect("read");
        let folder_grant = stored
            .iter()
            .find(|stored| stored.grant.capability == "folder")
            .map(|stored| stored.grant_id);
        let folders = installer.store.plugin_folders("test").expect("read");
        assert!(
            folders
                .iter()
                .all(|folder| Some(folder.grant_id) == folder_grant),
            "every folder belongs to the folder grant"
        );
        Self {
            record: installer.record(),
            grants: stored.into_iter().map(|stored| stored.grant).collect(),
            folders: folders
                .into_iter()
                .map(|folder| (folder.path, folder.access))
                .collect(),
        }
    }
}

/// Install version 1 asking for storage and two writable folders, both granted, and run it.
fn install_with_folders(scratch: &Scratch, signer: &TestKeyPair) -> Installer {
    let mut installer = Installer::start(scratch);
    let first = package(scratch, v(1), TWO_FOLDERS, Some(signer));
    let granted = [CapabilityKind::Storage, CapabilityKind::Folder];
    installer
        .install(&first, Some(key(signer)), &granted)
        .expect("installed");
    for name in ["first-vault", "second-vault"] {
        let vault = scratch.0.join(name);
        fs::create_dir(&vault).expect("a synthetic vault");
        installer
            .runtime
            .grant_folder("test", &vault, FolderAccess::ReadWrite)
            .expect("granted");
    }
    installer.echo();
    installer
}

/// Version 1, as `before` recorded it, is back in full and running.
fn assert_old_version_back(installer: &mut Installer, before: &Snapshot) {
    let after = Snapshot::of(installer);
    assert_eq!(after.record, before.record);
    assert_eq!(after.grants, before.grants);
    assert_eq!(after.folders, before.folders);
    assert_eq!(after.folders.len(), 2);
    let registered = installer.plugins.get("test").expect("registered again");
    assert_eq!(registered.manifest().version(), "0.1.0");
    assert!(installer.commands.find("test.echo").is_some());
    installer.echo();
}

/// Run `sql` on another connection to the same database, as a test's way to make one write fail.
fn run_sql(scratch: &Scratch, sql: &str) {
    let connection = rusqlite::Connection::open(scratch.database()).expect("a second connection");
    connection.execute_batch(sql).expect("synthetic triggers");
}

#[test]
fn an_update_whose_record_write_fails_puts_the_old_version_back() {
    let scratch = Scratch::new("record-refused");
    let signer = TestKeyPair::generate().expect("a key");
    let mut installer = install_with_folders(&scratch, &signer);
    let before = Snapshot::of(&installer);
    let newer = package(&scratch, v(2), TWO_FOLDERS, Some(&signer));
    let reviewed = installer.offer_update(&newer).expect("reviewed");

    // The new record is refused; restoring the old one is not.
    run_sql(
        &scratch,
        "CREATE TRIGGER refuse_new_record BEFORE UPDATE ON plugin_install
         WHEN NEW.version = '0.2.0' BEGIN SELECT RAISE(ABORT, 'synthetic refusal'); END;",
    );
    assert!(installer.update(reviewed, &[]).is_err());
    assert_old_version_back(&mut installer, &before);
    installer.close();
}

#[test]
fn an_update_failing_after_its_record_and_grants_are_written_puts_them_back() {
    let scratch = Scratch::new("grant-refused");
    let signer = TestKeyPair::generate().expect("a key");
    let mut installer = install_with_folders(&scratch, &signer);
    let before = Snapshot::of(&installer);
    let newer = package(
        &scratch,
        v(2),
        &format!("{ONE_READ_FOLDER}{OPEN_EXTERNAL}"),
        Some(&signer),
    );
    let reviewed = installer.offer_update(&newer).expect("reviewed").accept();

    // The record is written, the folder grant rescoped and its folders narrowed; then the
    // owner's new grant is refused.
    run_sql(
        &scratch,
        "CREATE TRIGGER refuse_open_external AFTER INSERT ON plugin_grant
         WHEN NEW.capability = 'open-external' BEGIN SELECT RAISE(ABORT, 'synthetic refusal'); END;",
    );
    let granted = [CapabilityKind::OpenExternal];
    assert!(installer.update(reviewed, &granted).is_err());
    assert_old_version_back(&mut installer, &before);
    installer.close();
}

#[test]
fn an_update_failing_to_load_after_the_old_version_stopped_loads_the_old_one_again() {
    let scratch = Scratch::new("load-refused");
    let signer = TestKeyPair::generate().expect("a key");
    let mut installer = install_with_folders(&scratch, &signer);
    let before = Snapshot::of(&installer);
    let newer = package(&scratch, v(2), TWO_FOLDERS, Some(&signer));
    let reviewed = installer.offer_update(&newer).expect("reviewed");

    // Writing the new record also writes a damaged one, so loading the new version, which
    // reads the records after the old version stopped, fails. Restoring the old record
    // removes the damage.
    run_sql(
        &scratch,
        "CREATE TRIGGER damage_records AFTER UPDATE ON plugin_install
         WHEN NEW.version = '0.2.0' BEGIN
             INSERT INTO plugin_install VALUES
                 ('DAMAGED', '1', zeroblob(32), zeroblob(32), 'synthetic', 1, 0, 0);
         END;
         CREATE TRIGGER repair_records AFTER UPDATE ON plugin_install
         WHEN NEW.version = '0.1.0' BEGIN
             DELETE FROM plugin_install WHERE plugin_id = 'DAMAGED';
         END;",
    );
    assert!(installer.update(reviewed, &[]).is_err());
    assert_old_version_back(&mut installer, &before);
    installer.close();
}

#[test]
fn uninstall_leaves_no_install_row_grant_setting_cache_entry_or_folder() {
    let scratch = Scratch::new("uninstall");
    let signer = TestKeyPair::generate().expect("a key");
    let mut installer = Installer::start(&scratch);
    let folder = package(&scratch, v(1), "", Some(&signer));
    installer
        .install(&folder, Some(key(&signer)), &[CapabilityKind::Storage])
        .expect("installed");
    installer.echo();
    installer.wait_for_compiled("test", module_hash(v(1)));
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
    assert_eq!(installer.cache_entries(), 0, "no compiled code is left");
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
