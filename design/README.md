# Atlas design

The live design is the canvas **Atlas browser design**
(<https://claude.ai/artifact/Cog1vuMS5rfYFmHLYPKj9f>, private to the owner).
`canvas/project/` is a snapshot of its files taken on 2026-10-09; when the two
disagree, the canvas wins. `reference/unified-workspace-mockup.html` is the
original two-board mockup the spec points at.

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
  and log, and the editor with the Atlas sidebar open.
- **Sidebar:** one shared component; the other boards import it.

## Decisions made after the spec

These change or add to `docs/spec.md`. The original decisions were made in
the 2026-10-09 design pass. The owner-approved 2026-10-10 additions below
(D43–D45) supersede the affected parts of the older canvas snapshot; changing
them again requires the owner's approval.

- **The browser is named Atlas** in the chrome (the mockup said "Perch").
- **No Extensions button.** Tauri's OS web views cannot run Chrome extensions.
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
  or other supported Atlas destinations. Plugin Favorites use the normal
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
  web page the sidebar sits above a separate native web view, so
  it needs the same layering fix as the command palette in Phase 0.
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
- **Tab moves between form fields** in an inline palette action. "Create and
  open" is Ctrl Enter. This replaces the spec's "Tab for more fields".
- **The chrome uses the operating system's UI font** (Segoe UI Variable on
  Windows, SF on macOS), with a monospace face only for the `>` `@` `/` hints.
- **Text contrast is at least 4.5:1** in every palette.
- **Open question:** the Passwords button has nothing behind it in v1, because
  password-manager support is out of scope. The recommendation is to drop it
  until it does.

## Forge

Forge takes PyCharm's workflow, not its look: tool windows docked around the
editor, split editor groups, language-server completions and inline type
hints, git commit, diff and log, a terminal, and a run control. Everything
is drawn in Atlas's own visual language; none of JetBrains' icons or styling
is copied.

- **A Forge tab hides the Atlas sidebar by default.** An IDE needs the width:
  with the sidebar hidden the editors get about 875px, enough for two groups
  side by side. Hovering the left edge peeks the sidebar over the editor, as
  everywhere in Atlas; pinning it with Ctrl Alt S narrows the editor and folds the
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
- **Forge's surfaces are opaque in every palette.** Only the Atlas title bar
  around the card picks up Haze or Glass.
- **The highlighter on the canvas is a stand-in** for Tree-sitter, and the
  completion for rust-analyzer over LSP; the code shown is illustrative.

## Base palettes

The base palette is the ground, surfaces and text; the accent is chosen
separately and sits on top of any of them. All five ship, and all five are
offered in the Customize panel. Each is named for how it looks, not for the
Windows or Apple feature that draws it, because Haze and Glass use a different
feature on each system.

| Palette | Look | Windows 11 | macOS | Linux |
|---|---|---|---|---|
| Graphite | Solid dark (the default) | Plain colours | Plain colours | Plain colours |
| Haze | Dark, softly coloured by the wallpaper | `mica` window effect | `sidebar` vibrancy | Falls back to Graphite |
| Glass | See-through, with light edges | `acrylic` window effect plus Atlas's glass styling | `liquidGlassRegular` on macOS 26+, `hudWindow` vibrancy plus the styling before that | Falls back to Graphite |
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
- **Acrylic has to be measured.** Tauri's docs say it drags and resizes slowly
  on Windows 10 1903+ and Windows 11 build 22000. The development machine runs
  build 26300, so it may be fine there; the Phase 0 spike has to confirm it.
- **Text gets lighter on glass.** The muted grey is raised in Haze and Glass so
  it stays at 4.5:1 over a mid-tone wallpaper. A bright wallpaper can still
  fail, so Atlas's "Reduce transparency" switch follows the OS setting and
  turns both into Graphite.

Zen gets its see-through look the same way, through the operating system:
Firefox's Mica support and Windows Acrylic. Zen's source
(<https://github.com/zen-browser/desktop>) is a useful reference for how its
styles cope with transparency. It is licensed under MPL 2.0, so any file copied
from it keeps that licence.
