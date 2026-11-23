//! Installing, updating and uninstalling the test plugin from offers (#140): which ids a new
//! plugin may take, an update that asks for more held while the old version runs, one that
//! asks for less narrowing its grants, a failed update putting the old version back in full,
//! a refused component withdrawn, and an uninstall that leaves nothing behind.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use syrinx_core::{Command, CommandAction, CommandId, CommandRegistry, CommandScope, Enablement};
use syrinx_plugin_host::bindings::syrinx::plugin::types::PluginError;
use syrinx_plugin_host::grants::DenyList;
use syrinx_plugin_host::manifest::CapabilityKind;
use syrinx_plugin_host::owner::{
    CacheSetup, Hasher, InstallRefusal, InstalledOffer, Offer, OwnerRuntime, Plan, Reviewed,
    UNSIGNED_PUBLISHER,
};
use syrinx_plugin_host::worker::PluginEvent;
use syrinx_plugin_host::{PluginRegistry, SignedIdentity};
use syrinx_store::{FolderAccess, PluginInstall, PluginState, Store};

use super::{FOLDER, READ_ONE_FOLDER, REQUIRED_STORAGE, STORAGE, TempStore, close, echo};
use crate::package::manifest_text;
use crate::support::sha256::sha256;
use crate::support::{self, FULL, Fixture, variant};

const WORLD: &str = "syrinx:plugin/plugin@0.1.0";
const PUBLISHER: &str = "synthetic-publisher";
const TWO_FOLDERS: &str = "[[capabilities]]\nkind = \"folder\"\naccess = \"read-write\"\ncount = 2\nreason = \"Reads two synthetic vaults.\"\nrequired = false\n";
const EXTRA_COMMAND: &str =
    "[[contributes.commands]]\nid = \"test.extra\"\ntitle = \"Synthetic test.extra\"\n";

/// The runtime, the registry and the palette's commands, as the application holds them.
struct Installer {
    runtime: OwnerRuntime,
    plugins: PluginRegistry,
    commands: CommandRegistry,
}

impl Installer {
    fn start(store: &Arc<Store>) -> Self {
        Self::start_with(store, None)
    }

    fn start_with(store: &Arc<Store>, cache: Option<CacheSetup>) -> Self {
        let runtime = OwnerRuntime::start(Arc::clone(store), "0.1.0-owner", DenyList::new(), cache)
            .expect("starts");
        Self {
            runtime,
            plugins: PluginRegistry::new(),
            commands: CommandRegistry::browser(),
        }
    }

    fn install(&mut self, offer: Offer, granted: &[CapabilityKind]) -> Result<(), InstallRefusal> {
        let reviewed = self.runtime.review_new(offer)?;
        self.runtime
            .install_reviewed(reviewed, granted, &mut self.plugins, &mut self.commands, 20)
    }

    fn update(
        &mut self,
        reviewed: Reviewed,
        granted: &[CapabilityKind],
    ) -> Result<(), InstallRefusal> {
        self.runtime
            .update_reviewed(reviewed, granted, &mut self.plugins, &mut self.commands, 30)
    }

    /// Review `offer` as an update of the installed version that `base` reproduces.
    fn review_update(&self, offer: Offer, base: Offer) -> Reviewed {
        self.runtime
            .review_update(offer, InstalledOffer::new(base))
            .expect("reviewed")
    }

    fn has_command(&self, id: &str) -> bool {
        self.commands.find(id).is_some()
    }
}

/// `plugin` at `version` with `extra` manifest text, from `publisher`, with `component`.
fn offer_of(
    plugin: &str,
    version: &str,
    extra: &str,
    publisher: &str,
    component: Fixture,
) -> Offer {
    let text = manifest_text(plugin).replace(
        "\nversion = \"0.1.0\"",
        &format!("\nversion = \"{version}\""),
    ) + extra;
    Offer::new(
        SignedIdentity::from_signed_lines(plugin, version, WORLD),
        text.into_bytes(),
        Arc::from(support::bytes(component)),
        publisher.to_owned(),
        hasher(),
    )
    .expect("an offer")
}

fn hasher() -> Hasher {
    Hasher::new(sha256).expect("SHA-256")
}

fn offer(plugin: &str, version: &str, capabilities: &str) -> Offer {
    offer_of(plugin, version, capabilities, PUBLISHER, FULL)
}

fn refusal(installer: &Installer, offer: Offer, base: Option<Offer>) -> InstallRefusal {
    let reviewed = match base {
        None => installer.runtime.review_new(offer),
        Some(base) => installer
            .runtime
            .review_update(offer, InstalledOffer::new(base)),
    };
    match reviewed {
        Ok(_) => panic!("the offer was accepted"),
        Err(refusal) => refusal,
    }
}

fn record(store: &Store) -> PluginInstall {
    store.plugin_installs().expect("read").remove(0)
}

fn grant_names(store: &Store) -> Vec<String> {
    store
        .plugin_grants()
        .expect("read")
        .into_iter()
        .map(|stored| stored.grant.capability)
        .collect()
}

#[test]
fn a_new_plugin_installs_with_what_the_owner_granted_and_records_the_bytes_it_loads() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut installer = Installer::start(&store);
    let offer = offer("test", "0.1.0", STORAGE);
    let manifest = (manifest_text("test") + STORAGE).into_bytes();
    installer
        .install(offer, &[CapabilityKind::Storage])
        .expect("installed");

    assert!(installer.has_command("test.echo"));
    assert_eq!(echo(&mut installer.runtime, "test"), Ok(Ok(())));
    let record = record(&store);
    assert_eq!(record.version, "0.1.0");
    assert_eq!(
        record.module_sha256,
        sha256(&support::bytes(FULL)).expect("hashed"),
        "the hash of the bytes that load"
    );
    assert_eq!(record.manifest_sha256, sha256(&manifest).expect("hashed"));
    assert_eq!(record.signer_key, PUBLISHER);
    assert_eq!(record.state, PluginState::Enabled);
    assert_eq!(grant_names(&store), ["storage"]);
    close(installer.runtime, store);
}

#[test]
fn a_hash_function_that_is_not_sha256_is_refused() {
    fn not_sha256(_: &[u8]) -> Result<[u8; 32], String> {
        Ok([0; 32])
    }
    assert!(matches!(
        Hasher::new(not_sha256),
        Err(InstallRefusal::Hash(_))
    ));
}

#[test]
fn an_unsigned_development_plugin_records_its_label() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut installer = Installer::start(&store);
    let unsigned = offer_of("test", "0.1.0", "", UNSIGNED_PUBLISHER, FULL);
    installer.install(unsigned, &[]).expect("installed");
    assert_eq!(record(&store).signer_key, "unsigned-development");
    close(installer.runtime, store);
}

#[test]
fn a_new_id_is_checked_against_syrinx_the_built_ins_and_every_install_record() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut installer = Installer::start(&store);
    installer
        .install(offer("test", "0.1.0", ""), &[])
        .expect("installed");

    for id in ["browser", "notes", "forge", "test"] {
        assert_eq!(
            refusal(&installer, offer(id, "0.1.0", ""), None),
            InstallRefusal::IdTaken(id.to_owned()),
            "{id}"
        );
    }

    // The record alone takes the id, even while its plugin is not registered this session.
    let restarted = Installer::start(&store);
    assert_eq!(
        refusal(&restarted, offer("test", "0.1.0", ""), None),
        InstallRefusal::IdTaken("test".to_owned())
    );
    restarted.runtime.shutdown();
    close(installer.runtime, store);
}

#[test]
fn a_new_install_reviewed_before_another_was_recorded_is_refused() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut installer = Installer::start(&store);
    let first = installer
        .runtime
        .review_new(offer("test", "0.1.0", STORAGE))
        .expect("reviewed");
    let second = installer
        .runtime
        .review_new(offer_of(
            "test",
            "0.1.0",
            STORAGE,
            "another-publisher",
            FULL,
        ))
        .expect("reviewed while the id was still free");
    let Installer {
        runtime,
        plugins,
        commands,
    } = &mut installer;
    runtime
        .install_reviewed(first, &[CapabilityKind::Storage], plugins, commands, 20)
        .expect("installed");
    let before = record(&store);

    assert_eq!(
        runtime.install_reviewed(second, &[CapabilityKind::Storage], plugins, commands, 25),
        Err(InstallRefusal::IdTaken("test".to_owned()))
    );
    assert_eq!(
        record(&store),
        before,
        "the first install's publisher stays"
    );
    assert_eq!(echo(&mut installer.runtime, "test"), Ok(Ok(())));
    installer.runtime.shutdown();

    // After a restart the first plugin is recorded but not registered or loaded, and a review
    // made elsewhere while the id was free is still refused rather than written over it.
    let elsewhere = TempStore::new();
    let elsewhere_store = elsewhere.open();
    let other = Installer::start(&elsewhere_store);
    let stale = offer_of("test", "0.1.0", STORAGE, "another-publisher", FULL);
    let reviewed = other.runtime.review_new(stale).expect("reviewed");
    close(other.runtime, elsewhere_store);
    let Installer {
        mut runtime,
        mut plugins,
        mut commands,
    } = Installer::start(&store);
    assert_eq!(
        runtime.install_reviewed(
            reviewed,
            &[CapabilityKind::Storage],
            &mut plugins,
            &mut commands,
            30
        ),
        Err(InstallRefusal::IdTaken("test".to_owned()))
    );
    assert_eq!(record(&store), before);
    close(runtime, store);
}

#[test]
fn full_trust_a_newer_syrinx_and_a_declined_required_capability_install_nothing() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut installer = Installer::start(&store);
    let terminal = "[[capabilities]]\nkind = \"terminal\"\ntrust = \"full\"\nreason = \"Runs synthetic builds.\"\nrequired = false\n";
    assert_eq!(
        refusal(&installer, offer("test", "0.1.0", terminal), None),
        InstallRefusal::FullTrust(CapabilityKind::Terminal)
    );

    let text = manifest_text("test").replace("min-syrinx = \"0.1.0\"", "min-syrinx = \"9.0.0\"");
    let future = Offer::new(
        SignedIdentity::from_signed_lines("test", "0.1.0", WORLD),
        text.into_bytes(),
        Arc::from(support::bytes(FULL)),
        PUBLISHER.to_owned(),
        hasher(),
    )
    .expect("an offer");
    assert_eq!(
        refusal(&installer, future, None),
        InstallRefusal::NeedsNewerSyrinx("9.0.0".to_owned())
    );

    assert_eq!(
        installer.install(offer("test", "0.1.0", REQUIRED_STORAGE), &[]),
        Err(InstallRefusal::RequiredDeclined(CapabilityKind::Storage))
    );
    assert_eq!(store.plugin_installs(), Ok(Vec::new()));
    assert!(!installer.has_command("test.echo"));
    assert!(installer.plugins.is_empty());
    close(installer.runtime, store);
}

#[test]
fn an_update_that_makes_an_optional_capability_required_is_held_until_granted() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut installer = Installer::start(&store);
    // Folder access is optional in version 0.1.0, and the owner never granted it.
    installer
        .install(
            offer("test", "0.1.0", &format!("{STORAGE}{FOLDER}")),
            &[CapabilityKind::Storage],
        )
        .expect("installed");
    let required = FOLDER.replace("required = false", "required = true");
    let newer = || offer("test", "0.2.0", &format!("{STORAGE}{required}"));
    let base = || offer("test", "0.1.0", &format!("{STORAGE}{FOLDER}"));

    let held = installer.review_update(newer(), base());
    let Plan::Held(changes) = held.plan().clone() else {
        panic!("not held: {:?}", held.plan());
    };
    assert_eq!(changes.widened, [CapabilityKind::Folder]);

    // Accepted without the now-required grant, the update is refused and nothing changes.
    let before = record(&store);
    assert_eq!(
        installer.update(held.accept(), &[]),
        Err(InstallRefusal::RequiredDeclined(CapabilityKind::Folder))
    );
    assert_eq!(record(&store), before);

    // Accepted with it, the update applies and the grant is kept.
    let accepted = installer.review_update(newer(), base()).accept();
    installer
        .update(accepted, &[CapabilityKind::Folder])
        .expect("updated");
    assert_eq!(record(&store).version, "0.2.0");
    assert_eq!(grant_names(&store), ["folder", "storage"]);
    assert_eq!(echo(&mut installer.runtime, "test"), Ok(Ok(())));
    close(installer.runtime, store);
}

#[test]
fn a_widened_update_is_held_while_the_old_version_runs_until_accepted() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut installer = Installer::start(&store);
    installer
        .install(offer("test", "0.1.0", STORAGE), &[CapabilityKind::Storage])
        .expect("installed");
    let installed = record(&store);

    let wider = || offer("test", "0.2.0", &format!("{STORAGE}{FOLDER}"));
    let base = || offer("test", "0.1.0", STORAGE);
    let held = installer.review_update(wider(), base());
    let Plan::Held(changes) = held.plan().clone() else {
        panic!("not held: {:?}", held.plan());
    };
    assert_eq!(changes.added, [CapabilityKind::Folder]);

    // Not accepted yet: nothing changes and the old version keeps running.
    assert_eq!(
        installer.update(held, &[]),
        Err(InstallRefusal::NeedsConsent)
    );
    assert_eq!(
        record(&store),
        installed,
        "no hash recorded for a held update"
    );
    assert_eq!(echo(&mut installer.runtime, "test"), Ok(Ok(())));

    // Accepted, with the new folder capability granted: the new version replaces it.
    let accepted = installer.review_update(wider(), base()).accept();
    installer
        .update(accepted, &[CapabilityKind::Folder])
        .expect("updated");
    let updated = record(&store);
    assert_eq!(updated.version, "0.2.0");
    assert_eq!(updated.installed_at, 20, "the first install's time is kept");
    assert_ne!(updated.manifest_sha256, installed.manifest_sha256);
    assert_eq!(grant_names(&store), ["folder", "storage"]);
    assert_eq!(echo(&mut installer.runtime, "test"), Ok(Ok(())));
    close(installer.runtime, store);
}

#[test]
fn an_update_of_a_disabled_plugin_keeps_it_disabled_and_not_running() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut installer = Installer::start(&store);
    installer
        .install(offer("test", "0.1.0", STORAGE), &[CapabilityKind::Storage])
        .expect("installed");
    installer.runtime.shutdown();

    // The owner disabled it; the next session does not load it.
    let disabled = PluginInstall {
        state: PluginState::Disabled,
        ..record(&store)
    };
    store
        .save_plugin_install(disabled.clone())
        .expect("disabled");
    let mut installer = Installer::start(&store);

    let narrower = installer.review_update(
        offer("test", "0.2.0", STORAGE),
        offer("test", "0.1.0", STORAGE),
    );
    installer.update(narrower, &[]).expect("updated");
    let updated = record(&store);
    assert_eq!(updated.version, "0.2.0");
    assert_eq!(
        updated.state,
        PluginState::Disabled,
        "an update never enables a plugin the owner disabled"
    );
    assert!(
        installer.runtime.status("test").is_none(),
        "a disabled plugin is not started by its update"
    );
    // As at start-up, nothing is registered to answer its commands.
    assert!(
        !installer.has_command("test.echo"),
        "no command stays in the palette"
    );
    assert!(installer.plugins.get("test").is_none(), "not registered");
    close(installer.runtime, store);
}

#[test]
fn an_update_reviewed_against_an_older_record_is_refused() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut installer = Installer::start(&store);
    installer
        .install(offer("test", "0.1.0", STORAGE), &[CapabilityKind::Storage])
        .expect("installed");
    let base = || offer("test", "0.1.0", STORAGE);
    let late = installer.review_update(offer("test", "0.2.0", STORAGE), base());
    let early = installer.review_update(offer("test", "0.3.0", STORAGE), base());
    installer
        .update(early, &[])
        .expect("updated to version 0.3.0");

    // Applied now, the review against version 0.1.0 would downgrade version 0.3.0.
    assert_eq!(installer.update(late, &[]), Err(InstallRefusal::Stale));
    assert_eq!(record(&store).version, "0.3.0");

    // The same version with other files recorded is a changed record too.
    let base = || offer("test", "0.3.0", STORAGE);
    let reviewed = installer.review_update(offer("test", "0.4.0", STORAGE), base());
    let current = record(&store);
    let changed = PluginInstall {
        manifest_sha256: [9; 32],
        ..current.clone()
    };
    store.save_plugin_install(changed).expect("changed");
    assert_eq!(installer.update(reviewed, &[]), Err(InstallRefusal::Stale));
    store.save_plugin_install(current).expect("put back");

    // An update must be newer than the installed version.
    assert_eq!(
        refusal(&installer, offer("test", "0.3.0", STORAGE), Some(base())),
        InstallRefusal::NotNewer
    );
    assert_eq!(echo(&mut installer.runtime, "test"), Ok(Ok(())));
    close(installer.runtime, store);
}

#[test]
fn a_narrower_update_narrows_grants_and_folders_and_deletes_dropped_grants() {
    let temp = TempStore::new();
    let vaults: Vec<PathBuf> = ["first", "second"]
        .iter()
        .map(|name| temp.folder.join(name))
        .collect();
    for vault in &vaults {
        fs::create_dir(vault).expect("a synthetic folder");
    }
    let store = temp.open();
    let mut installer = Installer::start(&store);
    installer
        .install(
            offer("test", "0.1.0", &format!("{STORAGE}{TWO_FOLDERS}")),
            &[CapabilityKind::Storage, CapabilityKind::Folder],
        )
        .expect("installed");
    for vault in &vaults {
        installer
            .runtime
            .grant_folder("test", vault, FolderAccess::ReadWrite)
            .expect("granted");
    }

    let base = offer("test", "0.1.0", &format!("{STORAGE}{TWO_FOLDERS}"));
    let reviewed = installer.review_update(offer("test", "0.2.0", READ_ONE_FOLDER), base);
    let Plan::Update(changes) = reviewed.plan().clone() else {
        panic!("held: {:?}", reviewed.plan());
    };
    assert_eq!(changes.removed, [CapabilityKind::Storage]);
    installer.update(reviewed, &[]).expect("updated");

    let grants = store.plugin_grants().expect("read");
    assert_eq!(grants.len(), 1, "the storage grant went with its request");
    let folder = &grants[0].grant;
    assert_eq!(
        folder.scope_json, "{\"count\":1,\"watch\":false,\"writable\":false}",
        "the scope the new version asks for"
    );
    assert_eq!(folder.granted_at, 20, "narrowing is not a new grant");
    let folders = store.plugin_folders("test").expect("read");
    assert_eq!(folders.len(), 1, "only as many folders as it now asks for");
    assert_eq!(folders[0].access, FolderAccess::Read);
    // The new version runs, and the storage it no longer asks for is no longer granted.
    assert!(matches!(
        echo(&mut installer.runtime, "test"),
        Ok(Err(PluginError::Denied(_)))
    ));
    close(installer.runtime, store);
}

/// The old version, everything kept for it and its commands, as a failed update must leave it.
fn assert_old_version_back(installer: &mut Installer, store: &Store, before: &PluginInstall) {
    assert_eq!(&record(store), before);
    assert_eq!(grant_names(store), ["storage"]);
    let registered = installer.plugins.get("test").expect("registered again");
    assert_eq!(registered.manifest().version(), "0.1.0");
    assert!(installer.has_command("test.echo"));
    assert!(!installer.has_command("test.extra"));
    assert_eq!(echo(&mut installer.runtime, "test"), Ok(Ok(())));
}

/// Register synthetic commands until only `free` slots remain in the palette's registry.
fn fill_commands(commands: &mut CommandRegistry, free: usize) {
    let mut number = 0;
    while commands.len() + free < CommandRegistry::MAX_COMMANDS {
        let plugin = format!(
            "filler{}",
            number / CommandRegistry::MAX_COMMANDS_PER_PLUGIN
        );
        let id = format!("{plugin}.n{number}");
        let command = Command {
            id: CommandId::new(&id).expect("a synthetic id"),
            title: "Synthetic filler".into(),
            keywords: String::new(),
            scope: CommandScope::Plugin {
                plugin: plugin.clone(),
            },
            enablement: Enablement::Always,
            action: CommandAction::Plugin { plugin, action: id },
            shortcut: None,
        };
        commands.register(command).expect("fits");
        number += 1;
    }
}

#[test]
fn an_update_refused_at_registration_or_by_its_component_puts_the_old_version_back() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut installer = Installer::start(&store);
    installer
        .install(offer("test", "0.1.0", STORAGE), &[CapabilityKind::Storage])
        .expect("installed");
    let before = record(&store);

    // Registration: the new version declares one command more than the registry has room for.
    fill_commands(&mut installer.commands, 0);
    let bigger = installer.review_update(
        offer("test", "0.2.0", &format!("{EXTRA_COMMAND}{STORAGE}")),
        offer("test", "0.1.0", STORAGE),
    );
    assert!(matches!(
        installer.update(bigger, &[]),
        Err(InstallRefusal::Registration(_))
    ));
    assert_old_version_back(&mut installer, &store, &before);

    // The component: it imports a capability its manifest does not request.
    let refused = offer_of(
        "test",
        "0.2.0",
        STORAGE,
        PUBLISHER,
        variant("imports-documents"),
    );
    let reviewed = installer.review_update(refused, offer("test", "0.1.0", STORAGE));
    assert!(matches!(
        installer.update(reviewed, &[]),
        Err(InstallRefusal::Owner(_))
    ));
    assert_old_version_back(&mut installer, &store, &before);
    close(installer.runtime, store);
}

#[test]
fn a_plugin_whose_background_compile_is_refused_is_withdrawn() {
    let temp = TempStore::new();
    let store = temp.open();
    // Plugins compile in the background only with a compiled-code cache.
    let cache = temp.folder.join("PluginCache");
    fs::create_dir(&cache).expect("a synthetic cache folder");
    let setup = CacheSetup {
        folder: cache,
        sha256,
    };
    let mut installer = Installer::start_with(&store, Some(setup));
    let refused = offer_of("test", "0.1.0", "", PUBLISHER, variant("imports-documents"));
    installer.install(refused, &[]).expect("installed");
    assert!(
        installer.has_command("test.echo"),
        "listed before any compile"
    );

    let deadline = Instant::now() + Duration::from_secs(120);
    let withdrawn = loop {
        let events = installer.runtime.take_events();
        let Installer {
            runtime,
            plugins,
            commands,
        } = &mut installer;
        if let Some(plugin) = events
            .iter()
            .find_map(|event| runtime.withdraw_refused(event, plugins, commands))
        {
            break plugin;
        }
        assert!(
            Instant::now() < deadline,
            "the background compile never failed"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(withdrawn, "test");
    assert!(!installer.has_command("test.echo"));
    assert!(installer.plugins.get("test").is_none());
    // The record stays, so the owner can see why and update or uninstall it.
    assert_eq!(record(&store).plugin_id, "test");

    // A fixed update installs over the refused version, which is not running, and runs.
    let base = offer_of("test", "0.1.0", "", PUBLISHER, variant("imports-documents"));
    let fixed = installer
        .review_update(offer("test", "0.2.0", STORAGE), base)
        .accept();
    installer
        .update(fixed, &[CapabilityKind::Storage])
        .expect("updated");
    assert_eq!(record(&store).version, "0.2.0");
    assert!(installer.has_command("test.echo"));
    assert_eq!(echo(&mut installer.runtime, "test"), Ok(Ok(())));
    let unrelated = PluginEvent::Deactivated {
        plugin: "test".into(),
        reason: syrinx_plugin_host::worker::StopReason::Uninstalled,
    };
    let Installer {
        runtime,
        plugins,
        commands,
    } = &mut installer;
    assert_eq!(
        runtime.withdraw_refused(&unrelated, plugins, commands),
        None
    );
    close(installer.runtime, store);
}

#[test]
fn an_update_offered_by_another_publisher_or_for_another_plugin_is_refused() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut installer = Installer::start(&store);
    installer
        .install(offer("test", "0.1.0", ""), &[])
        .expect("installed");

    let stranger = offer_of("test", "0.2.0", "", "another-publisher", FULL);
    assert_eq!(
        refusal(&installer, stranger, Some(offer("test", "0.1.0", ""))),
        InstallRefusal::NotCheckedAgainstRecord
    );
    assert_eq!(
        refusal(
            &installer,
            offer("other", "0.2.0", ""),
            Some(offer("test", "0.1.0", ""))
        ),
        InstallRefusal::WrongPlugin {
            expected: "test".into(),
            offered: "other".into()
        }
    );
    close(installer.runtime, store);
}

#[test]
fn uninstalling_withdraws_the_plugin_and_leaves_no_record_grant_or_setting() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut installer = Installer::start(&store);
    installer
        .install(offer("idle", "0.1.0", ""), &[])
        .expect("installed");
    installer
        .install(offer("test", "0.1.0", STORAGE), &[CapabilityKind::Storage])
        .expect("installed");
    assert_eq!(echo(&mut installer.runtime, "test"), Ok(Ok(())));
    assert!(
        store
            .plugin_setting("test", "test.greeting")
            .expect("read")
            .is_some()
    );

    let Installer {
        runtime,
        plugins,
        commands,
    } = &mut installer;
    runtime
        .uninstall("test", plugins, commands)
        .expect("uninstalled");
    assert!(!installer.has_command("test.echo"));
    assert!(installer.has_command("idle.echo"), "the other plugin stays");
    assert!(installer.plugins.get("test").is_none());
    let installs = store.plugin_installs().expect("read");
    assert_eq!(installs.len(), 1);
    assert_eq!(installs[0].plugin_id, "idle");
    assert_eq!(store.plugin_grants(), Ok(Vec::new()));
    assert_eq!(store.plugin_setting("test", "test.greeting"), Ok(None));

    // The id is free again.
    installer
        .install(offer("test", "0.1.0", ""), &[])
        .expect("installed again");
    close(installer.runtime, store);
}

#[test]
fn an_installed_plugin_loads_at_start_only_as_its_record_names_it() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut installer = Installer::start(&store);
    installer
        .install(offer("test", "0.1.0", STORAGE), &[CapabilityKind::Storage])
        .expect("installed");
    installer.runtime.shutdown();

    // At the next start, other bytes than the recorded ones are refused.
    let mut restarted = Installer::start(&store);
    let changed = offer_of(
        "test",
        "0.1.0",
        STORAGE,
        PUBLISHER,
        variant("version-0-2-0"),
    );
    let Installer {
        runtime,
        plugins,
        commands,
    } = &mut restarted;
    assert_eq!(
        runtime.load_installed(changed, plugins, commands),
        Err(InstallRefusal::NotRecorded)
    );
    assert!(plugins.is_empty());
    runtime
        .load_installed(offer("test", "0.1.0", STORAGE), plugins, commands)
        .expect("loaded as recorded");
    assert_eq!(echo(&mut restarted.runtime, "test"), Ok(Ok(())));
    close(restarted.runtime, store);
}
