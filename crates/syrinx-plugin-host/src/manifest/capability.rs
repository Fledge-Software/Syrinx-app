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

impl CapabilityKind {
    /// Every kind, in the order the manifest documentation lists them.
    pub const ALL: [Self; 9] = [
        Self::Storage,
        Self::Folder,
        Self::Drafts,
        Self::Index,
        Self::OpenExternal,
        Self::Parse,
        Self::Terminal,
        Self::Git,
        Self::LanguageServers,
    ];

    /// The kind a manifest names, or `None` for a kind this host does not know.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// The kind's name in a manifest's `kind` field.
    pub fn name(self) -> &'static str {
        match self {
            Self::Storage => "storage",
            Self::Folder => "folder",
            Self::Drafts => "drafts",
            Self::Index => "index",
            Self::OpenExternal => "open-external",
            Self::Parse => "parse",
            Self::Terminal => "terminal",
            Self::Git => "git",
            Self::LanguageServers => "language-servers",
        }
    }

    /// Whether the kind runs programs or reaches outside the sandbox, so the owner sees
    /// "Can run programs on your computer with your permissions".
    pub fn is_full_trust(self) -> bool {
        match self {
            Self::Terminal | Self::Git | Self::LanguageServers => true,
            Self::Storage
            | Self::Folder
            | Self::Drafts
            | Self::Index
            | Self::OpenExternal
            | Self::Parse => false,
        }
    }

    /// The `syrinx:plugin` interfaces a component may import once this kind is requested.
    pub fn interfaces(self) -> &'static [&'static str] {
        match self {
            // Storage is `host.settings-get` and `host.settings-set`, which every plugin imports.
            Self::Storage => &[],
            Self::Folder => &["documents", "watch"],
            Self::Drafts => &["drafts"],
            // An index is pulled through the exported `search-feed`; it imports nothing.
            Self::Index => &[],
            Self::OpenExternal => &["open-external"],
            Self::Parse => &["parse"],
            Self::Terminal => &["terminal"],
            Self::Git => &["git"],
            Self::LanguageServers => &["lsp"],
        }
    }

    /// The kind that unlocks a `syrinx:plugin` interface, if any kind does.
    pub fn unlocking(interface: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.interfaces().contains(&interface))
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
        for kind in CapabilityKind::ALL {
            assert_eq!(CapabilityKind::from_name(kind.name()), Some(kind));
        }
        assert_eq!(CapabilityKind::from_name("network"), None);
    }

    #[test]
    fn only_programs_git_and_language_servers_are_full_trust() {
        let full: Vec<&str> = CapabilityKind::ALL
            .into_iter()
            .filter(|kind| kind.is_full_trust())
            .map(CapabilityKind::name)
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
