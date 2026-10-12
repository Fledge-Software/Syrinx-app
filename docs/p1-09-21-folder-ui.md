# P1-09 and P1-21: folder editing in the sidebar, and remembered folder expansion

Status: **implemented on branch `claude/p1-09-21-folder-ui`, awaiting independent
review and the owner's look at the running window.** The branch starts from
`codex/integrated-browser-preview` at `94b3cb1`. It finishes the sidebar half
of P1-21 (the headless model and store were done earlier, see
`docs/p1-21-folders-core.md`) and closes P1-09's "folder expansion is not
persisted" gap. Search treatment of folders is still open (see "What remains").

## Why

D43 puts saved pages *and folders* inline in the sidebar, and P1-22's import
already fills the tree with folders. Until now the owner could see folders and
open or close them, but could not create, rename, remove or rearrange
anything, and every folder came back collapsed after a restart. Without this
slice, an imported bookmark tree can never be tidied, and a newly saved page
cannot be filed anywhere. The plan's rule still holds: the sidebar and the
palette read the one tab model in `atlas-core`, so every edit here is a model
command on the session worker, saved in the same store transaction as any
other tab change.

## What changed

### The model refuses unnamed folders (`crates/atlas-core`)

The owner decided on 2026-10-12 that empty folder names are refused, and that
the model must refuse them too, not only the form. `TabError::EmptyFolderTitle`
is new. `create_folder`, `rename_folder` and P1-22's `append_saved_folder`
(for the import's top folder and every folder inside it) all call one check,
`require_folder_title`, before they change anything. A name that is empty or
only whitespace (including the ideographic space) is refused; surrounding
spaces in an otherwise real name are kept as given. Import was already safe:
its commit step names an unnamed exported folder "Untitled folder".

Restored snapshots are **not** re-validated for names. A database written by
an earlier build could, in principle, hold an empty folder name (only tests
created one), and refusing it at start-up would lock the owner out of the
whole store. The sidebar draws such a folder with no title; renaming it is
the fix.

### One session command for folder edits (`crates/atlas-app/src/session/folders.rs`)

`SessionCommand::Folder(FolderCommand)` carries the four edits:
`Create { title, parent }`, `Rename { id, title }`, `Remove { id, removal }`
and `Move { entry, parent, index }`. They go through the existing
`apply_model` path, so a refused edit is a recoverable error that changes
nothing, and an accepted one is saved in one transaction and published as the
next state, exactly like opening or closing a tab. `Debug` output shows only
the kind of edit, never a folder name. Folder edits do not cancel a pending
Home palette action (`ui/home_palette.rs`), because they do not change which
page is shown.

### The sidebar's controls (`crates/atlas-app/src/ui/folder_editor.rs`, `sidebar_view.rs`)

In the **wide** sidebar:

- **New folder.** A "New folder" button sits on the "SAVED & OPEN TABS" header,
  beside the existing "+" (new tab). It opens a small naming form under the
  header, in the tile editor's card style (`atlas-ui` surface, line, radius and
  type tokens; the tile editor's `form_button` is now shared rather than
  copied). Enter or Save creates the folder at the end of the saved group;
  Cancel closes the form. A blank name shows "Give the folder a name." and
  keeps the form open. New folders start expanded.
- **Rename.** Each folder row has a "✎" action (named "Rename folder"), and F2
  on a focused folder row does the same. The form is filled with the current
  name, selected.
- **Remove.** Each folder row has a "−" action (named "Remove folder"), like the
  saved-page "−". An empty folder is removed at once, because nothing can be
  lost. Otherwise a card asks: "Remove “Name”? Its 3 saved pages and 1 folder
  will move up one level." with **Remove** (the default, also Enter), **Delete
  contents…** and **Cancel** (also Escape). Choosing Delete contents changes
  the card to an explicit confirmation, "Delete “Name” and everything in it?
  … will be deleted. Open pages stay open as ordinary tabs.", with **Delete**
  and **Cancel**. Only that second step sends `FolderRemoval::RemoveContents`.
  This is the owner's 2026-10-12 decision: moving contents up is the default,
  deleting them is a separate choice that asks first.
- **Move by dragging.** Any saved page or folder row can be dragged. Dropped on
  a folder, it becomes that folder's last row. Dropped on a saved page, it
  takes that page's place (the page and everything after it move down).
  Dropped on the "SAVED & OPEN TABS" header, it becomes the last top-level
  saved row, which is how a row leaves every folder with the mouse. A row
  under the pointer that can take the drop is highlighted with the selected-
  row colour. A folder dropped on itself or on anything inside it is ignored
  (the model also refuses that as `FolderCycle`).

Keyboard, in every width where rows show:

| Key on a focused row | Effect |
| --- | --- |
| Tab / Shift Tab | Moves focus between rows and buttons; the focused row has an accent outline. |
| Enter or Space | Opens or focuses a saved page or tab (through `activate()`), or toggles a folder. |
| Alt Up / Alt Down | Moves the row one place among its siblings. |
| Alt Left | Moves the row out of its folder, just after that folder. |
| Alt Right | Moves the row into the folder just above it, as its last row (the folder opens). |
| F2 | Renames a folder (wide sidebar). |
| Delete | Removes a folder (asks as above when it is not empty), or removes a saved page, as its "−" does. |
| Shift Delete | Goes straight to the delete-with-contents confirmation for a folder. |

These keys are bound only in the focused row's key context (`AtlasSavedRow`),
which GPUI ranks above the window-wide bindings, so Alt Left keeps meaning
Back everywhere else. The form and card buttons, and "New folder", are tab
stops that Enter or Space presses.

The pure rules behind the moves live in `ui/saved_moves.rs`: `keyboard_move`
and `drop_move` turn a gesture into the model's final-position move, and
return nothing for a move to the same place, off an edge, or into a folder's
own contents. Its tests check every case and replay 200 mixed moves against a
real `TabModel`, asserting each lands exactly where it was planned.

### Activation and the peek

Clicking or pressing Enter on a saved page row goes through the same
`activate(ActivationTarget::Saved(...))` as before (`open_saved_row`), so it
focuses an open instance or loads the saved address. The P1-17 peek draws
`sidebar_at`, the same function as the docked sidebar, so it shows the same
rows, controls and form. A form or card is closed when the peek opens or
closes, as the Favorites editor already is, because a text field belongs to
the window it was created in.

### Remembered expansion (`crates/atlas-app/src/ui/folder_expansion.rs`)

P1-21's `saved_folder_state` table holds only the folder ID watermark, so
expansion does not belong there; it is presentation, and it now uses P1-29's
settings table. The key `sidebar.expanded_folders` holds the IDs of **open**
folders, space-separated in ascending order (for example `3 7 12`). Storing
the open ones rather than the closed ones keeps P1-22's rule: a folder the
setting does not name (an import, or anything after a lost write) starts
collapsed, so a large import never builds every row at start-up. A missing or
unreadable value means "none open".

After every committed state and after every toggle, the view compares the
open set with the stored one and saves only when they differ. That one rule
covers a toggle, a new folder (open until the owner closes it), a removed
folder (its ID is dropped; IDs are never reused) and an import (its folders
are already marked closed before its state arrives, so nothing is saved).
Moving a row into a closed folder by keyboard or drop opens that folder, so
the row stays in view.

## Owner decision applied

- 2026-10-12: removing a folder moves its contents up by default; deleting
  contents is a separate choice that asks for confirmation; empty folder
  names are refused, in the model as well as the form.

## Owner questions

1. **Compact (favicon-only) folders, D44.** This branch keeps the minimal
   behaviour from P1-09 and adds nothing visual: a folder is an icon button
   showing ▸ or ▾, its contents are not indented, and click or Enter toggles
   it. Keyboard moves and dragging work there too. Naming, renaming and
   removal need the wide sidebar (or the peek, which is always wide), because
   the form does not fit in 52 px; F2 and Delete on a folder do nothing in
   that width. Should compact folders look different (for example a folder
   glyph, a contents count, or a flyout listing the folder's rows), and
   should editing be possible there? Recommendation: keep this until the
   owner has used it, then decide on a flyout.
2. **Where "New folder" lives and what it creates.** It is a text button on the
   saved section's header and always creates a top-level folder; the owner
   then drags or Alt-Right-moves rows into it. Alternatives are a "New folder
   inside" action on each folder row, or creating it inside the focused
   folder. Recommendation: keep the single top-level button for now.
3. **The "✎" glyph for Rename.** It is a text glyph like the existing "×" and
   "−", not a new icon set. If the owner prefers no visible rename control,
   F2 and the form remain.
4. **Deleting a saved page from the keyboard.** Delete on a focused saved page
   removes the saved row without asking, exactly as its "−" button already
   does (the live page stays open as an ordinary tab). Should a single saved
   page's removal also ask?
5. **Drop position.** Dropping onto a page inserts before it; there is no
   "after the last row of a folder" drop zone other than dropping on the
   folder itself. Is that enough, or should the lower half of a row mean
   "after"?

## Owner's manual checks

Run `cargo run --package atlas-app --profile local --offline --locked`
against a fresh synthetic profile (for example
`-- --test-data-dir target\folder-ui-check`), open two or three pages (for
example `https://example.com`) and save them with Ctrl D or the palette.

1. **Create.** Click "New folder" on the SAVED & OPEN TABS header, type a
   name, press Enter: a folder row appears, open (▾), after the saved pages.
   Try an empty or all-spaces name: the form says "Give the folder a name."
   and stays open.
2. **Move by dragging.** Drag a saved page onto the folder: it moves inside,
   indented. Drag it onto the header: it leaves the folder and goes last
   among saved rows. Drag a page onto another page: it takes that place.
   Drag the folder onto a page inside itself: nothing happens.
3. **Move by keyboard.** Tab to a saved row (accent outline), then Alt Right
   under a folder, Alt Up/Down, Alt Left. Make sure Alt Left on a page with
   focus in the web page still goes Back.
4. **Rename.** Click ✎ or press F2 on a focused folder, change the name, Enter.
5. **Remove.** Click − on a folder holding pages: the card offers Remove,
   Delete contents… and Cancel. Remove keeps the pages, one level up. Repeat
   with Delete contents… then Delete: the pages' saved rows go, and any that
   were open stay open as ordinary tabs below the saved group.
6. **Expansion survives restart.** Leave one folder open and one closed,
   quit Atlas, start it again with the same `--test-data-dir`: the same folder
   is open and the other closed. Import a bookmarks file with folders
   (P1-22): its folders stay closed after a restart until opened.
7. **Peek and compact width.** Hide the sidebar (Ctrl Alt S) and hover the left
   edge: the peek shows the same rows and buttons. In favicon-only width,
   folders toggle on click and rows can still be dragged.
8. **Narrator.** Rows are announced as tree items with their titles and
   levels; folders announce expanded/collapsed; the buttons are named.

## Verification

All test data is synthetic (`example.test` addresses, made-up titles) and each
worker test uses a disposable directory.

- `crates/atlas-core/tests/saved_folders.rs`: blank names refused by create,
  rename and subtree append without changing the model; spaced names kept.
- `crates/atlas-app/src/session/folders.rs`: each edit reaches the model,
  deleting contents leaves open pages as ordinary tabs, refused edits change
  nothing, and diagnostics redact names.
- `crates/atlas-app/src/session/folder_tests.rs`: a real worker creates,
  moves, renames, refuses an unnamed folder as a non-fatal error, stores the
  expansion setting, and after a restart restores the tree, the live tab's
  association and the setting; both removal modes survive another restart.
- `crates/atlas-app/src/ui/saved_moves.rs`: keyboard steps and drops,
  including edges, cycles, same-place drops and a 200-move replay against the
  model.
- `crates/atlas-app/src/ui/folder_expansion.rs`: the setting's format and
  round trip, refusal of unreadable values, only stored folders reopening,
  new folders counting as open, and a missing setting collapsing everything.
- `crates/atlas-app/src/ui/folder_editor.rs`: name trimming and refusal, and
  the removal card's counts.

On 2026-10-12 the full `cargo xtask check` passed on this branch: dependency
boundaries, deferred-work markers (none added), packaging, formatting, Clippy
with warnings denied, all 868 workspace tests and the `--smoke` application
run, with no Application Control (os error 4551) block. The log is
`target/p1-09-21-gate.log`. The agent could not look at the window, so the
owner's manual checks above are still needed.

## What remains

- **Search treatment of folders** (P1-21's other half): whether folders are
  palette rows and whether a page's folder path is matched. See
  `docs/p1-21-folders-core.md`, "What search indexing needs for folders".
  Saved-page search is unaffected by moves and folder renames.
- **Running-app restart acceptance** with the real window (the owner's checks
  above); this agent could not look at the window.
- **Compact-folder design** after the owner answers question 1 (D44).
