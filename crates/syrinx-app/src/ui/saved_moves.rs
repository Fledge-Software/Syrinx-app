//! Works out where a saved row goes for a keyboard move or a drop, without a window (P1-21).

use std::collections::HashMap;

use syrinx_core::{SavedEntryId, SavedFolderId, SavedPlacement};

/// A move the model accepts: `entry` ends at final position `index` among `parent`'s children.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SavedMove {
    pub(super) entry: SavedEntryId,
    pub(super) parent: Option<SavedFolderId>,
    pub(super) index: usize,
}

/// One keyboard step for a focused saved row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Step {
    /// One place earlier among its siblings (Alt Up).
    Up,
    /// One place later among its siblings (Alt Down).
    Down,
    /// Out of its folder, just after that folder (Alt Left).
    Out,
    /// Into the folder just above it, at the end (Alt Right).
    In,
}

/// Where a dragged row was released.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DropTarget {
    /// On a folder: into it, last. On a page: into that page's place, before it.
    Row(SavedEntryId),
    /// On the saved section's header: last at the top level.
    TopLevel,
}

/// The move a keyboard step makes, or `None` when the row is already at that edge.
pub(super) fn keyboard_move(
    layout: &[SavedPlacement],
    entry: SavedEntryId,
    step: Step,
) -> Option<SavedMove> {
    let parent = parent_of(layout, entry)?;
    let siblings = children(layout, parent);
    let position = siblings.iter().position(|&child| child == entry)?;
    let (parent, index) = match step {
        Step::Up => (parent, position.checked_sub(1)?),
        Step::Down => (
            parent,
            Some(position + 1).filter(|&next| next < siblings.len())?,
        ),
        Step::Out => {
            let folder = parent?;
            let outer = parent_of(layout, SavedEntryId::Folder(folder))?;
            let place = children(layout, outer)
                .iter()
                .position(|&child| child == SavedEntryId::Folder(folder))?;
            (outer, place + 1)
        }
        Step::In => {
            let SavedEntryId::Folder(above) = siblings.get(position.checked_sub(1)?)? else {
                return None;
            };
            (Some(*above), children(layout, Some(*above)).len())
        }
    };
    Some(SavedMove {
        entry,
        parent,
        index,
    })
}

/// The move a drop makes, or `None` for a drop onto itself, its own contents or its current
/// place.
pub(super) fn drop_move(
    layout: &[SavedPlacement],
    dragged: SavedEntryId,
    target: DropTarget,
) -> Option<SavedMove> {
    let (parent, index) = match target {
        DropTarget::TopLevel => (None, others(layout, None, dragged).len()),
        DropTarget::Row(SavedEntryId::Folder(folder)) => {
            (Some(folder), others(layout, Some(folder), dragged).len())
        }
        DropTarget::Row(row @ SavedEntryId::Page(_)) => {
            let parent = parent_of(layout, row)?;
            let place = others(layout, parent, dragged)
                .iter()
                .position(|&child| child == row)?;
            (parent, place)
        }
    };
    // A folder cannot go into itself or anything inside it; the model refuses that too.
    if let Some(folder) = parent
        && within(layout, SavedEntryId::Folder(folder), dragged)
    {
        return None;
    }
    let destination = SavedMove {
        entry: dragged,
        parent,
        index,
    };
    (current_place(layout, dragged)? != (parent, index)).then_some(destination)
}

/// The folder holding `entry` (`Some(None)` at the top level), or `None` if it is unknown.
fn parent_of(layout: &[SavedPlacement], entry: SavedEntryId) -> Option<Option<SavedFolderId>> {
    layout
        .iter()
        .find(|placement| placement.entry == entry)
        .map(|placement| placement.parent)
}

/// `parent`'s direct children in visible order.
fn children(layout: &[SavedPlacement], parent: Option<SavedFolderId>) -> Vec<SavedEntryId> {
    layout
        .iter()
        .filter(|placement| placement.parent == parent)
        .map(|placement| placement.entry)
        .collect()
}

/// `parent`'s children without `moving`, which is how the model counts a final position.
fn others(
    layout: &[SavedPlacement],
    parent: Option<SavedFolderId>,
    moving: SavedEntryId,
) -> Vec<SavedEntryId> {
    let mut rows = children(layout, parent);
    rows.retain(|&child| child != moving);
    rows
}

/// Where `entry` sits now, as the same final-position pair a move uses.
fn current_place(
    layout: &[SavedPlacement],
    entry: SavedEntryId,
) -> Option<(Option<SavedFolderId>, usize)> {
    let parent = parent_of(layout, entry)?;
    let index = children(layout, parent)
        .iter()
        .position(|&child| child == entry)?;
    Some((parent, index))
}

/// Whether `entry` is `ancestor` itself or nested anywhere inside it.
fn within(layout: &[SavedPlacement], entry: SavedEntryId, ancestor: SavedEntryId) -> bool {
    let parents: HashMap<SavedEntryId, Option<SavedFolderId>> = layout
        .iter()
        .map(|placement| (placement.entry, placement.parent))
        .collect();
    let mut current = Some(entry);
    // A validated layout has no cycles; the step bound keeps a damaged one from looping.
    for _ in 0..=layout.len() {
        let Some(row) = current else {
            return false;
        };
        if row == ancestor {
            return true;
        }
        current = parents
            .get(&row)
            .copied()
            .flatten()
            .map(SavedEntryId::Folder);
    }
    false
}

#[cfg(test)]
mod tests {
    use syrinx_core::{SavedPageId, TabModel};

    use super::*;

    fn folder(id: u64) -> SavedFolderId {
        SavedFolderId::new(id).expect("synthetic folder id")
    }

    fn page(id: u64) -> SavedEntryId {
        SavedEntryId::Page(SavedPageId::new(id).expect("synthetic page id"))
    }

    fn dir(id: u64) -> SavedEntryId {
        SavedEntryId::Folder(folder(id))
    }

    fn place(entry: SavedEntryId, parent: Option<u64>) -> SavedPlacement {
        SavedPlacement {
            entry,
            parent: parent.map(folder),
        }
    }

    /// Top: page 1, folder 1 { page 2, folder 2 { page 3 } }, page 4.
    fn tree() -> Vec<SavedPlacement> {
        vec![
            place(page(1), None),
            place(dir(1), None),
            place(page(2), Some(1)),
            place(dir(2), Some(1)),
            place(page(3), Some(2)),
            place(page(4), None),
        ]
    }

    fn moved(entry: SavedEntryId, parent: Option<u64>, index: usize) -> Option<SavedMove> {
        Some(SavedMove {
            entry,
            parent: parent.map(folder),
            index,
        })
    }

    #[test]
    fn up_and_down_stay_among_siblings_and_stop_at_the_edges() {
        let layout = tree();
        assert_eq!(
            keyboard_move(&layout, page(4), Step::Up),
            moved(page(4), None, 1)
        );
        assert_eq!(
            keyboard_move(&layout, page(1), Step::Down),
            moved(page(1), None, 1)
        );
        assert_eq!(
            keyboard_move(&layout, dir(2), Step::Up),
            moved(dir(2), Some(1), 0)
        );
        assert_eq!(keyboard_move(&layout, page(1), Step::Up), None);
        assert_eq!(keyboard_move(&layout, page(4), Step::Down), None);
        assert_eq!(keyboard_move(&layout, page(3), Step::Down), None);
    }

    #[test]
    fn out_lands_just_after_the_folder_it_left() {
        let layout = tree();
        assert_eq!(
            keyboard_move(&layout, page(2), Step::Out),
            moved(page(2), None, 2)
        );
        assert_eq!(
            keyboard_move(&layout, page(3), Step::Out),
            moved(page(3), Some(1), 2)
        );
        assert_eq!(keyboard_move(&layout, page(1), Step::Out), None);
    }

    #[test]
    fn in_joins_the_folder_just_above_at_its_end() {
        let layout = tree();
        // Page 4 sits just below folder 1, which already holds two rows.
        assert_eq!(
            keyboard_move(&layout, page(4), Step::In),
            moved(page(4), Some(1), 2)
        );
        // Nothing above, or a page above, cannot be entered.
        assert_eq!(keyboard_move(&layout, page(1), Step::In), None);
        assert_eq!(keyboard_move(&layout, dir(1), Step::In), None);
        assert_eq!(keyboard_move(&layout, page(2), Step::In), None);
    }

    /// `move_saved_row` lets an Alt arrow go on to the window (Alt Left is Back) exactly when
    /// no move is planned, so a top-level row never swallows Back.
    #[test]
    fn edge_steps_plan_nothing_so_the_key_reaches_the_window() {
        let layout = tree();
        for (entry, step) in [
            (page(1), Step::Out),
            (page(4), Step::Out),
            (dir(1), Step::Out),
            (page(1), Step::Up),
            (page(1), Step::In),
        ] {
            assert_eq!(
                keyboard_move(&layout, entry, step),
                None,
                "{entry:?} {step:?}"
            );
        }
        // Inside a folder, Alt Left is a real move and is consumed.
        assert!(keyboard_move(&layout, page(2), Step::Out).is_some());
    }

    #[test]
    fn unknown_rows_do_not_move() {
        let layout = tree();
        for step in [Step::Up, Step::Down, Step::Out, Step::In] {
            assert_eq!(keyboard_move(&layout, page(9), step), None);
        }
        assert_eq!(drop_move(&layout, page(9), DropTarget::TopLevel), None);
        assert_eq!(drop_move(&layout, page(1), DropTarget::Row(page(9))), None);
    }

    #[test]
    fn dropping_on_a_folder_puts_the_row_last_inside_it() {
        let layout = tree();
        assert_eq!(
            drop_move(&layout, page(1), DropTarget::Row(dir(2))),
            moved(page(1), Some(2), 1)
        );
        // Page 2 is already in folder 1; the count leaves it out.
        assert_eq!(
            drop_move(&layout, page(2), DropTarget::Row(dir(1))),
            moved(page(2), Some(1), 1)
        );
    }

    #[test]
    fn dropping_on_a_page_takes_its_place() {
        let layout = tree();
        assert_eq!(
            drop_move(&layout, page(4), DropTarget::Row(page(1))),
            moved(page(4), None, 0)
        );
        assert_eq!(
            drop_move(&layout, page(4), DropTarget::Row(page(3))),
            moved(page(4), Some(2), 0)
        );
        // Moving down past a sibling: the final position counts without the moved row.
        assert_eq!(
            drop_move(&layout, page(1), DropTarget::Row(page(4))),
            moved(page(1), None, 1)
        );
    }

    #[test]
    fn the_header_drop_moves_a_row_last_at_the_top_level() {
        let layout = tree();
        assert_eq!(
            drop_move(&layout, page(3), DropTarget::TopLevel),
            moved(page(3), None, 3)
        );
        assert_eq!(drop_move(&layout, page(4), DropTarget::TopLevel), None);
    }

    #[test]
    fn a_folder_cannot_be_dropped_into_itself_or_its_contents() {
        let layout = tree();
        assert_eq!(drop_move(&layout, dir(1), DropTarget::Row(dir(1))), None);
        assert_eq!(drop_move(&layout, dir(1), DropTarget::Row(dir(2))), None);
        assert_eq!(drop_move(&layout, dir(1), DropTarget::Row(page(3))), None);
        assert_eq!(drop_move(&layout, dir(1), DropTarget::Row(page(2))), None);
        assert_eq!(
            drop_move(&layout, dir(2), DropTarget::Row(page(1))),
            moved(dir(2), None, 0)
        );
    }

    #[test]
    fn a_drop_onto_its_own_place_changes_nothing() {
        let layout = tree();
        assert_eq!(drop_move(&layout, page(1), DropTarget::Row(page(1))), None);
        assert_eq!(drop_move(&layout, page(3), DropTarget::Row(dir(2))), None);
    }

    /// Drops of a folder onto anything inside it plan nothing, and the model refuses them too.
    fn assert_cycles_refused(
        model: &mut TabModel,
        rows: &[SavedEntryId],
        entry: SavedEntryId,
    ) -> usize {
        let mut refused = 0;
        for &target in rows {
            if target == entry || !within(model.saved_layout(), target, entry) {
                continue;
            }
            assert_eq!(
                drop_move(model.saved_layout(), entry, DropTarget::Row(target)),
                None
            );
            let destination = match target {
                SavedEntryId::Folder(folder) => Some(folder),
                SavedEntryId::Page(_) => parent_of(model.saved_layout(), target).flatten(),
            };
            let before = model.snapshot();
            assert!(matches!(
                model.move_saved_entry(entry, destination, 0),
                Err(syrinx_core::TabError::FolderCycle { .. })
            ));
            assert_eq!(model.snapshot(), before);
            refused += 1;
        }
        refused
    }

    /// Every computed move is one the model accepts and lands exactly where it said.
    #[test]
    fn computed_moves_are_accepted_by_the_model() -> Result<(), syrinx_core::TabError> {
        let mut model = TabModel::new();
        let mut pages = Vec::new();
        for number in 0..4 {
            pages.push(model.add_saved_page(
                syrinx_core::TabMetadata {
                    title: format!("Synthetic move {number}"),
                    address: format!("https://move-{number}.example.test/"),
                    ..syrinx_core::TabMetadata::default()
                },
                None,
            )?);
        }
        let outer = model.create_folder("Synthetic outer".to_owned(), None)?;
        let inner = model.create_folder("Synthetic inner".to_owned(), Some(outer))?;
        let rows: Vec<SavedEntryId> = pages
            .iter()
            .map(|&id| SavedEntryId::Page(id))
            .chain([SavedEntryId::Folder(outer), SavedEntryId::Folder(inner)])
            .collect();
        let steps = [Step::Up, Step::Down, Step::Out, Step::In];
        let (mut applied, mut refused_cycles) = (0, 0);
        for round in 0..200 {
            let entry = rows[round % rows.len()];
            let target = rows[(round * 7 + 3) % rows.len()];
            refused_cycles += assert_cycles_refused(&mut model, &rows, entry);
            let planned = if round % 3 == 0 {
                drop_move(model.saved_layout(), entry, DropTarget::Row(target))
            } else {
                keyboard_move(model.saved_layout(), entry, steps[round % steps.len()])
            };
            let Some(planned) = planned else {
                continue;
            };
            model.move_saved_entry(planned.entry, planned.parent, planned.index)?;
            applied += 1;
            assert_eq!(
                current_place(model.saved_layout(), planned.entry),
                Some((planned.parent, planned.index))
            );
            // The core's own snapshot validation checks the whole tree after every move.
            TabModel::from_snapshot(model.snapshot()).expect("the tree stays valid");
        }
        assert!(applied >= 60, "only {applied} moves were applied");
        assert!(refused_cycles > 0, "no cycle refusal was exercised");
        Ok(())
    }
}
