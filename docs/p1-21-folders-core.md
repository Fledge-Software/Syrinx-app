# P1-21 saved-page folders: headless core and storage

Status: **partial, headless part done** on branch `claude/p1-21-folders-core`.
The owner approved running Plan 1 slices in parallel on 2026-10-11, so this
slice proceeded while P1-20 and earlier slices are still open; that approval
is the exception to the strict phase order in `AGENTS.md`.
This note records what the core model and store now do for inline folders,
the decisions that had to be inferred, and the UI and search work that
remains before P1-21 can be called complete.

## Why

D43 makes saving a page create a persistent inline sidebar row, and says
saved pages *and folders* live inline beside ordinary tabs. P1-02 and P1-03
built the flat foundation; P1-21 adds the folders. Without them the owner
cannot organise saved destinations, and P1-22's HTML bookmark import has
nowhere to put the hierarchy it must preserve (D14/D43). The plan's rule is
that there is one model: the sidebar and palette are projections of the
records in `atlas-core`, never a second bookmark store.

## The model (`crates/atlas-core`)

The saved tree is one ordered list, `TabModel::saved_layout()`, of
`SavedPlacement { entry, parent }` values in visible depth-first order. An
entry is a `SavedEntryId::Page(SavedPageId)` or a
`SavedEntryId::Folder(SavedFolderId)`; `parent` is the enclosing folder, or
`None` at the top level. A depth-first list with parent links fully defines a
tree, including how folders and pages interleave among siblings, which an
import must preserve.

The page and folder records (`saved_pages()`, `saved_folders()`) are kept in
the same order as the layout, so `saved_pages()` is still the visible order
of pages and every earlier caller keeps working. One function,
`sync_saved_order`, re-derives that order and the order of open saved tabs
from the layout after each change, using stable sorts so no live tab changes
identity, selection or association. The rule that open saved tabs form a
prefix in saved-row order (D42/D43) therefore still holds with folders.

Operations (all validate before they change anything, and failures leave the
model untouched):

| Operation | Behaviour |
| --- | --- |
| `create_folder(title, parent)` | Appends an empty folder to `parent`'s children. Folder IDs are never reused. |
| `rename_folder(id, title)` | Changes only the title. |
| `add_saved_page(metadata, parent)` | Creates an unloaded saved page at the end of `parent`, opening nothing (for P1-22 import and "new bookmark"). |
| `edit_saved(id, metadata)` | Explicitly edits a saved title/address. The open page, if any, keeps its own live state; D43 forbids only *navigation* rewriting the saved record. |
| `move_saved_entry(entry, parent, index)` | Moves a page, or a folder with everything inside it, to a final zero-based position among `parent`'s children. A folder moved into itself or a descendant is refused with `TabError::FolderCycle`. |
| `move_saved(id, index)` | The P1-02 operation, now meaning "reorder within the current folder"; identical to before when there are no folders. |
| `remove_folder(id, removal)` | `FolderRemoval::MoveContentsUp` lifts the children into the folder's place, in order. `FolderRemoval::RemoveContents` removes every nested folder and page. |
| `remove_saved(id)` | Unchanged semantics: an open page stays open as an ordinary tab at the start of the ordinary group, and closed history can no longer resurrect the row. |

Moving a saved page never touches its live tab: the tab keeps its identity,
metadata, active state and association, and `open_saved` still focuses it
rather than opening a duplicate. When `RemoveContents` deletes pages that are
open, each open page follows `remove_saved`'s rule, and several such tabs keep
their visible order at the start of the ordinary group.

`sidebar_rows()` now yields `SidebarRow::Folder { folder, parent }` rows and
gives `SidebarRow::Saved` a `parent`, in layout order, followed by ordinary
tabs. Expanded/collapsed state is presentation and is not modelled.

`TabModelSnapshot` gains `saved_folders`, `saved_layout` and
`last_saved_folder_id`. `validate()` rejects, without repairing: duplicate
folder IDs, folder IDs above their watermark, records not in layout order
(`SavedLayoutMismatch`), records with no layout entry
(`SavedLayoutIncomplete`), and any entry whose parent is not an enclosing
folder that appeared earlier (`InvalidSavedParent`). That last check is what
makes cycles, self-parenting, unknown parents and subtrees split apart
unrepresentable. Folder titles are redacted from every `Debug` output, like
page metadata.

## Storage (`crates/atlas-store`)

**Migration number: schema version 2** (`user_version = 2`), added as the
second entry of `schema::MigrationScripts::migrations()`. It is
self-contained: it creates three new tables and fills them, and alters no
existing table, so another slice's migration can follow it as version 3. If
P1-12 (permissions) also wrote a "version 2", whichever merges second must
renumber to 3; the two do not touch the same tables.

| Table | Holds |
| --- | --- |
| `saved_folders` | Folder ID and title. |
| `saved_layout` | One row per page or folder: `entry_order` (depth-first position, the primary key), exactly one of `page_id`/`folder_id`, and `parent_folder_id`, all with foreign keys. |
| `saved_folder_state` | The single-row folder ID watermark, so removed folder IDs stay retired across restarts. |

The existing tables, including `saved_pages.saved_order`, are unchanged.
**The page order is therefore stored twice**: `saved_pages.saved_order` (the
order of pages alone) and `saved_layout.entry_order` (pages and folders
together). Neither is repaired from the other. The reader loads pages by
`saved_order` and the layout by `entry_order`, and core validation refuses
the snapshot (`SavedLayoutMismatch`) unless the pages appear in the same order
in both. Keeping `saved_order` avoids rebuilding `saved_pages` in the
migration; a later schema could drop it once nothing depends on it. New tables were chosen over `ALTER TABLE`
because the store matches the stored `CREATE TABLE` text exactly, and a
rebuilt `saved_pages` would need foreign keys switched off inside the
migration transaction.

Upgrade path: a version 1 database is recognised as
`SchemaGeneration::FlatSaved`, its exact schema and records are validated
first (so damaged data is refused before journal mode or schema change, as
before), and then the migration places every existing page at the top level
in its existing order. The migration runs in one transaction with a foreign
key check and a hook that re-reads and validates the whole result, so any
failure rolls back to the intact version 1 file. Read-only backup validation
accepts a version 1 backup as a flat tree without migrating it; restoring one
lets the next writer upgrade it, so D46 recovery is not broken by the new
schema. Reader connections still require the current schema.

Backups written by a newer schema are now skipped rather than fatal. Daily
rotation (`atlas-store/src/backup.rs`) skips them like invalid candidates and
never prunes them; recovery (`atlas-platform`) offers the newest backup it can
use and, if every candidate is newer, reports
`RecoveryError::OnlyNewerSchemaBackups { skipped }` instead of
`NoValidBackup`. This keeps a future version 2 → 3 downgrade from locking the
owner out of older backups.

### Known limitation: downgrading below this branch

Builds that predate this branch cannot use this fix. Their backup listing
(`atlas-store/src/backup.rs`, the `valid_candidates` loop) returns the first
error that is not an "invalid candidate", and their recovery listing does the
same, so `FutureSchema` from a single version 2 daily snapshot stops the whole
scan. Once this build has written one version 2 snapshot, running an older
build cannot rotate backups or list or restore any version 1 snapshot. The
database itself also refuses to open in the older build (`FutureSchema`), as
intended. Downgrading is therefore unsupported; recovering in that situation
means running this build or a later one.

One small `atlas-platform` change follows from the larger snapshot: the
private `RecoveryOffer::expected` field is now boxed, because the two new
vectors pushed `StartupStorage` over Clippy's `large_enum_variant` limit.
Behaviour and public API are unchanged.

## Gate

`cargo xtask check` passed on this machine after the review fixes (458 tests, smoke run included);
the log is `target/p1-21-gate.log`. No Application Control (os error 4551)
block occurred.

## Tests

All data is synthetic (`example.test` addresses, made-up titles) and every
database is disposable.

- `crates/atlas-core/tests/saved_folders.rs`: creation, rename and listing;
  moving an open saved page keeps its instance and selection; folders move
  with their subtrees and cycles are refused without change; final-position
  reordering and bad destinations; both removal modes, including open pages
  becoming ordinary and closed history not resurrecting rows; explicit edits;
  snapshot rejection of out-of-order, incomplete, cyclic, self-parented and
  escaped trees; folder ID exhaustion; redaction; and a 500-step mixed
  sequence checking no cycles, no orphans, stable order, projection and
  snapshot round trips after every step.
- The mixed sequence also saves, closes and reopens saved tabs while folders
  exist, and a dedicated test checks that moving a folder holding open tabs,
  and dissolving it with `MoveContentsUp`, leaves live tab IDs, associations
  and the active tab unchanged.
- `crates/atlas-store/src/backup/tests.rs`: a newer-schema backup is skipped
  by a fresh rotation scan and never pruned.
- `crates/atlas-platform/src/storage/recovery/tests.rs`: a newer-schema
  newest backup is skipped (left byte-identical) in favour of an older one,
  only-newer candidates are reported as such, and a version 1 backup restores
  and is migrated by the next store open. `rusqlite` became a dev-dependency
  of `atlas-platform` for that downgrade fixture (no new package).
- `crates/atlas-store/src/engine/tests/folders.rs`: upgrade from version 1
  with existing flat rows (order, revision, association preserved) followed by
  folder edits and restarts; damaged version 1 rows refused with the file
  unchanged; migration rollback for both a hook failure and a foreign key
  failure; tree round trips across restarts and both removal modes, with no
  rows left behind; folder watermark regression; damaged tree rows refused;
  and version 1 backups validating as flat trees.

## Decisions inferred (for the owner)

1. **Removing a folder: the API offers both behaviours and has no default.**
   The plan and decisions are silent. Moving contents up is the safer choice
   because it never deletes a saved page; removing contents is needed for an
   explicit "delete folder and everything in it" and for import cleanup.
   Recommendation: the UI's default "Remove folder" moves contents up, and
   deleting contents requires an explicit, confirmed choice. The owner should
   confirm before P1-21's UI ships.
2. **New folders and added pages go at the end of their parent**, matching
   D43's "saving appends". Saving a live page still appends at the top level.
3. **`move_saved` reorders within the page's current folder.** Moving between
   folders uses `move_saved_entry`.
4. **Explicit edits of a saved title/address are allowed** and leave the open
   page alone; only navigation is forbidden from rewriting saved records.
5. **Empty folder titles are accepted**; whether the UI requires a name is a
   UI decision.

## What remains for P1-21

The sidebar UI, the session routing and the model's refusal of empty names
were built on `claude/p1-09-21-folder-ui`; see `docs/p1-09-21-folder-ui.md`.
Inferred decision 5 above is superseded by the owner's 2026-10-12 decision: empty
names are refused. The list below is kept as the original hand-off.

- **UI (`atlas-ui`/`atlas-app`)**: render folder rows with indentation and
  expand/collapse, create/rename/remove/move commands, drag-and-drop with the
  final-position semantics above, the removal confirmation, and the D44
  compact-folder controls, which the owner reviews. The session worker must
  route the new operations and persist them.
- **Search (`atlas-search`, untouched here)**: see below.
- **Restart consistency in the running app**: the store round trips are
  proved headlessly; P1-19-style acceptance with the real app remains.

### What search indexing needs for folders

- Saved-page targets are unchanged (`Target::Saved(SavedPageId)`), so moving a
  page needs no reindex of its own text; a rename or explicit edit of a saved
  page must update its catalogue entry, which the existing sync already does
  for metadata changes.
- Decide whether folders are searchable rows. If they are, add a folder
  target (`Target::Folder(SavedFolderId)`) whose activation reveals/expands it
  rather than opening anything.
- Decide whether a page's folder path ("Work › Research") is shown in results
  or matched by queries. If matched, folder renames and moves change the
  searchable text of every page beneath them, so the catalogue sync must diff
  the layout as well as page metadata.
- `RemoveContents` deletes many pages at once; the catalogue sync must remove
  all of them in one pass, and P1-22's import adds many at once.
