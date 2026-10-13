# P1-30 and P1-31 command palette: what was built and what the owner checks

Plan 1's slices P1-30 and P1-31 turn the Ctrl K entry popup from P1-10 into
the command palette the spec puts at the centre of Syrinx (S5, spec section 3).
Before them, the popup searched open tabs and saved pages and could navigate
to an address, but it could not run a single command, had no scope chips,
showed no matched characters and offered no preview. Browser commands lived
only as hand-written key bindings in `crates/syrinx-app/src/ui.rs`, so a later
plugin would have had nowhere to register its own. This note records what the
two slices deliver, the choices made and what the owner needs to try in the
running app, because a passing gate does not prove that focus returned to a
page or that a highlight is drawn in the right place.

## P1-30: one command source and the full palette

**The command registry** (`crates/syrinx-core/src/commands.rs`) is the single
list of things Syrinx can run. A command has an identifier
(`browser.new-tab`), a title, optional search keywords ("refresh" finds
Reload), a scope (browser or a named plugin), an enablement rule, an action
and an optional shortcut. The action is an enum: `Browser(BrowserAction)` for
the built-in commands and `Plugin { plugin, action }` for the plugin host that
Plan 2 builds. The registry rejects malformed identifiers, empty titles, a
duplicate identifier and, importantly, a second command claiming the same
shortcut, so one keystroke can never run two commands depending on
registration order. Shortcuts are normalized first (lowercase, modifiers in
one fixed order, `cmd`/`super` read as `win`), so `Shift-Ctrl-T` conflicts
with `ctrl-shift-t`. The palette's own keys are reserved: Ctrl K, Esc, Up,
Down, Enter, Alt Enter, Ctrl Enter and Tab can never be claimed, and Ctrl L
belongs only to Edit address. It is generic on purpose; it is not a plugin
host and no plugin can reach it yet.

The eleven existing browser commands are registered with the same shortcuts as
before: New tab (Ctrl T), Close tab (Ctrl W), Reopen closed tab (Ctrl Shift
T), Reload page (Ctrl R), Go back (Alt Left), Go forward (Alt Right), Edit
address (Ctrl L), Next and Previous tab (Ctrl Tab, Ctrl Shift Tab), Save
page (Ctrl D) and, since P1-20 merged, Open history (Ctrl H), which runs the
history view's `toggle_history`. `ui::bind_keys` now derives every GPUI key binding from the
registry instead of listing them again; Ctrl K stays a separate binding
because the palette is where commands run, not a command itself. The native
accelerator router in `syrinx-platform` still maps the same keys for a focused
page; it belongs to another slice and was not changed.

The palette sample window (`--palette-sample`) was not registered as a
command: it is a development review window that quits the application when it
closes, so opening it from the running browser would close Syrinx.

Enablement uses a small `CommandContext` (is a tab selected, is a web page
showing, how many tabs are open). Disabled commands are hidden rather than
shown greyed out: on Home, typing "reload" finds nothing instead of offering
a row that cannot run.

**The palette itself** (`crates/syrinx-app/src/palette.rs` and
`palette/`) is still P1-10's single owned popup window; no second overlay
mechanism was added (D7). It now has:

- registry-backed command rows, ranked by the same matcher as everything else
  (`palette/commands.rs`);
- the spec's prefixes (`palette/query.rs`): `>` lists commands, `@` lists
  plugin actions, `/` lists open tabs. Only the very first character counts,
  so `a/b` or an address stays ordinary text. With no plugins installed, `@`
  shows a "No plugins yet" empty state;
- keyboard selection with Up and Down (the list scrolls to keep the selection
  visible), Enter to run or open the selected row, Esc to close and Alt Enter
  for Search the web, as before. After a prefix, Enter only runs or opens a
  listed row: `>rel` never becomes a web search for ">rel", and a prefix that
  matches nothing does nothing. Alt Enter is still an explicit web search;
- focus restoration through the existing path: after a command, an activation
  or Esc, the main window is activated and the shown page is focused. Edit
  address is the one exception: it reopens the entry popup with the current
  address, which keeps focus.

Commands are matched on the palette's own thread, not on the session worker.
Their enablement depends on what the window is showing, which only the UI
thread knows, and the registry is small and bounded (eleven built-in commands;
plugin commands arrive with Plan 2), so each keystroke ranks a few dozen
entries. Tab, saved-page and history search stays on the worker as before.
The alternative, indexing commands in the worker's catalogue, was rejected
because the worker cannot judge enablement and because its single command
count cannot tell browser commands from plugin actions, which the Plugin
actions chip needs. Nothing yet limits how many commands a plugin may add, so
the deferred-work register's DW-5 (owned by Plan 2, phase 1) requires capping
the registry or moving this ranking off the UI thread before plugins can
register commands; a `TODO(DW-5)` marks the ranking site.

## P1-31: chips, highlights, previews and one activation path

**Scope chips with counts.** Everything, Tabs, Plugin actions, Bookmarks and
History sit under the input, each with its count for the current text. The
tab, bookmark and history counts come from the worker's `ScopeCounts`, which
count every match before the display limit; the Plugin actions count comes
from the local command match. History rows and counts come from P1-20's
history in the worker's catalogue, and choosing a history row opens its address
as the history slice does. Everything includes commands only once text is typed: a blank
Ctrl K lists open tabs and bookmarks, and `>` lists every command. Choosing a
chip removes a typed prefix, because the prefix would otherwise override it.

**Highlights.** Matched characters are drawn in the accent ink at semibold
weight, as on the Palette board. `syrinx-search` already widens the matcher's
byte positions to whole UTF-8 characters (see the frizbee traps in
`docs/trapstoknow.md`), but GPUI's text renderer requires character
boundaries, so `palette/display.rs` checks every range again: ranges that
split a character, run past the text or are empty are dropped, unsorted or
overlapping ones are merged, and long text is clipped to a fixed number of
characters with the ranges clipped with it. A match that has no highlight
positions still shows its row, just without highlights.

**Bounded previews.** When the popup is at least 720 pixels wide (it is up to
880, the design's width), the selected row is described in a pane on the
right using the `panel_side` token: its kind, its title and address with the
same highlights, and what Enter will do. Text is clipped to a fixed length so
a very long address cannot stall layout. Previews use only the catalogue's
title and address; page-text previews (tier 2) need the full-text slices
(P1-23 onwards) and are not part of this work.

**One activation path.** `crates/syrinx-core/src/activation.rs` resolves an
`ActivationTarget` (a tab or a saved page) into one action: focus a live tab,
or load an unloaded saved page. A saved page that is already open is focused,
never loaded again (D43). `BrowserView::activate` is the one path for the
palette and the sidebar's saved and tab rows. A saved page always goes to the
session worker as `OpenSaved`, whose `open_saved` focuses an existing instance
against the worker's own current model; the UI's snapshot can be stale (a
queued Ctrl W may not have landed yet), so deciding there could turn a click
into an unknown-tab error. In the results, a saved page's live tab is shown
once, as its saved row, instead of twice.

**Ranking commands against pages.** In Everything a command leads the pages
only when the typed text is its whole title ("close tab"); otherwise pages come
first and matching commands follow them, so typing "close" and pressing Enter
opens a matching page rather than closing the current tab. `>` remains the
explicit commands-only path.

**Stale results.** Each query has a request identity. Commands are local, so
rows that will lead the list (every row after a prefix, an exact-title
command in Everything) show at once; the rest appear only together with that
query's own worker reply. Neither rows nor chip counts of an older query can
replace a newer one. A selection the owner moved stays on the same row when
the reply arrives. Model changes re-run the current query and keep the
selected destination by identity, as P1-10 already did.

No search request leaves the machine: commands and catalogue rows are ranked
locally, and the palette adds no network call (D9). P1-32's address-bar work,
Alt Enter web search changes and D48's speed target are out of scope here.

## Files

- `crates/syrinx-core/src/commands.rs`, `activation.rs` (new, with tests).
- `crates/syrinx-app/src/palette.rs` and `palette/` (`choices.rs`,
  `commands.rs`, `display.rs`, `query.rs`, `render.rs`).
- `crates/syrinx-app/src/ui.rs`: key bindings from the registry, command
  execution, the shared activation path, the popup's size and passing the
  command context; `ui/sidebar_view.rs` row clicks use the same activation path.
  The palette reads every colour from `theme::current()`, so it follows
  Customize.
- No change to `syrinx-search`'s public API or internals.

## What the owner checks in the running app

Use synthetic pages only (for example `https://example.com`); nothing here
needs a signed-in account.

1. Open two or three pages, save one with Ctrl D, then press Ctrl K while a
   page has focus. The palette opens over the page with chips under the
   input; the input has focus.
2. Type part of a page title. Matched letters are drawn in the accent colour
   and bold, in the row and in the preview on the right; the chip counts
   change as you type.
3. Type a title containing accented letters or an emoji (for example save a
   page titled "Café 😀" from a local test file) and search for "café". The
   highlight covers whole characters, never half of one.
4. Type `>`. Every available command is listed with its shortcut. Type `>rel`
   and press Enter straight away: the page reloads and keyboard focus is back
   in the page (type in a text field to confirm). Type `>zzz` and press Enter:
   nothing happens, and no Google search opens.
4a. Without a prefix, type `close` and press Enter while a page titled with
   "close" is open: that page opens and your tab is not closed. Typing the
   whole title `close tab` lists the command first.
5. Type `@`. "No plugins yet" is shown and the Plugin actions chip reads 0.
6. Type `/` and part of a tab title: only open tabs are listed.
7. Click the Bookmarks chip, then the Everything chip: the list narrows and
   widens; any typed prefix is removed.
8. With a saved page already open in a tab, search for it and press Enter on
   its row. Syrinx switches to the open tab; no second copy appears in the
   sidebar. Close that tab, search again and press Enter: it opens once.
9. Use Up and Down past the visible rows: the list scrolls with the
   selection. Esc closes the palette and focus returns to the page.
10. On Home (no page showing), type `>reload`: nothing is offered, because
    there is no page to reload.
11. Make the Syrinx window narrower than about 770 pixels and press Ctrl K: the
    palette still works, without the preview pane.
12. Press Ctrl T, Ctrl W, Ctrl Shift T, Ctrl R, Alt Left and Right, Ctrl L,
    Ctrl Tab, Ctrl D and Ctrl H outside the palette: each behaves as before.
13. Choose another palette in Customize, then press Ctrl K: the palette uses
    the new colours.
