//! The deny list on generated folders under each test's own temporary directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{DenyList, FolderRefusal, Protected};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A fresh folder tree that names nothing real; removed when dropped.
struct Tree(PathBuf);

impl Tree {
    fn new() -> Self {
        let name = format!(
            "syrinx-deny-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let root = std::env::temp_dir().join(name);
        fs::create_dir(&root).expect("a fresh synthetic folder");
        Self(root)
    }

    /// Create and return `relative` under the tree.
    fn folder(&self, relative: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(&path).expect("a synthetic folder");
        path
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        // Only the tree this test created is removed.
        if fs::remove_dir_all(&self.0).is_err() {
            eprintln!("a synthetic deny-list folder was left behind");
        }
    }
}

fn refused(list: &DenyList, folder: &Path) -> Option<FolderRefusal> {
    list.check(folder).err()
}

#[test]
fn a_folder_inside_the_profile_is_granted_but_the_profile_and_its_parent_are_not() {
    let tree = Tree::new();
    let profile = tree.folder("home/owner");
    let vault = tree.folder("home/owner/Documents/vault");
    let mut list = DenyList::new();
    list.protect(Protected::UserProfile, &profile);

    assert_eq!(
        list.check(&vault),
        Ok(fs::canonicalize(&vault).expect("vault"))
    );
    let profile_refused = Some(FolderRefusal::Protected(Protected::UserProfile));
    assert_eq!(refused(&list, &profile), profile_refused);
    assert_eq!(refused(&list, &tree.0.join("home")), profile_refused);
}

#[test]
fn syrinx_and_system_folders_are_refused_equal_containing_or_inside() {
    let tree = Tree::new();
    let data = tree.folder("local/Syrinx");
    let windows = tree.folder("Windows");
    let mut list = DenyList::new();
    list.protect(Protected::SyrinxData, &data);
    list.protect(Protected::System, &windows);

    let data_refused = Some(FolderRefusal::Protected(Protected::SyrinxData));
    assert_eq!(refused(&list, &data), data_refused);
    assert_eq!(refused(&list, &tree.0.join("local")), data_refused);
    assert_eq!(
        refused(&list, &tree.folder("local/Syrinx/state")),
        data_refused
    );
    assert_eq!(
        refused(&list, &tree.folder("Windows/System32")),
        Some(FolderRefusal::Protected(Protected::System))
    );
    assert!(list.check(&tree.folder("local/elsewhere")).is_ok());
}

#[test]
fn a_protected_folder_that_does_not_exist_yet_is_still_protected() {
    let tree = Tree::new();
    let parent = tree.folder("local");
    let mut list = DenyList::new();
    list.protect(Protected::Snapshots, &parent.join("Syrinx").join("backups"));
    assert_eq!(
        refused(&list, &parent),
        Some(FolderRefusal::Protected(Protected::Snapshots))
    );
}

#[test]
fn a_root_a_missing_folder_and_a_file_are_refused() {
    let tree = Tree::new();
    let list = DenyList::new();
    let root = tree
        .0
        .ancestors()
        .last()
        .expect("the temporary folder has a root");
    assert_eq!(refused(&list, root), Some(FolderRefusal::Root));
    assert_eq!(
        refused(&list, &tree.0.join("missing")),
        Some(FolderRefusal::Unresolved)
    );
    let file = tree.0.join("note.md");
    fs::write(&file, b"synthetic").expect("a synthetic file");
    assert_eq!(refused(&list, &file), Some(FolderRefusal::NotAFolder));
}

#[cfg(unix)]
#[test]
fn a_link_into_a_protected_folder_is_judged_by_where_it_leads() {
    let tree = Tree::new();
    let data = tree.folder("local/Syrinx");
    let link = tree.0.join("innocent");
    std::os::unix::fs::symlink(&data, &link).expect("a synthetic link");
    let mut list = DenyList::new();
    list.protect(Protected::SyrinxData, &data);
    assert_eq!(
        refused(&list, &link),
        Some(FolderRefusal::Protected(Protected::SyrinxData))
    );
}

#[test]
fn neither_the_list_nor_a_refusal_shows_a_path() {
    let tree = Tree::new();
    let data = tree.folder("private-name");
    let mut list = DenyList::new();
    list.protect(Protected::SyrinxData, &data);
    let refusal = refused(&list, &data).expect("refused");
    assert!(!format!("{list:?}{refusal:?}{refusal}").contains("private-name"));
}
