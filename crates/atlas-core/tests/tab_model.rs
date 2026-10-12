//! Exercises tab and saved-page behavior through the public model contract.

use std::collections::BTreeSet;

use atlas_core::{
    Activation, SavedFolderId, SavedPageId, SidebarRow, Tab, TabError, TabId, TabKind, TabMetadata,
    TabModel,
};

fn metadata(number: usize) -> TabMetadata {
    TabMetadata {
        title: format!("Synthetic page {number}"),
        address: format!("https://page-{number}.test/"),
        favicon: Some(format!("https://page-{number}.test/icon.svg")),
        badge: Some(number.to_string()),
    }
}

fn open_web(model: &mut TabModel, number: usize) -> Result<TabId, TabError> {
    model.open(TabKind::Web, metadata(number), Activation::Foreground)
}

fn open_ids(model: &TabModel) -> Vec<TabId> {
    model.tabs().iter().map(Tab::id).collect()
}

#[derive(Debug, Eq, PartialEq)]
enum RowIdentity {
    Folder(SavedFolderId),
    Saved(SavedPageId, Option<TabId>),
    Ordinary(TabId),
}

fn rows(model: &TabModel) -> Vec<RowIdentity> {
    model
        .sidebar_rows()
        .map(|row| match row {
            SidebarRow::Folder { folder, .. } => RowIdentity::Folder(folder.id()),
            SidebarRow::Saved { page, open_tab, .. } => {
                RowIdentity::Saved(page.id(), open_tab.map(Tab::id))
            }
            SidebarRow::Ordinary(tab) => RowIdentity::Ordinary(tab.id()),
        })
        .collect()
}

#[derive(Debug, Eq, PartialEq)]
struct Snapshot {
    tabs: Vec<Tab>,
    saved: Vec<(SavedPageId, TabMetadata)>,
    active: Option<TabId>,
    closed_count: usize,
    rows: Vec<RowIdentity>,
}

fn snapshot(model: &TabModel) -> Snapshot {
    Snapshot {
        tabs: model.tabs().to_vec(),
        saved: model
            .saved_pages()
            .iter()
            .map(|page| (page.id(), page.metadata().clone()))
            .collect(),
        active: model.active_id(),
        closed_count: model.closed_count(),
        rows: rows(model),
    }
}

fn assert_open_invariants(model: &TabModel) {
    // Identity and selection must remain coherent after every public transition.
    let ids: BTreeSet<_> = open_ids(model).into_iter().collect();
    assert_eq!(ids.len(), model.tabs().len());
    assert_eq!(model.active_id().is_none(), model.tabs().is_empty());
    if let Some(active) = model.active_id() {
        assert!(ids.contains(&active));
    }
    for tab in model.tabs() {
        assert_eq!(model.get(tab.id()), Some(tab));
    }
    assert!(model.closed_count() <= 20);

    // Every saved entry and open instance appears in exactly one projected row.
    let mut projected_tabs = Vec::new();
    let mut projected_saved = Vec::new();
    let mut ordinary_started = false;
    for row in model.sidebar_rows() {
        match row {
            SidebarRow::Folder { folder, .. } => {
                assert!(!ordinary_started);
                assert_eq!(model.get_folder(folder.id()), Some(folder));
            }
            SidebarRow::Saved { page, open_tab, .. } => {
                assert!(!ordinary_started);
                projected_saved.push(page.id());
                assert_eq!(
                    model.get_saved(page.id()).map(|saved| saved.metadata()),
                    Some(page.metadata())
                );
                if let Some(tab) = open_tab {
                    assert_eq!(tab.kind(), TabKind::Web);
                    assert_eq!(tab.saved_page_id(), Some(page.id()));
                    projected_tabs.push(tab.id());
                }
            }
            SidebarRow::Ordinary(tab) => {
                ordinary_started = true;
                assert_eq!(tab.saved_page_id(), None);
                projected_tabs.push(tab.id());
            }
        }
    }
    assert_eq!(projected_tabs, open_ids(model));
    assert_eq!(
        projected_saved.len(),
        projected_saved
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
    );
    assert_eq!(
        projected_saved,
        model
            .saved_pages()
            .iter()
            .map(|page| page.id())
            .collect::<Vec<_>>()
    );
}

#[test]
fn empty_and_background_open_preserve_a_valid_selection() -> Result<(), TabError> {
    let mut model = TabModel::new();
    assert!(model.tabs().is_empty());
    assert_eq!(model.active_id(), None);
    assert_eq!(model.reopen_closed()?, None);
    assert_eq!(model.closed_count(), 0);

    // A first background request still supplies the only possible active tab.
    let first = model.open(TabKind::Web, metadata(1), Activation::Background)?;
    let second = model.open(TabKind::Web, metadata(2), Activation::Background)?;
    assert_eq!(model.active_id(), Some(first));
    assert_eq!(open_ids(&model), vec![first, second]);
    let third = open_web(&mut model, 3)?;
    assert_eq!(model.active_id(), Some(third));
    assert_eq!(open_ids(&model), vec![first, second, third]);
    assert_open_invariants(&model);
    Ok(())
}

#[test]
fn active_close_uses_right_then_left_and_last_close_leaves_home() -> Result<(), TabError> {
    let mut model = TabModel::default();
    let left = open_web(&mut model, 1)?;
    let middle = open_web(&mut model, 2)?;
    let right = open_web(&mut model, 3)?;
    model.select(middle)?;

    // The visible right neighbor wins even when another tab was selected more recently.
    assert_eq!(model.close(middle)?.id(), middle);
    assert_eq!(model.active_id(), Some(right));
    model.close(right)?;
    assert_eq!(model.active_id(), Some(left));
    model.close(left)?;
    assert_eq!(model.active_id(), None);
    assert!(model.tabs().is_empty());
    assert_eq!(model.closed_count(), 3);
    assert_open_invariants(&model);
    Ok(())
}

#[test]
fn closing_an_inactive_tab_preserves_selection() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let left = open_web(&mut model, 1)?;
    let active = open_web(&mut model, 2)?;
    let right = open_web(&mut model, 3)?;
    model.select(active)?;
    model.close(left)?;
    model.close(right)?;
    assert_eq!(model.active_id(), Some(active));
    assert_eq!(open_ids(&model), vec![active]);
    Ok(())
}

#[test]
fn reorder_uses_final_group_positions_without_changing_selection() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let first = open_web(&mut model, 1)?;
    let second = open_web(&mut model, 2)?;
    let third = open_web(&mut model, 3)?;
    model.select(second)?;
    model.move_ordinary(first, 2)?;
    assert_eq!(open_ids(&model), vec![second, third, first]);
    model.move_ordinary(first, 0)?;
    assert_eq!(open_ids(&model), vec![first, second, third]);
    model.move_ordinary(second, 1)?;
    assert_eq!(open_ids(&model), vec![first, second, third]);
    assert_eq!(model.active_id(), Some(second));
    Ok(())
}

#[test]
fn reopen_is_lifo_restores_metadata_and_never_reuses_a_live_id() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let first = open_web(&mut model, 1)?;
    let second = open_web(&mut model, 2)?;
    model.close(first)?;
    model.close(second)?;

    // Every reopening creates a new lifetime so retired renderer events stay stale.
    let reopened_second = model.reopen_closed()?.expect("a web closure is available");
    assert_ne!(reopened_second, first);
    assert_ne!(reopened_second, second);
    assert_eq!(
        model.get(reopened_second).map(Tab::metadata),
        Some(&metadata(2))
    );
    assert_eq!(model.active_id(), Some(reopened_second));
    let reopened_first = model.reopen_closed()?.expect("the earlier closure remains");
    assert_ne!(reopened_first, first);
    assert_ne!(reopened_first, second);
    assert_ne!(reopened_first, reopened_second);
    assert_eq!(
        model.get(reopened_first).map(Tab::metadata),
        Some(&metadata(1))
    );
    assert_eq!(model.active_id(), Some(reopened_first));
    assert_eq!(model.reopen_closed()?, None);
    Ok(())
}

#[test]
fn reopening_clamps_a_vanished_ordinary_position() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let first = open_web(&mut model, 1)?;
    let second = open_web(&mut model, 2)?;
    let third = open_web(&mut model, 3)?;
    model.close(third)?;

    // Move surviving pages out of the ordinary group without adding newer closures.
    model.save_page(first)?;
    model.save_page(second)?;
    let reopened = model
        .reopen_closed()?
        .expect("the ordinary closure remains");
    assert_eq!(open_ids(&model), vec![first, second, reopened]);
    assert_eq!(
        model
            .get(reopened)
            .expect("reopened tab exists")
            .saved_page_id(),
        None
    );
    assert_eq!(model.active_id(), Some(reopened));
    Ok(())
}

#[test]
fn the_twenty_first_web_closure_evicts_only_the_oldest() -> Result<(), TabError> {
    let mut model = TabModel::new();
    for number in 0..21 {
        let id = open_web(&mut model, number)?;
        model.close(id)?;
        assert_eq!(model.closed_count(), (number + 1).min(20));
    }

    // The retained history is exactly pages 20 through 1; page 0 was evicted.
    for number in (1..21).rev() {
        let id = model
            .reopen_closed()?
            .expect("one of twenty closures remains");
        assert_eq!(model.get(id).map(Tab::metadata), Some(&metadata(number)));
        assert_eq!(model.closed_count(), number - 1);
    }
    assert_eq!(model.reopen_closed()?, None);
    assert_eq!(model.tabs().len(), 20);
    assert_open_invariants(&model);
    Ok(())
}

#[test]
fn all_kinds_are_supported_but_only_web_closures_enter_history() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let web = open_web(&mut model, 0)?;
    model.close(web)?;
    for kind in [
        TabKind::PluginView,
        TabKind::Note,
        TabKind::File,
        TabKind::Diff,
        TabKind::Terminal,
    ] {
        let id = model.open(kind, metadata(1), Activation::Foreground)?;
        assert_eq!(model.get(id).map(Tab::kind), Some(kind));
        assert_eq!(model.close(id)?.kind(), kind);
        assert_eq!(model.closed_count(), 1);
    }
    let reopened = model
        .reopen_closed()?
        .expect("the web closure was retained");
    assert_eq!(model.get(reopened).map(Tab::kind), Some(TabKind::Web));
    assert_eq!(model.reopen_closed()?, None);
    Ok(())
}

#[test]
fn numeric_ids_reject_zero_without_restricting_valid_nonzero_values() -> Result<(), TabError> {
    assert_eq!(TabId::new(0), Err(TabError::InvalidTabId));
    assert_eq!(TabId::new(1)?.get(), 1);
    assert_eq!(TabId::new(u64::MAX)?.get(), u64::MAX);
    assert_eq!(SavedPageId::new(0), Err(TabError::InvalidSavedPageId));
    assert_eq!(SavedPageId::new(1)?.get(), 1);
    assert_eq!(SavedPageId::new(u64::MAX)?.get(), u64::MAX);
    Ok(())
}

#[test]
fn saving_is_idempotent_and_navigation_cannot_change_the_saved_target() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let first = open_web(&mut model, 1)?;
    let ordinary = open_web(&mut model, 2)?;
    let second = open_web(&mut model, 3)?;
    let first_saved = model.save_page(first)?;
    let second_saved = model.save_page(second)?;
    assert_eq!(open_ids(&model), vec![first, second, ordinary]);
    assert_eq!(model.active_id(), Some(second));

    // A live title/address update must not silently edit a bookmark or move its row.
    model.update_metadata(first, metadata(10))?;
    assert_eq!(model.save_page(first)?, first_saved);
    assert_eq!(model.get(first).map(Tab::metadata), Some(&metadata(10)));
    assert_eq!(
        model.get_saved(first_saved).map(|page| page.metadata()),
        Some(&metadata(1))
    );
    assert_eq!(
        rows(&model),
        vec![
            RowIdentity::Saved(first_saved, Some(first)),
            RowIdentity::Saved(second_saved, Some(second)),
            RowIdentity::Ordinary(ordinary),
        ]
    );
    assert_open_invariants(&model);
    Ok(())
}

#[test]
fn closing_a_saved_page_leaves_an_unloaded_row_and_home() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let live = open_web(&mut model, 1)?;
    let saved = model.save_page(live)?;
    model.close(live)?;
    assert!(model.tabs().is_empty());
    assert_eq!(model.active_id(), None);
    assert_eq!(rows(&model), vec![RowIdentity::Saved(saved, None)]);
    assert_eq!(
        model.get_saved(saved).map(|page| page.metadata()),
        Some(&metadata(1))
    );
    assert_open_invariants(&model);
    Ok(())
}

#[test]
fn open_saved_reuses_live_instances_but_reloads_closed_ones_from_saved_target()
-> Result<(), TabError> {
    let mut model = TabModel::new();
    let original = open_web(&mut model, 1)?;
    let saved = model.save_page(original)?;
    let other = open_web(&mut model, 2)?;
    model.update_metadata(original, metadata(10))?;
    assert_eq!(model.open_saved(saved, Activation::Background)?, original);
    assert_eq!(model.active_id(), Some(other));
    assert_eq!(model.open_saved(saved, Activation::Foreground)?, original);
    assert_eq!(model.active_id(), Some(original));
    assert_eq!(model.get(original).map(Tab::metadata), Some(&metadata(10)));

    // A new live instance starts at the saved target, not its retired navigation state.
    model.close(original)?;
    let fresh = model.open_saved(saved, Activation::Background)?;
    assert_ne!(fresh, original);
    assert_eq!(model.active_id(), Some(other));
    assert_eq!(model.get(fresh).map(Tab::metadata), Some(&metadata(1)));
    assert_eq!(model.saved_pages().len(), 1);
    assert_eq!(
        rows(&model),
        vec![
            RowIdentity::Saved(saved, Some(fresh)),
            RowIdentity::Ordinary(other)
        ]
    );
    assert_open_invariants(&model);
    Ok(())
}

#[test]
fn saved_history_reuses_an_existing_instance_without_resetting_navigation() -> Result<(), TabError>
{
    let mut model = TabModel::new();
    let original = open_web(&mut model, 1)?;
    let saved = model.save_page(original)?;
    model.close(original)?;
    let existing = model.open_saved(saved, Activation::Foreground)?;
    model.update_metadata(existing, metadata(10))?;
    let other = open_web(&mut model, 2)?;

    // Ctrl Shift T consumes the closure and focuses the already-open association.
    assert_eq!(model.reopen_closed()?, Some(existing));
    assert_eq!(model.active_id(), Some(existing));
    assert_eq!(model.get(existing).map(Tab::metadata), Some(&metadata(10)));
    assert_eq!(open_ids(&model), vec![existing, other]);
    assert_eq!(model.saved_pages().len(), 1);
    assert_eq!(model.closed_count(), 0);
    assert_open_invariants(&model);
    Ok(())
}

#[test]
fn saved_reordering_includes_unloaded_rows_and_controls_reopen_position() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let first = open_web(&mut model, 1)?;
    let middle = open_web(&mut model, 2)?;
    let last = open_web(&mut model, 3)?;
    let first_saved = model.save_page(first)?;
    let middle_saved = model.save_page(middle)?;
    let last_saved = model.save_page(last)?;
    model.update_metadata(middle, metadata(20))?;
    model.close(middle)?;
    model.move_saved(middle_saved, 0)?;
    assert_eq!(
        rows(&model),
        vec![
            RowIdentity::Saved(middle_saved, None),
            RowIdentity::Saved(first_saved, Some(first)),
            RowIdentity::Saved(last_saved, Some(last)),
        ]
    );
    assert_eq!(open_ids(&model), vec![first, last]);

    // Reopening follows the surviving saved row's order and fixed target.
    let fresh = model.reopen_closed()?.expect("saved closure remains");
    assert_ne!(fresh, middle);
    assert_eq!(model.get(fresh).map(Tab::metadata), Some(&metadata(2)));
    assert_eq!(open_ids(&model), vec![fresh, first, last]);
    assert_eq!(model.active_id(), Some(fresh));
    assert_open_invariants(&model);
    Ok(())
}

#[test]
fn removing_a_saved_page_keeps_its_live_instance_at_the_ordinary_start() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let ordinary = open_web(&mut model, 1)?;
    let first = open_web(&mut model, 2)?;
    let remaining = open_web(&mut model, 3)?;
    let removed = model.save_page(first)?;
    let kept = model.save_page(remaining)?;
    model.update_metadata(first, metadata(20))?;
    model.select(first)?;
    assert_eq!(model.remove_saved(removed)?.id(), removed);
    assert_eq!(model.active_id(), Some(first));
    assert_eq!(model.get(first).map(Tab::metadata), Some(&metadata(20)));
    assert_eq!(
        model
            .get(first)
            .expect("live instance remains")
            .saved_page_id(),
        None
    );
    assert_eq!(
        rows(&model),
        vec![
            RowIdentity::Saved(kept, Some(remaining)),
            RowIdentity::Ordinary(first),
            RowIdentity::Ordinary(ordinary),
        ]
    );
    assert_eq!(model.closed_count(), 0);
    assert_open_invariants(&model);
    Ok(())
}

#[test]
fn removing_a_closed_saved_page_cannot_be_undone_by_tab_history() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let ordinary = open_web(&mut model, 1)?;
    let old = open_web(&mut model, 2)?;
    let removed = model.save_page(old)?;
    model.update_metadata(old, metadata(20))?;
    model.close(old)?;
    model.remove_saved(removed)?;
    let fresh = model
        .reopen_closed()?
        .expect("the live closure remains in history");
    assert_ne!(fresh, old);
    assert!(model.saved_pages().is_empty());
    assert_eq!(model.get_saved(removed), None);
    assert_eq!(model.get(fresh).map(Tab::metadata), Some(&metadata(20)));
    assert_eq!(
        model
            .get(fresh)
            .expect("reopened tab exists")
            .saved_page_id(),
        None
    );
    assert_eq!(open_ids(&model), vec![fresh, ordinary]);
    assert_open_invariants(&model);
    Ok(())
}

fn assert_stale_tab_is_rejected(model: &mut TabModel, stale: TabId) {
    let before = snapshot(model);
    assert_eq!(model.select(stale), Err(TabError::UnknownTab(stale)));
    assert_eq!(snapshot(model), before);
    assert_eq!(
        model.update_metadata(stale, metadata(99)),
        Err(TabError::UnknownTab(stale))
    );
    assert_eq!(snapshot(model), before);
    assert_eq!(model.close(stale), Err(TabError::UnknownTab(stale)));
    assert_eq!(snapshot(model), before);
    assert_eq!(model.save_page(stale), Err(TabError::UnknownTab(stale)));
    assert_eq!(snapshot(model), before);
    assert_eq!(
        model.move_ordinary(stale, 0),
        Err(TabError::UnknownTab(stale))
    );
    assert_eq!(snapshot(model), before);
}

fn assert_stale_saved_page_is_rejected(model: &mut TabModel, stale: SavedPageId) {
    let before = snapshot(model);
    assert_eq!(
        model.open_saved(stale, Activation::Foreground),
        Err(TabError::UnknownSavedPage(stale))
    );
    assert_eq!(snapshot(model), before);
    assert_eq!(
        model.remove_saved(stale),
        Err(TabError::UnknownSavedPage(stale))
    );
    assert_eq!(snapshot(model), before);
    assert_eq!(
        model.move_saved(stale, 0),
        Err(TabError::UnknownSavedPage(stale))
    );
    assert_eq!(snapshot(model), before);
}

#[test]
fn unknown_and_retired_identities_reject_every_mutation_atomically() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let stale = open_web(&mut model, 1)?;
    let removed = model.save_page(stale)?;
    model.close(stale)?;
    model.remove_saved(removed)?;
    let reopened = model
        .reopen_closed()?
        .expect("closed live instance remains");
    model.save_page(reopened)?;
    let history = open_web(&mut model, 2)?;
    model.close(history)?;

    // Reject both formerly valid identities and values that were never allocated.
    assert_stale_tab_is_rejected(&mut model, stale);
    assert_stale_tab_is_rejected(&mut model, TabId::new(u64::MAX)?);
    assert_stale_saved_page_is_rejected(&mut model, removed);
    assert_stale_saved_page_is_rejected(&mut model, SavedPageId::new(u64::MAX)?);
    let recovered = model
        .reopen_closed()?
        .expect("failed operations preserve history");
    assert_eq!(model.get(recovered).map(Tab::metadata), Some(&metadata(2)));
    assert_eq!(model.get(reopened).map(Tab::metadata), Some(&metadata(1)));
    assert_open_invariants(&model);
    Ok(())
}

#[test]
fn invalid_group_positions_and_kinds_leave_every_projection_unchanged() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let ordinary = open_web(&mut model, 1)?;
    let saved_tab = open_web(&mut model, 2)?;
    let saved = model.save_page(saved_tab)?;
    let before = snapshot(&model);
    assert_eq!(
        model.move_ordinary(saved_tab, 0),
        Err(TabError::NotOrdinaryTab(saved_tab))
    );
    assert_eq!(snapshot(&model), before);
    for index in [1, usize::MAX] {
        assert_eq!(
            model.move_ordinary(ordinary, index),
            Err(TabError::InvalidPosition {
                index,
                group_len: 1
            })
        );
        assert_eq!(snapshot(&model), before);
        assert_eq!(
            model.move_saved(saved, index),
            Err(TabError::InvalidPosition {
                index,
                group_len: 1
            })
        );
        assert_eq!(snapshot(&model), before);
    }

    // Plugin-owned kinds may be open but cannot become saved web pages.
    for kind in [
        TabKind::PluginView,
        TabKind::Note,
        TabKind::File,
        TabKind::Diff,
        TabKind::Terminal,
    ] {
        let id = model.open(kind, metadata(3), Activation::Background)?;
        let before = snapshot(&model);
        assert_eq!(model.save_page(id), Err(TabError::NotWebTab(id)));
        assert_eq!(snapshot(&model), before);
    }
    assert_open_invariants(&model);
    Ok(())
}

fn exercise_transition(model: &mut TabModel, step: usize) -> Result<(), TabError> {
    let tab = model
        .tabs()
        .get((step * 17) % model.tabs().len().max(1))
        .map(Tab::id);
    let saved = model
        .saved_pages()
        .get((step * 13) % model.saved_pages().len().max(1))
        .map(|page| page.id());

    // Interleave lifecycle, navigation, saved-row and ordering operations deterministically.
    match step % 12 {
        0 | 1 => {
            open_web(model, step)?;
        }
        2 => {
            if let Some(id) = tab {
                model.save_page(id)?;
            }
        }
        3 => {
            if let Some(id) = tab {
                model.update_metadata(id, metadata(1_000 + step))?;
            }
        }
        4 => {
            if let Some(id) = tab {
                model.close(id)?;
            }
        }
        5 => {
            model.reopen_closed()?;
        }
        6 => {
            if let Some(id) = saved {
                model.open_saved(id, Activation::Background)?;
            }
        }
        7 => {
            if let Some(id) = saved {
                model.remove_saved(id)?;
            }
        }
        8 => {
            if let Some(id) = saved {
                model.move_saved(id, model.saved_pages().len() - 1)?;
            }
        }
        9 => {
            if let Some(id) = model
                .tabs()
                .iter()
                .find(|tab| tab.saved_page_id().is_none())
                .map(Tab::id)
            {
                model.move_ordinary(id, 0)?;
            }
        }
        10 => {
            if let Some(id) = tab {
                model.select(id)?;
            }
        }
        _ => {
            assert_stale_tab_is_rejected(model, TabId::new(u64::MAX)?);
            assert_stale_saved_page_is_rejected(model, SavedPageId::new(u64::MAX)?);
        }
    }
    Ok(())
}

#[test]
fn long_operation_sequences_preserve_identity_order_and_projection_invariants()
-> Result<(), TabError> {
    let mut model = TabModel::new();
    let mut issued_tabs = BTreeSet::new();
    let mut issued_saved = BTreeSet::new();
    for step in 0..600 {
        let previous_tabs: BTreeSet<_> = open_ids(&model).into_iter().collect();
        let previous_saved: BTreeSet<_> =
            model.saved_pages().iter().map(|page| page.id()).collect();
        exercise_transition(&mut model, step)?;
        assert_open_invariants(&model);

        // An identity may remain present, but once retired it cannot be allocated again.
        for id in open_ids(&model)
            .into_iter()
            .filter(|id| !previous_tabs.contains(id))
        {
            assert!(
                issued_tabs.insert(id),
                "a retired tab ID was reused at step {step}"
            );
        }
        for id in model
            .saved_pages()
            .iter()
            .map(|page| page.id())
            .filter(|id| !previous_saved.contains(id))
        {
            assert!(
                issued_saved.insert(id),
                "a removed saved-page ID was reused at step {step}"
            );
        }
    }
    assert!(issued_tabs.len() > 20);
    assert!(issued_saved.len() > 20);
    Ok(())
}

#[test]
fn diagnostic_formatting_omits_synthetic_browsing_metadata() -> Result<(), TabError> {
    let mut model = TabModel::new();
    let secret = TabMetadata {
        title: "SYNTHETIC_PRIVATE_TITLE".to_owned(),
        address: "https://synthetic-private-address.invalid/".to_owned(),
        favicon: Some("SYNTHETIC_PRIVATE_ICON".to_owned()),
        badge: Some("SYNTHETIC_PRIVATE_BADGE".to_owned()),
    };
    let id = model.open(TabKind::Web, secret.clone(), Activation::Foreground)?;
    let saved = model.save_page(id)?;
    let diagnostic = format!(
        "{secret:?} {:?} {:?} {:?} {model:?}",
        model.get(id),
        model.get_saved(saved),
        model.sidebar_rows().collect::<Vec<_>>()
    );
    model.close(id)?;
    let closed_diagnostic = format!("{model:?}");
    for value in [
        secret.title.as_str(),
        secret.address.as_str(),
        secret.favicon.as_deref().expect("synthetic favicon exists"),
        secret.badge.as_deref().expect("synthetic badge exists"),
    ] {
        assert!(!diagnostic.contains(value));
        assert!(!closed_diagnostic.contains(value));
    }
    Ok(())
}
