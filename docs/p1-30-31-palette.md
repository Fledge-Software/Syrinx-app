# P1-30 and P1-31 command palette: what was built and what the owner checks

Plan 1's slices P1-30 and P1-31 turn the Ctrl K entry popup from P1-10 into
the command palette the spec puts at the centre of Atlas (S5, spec section 3).
Before them, the popup searched open tabs and saved pages and could navigate
to an address, but it could not run a single command, had no scope chips,
showed no matched characters and offered no preview. Browser commands lived
only as hand-written key bindings in `crates/atlas-app/src/ui.rs`, so a later
plugin would have had nowhere to register its own. This note records what the
two slices deliver, the choices made and what the owner needs to try in the
running app, because a passing gate does not prove that focus returned to a
page or that a highlight is drawn in the right place.

## P1-30: one command source and the full palette

**The command registry** (`crates/atlas-core/src/commands.rs`) is the single
list of things Atlas can run. A command has an identifier
(`browser.new-tab`), a title, optional search keywords ("refresh" finds
Reload), a scope (browser or a named plugin), an enablement rule, an action
and an optional shortcut. The action is an enum: `Browser(BrowserAction)` for
the built-in commands and `Plugin { plugin, action }` for the plugin host that
Plan 2 builds. The registry rejects malformed identifiers, empty titles, a
duplicate identifier and, importantly, a second command claiming the same
shortcut, so one keystroke can never run two commands depending on
registration order. It is generic on purpose; it is not a plugin host and no
plugin can reach it yet.

The ten existing browser commands are registered with the same shortcuts as
before: New tab (Ctrl T), Close tab (Ctrl W), Reopen closed tab (Ctrl Shift
T), Reload page (Ctrl R), Go back (Alt Left), Go forward (Alt Right), Edit
address (Ctrl L), Next and Previous tab (Ctrl Tab, Ctrl Shift Tab) and Save
page (Ctrl D). `ui::bind_keys` now derives every GPUI key binding from the
registry instead of listing them again; Ctrl K stays a separate binding
because the palette is where commands run, not a command itself. The native
accelerator router in `atlas-platform` still maps the same keys for a focused
page; it belongs to another slice and was not changed.

The palette sample window (`--palette-sample`) was not registered as a
command: it is a development review window that quits the application when it
closes, so opening it from the running browser would close Atlas.

Enablement uses a small `CommandContext` (is a tab selected, is a web page
showing, how many tabs are open). Disabled commands are hidden rather than
shown greyed out: on Home, typing "reload" finds nothing instead of offering
a row that cannot run.

**The palette itself** (`crates/atlas-app/src/palette.rs` and
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
  for Search the web, as before;
- focus restoration through the existing path: after a command, an activation
  or Esc, the main window is activated and the shown page is focused. Edit
  address is the one exception: it reopens the entry popup with the current
  address, which keeps focus.

Commands are matched on the palette's own thread, not on the session worker.
Their enablement depends on what the window is showing, which only the UI
thread knows, and the registry is small and bounded (ten built-in commands;
plugin commands arrive with Plan 2), so each keystroke ranks a few dozen
entries. Tab, saved-page and history search stays on the worker as before.
The alternative, indexing commands in the worker's catalogue, was rejected
because the worker cannot judge enablement and because its single command
count cannot tell browser commands from plugin actions, which the Plugin
actions chip needs.

## P1-31: chips, highlights, previews and one activation path

**Scope chips with counts.** Everything, Tabs, Plugin actions, Bookmarks and
History sit under the input, each with its count for the current text. The
tab, bookmark and history counts come from the worker's `ScopeCounts`, which
count every match before the display limit; the Plugin actions count comes
from the local command match. History shows 0 until the history slice feeds
the catalogue. Everything includes commands only once text is typed: a blank
Ctrl K lists open tabs and bookmarks, and `>` lists every command. Choosing a
chip removes a typed prefix, because the prefix would otherwise override it.

**Highlights.** Matched characters are drawn in the accent ink at semibold
weight, as on the Palette board. `atlas-search` already widens the matcher's
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

**One activation path.** `crates/atlas-core/src/destinations.rs` resolves a
destination (a tab or a saved page) into one action: focus a live tab, or load
an unloaded saved page. A saved page that is already open is focused, never
loaded again (D43). `BrowserView::activate` follows that route for the
palette, and the sidebar's saved and tab rows now call the same function. The
session worker's `open_saved` also focuses an existing instance, so even an
activation racing a model change cannot produce a duplicate. In the results,
a saved page's live tab is shown once, as its saved row, instead of twice.

**Stale results.** Each query has a request identity. Command rows for a
query are staged and appear only together with that query's own worker reply,
so neither the rows nor the chip counts of an older query can replace a newer
one. Model changes re-run the current query and keep the selected destination
by identity, as P1-10 already did.

No search request leaves the machine: commands and catalogue rows are ranked
locally, and the palette adds no network call (D9). P1-32's address-bar work,
Alt Enter web search changes and D48's speed target are out of scope here.

## Files

- `crates/atlas-core/src/commands.rs`, `destinations.rs` (new, with tests).
- `crates/atlas-app/src/palette.rs` and `palette/` (`choices.rs`,
  `commands.rs`, `display.rs`, `query.rs`, `render.rs`).
- `crates/atlas-app/src/ui.rs`: key bindings from the registry, command
  execution, the shared activation path, the popup's size and passing the
  command context. `crates/atlas-app/src/theme.rs` gained `tokens()`.
- No change to `atlas-search`'s public API or internals.

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
   and press Enter: the page reloads and keyboard focus is back in the page
   (type in a text field to confirm).
5. Type `@`. "No plugins yet" is shown and the Plugin actions chip reads 0.
6. Type `/` and part of a tab title: only open tabs are listed.
7. Click the Bookmarks chip, then the Everything chip: the list narrows and
   widens; any typed prefix is removed.
8. With a saved page already open in a tab, search for it and press Enter on
   its row. Atlas switches to the open tab; no second copy appears in the
   sidebar. Close that tab, search again and press Enter: it opens once.
9. Use Up and Down past the visible rows: the list scrolls with the
   selection. Esc closes the palette and focus returns to the page.
10. On Home (no page showing), type `>reload`: nothing is offered, because
    there is no page to reload.
11. Make the Atlas window narrower than about 770 pixels and press Ctrl K: the
    palette still works, without the preview pane.
12. Press Ctrl T, Ctrl W, Ctrl Shift T, Ctrl R, Alt Left and Right, Ctrl L,
    Ctrl Tab and Ctrl D outside the palette: each behaves as before.
