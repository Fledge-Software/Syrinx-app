# P1-28 and P1-29: the home page, Favorites and saved appearance

Plan 1's slices P1-28 and P1-29 exist so that the owner's quick destinations
and chosen look survive a restart. Before them, Home was a placeholder
("Your workspace, together."), the sidebar's Forge/Notes/Fledge rows were
labelled "coming soon", and every window drew Graphite with the default
accent because `crates/syrinx-app/src/theme.rs` held one fixed palette. This
note records what the two slices deliver, the storage they add, what is
deliberately left for other slices, and the checks the owner runs on the
window, because an agent cannot see it.

## What P1-28 delivers

- **The canvas home page** (`crates/syrinx-app/src/ui/home.rs`), drawn from the
  `Main` board in `design/canvas/project/`: the accent mark, "Syrinx", a
  Customize button, the large search
  pill with its `Ctrl K` hint and the `>` `@` `/` prefix hints, the Quick
  access grid (eight per row, 72px monogram tiles, a dashed "Add shortcut"
  tile) and the Plugins strip. The pill is a button that opens the existing
  command palette, as the canvas notes say, not a second text field.
- **Editable Quick access.** "Edit" turns on move-left, remove and
  move-right controls under each tile; clicking a tile in edit mode opens the
  form to change it. "Add shortcut" opens the same form. A website tile
  takes navigation's address rule (`navigation::recognized_address`), so
  `example.com` becomes `https://example.com/`, but it never falls back to
  a web search: "two words" is refused with an explanation.
- **Separate square Favorites (D45)** (`crates/syrinx-app/src/ui/favorites.rs`):
  a four-column grid at the top of the sidebar, with its own Edit switch and
  its own dashed add tile. Favorites are not saved pages and do not appear
  among the inline saved rows; bookmark import still never creates them.
- **One truthful open path.** A web tile opens in the Home tab when Home is
  showing and in a new tab otherwise (`ui/tiles.rs`, `open_action`). A tile
  can also target Forge, Notes or Fledge. Plugin tiles and plugin cards are
  drawn dimmed and, when clicked, say "Forge is not available yet. Plugins
  arrive with the plugin runtime." No plugin host was built (plan 2 owns it).
- **One colour per site.** A web tile's colour comes from its host (FNV-1a
  over the host name, into the palette's eight tile colours), so the same
  site has the same colour on Home and in Favorites, as `design/README.md`
  requires. Plugin tiles use the plugin colours.
- **Model and limits** (`crates/syrinx-core/src/destinations.rs`): each list
  holds at most 64 tiles; a name is 1–64 characters with no control
  characters; a stored target is an `http`/`https` address of at most 8 KiB
  with no whitespace or control characters, or a built-in plugin id. Tile
  labels and addresses are redacted from debug output.
- **Edits name the tile they were made against.** Every rename, move and
  removal carries its position and the tile the window showed there. The
  session worker applies it only if that tile is still at that position, and
  otherwise refuses it with a footer message ("that tile changed before the
  edit arrived"). So a rename saved after an earlier tile was removed cannot
  overwrite the tile that moved up, and a double-clicked × removes one tile,
  not two. When the lists change, an open change form whose tile moved or
  went away closes and says so. Comparing content rather than a stored id
  keeps the table and the model simple; two identical tiles are
  interchangeable, so matching either is harmless.

## What P1-29 delivers

- **The Customize panel** (`crates/syrinx-app/src/ui/customize.rs`): all five
  palettes with a miniature, name and description; the four accent presets
  plus a custom accent entered as `#RRGGBB`; and the Reduce transparency
  switch. Since DW-7 (2026-10-13) it also has a Site permissions section
  whose Manage button lists remembered permission choices and forgets one
  (`ui/site_permissions.rs`, described in `docs/p1-12-permissions.md`), and
  the panel scrolls inside the card when the window is too short to show it
  whole.
- **Live application.** Choosing anything replaces the palette that
  `theme.rs` hands to every render and asks Windows for the matching
  backdrop: Mica for Haze and Glass, opaque otherwise. The chrome redraws at
  once. The two opaque greys P1-07 left in `theme.rs` (selected row and the
  palette's secondary button) now read the canvas tokens `row_selected` and
  `field`, so Paper does not draw dark rows under dark text. The
  gpui-component widgets (the text fields in the palette, the tile form and
  the custom accent) switch to the library's light theme for Paper and its
  dark theme otherwise; they use the library's own colours, not the canvas
  tokens. An open command palette window picks the new palette up on its
  next redraw.
- **Persistence.** The worker saves the choice through the store's writer
  and restores it before the first tab state, so Syrinx starts in the chosen
  look. An unreadable stored value falls back to the default for that key
  and the footer says so.

## Storage: schema version 3

The migration adds two tables in one transaction
(`crates/syrinx-store/src/schema.rs`, `preferences_migration`). It only
creates its own tables, so it composes with other slices' migrations.
**Schema number: 3 on base `25a4fc7`.** Whoever merges second renumbers.
Two `syrinx-platform` recovery tests name schema numbers directly
(`crates/syrinx-platform/src/storage/recovery/tests.rs`: the "newer" marker
is now 4 and the flat downgrade also drops the two new tables); a renumber
must update them too.

- `settings(key TEXT PRIMARY KEY, value TEXT)`: owner preferences as text.
  Keys are at most 64 bytes and values at most 256. An absent key means the
  documented default. Other slices add their own keys here.
- `quick_destinations(surface, position, label, target_kind, target)`:
  surface 0 is Quick access and 1 is Favorites; positions are dense from 0;
  target kind 0 is a web address and 1 a plugin id (`forge`, `notes`,
  `fledge`). A save replaces one surface in one transaction.

The settings keys this slice owns:

| Key | Values | Default |
| --- | --- | --- |
| `appearance.palette` | `graphite`, `haze`, `glass`, `tint`, `paper` | `graphite` |
| `appearance.accent` | `#RRGGBB` (opaque) | `#FF612E` (`#FF8A65` before 2026-10-13; a stored `#FF8A65` now loads as `#FF612E`, any other stored colour is kept) |
| `appearance.reduce_transparency` | `true`, `false` | `false` |

P1-29 also provides the storage for the sidebar presentation, which the plan
assigns to it. The sidebar slice (`claude/p1-08-sidebar-states`) built the
drag states and left saving them as deferred item DW-2, owned by P1-29, at
the point where a drag is released. The hookup happens at integration under
DW-2, using this API in `crates/syrinx-ui/src/sidebar_setting.rs`:

| Key | Values | Default |
| --- | --- | --- |
| `sidebar.width_state` | `wide`, `favicon`, `hidden` | the sidebar slice's default (wide) |

- Save: send `SessionCommand::SaveSettings(vec![syrinx_ui::sidebar_setting(state)])`
  when a drag settles. Peek is transient and is never stored.
- Restore: in `BrowserView::restore_preferences`
  (`crates/syrinx-app/src/ui/customize.rs`), read
  `settings.get(syrinx_ui::SIDEBAR_WIDTH_STATE_KEY)` and parse it with
  `syrinx_ui::parse_sidebar_setting`; an unreadable value keeps the default.

The round trip through the worker and a restart is tested in
`crates/syrinx-app/src/session/preference_tests.rs`.

Opening a version 3 database validates the preference rows before any
write; damaged rows (a position gap, an unknown plugin, an untrimmed label,
a non-web address) are refused without repair, like damaged tab rows. Backup
validation reads them too. A failed migration rolls back to the intact
version 2 file. These paths, the round trips and the restart are tested on
disposable databases with synthetic labels and `example.test` addresses
(`crates/syrinx-store/src/engine/tests/preferences.rs`,
`crates/syrinx-app/src/session/preference_tests.rs`).

## Left for other slices or the owner

- **The header's local date is not shown yet.** Rust's standard library
  only knows UTC, and the dependency rule in `crates/xtask/src/dependency_policy.rs`
  lets only `syrinx-platform` call Win32 (`GetLocalTime`). Showing a UTC date
  would be wrong every evening in the owner's time zone, so the date waits
  for a one-function local-calendar helper in `syrinx-platform` (owned by the
  platform work, not this slice).
- **Sidebar presentation hookup.** Wired at the 2026-10-12 integration merge
  with the sidebar branch (DW-2, Done): a changed settled state is saved and
  start-up restores it. Customize does not show the presentation.
- **The default palette shows briefly at start-up.** The saved appearance
  arrives from the session worker after it opens the database; reading the
  database on the GUI thread before the window opens would break the rule
  that storage never runs on the event thread, and the worker holds the
  storage lease. The window therefore draws Graphite for the moment before
  the preferences event, then switches.
- **One damaged tile row stops start-up and offers recovery.** Damaged tile
  or settings rows are refused like damaged tab rows, so a single bad row
  sends the whole database to backup recovery rather than dropping only
  that row. This is kept deliberately consistent with the existing storage
  rule for now (review finding 5); a narrower repair would need its own
  decision.
- **Glass uses Mica, not Acrylic.** GPUI's `Blurred` background is not the
  Windows 11 Acrylic (see `docs/trapstoknow.md`), and drawing real Acrylic
  needs `DwmSetWindowAttribute` in `syrinx-platform`. Until then Glass shows
  its tint over Mica.
- **Reduce transparency is Syrinx's own switch.** The canvas says it
  "follows your system setting"; reading the Windows transparency setting
  was not added, so the panel says what the switch does instead. Whether it
  should follow Windows by default is a question for the owner;
  recommendation: yes, with the switch as an override.
- **The "Integrated app" legend and unread badges** are not drawn: no tile
  can be an integrated app or carry a badge until plugins exist.
- **Plugin cards** show "Not available yet" instead of the canvas's
  "where you left off" lines, which need the plugins.
- **Accessible names** for tiles and swatches come with P1-36.

## Manual checks for the owner

Run `cargo syrinx` (or the built `syrinx.exe`) and look at the window.

1. **Home.** With no page open (or after Ctrl T and Esc), the page card
   shows the accent mark, "Syrinx", a Customize button (no date yet),
   the search pill with "Ctrl K", the `>` `@` `/` hints, "Quick access" with
   only the dashed "Add shortcut" tile, and three dimmed plugin cards.
2. **Search pill.** Clicking the pill opens the command palette.
3. **Add a tile.** Click "Add shortcut", type a name and `example.com`,
   press Enter. A monogram tile appears. Type `two words` as an address
   instead: the form explains that it needs a website address.
4. **Open a tile.** On Home, clicking the tile opens the site in the same
   tab. With a page open, clicking a Favorite opens a new tab.
5. **Edit.** Click "Edit": each tile shows ‹ × ›. Move one, remove one,
   click one to rename it, then click "Done". With at least four tiles,
   open the form on the third, then remove the first with ×: the form
   closes with "That tile changed while you were editing it". Double-click
   a × quickly: only one tile goes.
6. **Plugin tile.** Add a tile, choose "Forge", save. It is dimmed; clicking
   it shows "Forge is not available yet…" in the footer.
7. **Favorites.** In the sidebar's FAVORITES grid, add two favorites with the
   dashed tile, reorder them with Edit, and check that they do not appear in
   Quick access or among saved pages.
8. **Same colour.** Add the same site to Quick access and Favorites; both
   tiles have the same colour.
9. **Palettes.** Open Customize and choose each of the five palettes. The
   whole window changes at once: Haze and Glass show the desktop backdrop,
   Tint follows the accent, Paper is light and still readable (selected
   sidebar row included). Open a text field (Ctrl K, or "Add shortcut") in
   Paper and in a dark palette: the field is light in Paper and dark
   otherwise.
10. **Accent.** Choose each preset, then "+" and enter `#12AB9C` and press
    Enter; the hex label and accent marks change. Enter `#12AB9` and check
    the error message.
11. **Reduce transparency.** With Haze or Glass chosen, turn it on: the
    window turns Graphite while Haze/Glass stays selected. Turn it off.
12. **Restart.** Choose Tint with a custom accent, Reduce transparency on,
    some tiles and favorites. Close Syrinx and start it again: the palette,
    accent, switch, tiles and their order are all as left.
