# P1-28 and P1-29: the home page, Favorites and saved appearance

Plan 1's slices P1-28 and P1-29 exist so that the owner's quick destinations
and chosen look survive a restart. Before them, Home was a placeholder
("Your workspace, together."), the sidebar's Forge/Notes/Fledge rows were
labelled "coming soon", and every window drew Graphite with the default
accent because `crates/atlas-app/src/theme.rs` held one fixed palette. This
note records what the two slices deliver, the storage they add, what is
deliberately left for other slices, and the checks the owner runs on the
window, because an agent cannot see it.

## What P1-28 delivers

- **The canvas home page** (`crates/atlas-app/src/ui/home.rs`), drawn from the
  `Main` board in `design/canvas/project/`: the accent mark, "Atlas", a
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
- **Separate square Favorites (D45)** (`crates/atlas-app/src/ui/favorites.rs`):
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
- **Model and limits** (`crates/atlas-core/src/destinations.rs`): each list
  holds at most 64 tiles; a name is 1–64 characters with no control
  characters; a stored target is an `http`/`https` address of at most 8 KiB
  or a built-in plugin id. Edits are resolved by the session worker against
  its latest lists, the same way tab commands are, and a refused edit is a
  recoverable footer message. Tile labels and addresses are redacted from
  debug output.

## What P1-29 delivers

- **The Customize panel** (`crates/atlas-app/src/ui/customize.rs`): all five
  palettes with a miniature, name and description; the four accent presets
  plus a custom accent entered as `#RRGGBB`; and the Reduce transparency
  switch.
- **Live application.** Choosing anything replaces the palette that
  `theme.rs` hands to every render and asks Windows for the matching
  backdrop: Mica for Haze and Glass, opaque otherwise. The chrome redraws at
  once. The two opaque greys P1-07 left in `theme.rs` (selected row and the
  palette's secondary button) now read the canvas tokens `row_selected` and
  `field`, so Paper does not draw dark rows under dark text. An open command
  palette window picks the new palette up on its next redraw.
- **Persistence.** The worker saves the choice through the store's writer
  and restores it before the first tab state, so Atlas starts in the chosen
  look. An unreadable stored value falls back to the default for that key
  and the footer says so.

## Storage: schema version 3

The migration adds two tables in one transaction
(`crates/atlas-store/src/schema.rs`, `preferences_migration`). It only
creates its own tables, so it composes with other slices' migrations.
**Schema number: 3 on base `9c90d91`.** Whoever merges second renumbers.
Two `atlas-platform` recovery tests name schema numbers directly
(`crates/atlas-platform/src/storage/recovery/tests.rs`: the "newer" marker
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
| `appearance.accent` | `#RRGGBB` (opaque) | `#FF8A65` |
| `appearance.reduce_transparency` | `true`, `false` | `false` |

The sidebar slice owns the sidebar width/presentation and should store it
under its own `sidebar.` keys in the same table.

Opening a version 3 database validates the preference rows before any
write; damaged rows (a position gap, an unknown plugin, an untrimmed label,
a non-web address) are refused without repair, like damaged tab rows. Backup
validation reads them too. A failed migration rolls back to the intact
version 2 file. These paths, the round trips and the restart are tested on
disposable databases with synthetic labels and `example.test` addresses
(`crates/atlas-store/src/engine/tests/preferences.rs`,
`crates/atlas-app/src/session/preference_tests.rs`).

## Left for other slices or the owner

- **The header's local date is not shown yet.** Rust's standard library
  only knows UTC, and the dependency rule in `crates/xtask/src/dependency_policy.rs`
  lets only `atlas-platform` call Win32 (`GetLocalTime`). Showing a UTC date
  would be wrong every evening in the owner's time zone, so the date waits
  for a one-function local-calendar helper in `atlas-platform` (owned by the
  platform work, not this slice).
- **Sidebar presentation** (wide, favicon-only, hidden) is persisted by the
  sidebar slice, which owns the width and drag code. Customize does not
  show it yet.
- **Glass uses Mica, not Acrylic.** GPUI's `Blurred` background is not the
  Windows 11 Acrylic (see `docs/trapstoknow.md`), and drawing real Acrylic
  needs `DwmSetWindowAttribute` in `atlas-platform`. Until then Glass shows
  its tint over Mica.
- **Reduce transparency is Atlas's own switch.** The canvas says it
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

Run `cargo atlas` (or the built `atlas-app.exe`) and look at the window.

1. **Home.** With no page open (or after Ctrl T and Esc), the page card
   shows the accent mark, "Atlas", a Customize button (no date yet),
   the search pill with "Ctrl K", the `>` `@` `/` hints, "Quick access" with
   only the dashed "Add shortcut" tile, and three dimmed plugin cards.
2. **Search pill.** Clicking the pill opens the command palette.
3. **Add a tile.** Click "Add shortcut", type a name and `example.com`,
   press Enter. A monogram tile appears. Type `two words` as an address
   instead: the form explains that it needs a website address.
4. **Open a tile.** On Home, clicking the tile opens the site in the same
   tab. With a page open, clicking a Favorite opens a new tab.
5. **Edit.** Click "Edit": each tile shows ‹ × ›. Move one, remove one,
   click one to rename it, then click "Done".
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
   sidebar row included).
10. **Accent.** Choose each preset, then "+" and enter `#12AB9C` and press
    Enter; the hex label and accent marks change. Enter `#12AB9` and check
    the error message.
11. **Reduce transparency.** With Haze or Glass chosen, turn it on: the
    window turns Graphite while Haze/Glass stays selected. Turn it off.
12. **Restart.** Choose Tint with a custom accent, Reduce transparency on,
    some tiles and favorites. Close Atlas and start it again: the palette,
    accent, switch, tiles and their order are all as left.
