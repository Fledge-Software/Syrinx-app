# Syrinx design

The live design is the owner's design canvas
(<https://claude.ai/artifact/Cog1vuMS5rfYFmHLYPKj9f>, private to the owner).
`canvas/project/` is a snapshot of its files taken on 2026-10-09; when the two
disagree, the canvas wins. `reference/unified-workspace-mockup.html` is the
original two-board mockup the spec points at.

The canvas and the mockup predate the product's name and may still show the
working name Atlas in titles and labels; read it as Syrinx, which is what
the app says. The snapshot in `canvas/project/` was renamed to Syrinx on
2026-10-17; renaming the live canvas is the owner's call.

The logo, the app-icon tile, the brand colours and the favicon kit are in
`brand/`; `brand/README.md` says which to use where and how to regenerate them.

## What is on the canvas

- **Home, command palette and customize:** the new-tab page, the palette open
  over it, and the Customize panel where the palette and accent are chosen.
- **Browsing:** a web page in the page card, with the sidebar open, hidden,
  and peeking in from the left edge. With the sidebar hidden, back, forward,
  reload and the address move into the title bar.
- **Base palettes:** the home page and a browsing view in each of the five base
  palettes, side by side. Every board also has a `theme` tweak.
- **Forge, the code plugin:** editing (project tree, split editors, a
  completion from the language server, terminal, context rail), commit, diff
  and log, and the editor with the Syrinx sidebar open.
- **Sidebar:** one shared component; the other boards import it.

## Decisions made after the spec

These change or add to `docs/spec.md`. The original decisions were made in
the 2026-10-09 design pass. The owner-approved 2026-10-10 additions below
(D43–D45) supersede the affected parts of the older canvas snapshot; changing
them again requires the owner's approval.

- **The browser is named Syrinx** in the chrome (the mockup said "Perch").
- **No Extensions button.** Syrinx v1 runs no Chrome extensions, a known
  limit in the spec and `AGENTS.md`. (This first named Tauri's OS web
  views; D3 and D4 replaced Tauri, and pages render in WebView2 driven
  from Rust through wry.)
- **New tab is a `+` on the Open tabs header (Ctrl T)**, not a row, so no two
  rows read "New tab".
- **Saved pages and folders live inline with ordinary tabs (D43).** Pin
  means Save. A saved row remains after its live page closes; clicking it
  opens the saved address or focuses its existing open page. Navigation does
  not replace the saved title or address. Remove/Unpin deletes the saved
  entry while leaving an open page as an ordinary tab. Saved entries come
  first; saving appends to that group, and removing an open saved entry
  places its page at the start of ordinary tabs. Imported HTML bookmarks
  preserve their folder hierarchy here and start unloaded. They are not a
  second list of automatically opened tabs.
  D42's 20-entry closed-web-tab history includes saved-page instances.
  Reopen focuses an existing instance or opens the fixed saved destination
  when the saved row survives. If it was removed, history can reopen an
  ordinary page but cannot recreate the saved row. Plugins stay outside
  this history and reopen through their permanent rows.
- **The top square tiles are Favorites (D45).** They remain separate from
  the inline saved-page/folder records and may target websites, plugin pages
  or other supported Syrinx destinations. Plugin Favorites use the normal
  plugin lifecycle; before implementation, they show that the plugin is not
  yet available. This renames the spec's sidebar "pinned app tiles", not
  the home page's Quick access section.
- **The sidebar supports smooth width dragging (D44).** It moves from wide
  with labels to favicon-only to hidden; reverse dragging restores it. This
  is an additional way to size/show the sidebar, preserving hover peek and
  Ctrl Alt S. The owner's screenshot is visual direction only and is not
  copied into the repository. Exact width thresholds and compact-folder
  affordances are open for the relevant UI slice; no pixel values are fixed
  by this decision.
- **A hidden sidebar peeks over the page.** Hovering the left edge slides it
  in on top of whatever is showing, on every kind of tab, without resizing
  anything under it. A persistently visible sidebar, set by Ctrl Alt S or
  the D44 drag control, reserves its current width beside the page. Over a
  web page the sidebar sits above a separate native web view, so the peek,
  like the command palette, is its own popup window owned by the main
  window, the arrangement Phase 0 proved (`docs/dev/trapstoknow.md`).
- **A plugin is a tab that never leaves the Plugins list.** It can be opened
  and closed like a tab. Open, its row shows what it has open after its name
  ("Forge · fledge-core"), and the active one is highlighted with a close
  button. Closed, it stays in the list, dimmed, with no badge, and opens again
  on a click. Its views therefore don't also appear under Open tabs, which
  holds web pages, inline saved pages/folders and integrated apps.
- **A site keeps one tile colour everywhere**, and every unread badge uses the
  accent.
- **One colour rule:** the accent marks selection and what Enter will do; plugin
  colours only identify a plugin.
- **The palette always offers "Search the web" (Alt Enter)**, because it is
  also the address bar.
- **Each installed plugin's feeds get their own palette scope chip** (owner,
  2026-10-21, research note 10's decision 8), for example "Notes", shown only
  while that plugin is installed, in addition to Everything (where its
  results also appear) and the spec's other chips. Where the plugin chips
  sit in the row is not decided yet; #141 builds them.
- **Tab moves between form fields** in an inline palette action. "Create and
  open" is Ctrl Enter. This replaces the spec's "Tab for more fields".
- **The chrome uses the operating system's UI font** (Segoe UI Variable on
  Windows, SF on macOS), with a monospace face only for the `>` `@` `/` hints.
- **Text contrast is at least 4.5:1** in every palette.
- **The default accent is the logo's orange, `#FF612E`** (owner decision
  2026-10-13), replacing the spec's `#FF8A65` and the canvas's default. It is
  still customisable, and it replaces `#FF8A65` as the first Customize
  swatch. A saved `#FF8A65` is read as the default, so it becomes `#FF612E`
  too (owner decision 2026-10-13); any other saved colour stays as chosen.
  The orange is darker
  than the old default, so its contrast was rechecked in all five palettes
  (`crates/syrinx-ui/src/contrast.rs`). Dark badge text on it measures 6.08:1
  everywhere. As accent text on palette rows it measures, at worst (the
  selected row), 4.67:1 on Graphite, 4.63:1 on Tint and 5.58:1 on Paper, whose
  ink is already a darker tone. On Haze and Glass it fell to 4.37:1 and
  4.38:1, and as a selection mark on Glass to 2.37:1 against the 3:1 minimum.
  **On Haze and Glass, an accent whose ink (accent text and marks) fails
  4.5:1 as text or 3:1 as a mark is drawn in a lighter tone of its own hue,
  the first of 70%, 72% … 90% lightness that passes** (owner decision
  2026-10-13). An accent that passes keeps its own colour, as the green and
  gold presets do everywhere. For the default the lifted ink is 70%, about
  `#FF8C66`, close to the old default: Haze then measures 5.74:1 and Glass
  5.76:1 on selected rows, and Glass's marks 3.11:1 at worst. The blue preset
  failed only as a mark on Glass's card (2.94:1); on Glass it becomes
  `#7AAFFF` (3.18:1), which resolves that recorded P1-07 failure. Badges,
  filled buttons and washes always keep the chosen colour, with dark badge
  text on it.
- **Open question:** the Passwords button has nothing behind it in v1, because
  password-manager support is out of scope. The recommendation is to drop it
  until it does.

## Built without a canvas board (awaiting the owner's sign-off)

These controls exist in the app but not on the canvas, so their look follows
nearby chrome and still needs the owner's approval before it counts as the
design.

- **Zoom stepper (#90).** Away from 100%, the title bar's corner shows a pill
  with − , the level (for example "125%") and +, as Chrome does; − and + step
  the zoom and a click on the percentage resets it. It replaced the earlier
  reset-only "110%" pill and uses the same pill shape and line colour.
- **The page menu's Zoom row (#90).** The ☰ menu's Zoom in and Zoom out rows
  became one "Zoom" row carrying the same stepper, so zooming does not close
  the menu; Left and Right step it from the keyboard. "Reset zoom" (Ctrl 0)
  stays as its own row. The row follows the existing menu-row style.
- **Downloads flyout (#143, #89).** ↓, Ctrl J, the ☰ menu's Downloads row and
  the palette open Syrinx's own list instead of WebView2's per-page dialog, so
  it opens from any tab, Home included, and keeps a download whose popup has
  closed. It is the drop-down menu's card and rows at 380px wide, hanging
  from the title bar's right corner like the ☰ menu, and ↓ closes it again.
  Each row shows the file name, truncated, with its status dimmed on the
  right ("1.5 of 4.0 MB", "Done, 4.0 MB", "Blocked: unsafe file") and,
  while it can act, small round Pause, Resume and Cancel glyph buttons in the
  zoom stepper's style (Enter or Space presses the first, Delete cancels); up
  to eight rows, newest first, then "Clear finished downloads". A file
  SmartScreen blocks shows as blocked with its reason and offers no button:
  Syrinx never keeps a blocked file. Rows do not open the file or its folder
  yet. The list lives in memory for the session only, and WebView2's own
  download flyout no longer appears (owner, 2026-10-24). The canvas has no
  board for it; adding one is the owner's.
- **The compact sidebar's close badge (#128).** In the favicon-only sidebar
  an open tab's row shows a 16 px round ⓧ on its icon's top-right corner,
  inside the row's 30 px square, while the pointer is over the row or the
  row shows its focus ring. It is a × in the icon colour on the sidebar
  ground with the usual 1 px line, and darkens like an icon button when
  hovered. On an ordinary tab it closes the tab; on a saved page that is open
  it unloads the page and the saved row stays, as the wide row's × does. An
  unopened saved page and a folder get none. Clicking the icon beside it
  still selects the tab. Firefox's vertical tabs are the reference. Open for
  the owner: whether the active row shows it without hover or focus, and
  how it reads over a real favicon once favicons land.
- **The sidebar row menu and the ⋯ menu's Close tab (#95, #58; owner's
  Direction 1, 2026-10-27).** A right-click on a sidebar row, or Shift F10
  or the Menu key on a focused row, opens that row's menu: the drop-down
  menu's 220 px card and 30 px rows, with the key that does the same
  dimmed on the right, and groups split by a 9 px separator holding a 1 px
  line in the panel divider colour. The items per row kind are recorded in
  `docs/decisions.md` (2026-10-27). The ⋯ menu ends with a separator and
  Close tab (Ctrl W). Close other tabs and Close tabs below first ask in
  the same card at the same place: the question in semibold ("Close 3
  other tabs?"), a separator, then Close 3 tabs, Close and don't ask again,
  and Cancel. Opened from the keyboard, the question starts on Close 3 tabs,
  as Firefox's does: a mistaken Enter is undone by Reopen closed tab, while
  starting on Cancel would make the common answer cost an extra key. The
  pointer selects a row only after it has moved, as in native Windows menus,
  so a card that opens under a still pointer keeps that keyboard choice. The
  wide row's mute button and × share one 20 px square. Settings gains a Tabs
  section whose one switch, "Ask before closing several tabs", turns the
  question back on.
- **The muted marker (#95; "Mute icon only", 2026-10-27).** A muted tab's
  wide row shows the Segoe MDL2 mute glyph (E74F) in the icon colour before
  its ×, which unmutes on a click. In the favicon-only sidebar it is a
  12 px round badge with the same glyph, at 8 px, on the icon's bottom-right
  corner, clear of the close badge above it, in the close badge's colours,
  and shown only. No speaker icon appears while a page plays sound. The
  canvas has no board for any of these; adding one is the owner's.

## Forge

Forge takes PyCharm's workflow, not its look: tool windows docked around the
editor, split editor groups, language-server completions and inline type
hints, git commit, diff and log, a terminal, and a run control. Everything
is drawn in Syrinx's own visual language; none of JetBrains' icons or styling
is copied.

- **A Forge tab hides the Syrinx sidebar by default.** An IDE needs the width:
  with the sidebar hidden the editors get about 875px, enough for two groups
  side by side. Hovering the left edge peeks the sidebar over the editor, as
  everywhere in Syrinx; pinning it with Ctrl Alt S narrows the editor and folds the
  context rail into a strip of icons.
- **Back and forward move between code locations** in a Forge tab, as Alt Left
  and Alt Right do in PyCharm. Reload is hidden, because it means nothing there.
- **Every tab has an address.** A Forge tab shows its project and file path
  (`fledge-core/src/core/workspace.rs`) where a web tab shows its URL.
- **The context rail is the shared index made visible.** For the open file it
  shows its outline, the notes that mention it and the commits that touched it.
- **The run control is an addition to the spec.** PyCharm's run configurations
  are central to how it is used. To stay language-agnostic, Forge would read
  its runnable tasks from the project (Cargo, npm scripts, pyproject, a
  justfile) instead of keeping per-language settings.
- **Git status uses a letter as well as a colour** (M, A, U), so it doesn't
  rely on hue alone.
- **Forge's surfaces are opaque in every palette.** Only the Syrinx title bar
  around the card picks up Haze or Glass.
- **The highlighter on the canvas is a stand-in** for Tree-sitter, and the
  completion for rust-analyzer over LSP; the code shown is illustrative.

## Base palettes

**Replaced on 2026-10-15** (`docs/decisions.md`, `docs/theme-editor.md`).
Graphite, the default, and Paper were the only presets; since 2026-10-21
Glass is a preset again (#93, built in #124; `docs/decisions.md`). Haze and
Tint are no longer fixed palettes: the owner builds looks like them in the
theme editor (mode, background, gradient, Mica, Mica Alt or Acrylic,
transparency, glass edges, grain) and saves them by name. The table and
rules below describe the five palettes as first designed. Its see-through
rules (opaque pages, near-opaque palette, Reduce transparency, measuring
Acrylic) still hold for custom themes.

**Transparency, decided by the owner on 2026-10-22** (`docs/decisions.md`):
with Acrylic, a see-through theme shows the windows behind Syrinx, frosted.
The Transparency slider (0–90 %) lowers Syrinx's own tint until, at 90 %,
only Windows' material is drawn; a dark theme under Acrylic keeps a tint of
about 12.5 % (32/255, owner decision 2026-10-24) so white text stays at
4.5:1 over a bright desktop. The backdrop is light or dark as the theme
is. The window background, title bar and sidebar follow the slider, and
Home's card does too when the theme's Home choice is See-through. Web pages,
the command palette, menus and the Customize panel stay opaque. There is no
"Clear" (unblurred) material, and the page itself is never see-through.

The base palette is the ground, surfaces and text; the accent is chosen
separately and sits on top of any of them. All five ship, and all five are
offered in the Customize panel. Each is named for how it looks, not for the
Windows or Apple feature that draws it, because Haze and Glass use a different
feature on each system.

| Palette | Look | Windows 11 | macOS | Linux |
|---|---|---|---|---|
| Graphite | Solid dark (the default) | Plain colours | Plain colours | Plain colours |
| Haze | Dark, softly coloured by the wallpaper | `mica` window effect | `sidebar` vibrancy | Falls back to Graphite |
| Glass | See-through, with light edges | `acrylic` window effect plus Syrinx's glass styling | `liquidGlassRegular` on macOS 26+, `hudWindow` vibrancy plus the styling before that | Falls back to Graphite |
| Tint | Dark gradient in the accent's hue, with grain | Plain colours | Plain colours | Plain colours |
| Paper | Light | Plain colours | Plain colours | Plain colours |

The ids in the canvas files are `graphite`, `haze`, `glass`, `tint` and `paper`.
On Windows, Glass has everything except Apple's lens effect, where the edge
of the glass bends what is behind it. Windows 10 has no Mica, and Acrylic is
slow there, so on Windows 10 both Haze and Glass fall back to Graphite.

Rules that hold for the see-through palettes:

- **Only the window background is see-through.** Web pages stay opaque; a
  transparent page background breaks sites that assume white.
- **The command palette stays near-opaque.** Over a web page it sits above a
  separate native web view, which CSS blur cannot see.
- **Acrylic has to be measured.** Tauri's docs, read before D3 and D4
  replaced Tauri, say it drags and resizes slowly on Windows 10
  1903+ and Windows 11 build 22000. The development machine runs build
  26300, so it may be fine there. Phase 0 did not measure it; the
  measurement waits on the owner's own drag (`docs/theme-editor.md`,
  "Building it", step 4).
- **Text gets lighter on glass.** The muted grey is raised in Haze and Glass so
  it stays at 4.5:1 over a mid-tone wallpaper. A bright wallpaper can still
  fail, so Syrinx's "Reduce transparency" switch follows the OS setting and
  turns both into Graphite.

Zen gets its see-through look the same way, through the operating system:
Firefox's Mica support and Windows Acrylic. Zen's source
(<https://github.com/zen-browser/desktop>) is a useful reference for how its
styles cope with transparency. It is licensed under MPL 2.0, so any file copied
from it keeps that licence.
