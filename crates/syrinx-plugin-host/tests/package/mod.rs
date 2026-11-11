//! The test component as a package (#134): a manifest whose signed lines match it registers
//! without any component, and the component's interface names are checked when it loads.

use crate::support::{self, FULL, Fixture, variant};
use syrinx_core::{CommandError, CommandRegistry};
use syrinx_plugin_host::manifest::{
    CapabilityKind, DiscoveryOptions, ManifestError, PluginManifest,
};
use syrinx_plugin_host::test_host::{Declarations, TestHost};
use syrinx_plugin_host::{
    HostError, IdentityMismatch, PluginRegistry, RegisteredPlugin, RegistrationError, Runtime,
    SignedIdentity, check_signed,
};

const WORLD: &str = "syrinx:plugin/plugin@0.1.0";

/// The tables every synthetic manifest shares, for `plugin`.
fn header(plugin: &str) -> String {
    format!(
        r#"schema = 1
[plugin]
id = "{plugin}"
name = "Synthetic test plugin"
version = "0.1.0"
description = "The synthetic fixture every contract test loads."
authors = ["Example Author <author@example.invalid>"]
repository = "https://example.invalid/test-plugin"
licence = "MPL-2.0"
min-syrinx = "0.1.0"
[interface]
world = "{WORLD}"
[entry]
module = "plugin.wasm"
ui = "ui/"
activation = ["on-command", "on-view", "on-search"]
"#
    )
}

/// A manifest for `plugin` declaring what the full fixture implements.
fn manifest_text(plugin: &str) -> String {
    let declarations = support::full_declarations();
    let mut text = header(plugin);
    for command in &declarations.commands {
        let id = command.id.replacen("test.", &format!("{plugin}."), 1);
        text.push_str(&format!(
            "[[contributes.commands]]\nid = \"{id}\"\ntitle = \"Synthetic {id}\"\nform = {}\n",
            command.form
        ));
    }
    text.push_str(&format!(
        "[[contributes.tab-types]]\nid = \"{plugin}.page\"\ntitle = \"Synthetic page\"\n\
         [[contributes.search-feeds]]\nid = \"{plugin}.items\"\ntitle = \"Items\"\ntiers = [\"catalogue\"]\n"
    ));
    text
}

fn manifest(plugin: &str) -> PluginManifest {
    PluginManifest::parse(
        manifest_text(plugin).as_bytes(),
        DiscoveryOptions::default(),
    )
    .expect("the fixture's manifest is valid")
}

/// A plugin declaring `count` plain commands and nothing else.
fn filler(plugin: &str, count: usize) -> PluginManifest {
    let mut text = header(plugin);
    for index in 0..count {
        text.push_str(&format!(
            "[[contributes.commands]]\nid = \"{plugin}.action-{index}\"\ntitle = \"Action {index}\"\n"
        ));
    }
    PluginManifest::parse(text.as_bytes(), DiscoveryOptions::default()).expect("a filler manifest")
}

fn signed(id: &str, version: &str) -> SignedIdentity {
    SignedIdentity::from_signed_lines(id, version, WORLD)
}

/// Register `manifest` with signed lines that match it, reading no component at all.
fn register<'a>(
    plugins: &'a mut PluginRegistry,
    manifest: PluginManifest,
    commands: &mut CommandRegistry,
) -> Result<&'a RegisteredPlugin, RegistrationError> {
    let lines = signed(manifest.id(), manifest.version());
    let signed_manifest = check_signed(&lines, manifest).expect("the lines match the manifest");
    plugins.register(signed_manifest, commands)
}

/// Compile `fixture` as the registered `test` plugin's component.
fn compile(fixture: Fixture) -> Result<(), HostError> {
    let mut commands = CommandRegistry::browser();
    let mut plugins = PluginRegistry::new();
    let registered = register(&mut plugins, manifest("test"), &mut commands).expect("registers");
    Runtime::new()
        .expect("engine")
        .compile_package(registered, &support::bytes(fixture))
        .map(drop)
}

#[test]
fn the_test_component_registers_then_loads_and_runs_from_its_manifest() {
    let manifest = manifest("test");
    let mut commands = CommandRegistry::browser();
    let before = commands.len();
    let mut plugins = PluginRegistry::new();
    let registered =
        register(&mut plugins, manifest.clone(), &mut commands).expect("the fixture registers");
    assert_eq!(registered.command_keys().len(), manifest.commands().len());
    assert_eq!(commands.len(), before + manifest.commands().len());
    assert!(
        commands
            .find("test.echo")
            .is_some_and(|(_, command)| command.is_plugin_action())
    );

    // Loading compiles the component against the registered manifest.
    let bytes = support::bytes(FULL);
    Runtime::new()
        .expect("engine")
        .compile_package(registered, &bytes)
        .expect("the component agrees with its manifest");
    let mut host = TestHost::load(&bytes, Declarations::from(&manifest)).expect("loads");
    assert_eq!(host.run_command("test.echo").expect("call"), Ok(()));
}

#[test]
fn registration_reads_only_the_signed_manifest_and_needs_no_component() {
    // No component bytes exist anywhere in this test: the palette fills from the manifest.
    let mut commands = CommandRegistry::browser();
    let before = commands.len();
    let mut plugins = PluginRegistry::new();
    let registered =
        register(&mut plugins, manifest("idle"), &mut commands).expect("registers alone");
    assert!(!registered.command_keys().is_empty());
    assert_eq!(commands.len(), before + registered.command_keys().len());
    assert_eq!(registered.tab_types()[0].id, "idle.page");
    assert_eq!(registered.search_feeds()[0].id, "idle.items");
}

#[test]
fn a_package_signed_for_another_plugin_registers_nothing() {
    // A package signed for slot `evil` whose manifest claims to be `test`.
    let mismatch = check_signed(&signed("evil", "0.1.0"), manifest("test"));
    // Nothing is registered: `register` accepts only a `SignedManifest`, which a mismatch never yields.
    assert_eq!(
        mismatch.err(),
        Some(IdentityMismatch::Id {
            signed: "evil".into(),
            manifest: "test".into(),
        })
    );
}

#[test]
fn a_duplicate_or_reserved_plugin_id_is_refused_and_registers_nothing() {
    let mut commands = CommandRegistry::browser();
    let mut plugins = PluginRegistry::new();
    register(&mut plugins, manifest("test"), &mut commands).expect("first copy");
    let after_first = commands.len();
    assert_eq!(
        register(&mut plugins, manifest("test"), &mut commands).err(),
        Some(RegistrationError::DuplicatePlugin("test".into()))
    );
    assert_eq!(
        register(&mut plugins, manifest("browser"), &mut commands).err(),
        Some(RegistrationError::ReservedId("browser".into()))
    );
    assert_eq!(commands.len(), after_first);
    assert_eq!(plugins.len(), 1);
}

#[test]
fn a_duplicate_or_foreign_command_never_reaches_the_palette() {
    // A command id twice, and one owned by another plugin, fail at discovery.
    let duplicated = format!(
        "{}[[contributes.commands]]\nid = \"test.echo\"\ntitle = \"Again\"\n",
        manifest_text("test")
    );
    assert_eq!(
        PluginManifest::parse(duplicated.as_bytes(), DiscoveryOptions::default()),
        Err(ManifestError::DuplicateContribution {
            id: "test.echo".into()
        })
    );
    let foreign = manifest_text("test").replacen("id = \"test.echo\"", "id = \"notes.echo\"", 1);
    assert_eq!(
        PluginManifest::parse(foreign.as_bytes(), DiscoveryOptions::default()),
        Err(ManifestError::MissingPrefix {
            id: "notes.echo".into()
        })
    );
}

#[test]
fn a_full_palette_refuses_the_whole_plugin_and_names_the_command() {
    // Fill the registry to its cap with other plugins, each within its own quota.
    let mut commands = CommandRegistry::browser();
    let mut plugins = PluginRegistry::new();
    let mut index = 0;
    while commands.len() < CommandRegistry::MAX_COMMANDS {
        let room = CommandRegistry::MAX_COMMANDS - commands.len();
        let count = room.min(CommandRegistry::MAX_COMMANDS_PER_PLUGIN);
        register(
            &mut plugins,
            filler(&format!("filler{index}"), count),
            &mut commands,
        )
        .expect("each filler fits");
        index += 1;
    }
    let before = plugins.len();
    let error =
        register(&mut plugins, manifest("late"), &mut commands).expect_err("the registry is full");
    assert!(
        matches!(
            &error,
            RegistrationError::Command {
                error: CommandError::TooManyCommands,
                ..
            }
        ),
        "{error}"
    );
    assert_eq!(plugins.len(), before);
    assert!(plugins.get("late").is_none());
    assert!(commands.find("late.echo").is_none());
}

#[test]
fn a_view_request_for_an_undeclared_tab_type_is_refused() {
    let mut commands = CommandRegistry::browser();
    let mut plugins = PluginRegistry::new();
    register(&mut plugins, manifest("test"), &mut commands).expect("registers");
    assert!(plugins.tab_type("test", "test.page").is_ok());
    assert_eq!(
        plugins.tab_type("test", "test.missing").err(),
        Some(RegistrationError::UnknownTabType {
            plugin: "test".into(),
            tab_type: "test.missing".into()
        })
    );
    assert_eq!(
        plugins.tab_type("absent", "absent.page").err(),
        Some(RegistrationError::UnknownPlugin("absent".into()))
    );
}

#[test]
fn a_manifest_whose_world_disagrees_with_the_component_is_refused_by_name() {
    let error = compile(variant("version-0-2-0")).expect_err("a 0.2.0 component under 0.1.0");
    let HostError::Identity(IdentityMismatch::ComponentWorld {
        manifest,
        component,
    }) = error
    else {
        panic!("not a world mismatch: {error}");
    };
    assert_eq!(manifest, WORLD);
    assert!(
        component.iter().any(|name| name.ends_with("@0.2.0")),
        "{component:?}"
    );
}

#[test]
fn a_component_importing_an_unrequested_capability_is_refused_at_load() {
    let error = compile(variant("imports-documents")).expect_err("the manifest requests no folder");
    let HostError::Identity(IdentityMismatch::UnrequestedCapability { interface, kind }) = error
    else {
        panic!("not an unrequested capability: {error}");
    };
    assert_eq!(interface, "syrinx:plugin/documents@0.1.0");
    assert_eq!(kind, CapabilityKind::Folder);
}

#[test]
fn signed_lines_that_disagree_with_the_manifest_are_named() {
    for (identity, expected) in [
        (signed("notes", "0.1.0"), "signed as notes"),
        (signed("test", "0.0.9"), "version 0.0.9"),
    ] {
        let error = check_signed(&identity, manifest("test")).expect_err("mismatch");
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn a_precompiled_module_is_refused_at_load() {
    let mut commands = CommandRegistry::browser();
    let mut plugins = PluginRegistry::new();
    let registered = register(&mut plugins, manifest("test"), &mut commands).expect("registers");
    let runtime = Runtime::new().expect("engine");
    let precompiled = runtime
        .engine()
        .precompile_component(&support::bytes(FULL))
        .expect("precompiles");
    assert_eq!(
        runtime.compile_package(registered, &precompiled).err(),
        Some(HostError::Precompiled)
    );
}
