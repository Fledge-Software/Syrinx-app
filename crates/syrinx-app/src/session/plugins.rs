//! Starts and stops the plugin runtime the session owns (#136): the owner's grants read from
//! the store, and the deny list built from Syrinx's own folders and the system's.

use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use syrinx_plugin_host::grants::{DenyList, Protected};
use syrinx_plugin_host::owner::{OwnerError, OwnerRuntime};
use syrinx_store::{Store, StoreError};

/// Environment variables naming system folders no plugin may be granted, by kind. A
/// variable this system does not set protects nothing.
const SYSTEM_FOLDERS: [(&str, Protected); 6] = [
    ("WINDIR", Protected::System),
    ("ProgramFiles", Protected::ProgramFiles),
    ("ProgramFiles(x86)", Protected::ProgramFiles),
    ("ProgramW6432", Protected::ProgramFiles),
    ("USERPROFILE", Protected::UserProfile),
    ("HOME", Protected::UserProfile),
];

/// Read the owner's grants. No plugin is installed from a package yet (2d), so no plugin
/// code, compiler or worker starts here; each starts on first use.
pub(super) fn start(store: &Arc<Store>, state_dir: &Path) -> Result<OwnerRuntime, OwnerError> {
    OwnerRuntime::start(
        Arc::clone(store),
        env!("CARGO_PKG_VERSION"),
        deny_list(state_dir),
    )
}

/// Stop every plugin, then hand back the store, which nothing else holds any more, for the
/// session to close.
pub(super) fn close(plugins: Option<OwnerRuntime>, store: Arc<Store>) -> Result<(), StoreError> {
    if let Some(plugins) = plugins {
        plugins.shutdown();
    }
    match Arc::try_unwrap(store) {
        Ok(store) => store.shutdown(),
        // Dropping the last holder closes it anyway, reporting only to stderr.
        Err(_) => Err(StoreError::WorkerUnavailable),
    }
}

/// Syrinx's folders and the system's, read from the environment.
fn deny_list(state_dir: &Path) -> DenyList {
    let mut deny = DenyList::new();
    protect_syrinx(&mut deny, state_dir);
    protect_named(&mut deny, &SYSTEM_FOLDERS, |variable| {
        std::env::var_os(variable)
    });
    deny
}

/// The web engine's profile, then the state folder (which holds the snapshot folder, so
/// that is refused with it) and the data folder around both.
fn protect_syrinx(deny: &mut DenyList, state_dir: &Path) {
    let data_dir = state_dir.parent();
    if let Some(data_dir) = data_dir {
        let profile = data_dir.join(syrinx_platform::browser::PROFILE_FOLDER);
        deny.protect(Protected::WebViewData, &profile);
    }
    deny.protect(Protected::SyrinxData, state_dir);
    if let Some(data_dir) = data_dir {
        deny.protect(Protected::SyrinxData, data_dir);
    }
}

/// Protect each folder a named variable holds, as `lookup` reads it.
fn protect_named(
    deny: &mut DenyList,
    variables: &[(&str, Protected)],
    lookup: impl Fn(&str) -> Option<OsString>,
) {
    for (variable, kind) in variables {
        if let Some(folder) = lookup(variable) {
            deny.protect(*kind, Path::new(&folder));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use syrinx_plugin_host::grants::FolderRefusal;

    use super::*;

    /// Fresh synthetic folders under the test's own temporary folder, removed afterwards.
    struct Tree(PathBuf);

    impl Tree {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("syrinx-plugin-deny-{name}-{}", std::process::id()));
            fs::create_dir_all(&root).expect("a synthetic folder");
            Self(root)
        }

        fn folder(&self, relative: &str) -> PathBuf {
            let folder = self.0.join(relative);
            fs::create_dir_all(&folder).expect("a synthetic folder");
            folder
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            // Only the folder this test created is removed.
            if fs::remove_dir_all(&self.0).is_err() {
                eprintln!("a synthetic deny-list folder was left behind");
            }
        }
    }

    fn refused(deny: &DenyList, folder: &Path) -> Option<FolderRefusal> {
        deny.check(folder).err()
    }

    #[test]
    fn syrinx_its_profile_and_its_snapshots_are_refused_and_a_folder_beside_them_is_not() {
        let tree = Tree::new("syrinx");
        let state = tree.folder("Syrinx/state");
        let profile = tree.folder(&format!(
            "Syrinx/{}",
            syrinx_platform::browser::PROFILE_FOLDER
        ));
        let snapshots = tree.folder("Syrinx/state/backups");
        let beside = tree.folder("vault");
        let mut deny = DenyList::new();
        protect_syrinx(&mut deny, &state);
        let syrinx = Some(FolderRefusal::Protected(Protected::SyrinxData));
        assert_eq!(
            refused(&deny, &profile),
            Some(FolderRefusal::Protected(Protected::WebViewData))
        );
        assert_eq!(refused(&deny, &state), syrinx);
        assert_eq!(
            refused(&deny, &snapshots),
            syrinx,
            "inside the state folder"
        );
        // A folder containing Syrinx's is refused for the first protected folder it holds.
        for containing in [tree.0.join("Syrinx"), tree.0.clone()] {
            assert!(matches!(
                refused(&deny, &containing),
                Some(FolderRefusal::Protected(_))
            ));
        }
        assert!(deny.check(&beside).is_ok());
    }

    #[test]
    fn each_system_folder_variable_protects_its_folder() {
        let tree = Tree::new("system");
        let named: Vec<(&str, PathBuf)> = vec![
            ("WINDIR", tree.folder("Windows")),
            ("ProgramFiles", tree.folder("Program Files")),
            ("ProgramFiles(x86)", tree.folder("Program Files (x86)")),
            ("ProgramW6432", tree.folder("Program Files W6432")),
            ("USERPROFILE", tree.folder("Users/owner")),
            ("HOME", tree.folder("home/owner")),
        ];
        let mut deny = DenyList::new();
        protect_named(&mut deny, &SYSTEM_FOLDERS, |variable| {
            named
                .iter()
                .find(|(name, _)| *name == variable)
                .map(|(_, folder)| folder.clone().into_os_string())
        });
        for ((variable, kind), (_, folder)) in SYSTEM_FOLDERS.iter().zip(&named) {
            assert_eq!(
                refused(&deny, folder),
                Some(FolderRefusal::Protected(*kind)),
                "{variable}"
            );
        }
        let inside_system = tree.folder("Windows/System32");
        assert_eq!(
            refused(&deny, &inside_system),
            Some(FolderRefusal::Protected(Protected::System))
        );
        let inside_profile = tree.folder("Users/owner/Documents/vault");
        assert!(
            deny.check(&inside_profile).is_ok(),
            "the owner's own folders"
        );
    }

    #[test]
    fn the_real_deny_list_refuses_the_profile_this_process_is_given() {
        // The test only reads the profile's path; it creates nothing there and prints nothing.
        let profile = ["USERPROFILE", "HOME"]
            .into_iter()
            .find_map(std::env::var_os)
            .map(PathBuf::from)
            .filter(|profile| profile.is_dir())
            .expect("the test process has a profile folder");
        // A relative state folder protects nothing, so only the environment's folders count.
        assert_eq!(
            refused(&deny_list(Path::new("state")), &profile),
            Some(FolderRefusal::Protected(Protected::UserProfile))
        );
    }
}
