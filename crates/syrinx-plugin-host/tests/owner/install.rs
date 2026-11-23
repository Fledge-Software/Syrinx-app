//! Installing, updating and uninstalling the test plugin from offers (#140): which ids a new
//! plugin may take, an update that asks for more held while the old version runs, one that
//! asks for less narrowing its grants, and an uninstall that leaves nothing behind.

use std::sync::Arc;

use syrinx_core::CommandRegistry;
use syrinx_plugin_host::bindings::syrinx::plugin::types::PluginError;
use syrinx_plugin_host::grants::DenyList;
use syrinx_plugin_host::manifest::CapabilityKind;
use syrinx_plugin_host::owner::{InstallRefusal, Offer, OwnerRuntime, Plan};
use syrinx_plugin_host::{PluginRegistry, SignedIdentity};
use syrinx_store::{PluginState, Store};

use super::{FOLDER, READ_ONE_FOLDER, REQUIRED_STORAGE, STORAGE, TempStore, close, echo};
use crate::package::manifest_text;
use crate::support::{self, FULL};

const WORLD: &str = "syrinx:plugin/plugin@0.1.0";
const PUBLISHER: &str = "synthetic-publisher";

/// The runtime, the registry and the palette's commands, as the application holds them.
struct Installer {
    runtime: OwnerRuntime,
    plugins: PluginRegistry,
    commands: CommandRegistry,
}

impl Installer {
    fn start(store: &Arc<Store>) -> Self {
        let runtime = OwnerRuntime::start(Arc::clone(store), "0.1.0-owner", DenyList::new(), None)
            .expect("starts");
        Self {
            runtime,
            plugins: PluginRegistry::new(),
            commands: CommandRegistry::browser(),
        }
    }

    fn install(&mut self, offer: Offer, granted: &[CapabilityKind]) -> Result<(), InstallRefusal> {
        let reviewed = self.runtime.review(offer, None)?;
        self.runtime
            .install_reviewed(reviewed, granted, &mut self.plugins, &mut self.commands, 20)
    }

    fn has_command(&self, id: &str) -> bool {
        self.commands.find(id).is_some()
    }
}

/// `plugin` at `version`, asking for `capabilities`, from `publisher`; `seed` marks its hashes.
fn offer_from(plugin: &str, version: &str, capabilities: &str, publisher: &str, seed: u8) -> Offer {
    let text = manifest_text(plugin).replace(
        "\nversion = \"0.1.0\"",
        &format!("\nversion = \"{version}\""),
    ) + capabilities;
    Offer {
        signed: SignedIdentity::from_signed_lines(plugin, version, WORLD),
        manifest: text.into_bytes(),
        manifest_sha256: [seed; 32],
        component: Arc::from(support::bytes(FULL)),
        module_sha256: [seed.wrapping_add(1); 32],
        publisher: publisher.to_owned(),
        unsigned: false,
    }
}

fn offer(plugin: &str, version: &str, capabilities: &str) -> Offer {
    offer_from(plugin, version, capabilities, PUBLISHER, 1)
}

fn refusal(installer: &Installer, offer: Offer, updating: Option<&str>) -> InstallRefusal {
    match installer.runtime.review(offer, updating) {
        Ok(_) => panic!("the offer was accepted"),
        Err(refusal) => refusal,
    }
}

#[test]
fn a_new_plugin_installs_with_what_the_owner_granted_and_records_its_hashes() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut installer = Installer::start(&store);
    installer
        .install(offer("test", "0.1.0", STORAGE), &[CapabilityKind::Storage])
        .expect("installed");

    assert!(installer.has_command("test.echo"));
    assert_eq!(echo(&mut installer.runtime, "test"), Ok(Ok(())));
    let record = &store.plugin_installs().expect("read")[0];
    assert_eq!(record.version, "0.1.0");
    assert_eq!(
        record.module_sha256, [2; 32],
        "the hash of the bytes that load"
    );
    assert_eq!(record.manifest_sha256, [1; 32]);
    assert_eq!(record.signer_key, PUBLISHER);
    assert_eq!(record.state, PluginState::Enabled);
    let grants = store.plugin_grants().expect("read");
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].grant.capability, "storage");
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
fn full_trust_a_newer_syrinx_and_a_declined_required_capability_install_nothing() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut installer = Installer::start(&store);
    let terminal = "[[capabilities]]\nkind = \"terminal\"\ntrust = \"full\"\nreason = \"Runs synthetic builds.\"\nrequired = false\n";
    assert_eq!(
        refusal(&installer, offer("test", "0.1.0", terminal), None),
        InstallRefusal::FullTrust(CapabilityKind::Terminal)
    );

    let mut future = offer("test", "0.1.0", "");
    future.manifest = String::from_utf8(future.manifest)
        .expect("text")
        .replace("min-syrinx = \"0.1.0\"", "min-syrinx = \"9.0.0\"")
        .into_bytes();
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
fn a_widened_update_is_held_while_the_old_version_runs_until_accepted() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut installer = Installer::start(&store);
    installer
        .install(offer("test", "0.1.0", STORAGE), &[CapabilityKind::Storage])
        .expect("installed");

    let wider = offer_from("test", "0.2.0", &format!("{STORAGE}{FOLDER}"), PUBLISHER, 7);
    let held = installer
        .runtime
        .review(wider, Some("test"))
        .expect("reviewed");
    let Plan::Held(changes) = held.plan().clone() else {
        panic!("not held: {:?}", held.plan());
    };
    assert_eq!(changes.added, [CapabilityKind::Folder]);

    // Not accepted yet: nothing changes and the old version keeps running.
    let Installer {
        runtime,
        plugins,
        commands,
    } = &mut installer;
    let attempt = runtime.update_reviewed(held, &[], plugins, commands, 30);
    assert_eq!(attempt, Err(InstallRefusal::NeedsConsent));
    let record = &store.plugin_installs().expect("read")[0];
    assert_eq!(record.version, "0.1.0");
    assert_eq!(
        record.module_sha256, [2; 32],
        "no hash recorded for a held update"
    );
    assert_eq!(echo(&mut installer.runtime, "test"), Ok(Ok(())));

    // Accepted, with the new folder capability granted: the new version replaces it.
    let wider = offer_from("test", "0.2.0", &format!("{STORAGE}{FOLDER}"), PUBLISHER, 7);
    let accepted = installer
        .runtime
        .review(wider, Some("test"))
        .expect("reviewed")
        .accept();
    let Installer {
        runtime,
        plugins,
        commands,
    } = &mut installer;
    runtime
        .update_reviewed(accepted, &[CapabilityKind::Folder], plugins, commands, 30)
        .expect("updated");
    let record = &store.plugin_installs().expect("read")[0];
    assert_eq!(
        (record.version.as_str(), record.module_sha256),
        ("0.2.0", [8; 32])
    );
    assert_eq!(record.installed_at, 20, "the first install's time is kept");
    let kinds: Vec<String> = store
        .plugin_grants()
        .expect("read")
        .into_iter()
        .map(|stored| stored.grant.capability)
        .collect();
    assert_eq!(kinds, ["folder", "storage"]);
    assert_eq!(echo(&mut installer.runtime, "test"), Ok(Ok(())));
    close(installer.runtime, store);
}

#[test]
fn a_narrower_update_replaces_at_once_narrows_grants_and_deletes_dropped_ones() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut installer = Installer::start(&store);
    installer
        .install(
            offer("test", "0.1.0", &format!("{STORAGE}{FOLDER}")),
            &[CapabilityKind::Storage, CapabilityKind::Folder],
        )
        .expect("installed");

    let narrower = offer_from("test", "0.2.0", READ_ONE_FOLDER, PUBLISHER, 7);
    let reviewed = installer
        .runtime
        .review(narrower, Some("test"))
        .expect("reviewed");
    let Plan::Update(changes) = reviewed.plan().clone() else {
        panic!("held: {:?}", reviewed.plan());
    };
    assert_eq!(changes.removed, [CapabilityKind::Storage]);
    let Installer {
        runtime,
        plugins,
        commands,
    } = &mut installer;
    runtime
        .update_reviewed(reviewed, &[], plugins, commands, 30)
        .expect("updated");

    let grants = store.plugin_grants().expect("read");
    assert_eq!(grants.len(), 1, "the storage grant went with its request");
    let folder = &grants[0].grant;
    assert_eq!(folder.capability, "folder");
    assert_eq!(
        folder.scope_json, "{\"count\":1,\"watch\":false,\"writable\":false}",
        "the scope the new version asks for"
    );
    assert_eq!(folder.granted_for_manifest, [7; 32]);
    assert_eq!(folder.granted_at, 20, "narrowing is not a new grant");
    // The new version runs, and the storage it no longer asks for is no longer granted.
    assert!(matches!(
        echo(&mut installer.runtime, "test"),
        Ok(Err(PluginError::Denied(_)))
    ));
    close(installer.runtime, store);
}

#[test]
fn an_update_from_another_publisher_or_for_another_plugin_is_refused() {
    let temp = TempStore::new();
    let store = temp.open();
    let mut installer = Installer::start(&store);
    installer
        .install(offer("test", "0.1.0", ""), &[])
        .expect("installed");

    let stranger = offer_from("test", "0.2.0", "", "another-publisher", 7);
    assert_eq!(
        refusal(&installer, stranger, Some("test")),
        InstallRefusal::DifferentPublisher
    );
    assert_eq!(
        refusal(&installer, offer("other", "0.2.0", ""), Some("test")),
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
