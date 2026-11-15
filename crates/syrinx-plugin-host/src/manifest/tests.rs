//! Every refusal `PluginManifest::parse` names, on synthetic manifests.

use super::*;

/// Note 10's Notes draft with synthetic values: every table and kind of contribution.
const VALID: &str = r#"
schema = 1

[plugin]
id = "notes"
name = "Notes"
version = "0.3.0"
description = "Markdown notes in a synthetic vault."
authors = ["Example Author <author@example.invalid>"]
repository = "https://example.invalid/notes-plugin"
licence = "MPL-2.0"
min-syrinx = "0.9.0"

[interface]
world = "syrinx:plugin/plugin@0.1.0"

[entry]
module = "plugin.wasm"
ui = "ui/"
activation = ["on-command", "on-view", "on-search"]

[[contributes.commands]]
id = "notes.new-note"
title = "New note"
keywords = ["create"]
form = true

[[contributes.tab-types]]
id = "notes.note"
title = "Note"
ui-methods = ["open-note", "rename"]

[[contributes.search-feeds]]
id = "notes.notes"
title = "Notes"
tiers = ["catalogue", "full-text"]

[[capabilities]]
kind = "storage"
reason = "Remember your vault choice and editor settings."
required = true

[[capabilities]]
kind = "folder"
access = "read-write"
count = 1
watch = true
reason = "Read and save the Markdown notes in the vault folder you choose."
required = true

[[capabilities]]
kind = "index"
feeds = ["notes.notes"]
reason = "Let the command palette find notes by title and text."
required = true
"#;

fn parse(text: &str) -> Result<PluginManifest, ManifestError> {
    PluginManifest::parse(text.as_bytes(), DiscoveryOptions::default())
}

fn developer(text: &str) -> Result<PluginManifest, ManifestError> {
    PluginManifest::parse(
        text.as_bytes(),
        DiscoveryOptions {
            developer_mode: true,
        },
    )
}

/// The valid manifest with one exact piece of text replaced.
fn changed(from: &str, to: &str) -> String {
    assert!(VALID.contains(from), "{from:?} is not in the manifest");
    VALID.replacen(from, to, 1)
}

/// The valid manifest with a TOML fragment appended.
fn with(extra: &str) -> String {
    format!("{VALID}\n{extra}\n")
}

#[test]
fn the_draft_manifest_parses_with_every_contribution() {
    let manifest = parse(VALID).expect("the synthetic manifest is valid");
    assert_eq!(manifest.id(), "notes");
    assert_eq!(manifest.version(), "0.3.0");
    assert_eq!(manifest.world(), "syrinx:plugin/plugin@0.1.0");
    assert_eq!(manifest.entry().module, "plugin.wasm");
    assert_eq!(manifest.commands()[0].id.as_str(), "notes.new-note");
    assert!(manifest.commands()[0].form);
    assert_eq!(manifest.tab_types()[0].ui_methods, ["open-note", "rename"]);
    assert_eq!(manifest.search_feeds()[0].tiers, ["catalogue", "full-text"]);
    assert!(manifest.requests(CapabilityKind::Folder));
    assert!(!manifest.requests(CapabilityKind::Terminal));
    assert_eq!(
        manifest.capabilities()[1].scope,
        Scope::Folder {
            writable: true,
            count: FolderCount::Limited(1),
            watch: true
        }
    );
}

#[test]
fn an_unsupported_schema_is_refused_before_anything_else() {
    // The rest of this manifest is nonsense; the schema is still what is named.
    let error = parse("schema = 2\n[plugin]\nid = 7\n").expect_err("schema 2");
    assert_eq!(error, ManifestError::UnsupportedSchema("2".into()));
    let missing = parse("[plugin]\nid = \"notes\"\n").expect_err("no schema");
    assert_eq!(
        missing,
        ManifestError::UnsupportedSchema("(missing)".into())
    );
}

#[test]
fn an_unsupported_world_is_refused_before_the_other_fields() {
    let text = changed("syrinx:plugin/plugin@0.1.0", "syrinx:plugin/plugin@0.2.0").replacen(
        "id = \"notes\"",
        "id = \"Not An Id\"",
        1,
    );
    assert_eq!(
        parse(&text),
        Err(ManifestError::UnsupportedWorld(
            "syrinx:plugin/plugin@0.2.0".into()
        ))
    );
}

#[test]
fn a_misspelt_or_unknown_key_is_refused_not_ignored() {
    let text = changed("min-syrinx", "minimum-syrinx");
    assert!(matches!(parse(&text), Err(ManifestError::Syntax(_))));
    let extra = changed("[entry]", "[entry]\nautostart = true");
    assert!(matches!(parse(&extra), Err(ManifestError::Syntax(_))));
}

#[test]
fn bad_plugin_ids_and_versions_are_named() {
    for id in [
        "Notes",
        "notes.extra",
        "",
        "a-plugin-id-that-is-far-too-long-for-syrinx",
    ] {
        let text = changed("id = \"notes\"", &format!("id = {id:?}"));
        let error = parse(&text).expect_err(id);
        assert!(
            matches!(&error, ManifestError::InvalidField { field, .. } if field == "plugin.id"),
            "{id}: {error}"
        );
    }
    for version in ["0.3", "0.03.0", "0.3.0-beta", "v0.3.0"] {
        let text = changed("version = \"0.3.0\"", &format!("version = {version:?}"));
        let error = parse(&text).expect_err(version);
        assert!(
            matches!(&error, ManifestError::InvalidField { field, .. } if field == "plugin.version"),
            "{version}: {error}"
        );
    }
}

#[test]
fn a_contributed_id_without_the_plugin_prefix_is_refused() {
    for (from, to) in [
        ("id = \"notes.new-note\"", "id = \"forge.new-note\""),
        ("id = \"notes.note\"", "id = \"browser.note\""),
        ("id = \"notes.notes\"", "id = \"notesx.notes\""),
    ] {
        let error = parse(&changed(from, to)).expect_err(to);
        assert!(
            matches!(error, ManifestError::MissingPrefix { .. }),
            "{error}"
        );
    }
}

#[test]
fn a_duplicate_command_or_contribution_is_refused() {
    let command = with("[[contributes.commands]]\nid = \"notes.new-note\"\ntitle = \"Again\"");
    assert_eq!(
        parse(&command),
        Err(ManifestError::DuplicateContribution {
            id: "notes.new-note".into()
        })
    );
    // A tab type may not reuse a command's id either.
    let tab = with("[[contributes.tab-types]]\nid = \"notes.new-note\"\ntitle = \"Clash\"");
    assert!(matches!(
        parse(&tab),
        Err(ManifestError::DuplicateContribution { .. })
    ));
}

#[test]
fn more_commands_than_the_quota_are_refused_whole() {
    let limit = CommandRegistry::MAX_COMMANDS_PER_PLUGIN;
    let extra: String = (0..limit)
        .map(|index| {
            format!("[[contributes.commands]]\nid = \"notes.extra-{index}\"\ntitle = \"Extra\"\n")
        })
        .collect();
    assert_eq!(
        parse(&with(&extra)),
        Err(ManifestError::TooManyCommands {
            declared: limit + 1,
            limit
        })
    );
}

#[test]
fn an_unknown_capability_kind_is_refused() {
    let text = with("[[capabilities]]\nkind = \"network\"\nreason = \"Talk to a synthetic host.\"");
    assert_eq!(
        parse(&text),
        Err(ManifestError::UnknownCapability("network".into()))
    );
}

#[test]
fn a_capability_without_a_reason_is_refused() {
    let missing = with("[[capabilities]]\nkind = \"drafts\"");
    assert_eq!(
        parse(&missing),
        Err(ManifestError::MissingReason(CapabilityKind::Drafts))
    );
    let blank = with("[[capabilities]]\nkind = \"drafts\"\nreason = \"   \"");
    assert_eq!(
        parse(&blank),
        Err(ManifestError::MissingReason(CapabilityKind::Drafts))
    );
}

#[test]
fn a_full_trust_kind_must_say_so_and_cannot_be_downgraded() {
    let unmarked =
        with("[[capabilities]]\nkind = \"terminal\"\nreason = \"Run synthetic builds.\"");
    assert_eq!(
        parse(&unmarked),
        Err(ManifestError::FullTrustNotDeclared(
            CapabilityKind::Terminal
        ))
    );
    let downgraded = with(
        "[[capabilities]]\nkind = \"git\"\ntrust = \"sandboxed\"\nreason = \"Show a synthetic log.\"",
    );
    assert_eq!(
        parse(&downgraded),
        Err(ManifestError::FullTrustNotDeclared(CapabilityKind::Git))
    );
    let declared = with(
        "[[capabilities]]\nkind = \"terminal\"\ntrust = \"full\"\nreason = \"Run synthetic builds.\"",
    );
    assert!(parse(&declared).is_ok());
    let inflated = with(
        "[[capabilities]]\nkind = \"drafts\"\ntrust = \"full\"\nreason = \"Keep synthetic drafts.\"",
    );
    assert_eq!(
        parse(&inflated),
        Err(ManifestError::TrustMismatch(CapabilityKind::Drafts))
    );
}

#[test]
fn scope_fields_follow_their_kind() {
    let duplicate = with("[[capabilities]]\nkind = \"storage\"\nreason = \"Again.\"");
    assert_eq!(
        parse(&duplicate),
        Err(ManifestError::DuplicateCapability(CapabilityKind::Storage))
    );
    for (from, to) in [
        ("access = \"read-write\"", "access = \"everything\""),
        ("count = 1", "count = 0"),
        ("count = 1", "count = 65"),
        ("count = 1", "count = -1"),
        ("count = 1", "count = 4294967297"),
        ("feeds = [\"notes.notes\"]", "feeds = [\"notes.other\"]"),
        ("kind = \"storage\"", "kind = \"storage\"\nwatch = true"),
    ] {
        let error = parse(&changed(from, to)).expect_err(to);
        assert!(
            matches!(error, ManifestError::InvalidScope { .. }),
            "{to}: {error}"
        );
    }
    let many = changed("count = 1", "count = \"many\"");
    let manifest = parse(&many).expect("count = \"many\" is allowed");
    assert!(matches!(
        manifest.capabilities()[1].scope,
        Scope::Folder {
            count: FolderCount::Many,
            ..
        }
    ));
}

#[test]
fn an_over_long_capability_reason_is_an_invalid_field() {
    let long = "x".repeat(limits::MAX_MESSAGE_CHARS + 1);
    let text = with(&format!(
        "[[capabilities]]\nkind = \"drafts\"\nreason = \"{long}\""
    ));
    let Err(ManifestError::InvalidField { field, .. }) = parse(&text) else {
        panic!("an over-long reason was not refused as a field");
    };
    assert_eq!(field, "capabilities.reason");
}

#[test]
fn the_entry_names_a_plain_component_and_a_relative_ui_folder() {
    for module in [
        "plugin.cwasm",
        "lib/plugin.wasm",
        "../plugin.wasm",
        "plugin",
    ] {
        let text = changed("module = \"plugin.wasm\"", &format!("module = {module:?}"));
        let error = parse(&text).expect_err(module);
        assert!(
            matches!(&error, ManifestError::InvalidField { field, .. } if field == "entry.module"),
            "{module}: {error}"
        );
    }
    for ui in ["ui", "/ui/", "../ui/", "ui/../x/"] {
        let text = changed("ui = \"ui/\"", &format!("ui = {ui:?}"));
        assert!(parse(&text).is_err(), "{ui}");
    }
    let unknown = changed("\"on-search\"]", "\"on-start\"]");
    assert!(parse(&unknown).is_err());
}

#[test]
fn widgets_need_a_developer_mode_plugin_with_home_widgets() {
    let widget = "[[contributes.widgets]]\nid = \"notes.recent\"\ntitle = \"Recent notes\"";
    assert_eq!(parse(&with(widget)), Err(ManifestError::WidgetsNotAllowed));
    assert_eq!(
        developer(&with(widget)),
        Err(ManifestError::WidgetsNotAllowed)
    );

    let gated = changed(
        "world = \"syrinx:plugin/plugin@0.1.0\"",
        "world = \"syrinx:plugin/plugin@0.1.0\"\nunstable = [\"home-widgets\"]",
    );
    // Outside developer mode even the flag alone is refused.
    assert!(matches!(
        parse(&gated),
        Err(ManifestError::InvalidField { .. })
    ));
    let manifest = developer(&format!("{gated}\n{widget}\n")).expect("a developer widget");
    assert_eq!(manifest.widgets()[0].id, "notes.recent");
}

#[test]
fn an_oversized_or_non_utf8_manifest_is_refused() {
    let large = vec![b'#'; MAX_MANIFEST_BYTES + 1];
    assert_eq!(
        PluginManifest::parse(&large, DiscoveryOptions::default()),
        Err(ManifestError::TooLarge)
    );
    assert!(matches!(
        PluginManifest::parse(&[0xff, 0xfe], DiscoveryOptions::default()),
        Err(ManifestError::Syntax(_))
    ));
}

#[test]
fn a_version_part_a_package_cannot_sign_is_refused() {
    // `docs/update-format.md` allows each part 0 to 4,294,967,295, the most a `package` holds.
    let largest = changed("version = \"0.3.0\"", "version = \"0.3.4294967295\"");
    assert!(parse(&largest).is_ok());
    for (field, from) in [("version", "0.3.0"), ("min-syrinx", "0.9.0")] {
        let text = changed(
            &format!("{field} = {from:?}"),
            &format!("{field} = \"0.3.4294967296\""),
        );
        let error = parse(&text).expect_err(field);
        let expected = format!("plugin.{field}");
        assert!(
            matches!(&error, ManifestError::InvalidField { field: refused, .. } if *refused == expected),
            "{field}: {error}"
        );
    }
}

#[test]
fn a_command_title_the_palette_would_refuse_is_refused_here() {
    // The palette's registry refuses a longer title, so the manifest must not accept one.
    let limit = CommandRegistry::MAX_TITLE_CHARS;
    let longest = changed(
        "title = \"New note\"",
        &format!("title = {:?}", "n".repeat(limit)),
    );
    assert!(parse(&longest).is_ok());
    let over = changed(
        "title = \"New note\"",
        &format!("title = {:?}", "n".repeat(limit + 1)),
    );
    let error = parse(&over).expect_err("an over-long command title");
    assert!(
        matches!(&error, ManifestError::InvalidField { field, .. } if field == "notes.new-note"),
        "{error}"
    );
}

#[test]
fn an_unstable_feature_listed_twice_is_refused() {
    let twice = changed(
        "world = \"syrinx:plugin/plugin@0.1.0\"",
        "world = \"syrinx:plugin/plugin@0.1.0\"\nunstable = [\"home-widgets\", \"home-widgets\"]",
    );
    let error = developer(&twice).expect_err("a repeated unstable feature");
    assert!(
        matches!(&error, ManifestError::InvalidField { field, reason }
            if field == "interface.unstable" && reason.contains("twice")),
        "{error}"
    );
}

#[test]
fn an_index_feed_listed_twice_is_a_scope_error() {
    let twice = changed(
        "feeds = [\"notes.notes\"]",
        "feeds = [\"notes.notes\", \"notes.notes\"]",
    );
    let error = parse(&twice).expect_err("a repeated feed");
    assert!(
        matches!(&error, ManifestError::InvalidScope { kind: CapabilityKind::Index, reason }
            if reason.contains("twice")),
        "{error}"
    );
}

#[test]
fn the_repository_address_is_bounded_in_bytes() {
    // The host bounds every address in bytes; two-byte characters reach that bound sooner.
    let limit = limits::MAX_ADDRESS_BYTES;
    let fits = changed(
        "repository = \"https://example.invalid/notes-plugin\"",
        &format!("repository = {:?}", "r".repeat(limit)),
    );
    assert!(parse(&fits).is_ok());
    let over = changed(
        "repository = \"https://example.invalid/notes-plugin\"",
        &format!("repository = {:?}", "\u{e9}".repeat(limit / 2 + 1)),
    );
    let error = parse(&over).expect_err("an address over the byte bound");
    assert!(
        matches!(&error, ManifestError::InvalidField { field, .. } if field == "plugin.repository"),
        "{error}"
    );
}
