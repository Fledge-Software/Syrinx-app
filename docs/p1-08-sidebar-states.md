# P1-08 and P1-09: sidebar width states and folder rows

Status: **implemented on branch `claude/p1-08-sidebar-states`, awaiting the
owner's look at the running window and review.** This covers the sidebar part
of P1-08 (D44 width dragging, with the page card following) and the folder
rows of P1-09. The owner approved running Plan 1 slices in parallel on
2026-10-11; this work ran beside the palette, home, history and import slices
and stays out of their files.

## Why

D44 asks for a sidebar the owner can size by dragging: wide with labels,
favicon-only, or hidden, with a reverse drag restoring it. Until now the
sidebar was a fixed 264 px column (`SIDEBAR_WIDTH` in
`crates/atlas-app/src/ui.rs`), so the page could never use that space. The
page card is a separate native WebView2 window, which does not follow the
GPUI layout on its own (`docs/trapstoknow.md`, "Child web views do not follow
the window's size"); if the card is not moved with the sidebar on every
change, the page either overlaps the sidebar or leaves a gap.

P1-21 added saved-page folders to the core model and store, but the sidebar
still drew saved pages as one flat list. P1-09 renders the saved tree, so
folders that an import or a later editing UI creates are visible and can be
expanded and collapsed.

## What changed

**The drag state machine (`crates/atlas-ui/src/sidebar.rs`).**
`SidebarSizing` holds the settled `SidebarWidthState` plus any drag in
progress. `begin_drag` records where the pointer gripped the edge so the
sidebar does not jump; `drag_to` follows the pointer, clamped to 0–264 px;
`release` snaps with the thresholds P1-07 proposed and the owner reviewed in
the sample window: 158 px or wider settles on wide, 26 px or wider on
favicon-only (52 px), anything narrower hides it. `labels_visible` is true
exactly when a release at the current width would settle on wide, so titles
appear and disappear at the same point in both drag directions. The type has
no toolkit dependency and is unit-tested (snap boundaries, label rule,
forward and reverse drags through every state, click without movement,
non-finite pointer values).

**The page card (`crates/atlas-app/src/ui/page_card.rs`).** One pure
function, `page_card_bounds`, computes the web view's physical-pixel bounds
from the window's logical size, its scale and the sidebar's current width,
inset by the `CARD_INSET` token. It rounds each edge rather than each size,
so the card's right edge stays put while a fractional drag moves its left
edge. `BrowserView::resize_pages` calls it from every render, and GPUI
renders on every drag step, window resize and DPI change, so the card
follows all three. Unchanged bounds are not resent. Tests cover each state,
scaling, the fixed right edge and tiny or invalid windows. `SIDEBAR_WIDTH` is
gone; the sidebar reads `atlas_ui::sidebar` tokens.

**The edge handle and drawing (`crates/atlas-app/src/ui/sidebar_view.rs`).**
An 8 px strip between the sidebar and the page card (the card's inset, so it
never covers the page) shows a resize cursor and is announced to screen
readers as "Resize sidebar". Pressing it starts the drag. GPUI's Windows
backend captures the mouse on press, so moves over the page's web view still
reach Atlas; the root element follows them and ends the drag on release
inside or outside the window, and a move with no button held also ends a
drag whose release was lost. The sidebar function moved here from `ui.rs`.

Review fixes (2026-10-12):

- **The release position decides the snap.** GPUI stops reporting moves once
  the pointer leaves the window, so a flick left past the window edge used to
  settle on the last in-window width (often favicon-only). `release_at` now
  applies the mouse-up position before snapping.
- **The strip and the card share one edge calculation.** Below the
  favicon-only width the sidebar draws only its unpadded background
  (`atlas_ui::sidebar::draws_content`), so the 12 + 10 px padding can no
  longer hold it at 22 px while the card assumes the real width. The strip's
  span and the card's left edge both come from `page_card::edge_strip`; a
  test checks they never overlap at any width from 0 to 264 px and five
  scales.
- **A drag cannot outlive the window's activation.** Alt Tab or a dialog
  (for example a WebView2 prompt) takes the mouse capture and the release
  with it, so a window-activation observer settles any drag in progress at
  its current width. A capture loss without deactivation still ends at the
  next move with no button held.
- **Only the shown page follows a drag.** `atlas-platform` resizes the
  visible web view on each step; hidden pages receive the card's bounds once,
  just before they are shown (`browser/placement.rs`, tested).
- **Keyboard and roles.** The edge is a focusable splitter (tab stop) named
  "Resize sidebar", described with its current state; Left narrows and Right
  widens it by one state. All rows are tree items with a nesting level inside
  one "Saved and open tabs" tree, and folders report whether they are
  expanded. The Ctrl Alt S pin shortcut is P1-17's (DW-3).

- **Wide** draws as before, with titles truncated while a drag narrows it.
- **Favicon-only** draws each row as a 30 px icon button. Rows have no
  favicons yet, so each shows a monogram (the title's first letter or digit),
  accent-coloured for saved pages, as the spec's placeholder tiles do. Every
  row, the search button, the plugin placeholders and the new-tab button keep
  their names as accessible labels (`aria_label`), and rows report their
  selection. Close, unload and remove buttons are not drawn in this width;
  Ctrl W still closes the active tab.
- **Hidden** draws nothing and reserves no width. The selection is untouched:
  the width state never changes the model's active tab, and the toolbar's
  back, forward, reload and address field stay above the page.

**Folder rows (`crates/atlas-app/src/ui/sidebar_rows.rs`).**
`visible_saved_rows` walks the snapshot's depth-first `saved_layout` once,
giving each row its depth and skipping everything inside a collapsed folder.
The wide sidebar indents each level by 12 px; a folder row shows ▾ or ▸,
reports `aria_expanded` and toggles on click. Collapsing is view state in
`BrowserView::collapsed_folders`, not a model change, and every folder starts
expanded. No folder editing UI was added; that remains P1-21's.

## Decisions made here, for review

- **Width state was kept in memory on this branch.** `atlas-store` had no
  settings table yet, so the branch tracked persistence as DW-2 in the plan's
  "Deferred work" register; P1-29 added the table. At the integration merge
  (2026-10-12) a drag release or key step that changes the state saves
  `sidebar.width_state`, and start-up restores it; a missing or unreadable
  value keeps wide. DW-2 is Done. Folder expansion is still in memory only.
- **Favorites follow the width.** P1-29's Favorites sit below the search
  button. Wide shows the four-column grid with its editor; favicon-only shows
  one column of named icon tiles, without the add tile or editor; hidden draws
  none.
- **Favicon-only folders.** D44 leaves compact-folder affordances for later
  review. Until then a folder in the favicon-only width is an icon button
  with its arrow, still toggling, and its contents are not indented.
- **Page focus after a drag is unchanged.** Pressing the handle activates
  Atlas's own window, as clicking any sidebar row already does. Returning
  keys to the page after a drag belongs to P1-10's focus work.
- **The hidden sidebar's handle and P1-17's peek share the left edge.** With
  the sidebar hidden, the handle occupies x 0–8 px, which is also the
  proposed 8 px peek zone. P1-17 must make hover peek and press-to-drag
  coexist there (for example, peek on hover, drag on press).

## Verification

Unit tests: `atlas-ui` (drag state machine) and `atlas-app` (`page_card`,
`sidebar_rows`). On 2026-10-12 the full `cargo xtask check` passed on this
branch: dependency boundaries, registered deferred-work markers (DW-2),
formatting, Clippy with warnings denied, all tests (96 in `atlas-app`, 36 in
`atlas-ui`) and the `--smoke` application run. An earlier Clippy run failed
once with "can't find crate for `strum_macros`" while other agents were
building on the machine; the unchanged retry passed. After the review fixes
the full gate passed again on 2026-10-12, with 97 tests in `atlas-app`, 39 in
`atlas-ui` and 73 in `atlas-platform` (including `placement`). The agent
could not look at the window, so the owner's manual checks follow.

## Owner's manual checks

Run `cargo run --package atlas-app --profile local --offline --locked`, open a
page (for example `https://example.com`), and then:

1. **Drag through all states.** Press the thin strip between the sidebar and
   the page card (the cursor becomes a resize arrow) and drag left slowly.
   The page card's left edge should follow the pointer with no gap or
   overlap. Titles should disappear at about 158 px. Release at about 100 px:
   the sidebar should settle on the 52 px favicon-only column with letter
   icons. Drag again to about 10 px and release: the sidebar should hide and
   the page card should start 8 px from the window's left edge.
2. **Reverse drag.** From hidden, press the strip at the window's left edge
   and drag right across the page. The sidebar should grow over where the
   page was, and the card should follow. Release at about 40 px for
   favicon-only, then drag again past 158 px for wide with labels. Also try
   hidden straight to wide in one drag.
3. **Release outside the window.** From wide, flick the strip quickly left
   past the window's left edge and release there: the sidebar should hide,
   not stop at favicon-only. Flick right past the window's right edge from
   hidden: it should settle wide. Start a drag and press Alt Tab mid-drag:
   the sidebar should settle and stop following the mouse.
4. **Keyboard.** Press Tab until the strip is highlighted (or click it), then
   use Left and Right: it should step wide, favicon-only, hidden and back,
   with the page card following.
5. **Selection survives.** With a page selected, hide the sidebar and bring it
   back; the same row should still be highlighted and the same page shown.
6. **Resize and DPI.** In each of the three states, resize the window from
   its edges and maximise/restore it: the page card should stay 8 px from the
   sidebar and the window edges. If a second monitor with a different scale
   is available, drag the window across and check the same.
7. **Accessible names.** In favicon-only, hover or inspect with Narrator: rows
   should be announced by their page titles, not their letters.
8. **Folders.** Only if the store already holds folders (for example from a
   later import): folders should show indented contents in the wide sidebar
   and collapse/expand on click.
