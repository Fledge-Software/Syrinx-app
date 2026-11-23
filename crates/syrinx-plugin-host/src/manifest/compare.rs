//! Compares what two versions of a plugin ask for, so an update that asks for more waits for
//! the owner and one that asks for less narrows its grants silently (note 10, section 3).

use super::{CapabilityKind, CapabilityRequest, FolderCount, PluginManifest, Scope};

/// How an update's capability requests differ from the installed version's, by kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilityChanges {
    /// Kinds asked for afresh, a full-trust kind included.
    pub added: Vec<CapabilityKind>,
    /// Kinds asked for with a wider scope in any respect, even if narrower in another.
    pub widened: Vec<CapabilityKind>,
    /// Kinds asked for with a scope narrower in some respect and wider in none.
    pub narrowed: Vec<CapabilityKind>,
    /// Kinds no longer asked for.
    pub removed: Vec<CapabilityKind>,
    /// Kinds asked for with the same scope and a different reason: a one-line notice.
    pub reworded: Vec<CapabilityKind>,
}

impl CapabilityChanges {
    /// Whether the update asks for anything the owner has not agreed to.
    pub fn needs_consent(&self) -> bool {
        !self.added.is_empty() || !self.widened.is_empty()
    }
}

/// How one scope compares with another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    Same,
    Wider,
    Narrower,
}

/// What `new` asks for compared with `old`.
pub fn compare(old: &PluginManifest, new: &PluginManifest) -> CapabilityChanges {
    let mut changes = CapabilityChanges::default();
    for request in new.capabilities() {
        let Some(before) = requested(old, request.kind) else {
            changes.added.push(request.kind);
            continue;
        };
        match reach(&before.scope, &request.scope) {
            Reach::Wider => changes.widened.push(request.kind),
            Reach::Narrower => changes.narrowed.push(request.kind),
            Reach::Same if before.reason != request.reason => changes.reworded.push(request.kind),
            Reach::Same => {}
        }
    }
    changes.removed = old
        .capabilities()
        .iter()
        .map(|request| request.kind)
        .filter(|kind| requested(new, *kind).is_none())
        .collect();
    changes
}

fn requested(manifest: &PluginManifest, kind: CapabilityKind) -> Option<&CapabilityRequest> {
    manifest
        .capabilities()
        .iter()
        .find(|request| request.kind == kind)
}

/// Wider if any respect widens; narrower only if some respect narrows and none widens.
fn reach(old: &Scope, new: &Scope) -> Reach {
    let respects = match (old, new) {
        (
            Scope::Folder {
                writable: was_writable,
                count: was_count,
                watch: was_watching,
            },
            Scope::Folder {
                writable,
                count,
                watch,
            },
        ) => vec![
            flag(*was_writable, *writable),
            folders(*was_count, *count),
            flag(*was_watching, *watch),
        ],
        (Scope::Index { feeds: old_feeds }, Scope::Index { feeds }) => {
            vec![feed_reach(old_feeds, feeds)]
        }
        // Each kind has one scope shape, so anything else is `None` against `None`.
        _ => vec![Reach::Same],
    };
    if respects.contains(&Reach::Wider) {
        return Reach::Wider;
    }
    if respects.contains(&Reach::Narrower) {
        return Reach::Narrower;
    }
    Reach::Same
}

/// A switch such as write access or watching: turning it on widens.
fn flag(old: bool, new: bool) -> Reach {
    match (old, new) {
        (false, true) => Reach::Wider,
        (true, false) => Reach::Narrower,
        _ => Reach::Same,
    }
}

/// More folders, or `"many"`, widens.
fn folders(old: FolderCount, new: FolderCount) -> Reach {
    let limit = |count: FolderCount| match count {
        FolderCount::Limited(limit) => u64::from(limit),
        FolderCount::Many => u64::MAX,
    };
    match limit(new).cmp(&limit(old)) {
        std::cmp::Ordering::Greater => Reach::Wider,
        std::cmp::Ordering::Less => Reach::Narrower,
        std::cmp::Ordering::Equal => Reach::Same,
    }
}

/// A feed not indexed before widens; dropping feeds narrows.
fn feed_reach(old: &[String], new: &[String]) -> Reach {
    if new.iter().any(|feed| !old.contains(feed)) {
        return Reach::Wider;
    }
    if old.iter().any(|feed| !new.contains(feed)) {
        return Reach::Narrower;
    }
    Reach::Same
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::DiscoveryOptions;

    const HEADER: &str = r#"schema = 1
[plugin]
id = "notes"
name = "Synthetic notes"
version = "0.1.0"
description = "A synthetic plugin for the capability comparison."
authors = ["Example Author <author@example.invalid>"]
repository = "https://example.invalid/notes"
licence = "MPL-2.0"
min-syrinx = "0.1.0"
[interface]
world = "syrinx:plugin/plugin@0.1.0"
[entry]
module = "plugin.wasm"
[[contributes.search-feeds]]
id = "notes.notes"
title = "Notes"
tiers = ["catalogue"]
[[contributes.search-feeds]]
id = "notes.tags"
title = "Tags"
tiers = ["catalogue"]
"#;

    /// A manifest asking for each `(kind, extra fields, reason)`.
    fn manifest(requests: &[(&str, &str, &str)]) -> PluginManifest {
        let mut text = HEADER.to_owned();
        for (kind, fields, reason) in requests {
            text.push_str(&format!(
                "[[capabilities]]\nkind = \"{kind}\"\n{fields}\nreason = \"{reason}\"\n"
            ));
        }
        PluginManifest::parse(text.as_bytes(), DiscoveryOptions::default()).expect("manifest")
    }

    fn folder(fields: &str) -> PluginManifest {
        manifest(&[("folder", fields, "Reads a synthetic vault.")])
    }

    #[test]
    fn the_same_requests_change_nothing() {
        let old = folder("access = \"read\"");
        assert_eq!(compare(&old, &old), CapabilityChanges::default());
        assert!(!compare(&old, &old).needs_consent());
    }

    #[test]
    fn a_new_kind_is_added_and_needs_consent() {
        let old = manifest(&[("storage", "", "Keeps settings.")]);
        let new = manifest(&[
            ("storage", "", "Keeps settings."),
            ("terminal", "trust = \"full\"", "Runs a synthetic build."),
        ]);
        let changes = compare(&old, &new);
        assert_eq!(changes.added, [CapabilityKind::Terminal]);
        assert!(changes.needs_consent());
    }

    #[test]
    fn each_folder_respect_that_widens_needs_consent() {
        let old = folder("access = \"read\"\ncount = 2");
        for wider in [
            "access = \"read-write\"\ncount = 2",
            "access = \"read\"\ncount = 3",
            "access = \"read\"\ncount = \"many\"",
            "access = \"read\"\ncount = 2\nwatch = true",
            // Wider in one respect and narrower in another is still wider.
            "access = \"read-write\"\ncount = 1",
        ] {
            let changes = compare(&old, &folder(wider));
            assert_eq!(changes.widened, [CapabilityKind::Folder], "{wider}");
            assert!(changes.needs_consent(), "{wider}");
        }
    }

    #[test]
    fn a_narrower_scope_and_a_dropped_request_need_no_consent() {
        let old = manifest(&[
            ("folder", "access = \"read-write\"\ncount = 2", "Vault."),
            (
                "index",
                "feeds = [\"notes.notes\", \"notes.tags\"]",
                "Search.",
            ),
            ("storage", "", "Settings."),
        ]);
        let new = manifest(&[
            ("folder", "access = \"read\"\ncount = 1", "Vault."),
            ("index", "feeds = [\"notes.notes\"]", "Search."),
        ]);
        let changes = compare(&old, &new);
        assert_eq!(
            changes.narrowed,
            [CapabilityKind::Folder, CapabilityKind::Index]
        );
        assert_eq!(changes.removed, [CapabilityKind::Storage]);
        assert!(!changes.needs_consent());
    }

    #[test]
    fn a_new_feed_widens_the_index() {
        let old = manifest(&[("index", "feeds = [\"notes.notes\"]", "Search.")]);
        let new = manifest(&[("index", "feeds = [\"notes.tags\"]", "Search.")]);
        assert_eq!(compare(&old, &new).widened, [CapabilityKind::Index]);
    }

    #[test]
    fn a_changed_reason_alone_is_a_notice() {
        let old = manifest(&[("storage", "", "Keeps settings.")]);
        let new = manifest(&[("storage", "", "Keeps your editor settings.")]);
        let changes = compare(&old, &new);
        assert_eq!(changes.reworded, [CapabilityKind::Storage]);
        assert!(!changes.needs_consent());
    }
}
