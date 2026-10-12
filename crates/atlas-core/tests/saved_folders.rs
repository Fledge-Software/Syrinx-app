//! Exercises inline saved folders (P1-21) through the public model contract.

use std::collections::BTreeSet;

use atlas_core::{
    Activation, FolderContents, FolderRemoval, NewSavedItem, SavedEntryId, SavedFolderId,
    SavedFolderSnapshot, SavedPageId, SavedPlacement, SidebarRow, SnapshotError, Tab, TabError,
    TabId, TabKind, TabMetadata, TabModel, TabModelSnapshot, saved_folder_contents,
};

fn metadata(number: usize) -> TabMetadata {
    TabMetadata {
        title: format!("Synthetic folder page {number}"),
        address: format!("https://folder-page-{number}.example.test/"),
        favicon: None,
        badge: None,
    }
}

fn open_web(model: &mut TabModel, number: usize) -> Result<TabId, TabError> {
    model.open(TabKind::Web, metadata(number), Activation::Foreground)
}

fn folder(model: &mut TabModel, name: &str, parent: Option<SavedFolderId>) -> SavedFolderId {
    model
        .create_folder(format!("Synthetic {name}"), parent)
        .expect("synthetic folder is created")
}

fn page(id: SavedPageId) -> SavedEntryId {
    SavedEntryId::Page(id)
}

fn dir(id: SavedFolderId) -> SavedEntryId {
    SavedEntryId::Folder(id)
}

/// The tree as (entry, parent) pairs in visible order.
fn layout(model: &TabModel) -> Vec<(SavedEntryId, Option<SavedFolderId>)> {
    model
        .saved_layout()
        .iter()
        .map(|placement| (placement.entry, placement.parent))
        .collect()
}

fn open_ids(model: &TabModel) -> Vec<TabId> {
    model.tabs().iter().map(Tab::id).collect()
}

/// Checks every structural invariant the sidebar, palette and store rely on.
fn assert_tree_invariants(model: &TabModel) {
    // Each entry is placed once, after an enclosing parent: no cycles, no orphans.
    let mut seen = BTreeSet::new();
    let mut open_folders: Vec<SavedFolderId> = Vec::new();
    for placement in model.saved_layout() {
        assert!(seen.insert(placement.entry), "entry placed twice");
        match placement.parent {
            None => open_folders.clear(),
            Some(parent) => {
                while open_folders.last().is_some_and(|&top| top != parent) {
                    open_folders.pop();
                }
                assert_eq!(
                    open_folders.last(),
                    Some(&parent),
                    "parent does not enclose"
                );
            }
        }
        if let SavedEntryId::Folder(id) = placement.entry {
            open_folders.push(id);
        }
    }

    // Records follow the layout exactly, so none is orphaned or missing a row.
    let pages: Vec<_> = model.saved_pages().iter().map(|page| page.id()).collect();
    let placed_pages: Vec<_> = model
        .saved_layout()
        .iter()
        .filter_map(|placement| match placement.entry {
            SavedEntryId::Page(id) => Some(id),
            SavedEntryId::Folder(_) => None,
        })
        .collect();
    assert_eq!(pages, placed_pages);
    let folders: Vec<_> = model
        .saved_folders()
        .iter()
        .map(|folder| folder.id())
        .collect();
    let placed_folders: Vec<_> = model
        .saved_layout()
        .iter()
        .filter_map(|placement| match placement.entry {
            SavedEntryId::Folder(id) => Some(id),
            SavedEntryId::Page(_) => None,
        })
        .collect();
    assert_eq!(folders, placed_folders);

    // The sidebar projects every saved row once, then every live tab once.
    let mut projected_entries = Vec::new();
    let mut projected_tabs = Vec::new();
    for row in model.sidebar_rows() {
        match row {
            SidebarRow::Folder { folder, parent } => {
                projected_entries.push((dir(folder.id()), parent));
            }
            SidebarRow::Saved {
                page: saved,
                parent,
                open_tab,
            } => {
                projected_entries.push((page(saved.id()), parent));
                if let Some(tab) = open_tab {
                    assert_eq!(tab.saved_page_id(), Some(saved.id()));
                    projected_tabs.push(tab.id());
                }
            }
            SidebarRow::Ordinary(tab) => projected_tabs.push(tab.id()),
        }
    }
    assert_eq!(projected_entries, layout(model));
    assert_eq!(projected_tabs, open_ids(model));

    // The persistence boundary accepts exactly what the model holds.
    let snapshot = model.snapshot();
    assert_eq!(snapshot.validate(), Ok(()));
    let restored = TabModel::from_snapshot(snapshot.clone()).expect("model snapshot restores");
    assert_eq!(restored.snapshot(), snapshot);
}

#[test]
fn folders_are_created_renamed_and_listed_in_visible_order() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let first = open_web(&mut model, 1)?;
    let saved = model.save_page(first)?;
    let work = folder(&mut model, "work", None);
    let nested = folder(&mut model, "nested", Some(work));
    let unloaded = model.add_saved_page(metadata(2), Some(work))?;
    model.rename_folder(nested, "Synthetic renamed".to_owned())?;

    assert_eq!(
        layout(&model),
        vec![
            (page(saved), None),
            (dir(work), None),
            (dir(nested), Some(work)),
            (page(unloaded), Some(work)),
        ]
    );
    assert_eq!(model.saved_children(None)?, vec![page(saved), dir(work)]);
    assert_eq!(
        model.saved_children(Some(work))?,
        vec![dir(nested), page(unloaded)]
    );
    assert_eq!(model.saved_parent(page(unloaded))?, Some(work));
    assert_eq!(
        model.get_folder(nested).map(|folder| folder.title()),
        Some("Synthetic renamed")
    );
    // Adding an unloaded page opens nothing.
    assert_eq!(open_ids(&model), vec![first]);
    assert_tree_invariants(&model);
    Ok(())
}

/// Live identities, associations and selection, independent of display order.
fn live_state(model: &TabModel) -> (BTreeSet<(TabId, Option<SavedPageId>)>, Option<TabId>) {
    let tabs = model
        .tabs()
        .iter()
        .map(|tab| (tab.id(), tab.saved_page_id()))
        .collect();
    (tabs, model.active_id())
}

#[test]
fn moving_or_dissolving_a_folder_keeps_its_open_tabs_and_selection() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let top = model.add_saved_page(metadata(1), None)?;
    let group = folder(&mut model, "group", None);
    let nested = folder(&mut model, "nested", Some(group));
    let first = model.add_saved_page(metadata(2), Some(group))?;
    let second = model.add_saved_page(metadata(3), Some(nested))?;
    let target = folder(&mut model, "target", None);
    open_web(&mut model, 4)?;
    let first_tab = model.open_saved(first, Activation::Background)?;
    let second_tab = model.open_saved(second, Activation::Foreground)?;
    model.open_saved(top, Activation::Background)?;
    let before = live_state(&model);
    assert_eq!(before.1, Some(second_tab));

    // Moving the folder carries both open pages; nothing closes, reopens or changes focus.
    model.move_saved_entry(dir(group), Some(target), 0)?;
    assert_eq!(live_state(&model), before);
    assert_eq!(
        model.get(first_tab).and_then(Tab::saved_page_id),
        Some(first)
    );
    assert_tree_invariants(&model);

    // Dissolving it keeps the same instances, associations and selection.
    model.remove_folder(group, FolderRemoval::MoveContentsUp)?;
    assert_eq!(live_state(&model), before);
    assert_eq!(model.saved_parent(page(first))?, Some(target));
    assert_eq!(model.saved_parent(dir(nested))?, Some(target));
    assert_tree_invariants(&model);
    Ok(())
}

#[test]
fn moving_a_saved_page_keeps_its_open_instance_identity_and_selection() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let first = open_web(&mut model, 1)?;
    let second = open_web(&mut model, 2)?;
    let ordinary = open_web(&mut model, 3)?;
    let first_saved = model.save_page(first)?;
    let second_saved = model.save_page(second)?;
    model.update_metadata(first, metadata(10))?;
    model.select(first)?;
    let archive = folder(&mut model, "archive", None);
    let live_before = model.get(first).cloned();

    // Moving the first page after the second page reorders the open saved prefix to match.
    model.move_saved_entry(page(first_saved), Some(archive), 0)?;
    assert_eq!(open_ids(&model), vec![second, first, ordinary]);
    assert_eq!(model.get(first).cloned(), live_before);
    assert_eq!(model.active_id(), Some(first));
    assert_eq!(
        model.get_saved(first_saved).map(|saved| saved.metadata()),
        Some(&metadata(1))
    );

    // Moving it back to the top keeps the same instance; open_saved still focuses it.
    model.move_saved_entry(page(first_saved), None, 0)?;
    assert_eq!(open_ids(&model), vec![first, second, ordinary]);
    model.select(ordinary)?;
    assert_eq!(
        model.open_saved(first_saved, Activation::Foreground),
        Ok(first)
    );
    assert_eq!(model.tabs().len(), 3);
    assert_eq!(model.saved_parent(page(second_saved))?, None);
    assert_tree_invariants(&model);
    Ok(())
}

#[test]
fn folders_move_with_their_contents_and_reject_cycles() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let outer = folder(&mut model, "outer", None);
    let inner = folder(&mut model, "inner", Some(outer));
    let deepest = folder(&mut model, "deepest", Some(inner));
    let inside = model.add_saved_page(metadata(1), Some(deepest))?;
    let target = folder(&mut model, "target", None);
    let before = model.snapshot();

    // A folder cannot enter itself or any descendant; nothing changes on refusal.
    for parent in [outer, inner, deepest] {
        assert_eq!(
            model.move_saved_entry(dir(outer), Some(parent), 0),
            Err(TabError::FolderCycle {
                folder: outer,
                parent
            })
        );
    }
    assert_eq!(
        model.move_saved_entry(dir(inner), Some(deepest), 0),
        Err(TabError::FolderCycle {
            folder: inner,
            parent: deepest
        })
    );
    assert_eq!(model.snapshot(), before);

    // Moving a folder carries its whole subtree, keeping nested parents.
    model.move_saved_entry(dir(inner), Some(target), 0)?;
    assert_eq!(
        layout(&model),
        vec![
            (dir(outer), None),
            (dir(target), None),
            (dir(inner), Some(target)),
            (dir(deepest), Some(inner)),
            (page(inside), Some(deepest)),
        ]
    );
    assert_tree_invariants(&model);
    Ok(())
}

#[test]
fn reordering_uses_final_positions_and_rejects_bad_destinations() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let group = folder(&mut model, "group", None);
    let pages: Vec<_> = (1..=4)
        .map(|number| model.add_saved_page(metadata(number), Some(group)))
        .collect::<Result<_, _>>()?;

    model.move_saved(pages[0], 3)?;
    model.move_saved(pages[3], 0)?;
    assert_eq!(
        model.saved_children(Some(group))?,
        vec![
            page(pages[3]),
            page(pages[1]),
            page(pages[2]),
            page(pages[0])
        ]
    );

    // Out-of-range positions and unknown identities change nothing.
    let before = model.snapshot();
    assert_eq!(
        model.move_saved(pages[1], 4),
        Err(TabError::InvalidPosition {
            index: 4,
            group_len: 4
        })
    );
    assert_eq!(
        model.move_saved_entry(dir(group), None, 1),
        Err(TabError::InvalidPosition {
            index: 1,
            group_len: 1
        })
    );
    let stale_folder = SavedFolderId::new(u64::MAX)?;
    assert_eq!(
        model.move_saved_entry(page(pages[1]), Some(stale_folder), 0),
        Err(TabError::UnknownSavedFolder(stale_folder))
    );
    assert_eq!(
        model.create_folder("Synthetic orphan".to_owned(), Some(stale_folder)),
        Err(TabError::UnknownSavedFolder(stale_folder))
    );
    assert_eq!(
        model.rename_folder(stale_folder, "Synthetic stale".to_owned()),
        Err(TabError::UnknownSavedFolder(stale_folder))
    );
    assert_eq!(
        model.remove_folder(stale_folder, FolderRemoval::MoveContentsUp),
        Err(TabError::UnknownSavedFolder(stale_folder))
    );
    assert_eq!(model.snapshot(), before);
    assert_tree_invariants(&model);
    Ok(())
}

#[test]
fn removing_a_folder_can_lift_its_contents_into_its_place() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let before_page = model.add_saved_page(metadata(1), None)?;
    let doomed = folder(&mut model, "doomed", None);
    let child_folder = folder(&mut model, "child", Some(doomed));
    let grandchild = model.add_saved_page(metadata(2), Some(child_folder))?;
    let child_page = model.add_saved_page(metadata(3), Some(doomed))?;
    let after_page = model.add_saved_page(metadata(4), None)?;
    let live = model.open_saved(child_page, Activation::Foreground)?;

    let removed = model.remove_folder(doomed, FolderRemoval::MoveContentsUp)?;
    assert_eq!(
        removed
            .folders
            .iter()
            .map(|folder| folder.id())
            .collect::<Vec<_>>(),
        vec![doomed]
    );
    assert!(removed.pages.is_empty());
    assert_eq!(
        layout(&model),
        vec![
            (page(before_page), None),
            (dir(child_folder), None),
            (page(grandchild), Some(child_folder)),
            (page(child_page), None),
            (page(after_page), None),
        ]
    );
    assert_eq!(
        model.get(live).and_then(Tab::saved_page_id),
        Some(child_page)
    );
    assert_eq!(model.get_folder(doomed), None);
    assert_tree_invariants(&model);
    Ok(())
}

#[test]
fn removing_a_folder_with_contents_keeps_open_pages_as_ordinary_tabs() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let kept = model.add_saved_page(metadata(1), None)?;
    let doomed = folder(&mut model, "doomed", None);
    let nested = folder(&mut model, "nested", Some(doomed));
    let first = model.add_saved_page(metadata(2), Some(nested))?;
    let second = model.add_saved_page(metadata(3), Some(doomed))?;
    let unloaded = model.add_saved_page(metadata(4), Some(doomed))?;
    let ordinary = open_web(&mut model, 5)?;
    let first_tab = model.open_saved(first, Activation::Foreground)?;
    let second_tab = model.open_saved(second, Activation::Background)?;

    // A closed instance of a removed page must not resurrect its saved row.
    model.close(second_tab)?;
    let removed = model.remove_folder(doomed, FolderRemoval::RemoveContents)?;
    assert_eq!(
        removed
            .pages
            .iter()
            .map(|page| page.id())
            .collect::<Vec<_>>(),
        vec![first, second, unloaded]
    );
    assert_eq!(
        removed
            .folders
            .iter()
            .map(|folder| folder.id())
            .collect::<Vec<_>>(),
        vec![doomed, nested]
    );
    assert_eq!(layout(&model), vec![(page(kept), None)]);
    assert_eq!(open_ids(&model), vec![first_tab, ordinary]);
    assert_eq!(model.get(first_tab).and_then(Tab::saved_page_id), None);
    assert_eq!(model.active_id(), Some(first_tab));
    let reopened = model.reopen_closed()?.expect("closed history reopens");
    assert_eq!(model.get(reopened).and_then(Tab::saved_page_id), None);
    assert_eq!(model.saved_pages().len(), 1);

    // Removed identities stay retired.
    let fresh = folder(&mut model, "fresh", None);
    assert!(fresh.get() > nested.get());
    assert_tree_invariants(&model);
    Ok(())
}

#[test]
fn explicit_edits_change_the_saved_record_but_never_the_live_page() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let tab = open_web(&mut model, 1)?;
    let saved = model.save_page(tab)?;
    model.edit_saved(saved, metadata(2))?;
    assert_eq!(model.get(tab).map(Tab::metadata), Some(&metadata(1)));
    assert_eq!(
        model.get_saved(saved).map(|saved| saved.metadata()),
        Some(&metadata(2))
    );
    let stale = SavedPageId::new(u64::MAX)?;
    assert_eq!(
        model.edit_saved(stale, metadata(3)),
        Err(TabError::UnknownSavedPage(stale))
    );
    Ok(())
}

#[test]
fn snapshots_reject_trees_that_are_out_of_order_incomplete_or_cyclic() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let outer = folder(&mut model, "outer", None);
    let inner = folder(&mut model, "inner", Some(outer));
    let inside = model.add_saved_page(metadata(1), Some(inner))?;
    let original = model.snapshot();

    let mut cases: Vec<(TabModelSnapshot, SnapshotError)> = Vec::new();
    let mut reversed = original.clone();
    reversed.saved_folders.swap(0, 1);
    cases.push((reversed, SnapshotError::SavedLayoutMismatch(dir(outer))));
    let mut missing = original.clone();
    missing.saved_layout.pop();
    cases.push((missing, SnapshotError::SavedLayoutIncomplete));
    let mut cyclic = original.clone();
    cyclic.saved_layout[0].parent = Some(inner);
    cases.push((cyclic, SnapshotError::InvalidSavedParent(dir(outer))));
    let mut self_parent = original.clone();
    self_parent.saved_layout[1].parent = Some(inner);
    cases.push((self_parent, SnapshotError::InvalidSavedParent(dir(inner))));
    let mut escaped = original.clone();
    escaped.saved_layout.swap(1, 2);
    escaped.saved_layout[1] = SavedPlacement {
        entry: page(inside),
        parent: Some(inner),
    };
    cases.push((escaped, SnapshotError::InvalidSavedParent(page(inside))));
    let mut duplicate = original.clone();
    duplicate.saved_folders[1] = SavedFolderSnapshot {
        id: outer,
        title: String::new(),
    };
    cases.push((duplicate, SnapshotError::DuplicateSavedFolderId(outer)));
    let mut regressed = original.clone();
    regressed.last_saved_folder_id = 1;
    cases.push((regressed, SnapshotError::SavedFolderIdAboveWatermark(inner)));

    for (snapshot, error) in cases {
        assert_eq!(snapshot.validate(), Err(error));
        assert_eq!(TabModel::from_snapshot(snapshot).err(), Some(error));
    }
    Ok(())
}

#[test]
fn folder_id_exhaustion_leaves_the_tree_unchanged() -> Result<(), TabError> {
    let mut model = TabModel::new();
    folder(&mut model, "only", None);
    let mut snapshot = model.snapshot();
    snapshot.last_saved_folder_id = u64::MAX;
    let mut model = TabModel::from_snapshot(snapshot.clone()).expect("raised watermark restores");
    assert_eq!(
        model.create_folder("Synthetic overflow".to_owned(), None),
        Err(TabError::SavedFolderIdExhausted)
    );
    assert_eq!(model.snapshot(), snapshot);
    Ok(())
}

#[test]
fn folder_diagnostics_omit_folder_names() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let secret = "SYNTHETIC_PRIVATE_FOLDER";
    let id = model.create_folder(secret.to_owned(), None)?;
    let snapshot = model.snapshot();
    let diagnostic = format!(
        "{model:?} {:?} {:?} {:?} {:?}",
        model.get_folder(id),
        model.sidebar_rows().collect::<Vec<_>>(),
        snapshot,
        snapshot.saved_folders
    );
    assert!(!diagnostic.contains(secret));
    Ok(())
}

fn pick<T: Copy>(items: &[T], step: usize, salt: usize) -> Option<T> {
    if items.is_empty() {
        return None;
    }
    items.get((step * salt) % items.len()).copied()
}

fn exercise_folder_transition(model: &mut TabModel, step: usize) -> Result<(), TabError> {
    let folders: Vec<_> = model
        .saved_folders()
        .iter()
        .map(|folder| folder.id())
        .collect();
    let pages: Vec<_> = model.saved_pages().iter().map(|page| page.id()).collect();
    let entries: Vec<_> = model
        .saved_layout()
        .iter()
        .map(|placement| placement.entry)
        .collect();
    let parent = if step.is_multiple_of(3) {
        None
    } else {
        pick(&folders, step, 7)
    };

    // Interleave tree edits with the existing live-tab lifecycle deterministically.
    match step % 13 {
        0 | 1 => {
            folder(model, "generated", parent);
        }
        2 | 3 => {
            model.add_saved_page(metadata(step), parent)?;
        }
        4 => {
            if let Some(id) = pick(&pages, step, 5) {
                model.open_saved(id, Activation::Background)?;
            }
        }
        5 | 6 => {
            let Some(entry) = pick(&entries, step, 11) else {
                return Ok(());
            };
            let siblings = model.saved_children(parent)?.len();
            let index = step % (siblings + 1);
            match model.move_saved_entry(entry, parent, index) {
                Ok(()) | Err(TabError::FolderCycle { .. } | TabError::InvalidPosition { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        7 => {
            if let Some(id) = pick(&folders, step, 3) {
                let removal = if step.is_multiple_of(4) {
                    FolderRemoval::RemoveContents
                } else {
                    FolderRemoval::MoveContentsUp
                };
                model.remove_folder(id, removal)?;
            }
        }
        8 => {
            if let Some(id) = pick(&pages, step, 13) {
                model.remove_saved(id)?;
            }
        }
        9 => {
            open_web(model, step)?;
        }
        10 => {
            // Save an ordinary live page while folders exist; it joins the top level.
            if let Some(id) = model
                .tabs()
                .iter()
                .find(|tab| tab.saved_page_id().is_none() && tab.kind() == TabKind::Web)
                .map(Tab::id)
            {
                model.save_page(id)?;
            }
        }
        11 => {
            // Close an open saved page; its row, inside any folder, must survive.
            if let Some(id) = model
                .tabs()
                .iter()
                .find(|tab| tab.saved_page_id().is_some())
                .map(Tab::id)
            {
                let saved = model.get(id).and_then(Tab::saved_page_id);
                model.close(id)?;
                assert!(saved.is_some_and(|saved| model.get_saved(saved).is_some()));
            }
        }
        _ => {
            model.reopen_closed()?;
        }
    }
    Ok(())
}

#[test]
fn long_folder_operation_sequences_preserve_tree_invariants() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let mut issued_folders = BTreeSet::new();
    for step in 0..500 {
        let previous: BTreeSet<_> = model
            .saved_folders()
            .iter()
            .map(|folder| folder.id())
            .collect();
        exercise_folder_transition(&mut model, step)?;
        assert_tree_invariants(&model);
        for id in model
            .saved_folders()
            .iter()
            .map(|folder| folder.id())
            .filter(|id| !previous.contains(id))
        {
            assert!(
                issued_folders.insert(id),
                "a folder ID was reused at step {step}"
            );
        }
    }
    assert!(issued_folders.len() > 20);
    Ok(())
}

#[test]
fn a_confirmed_delete_is_refused_when_the_folder_changed() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let target = folder(&mut model, "target", None);
    let inner = folder(&mut model, "inner", Some(target));
    let first = model.add_saved_page(metadata(1), Some(inner))?;
    let shown = saved_folder_contents(model.saved_layout(), target);
    assert_eq!((shown.pages(), shown.folders()), (1, 1));
    assert_eq!(
        shown,
        [dir(inner), page(first)]
            .into_iter()
            .collect::<FolderContents>()
    );

    // A page dragged in after the owner confirmed must not be deleted unseen.
    let second = model.add_saved_page(metadata(2), Some(target))?;
    let before = model.snapshot();
    assert_eq!(
        model.delete_folder_confirmed(target, &shown),
        Err(TabError::FolderContentsChanged(target))
    );
    assert_eq!(model.snapshot(), before);

    // A swap keeps the counts equal (one page in, another out), but not the rows.
    let swapped = saved_folder_contents(model.saved_layout(), target);
    model.move_saved_entry(page(first), None, 0)?;
    model.add_saved_page(metadata(3), Some(inner))?;
    let after_swap = saved_folder_contents(model.saved_layout(), target);
    assert_eq!(
        (after_swap.pages(), after_swap.folders()),
        (swapped.pages(), swapped.folders())
    );
    let before = model.snapshot();
    assert_eq!(
        model.delete_folder_confirmed(target, &swapped),
        Err(TabError::FolderContentsChanged(target))
    );
    assert_eq!(model.snapshot(), before);

    let removed = model.delete_folder_confirmed(target, &after_swap)?;
    assert_eq!(removed.pages.len(), 2);
    assert!(model.get_saved(second).is_none());
    assert!(model.get_saved(first).is_some());
    assert!(model.saved_folders().is_empty());
    let unknown = SavedFolderId::new(u64::MAX)?;
    assert_eq!(
        model.delete_folder_confirmed(unknown, &FolderContents::default()),
        Err(TabError::UnknownSavedFolder(unknown))
    );
    assert!(saved_folder_contents(model.saved_layout(), unknown).is_empty());
    Ok(())
}

#[test]
fn empty_or_blank_folder_names_are_refused_without_change() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let named = folder(&mut model, "named", None);
    let before = model.snapshot();
    for blank in ["", " ", "\t\n", "\u{3000}"] {
        assert_eq!(
            model.create_folder(blank.to_owned(), None),
            Err(TabError::EmptyFolderTitle),
            "{blank:?}"
        );
        assert_eq!(
            model.rename_folder(named, blank.to_owned()),
            Err(TabError::EmptyFolderTitle),
            "{blank:?}"
        );
    }
    // A whole subtree is refused when any folder inside it is unnamed.
    let nested_blank = vec![NewSavedItem::Folder {
        title: "Synthetic outer".to_owned(),
        items: vec![NewSavedItem::Folder {
            title: " ".to_owned(),
            items: vec![NewSavedItem::Page(metadata(1))],
        }],
    }];
    assert_eq!(
        model.append_saved_folder("Synthetic import".to_owned(), nested_blank),
        Err(TabError::EmptyFolderTitle)
    );
    assert_eq!(
        model.append_saved_folder(String::new(), Vec::new()),
        Err(TabError::EmptyFolderTitle)
    );
    assert_eq!(model.snapshot(), before);
    assert_eq!(
        TabError::EmptyFolderTitle.to_string(),
        "folder names cannot be empty"
    );

    // Surrounding spaces are allowed and kept; only an all-blank name is refused.
    model.rename_folder(named, " Synthetic spaced ".to_owned())?;
    assert_eq!(
        model.get_folder(named).map(|folder| folder.title()),
        Some(" Synthetic spaced ")
    );
    Ok(())
}
