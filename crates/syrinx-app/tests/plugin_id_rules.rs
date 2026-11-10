//! A plugin id is checked in two crates that cannot call each other: the signed package list
//! (`syrinx-update`) and the manifest (`syrinx-plugin-host`). This table keeps the rules equal.

use syrinx_plugin_host::manifest::{DiscoveryOptions, PluginManifest};
use syrinx_update::{
    FileEntry, MAX_PLUGIN_ID_BYTES, PackagePath, PluginPackageList, Version, sha256,
};

const WORLD: &str = "syrinx:plugin/plugin@0.1.0";

/// Whether a manifest naming `id` passes the plugin host's rules.
fn manifest_accepts(id: &str) -> bool {
    let text = format!(
        r#"schema = 1
[plugin]
id = "{id}"
name = "Synthetic plugin"
version = "0.1.0"
description = "A synthetic plugin for the id table."
authors = ["Example Author <author@example.invalid>"]
repository = "https://example.invalid/plugin"
licence = "MPL-2.0"
min-syrinx = "0.1.0"
[interface]
world = "{WORLD}"
[entry]
module = "plugin.wasm"
"#
    );
    PluginManifest::parse(text.as_bytes(), DiscoveryOptions::default()).is_ok()
}

/// Whether a signed package list naming `id` passes the update verifier's rules.
fn package_accepts(id: &str) -> bool {
    let module = b"synthetic component";
    let path = PackagePath::parse("plugin.wasm").expect("a plain file name");
    let entry = FileEntry::new(path, module.len() as u64, sha256(module).expect("hashes"));
    PluginPackageList::new(id, Version::new(0, 1, 0), WORLD, vec![entry]).is_ok()
}

#[test]
fn the_package_list_and_the_manifest_accept_the_same_plugin_ids() {
    let longest = "x".repeat(MAX_PLUGIN_ID_BYTES);
    let too_long = "x".repeat(MAX_PLUGIN_ID_BYTES + 1);
    let table: [(&str, bool); 14] = [
        ("notes", true),
        ("a", true),
        ("forge-2", true),
        ("9lives", true),
        ("-x", true),
        ("x-", true),
        (&longest, true),
        ("", false),
        (&too_long, false),
        ("Notes", false),
        ("no.tes", false),
        ("no_tes", false),
        ("no tes", false),
        ("n\u{f6}tes", false),
    ];
    for (id, expected) in table {
        assert_eq!(manifest_accepts(id), expected, "manifest, {id:?}");
        assert_eq!(package_accepts(id), expected, "package list, {id:?}");
    }
}
