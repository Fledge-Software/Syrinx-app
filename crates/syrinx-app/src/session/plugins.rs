//! Starts and stops the plugin runtime the session owns (#136): the owner's grants read from
//! the store, and the deny list built from Syrinx's own folders and the system's.

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

/// The folder of D13's daily snapshots inside the session's state folder.
const SNAPSHOT_FOLDER: &str = "backups";

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

/// Syrinx's folders (`state_dir` and the data folder around it, which holds the web engine's
/// profile) and the system's, read from the environment.
fn deny_list(state_dir: &Path) -> DenyList {
    let mut deny = DenyList::new();
    deny.protect(Protected::SyrinxData, state_dir);
    deny.protect(Protected::Snapshots, &state_dir.join(SNAPSHOT_FOLDER));
    if let Some(data_dir) = state_dir.parent() {
        deny.protect(Protected::SyrinxData, data_dir);
        let profile = data_dir.join(syrinx_platform::browser::PROFILE_FOLDER);
        deny.protect(Protected::WebViewData, &profile);
    }
    for (variable, kind) in SYSTEM_FOLDERS {
        if let Some(folder) = std::env::var_os(variable) {
            deny.protect(kind, Path::new(&folder));
        }
    }
    deny
}

#[cfg(test)]
mod tests {
    use std::fs;

    use syrinx_plugin_host::grants::FolderRefusal;

    use super::*;

    #[test]
    fn syrinx_folders_are_refused_and_a_folder_beside_them_is_not() {
        let root = std::env::temp_dir().join(format!("syrinx-plugin-deny-{}", std::process::id()));
        let state = root.join("Syrinx").join("state");
        let beside = root.join("vault");
        for folder in [&state, &beside] {
            fs::create_dir_all(folder).expect("a synthetic folder");
        }
        let deny = deny_list(&state);
        let refused = |folder: &Path| deny.check(folder).err();
        let syrinx = Some(FolderRefusal::Protected(Protected::SyrinxData));
        assert_eq!(refused(&state), syrinx);
        assert_eq!(refused(&root.join("Syrinx")), syrinx);
        assert_eq!(refused(&root), syrinx, "a folder containing Syrinx's");
        assert!(deny.check(&beside).is_ok());
        // Only the folder this test created is removed.
        fs::remove_dir_all(&root).expect("remove the synthetic folders");
    }
}
