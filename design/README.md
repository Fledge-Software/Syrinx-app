# Atlas design

The live design is the canvas **Atlas browser design**
(<https://claude.ai/artifact/Cog1vuMS5rfYFmHLYPKj9f>, private to the owner).
`canvas/project/` is a snapshot of its files taken on 2026-10-09; when the two
disagree, the canvas wins. `reference/unified-workspace-mockup.html` is the
original two-board mockup the spec points at.

## What is on the canvas

- **Home, command palette and customize:** the new-tab page, the palette open
  over it, and the Customize panel where the palette and accent are chosen.
- **Browsing:** a web page in the page card, with the sidebar open and
  collapsed. With the sidebar hidden, back, forward, reload and the address
  move into the title bar.
- **Base palettes:** the home page and a browsing view in each of the five base
  palettes, side by side. Every board also has a `theme` tweak.
- **Sidebar:** one shared component; the other boards import it.

## Decisions made after the spec

These change or add to `docs/spec.md`. Each was made in the 2026-10-09 design
pass and can be reversed.

- **The browser is named Atlas** in the chrome (the mockup said "Perch").
- **No Extensions button.** Tauri's OS web views cannot run Chrome extensions.
- **New tab is a `+` on the Open tabs header (Ctrl T)**, not a row, so no two
  rows read "New tab".
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
