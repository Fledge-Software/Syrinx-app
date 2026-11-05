//! The capability kinds a manifest may request, which of them are full trust, and the
//! `syrinx:plugin` interfaces each one unlocks (the mapping stated in `wit/v0_1/plugin.wit`).

use std::fmt;

/// A kind of power a plugin asks the owner for. The host, not the manifest, decides its trust.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CapabilityKind {
    Storage,
    Folder,
    Drafts,
    Index,
    OpenExternal,
    Parse,
    Terminal,
    Git,
    LanguageServers,
}

/// Every kind, with its manifest name, whether it is full trust, and the interfaces it unlocks.
const KINDS: &[(CapabilityKind, &str, bool, &[&str])] = &[
    // Storage is `host.settings-get` and `host.settings-set`, which every plugin imports.
    (CapabilityKind::Storage, "storage", false, &[]),
    (
        CapabilityKind::Folder,
        "folder",
        false,
        &["documents", "watch"],
    ),
    (CapabilityKind::Drafts, "drafts", false, &["drafts"]),
    // An index is pulled through the exported `search-feed`; it imports nothing.
    (CapabilityKind::Index, "index", false, &[]),
    (
        CapabilityKind::OpenExternal,
        "open-external",
        false,
        &["open-external"],
    ),
    (CapabilityKind::Parse, "parse", false, &["parse"]),
    (CapabilityKind::Terminal, "terminal", true, &["terminal"]),
    (CapabilityKind::Git, "git", true, &["git"]),
    (
        CapabilityKind::LanguageServers,
        "language-servers",
        true,
        &["lsp"],
    ),
];

impl CapabilityKind {
    /// The kind a manifest names, or `None` for a kind this host does not know.
    pub fn from_name(name: &str) -> Option<Self> {
        KINDS
            .iter()
            .find(|(_, known, _, _)| *known == name)
            .map(|(kind, ..)| *kind)
    }

    pub fn name(self) -> &'static str {
        self.row().1
    }

    /// Whether the kind runs programs or reaches outside the sandbox, so the owner sees
    /// "Can run programs on your computer with your permissions".
    pub fn is_full_trust(self) -> bool {
        self.row().2
    }

    /// The `syrinx:plugin` interfaces a component may import once this kind is requested.
    pub fn interfaces(self) -> &'static [&'static str] {
        self.row().3
    }

    /// The kind that unlocks a `syrinx:plugin` interface, if any kind does.
    pub fn unlocking(interface: &str) -> Option<Self> {
        KINDS
            .iter()
            .find(|(.., interfaces)| interfaces.contains(&interface))
            .map(|(kind, ..)| *kind)
    }

    fn row(self) -> &'static (CapabilityKind, &'static str, bool, &'static [&'static str]) {
        KINDS
            .iter()
            .find(|(kind, ..)| *kind == self)
            .expect("every kind has a row in KINDS")
    }
}

impl fmt::Display for CapabilityKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_round_trips_through_its_name() {
        for (kind, name, ..) in KINDS {
            assert_eq!(CapabilityKind::from_name(name), Some(*kind));
            assert_eq!(kind.name(), *name);
        }
        assert_eq!(CapabilityKind::from_name("network"), None);
    }

    #[test]
    fn only_programs_git_and_language_servers_are_full_trust() {
        let full: Vec<&str> = KINDS
            .iter()
            .filter(|(kind, ..)| kind.is_full_trust())
            .map(|(_, name, ..)| *name)
            .collect();
        assert_eq!(full, ["terminal", "git", "language-servers"]);
    }

    #[test]
    fn folder_unlocks_documents_and_watch() {
        assert_eq!(
            CapabilityKind::unlocking("documents"),
            Some(CapabilityKind::Folder)
        );
        assert_eq!(
            CapabilityKind::unlocking("watch"),
            Some(CapabilityKind::Folder)
        );
        assert_eq!(CapabilityKind::unlocking("host"), None);
    }
}
