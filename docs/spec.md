# Atlas product spec

Atlas is the working name. This is the owner's spec as written on 2026-10-09,
kept verbatim apart from formatting. Design decisions made since then are in
`design/README.md`.

## What this is

A desktop web browser in the style of Arc and Zen, with a vertical-tab sidebar.
Code editing, notes and my business software (Fledge) live inside it as
built-in plugins that open as tabs. The goal is that I never need a separate
IDE, notes app and browser open at once, and that all of them share one search
and one source of context.

It is a browser first. It is NOT an IDE with a browser attached. The default
experience on launch is a browsing experience.

## Decisions already made (don't relitigate these)

- No profiles or work/dev modes. There is one workspace.
- The browser engine is not built from scratch. Embed an existing web view.
- Rust owns all state and logic. The UI layer is thin.
- Graph view is excluded. The command palette is central.

## Product spec

### 1. Window layout

- Left sidebar, 264px, collapsible:
  - Top: back, forward and reload buttons, then an address/search field (Ctrl K).
  - A 4-column grid of pinned app tiles.
  - A "Plugins" section: Forge, Notes, Fledge, plus "Add plugin".
  - An "Open tabs" section. Tabs can be web pages, plugin views, bookmarks or
    integrated apps. Each shows an icon, a title, and an optional unread badge.
  - Bottom row: history, bookmarks, passwords, settings.
- The right side is a rounded "page card" inset from the window edge. This is
  where the active tab renders.
- Dark theme. The accent color is user-customizable (default #FF8A65).

### 2. Home page (the new-tab page)

- A header with the browser name and the date, plus a "Customize" button.
- A large search pill. It hints at the palette prefixes: `>` commands,
  `@` plugins, `/` open tabs.
- A "Quick access" grid of user-editable app and website tiles, 8 per row, with
  a dashed "Add shortcut" tile. Tiles for integrated apps get a small accent
  dot. Tiles can show a badge, such as unread mail count.
- A "Plugins" strip of cards: Forge, Notes, Fledge, and "Browse plugins".

### 3. Command palette (Ctrl K)

- A centered modal over a dimmed window, with a split layout: results on the
  left, a preview on the right.
- Scope chips with counts: Everything, Tabs, Plugin actions, Bookmarks, History.
- It searches open tabs, bookmarks, history, notes, and every plugin's commands
  and content from one query, with matched characters highlighted.
- Plugins can register commands. A command can render an inline form in the
  preview pane. Example: typing "fled" and selecting "New lead" shows Name,
  Company, Source and Est. value fields. Enter creates the lead in Fledge
  without opening a tab.
- Keyboard-first: arrows to move, Enter to run, Tab for more fields, Esc to close.

### 4. Built-in plugins (shipped with the app, like Obsidian's core plugins)

- **Forge** (development): a PyCharm-style experience. It has a project tree,
  split editor groups, a git tool window (log, diff, commit, branches), a
  terminal, and a context rail. Use libgit2 (git2 crate) for git. Use
  Tree-sitter for syntax and an LSP client for language features. I use PyCharm
  for every language, so it must be language-agnostic.
- **Notes**: roughly Obsidian crossed with Notion, with Craft-style polish.
  Local markdown files, wikilinks, backlinks, an outline panel, and
  database-style views. No graph view.
- **Fledge**: integrates my SaaS (leads, mail, calendar, docs) through its API.
  Build it against a trait with a stubbed client first, so the plugin works
  before the real API is wired in.

## Architecture direction

- **Shell:** Tauri 2 with a Rust core. Verify that multiple child webviews in
  one window work on Windows, macOS and Linux. In Tauri 2 this sits behind the
  `unstable` feature. This is the biggest technical risk. If it fails, evaluate
  CEF as the fallback and say so plainly.
- **Chrome UI:** Svelte 5 with TypeScript. It renders the sidebar, home page and
  palette, and holds no business logic.
- **Core model:** one generic tab model. A tab has a type (web, plugin view,
  note, file, diff, terminal) and a state. The sidebar and palette read from
  this model, not from per-feature code.
- **Plugin API (Rust trait):** a manifest, the tab types a plugin provides, the
  commands it registers, and a search provider. Built-in plugins use the same
  API that third-party plugins will use later.
- **Search:** a shared local index (SQLite with FTS5, or tantivy). Tabs,
  bookmarks, history, notes and plugin content all feed it. This shared index
  is what makes "all context from one source" real.
- **Storage:** local-first. Settings, tiles, pins and bookmarks are stored in
  SQLite.

## Phased plan

0. **Feasibility spike** (do this first, before anything else):
   - A window with a sidebar and a content webview side by side.
   - Vertical tabs that create, switch and close real web views.
   - Navigation and the address bar working.
   - A written verdict on whether multi-webview is viable on each OS.
1. Browser core: pinned tiles, the home page, bookmarks, history, session restore.
2. Command palette, with tab and bookmark search first.
3. Plugin runtime, then the Fledge plugin and its inline palette actions.
4. Notes plugin.
5. Forge plugin.

## How I want you to work

- Start with Phase 0 only. Stop afterward and report: what worked, what didn't,
  and what you recommend.
- I'm new to some of this stack (web view embedding, plugin architectures).
  When you make a technical choice, explain the concept briefly, say why you
  chose it, and name the main alternative.
- Make direct recommendations rather than listing options for me to pick from.
- Keep the code production-quality: tests for the core model and the search
  index, and clear module boundaries between the shell, the core and each
  plugin.
- Known limits to flag, not hide: Tauri's OS web views can't run Chrome
  extensions, and password-manager and ad-blocking support are out of scope
  for v1.

## Reference

The two mockup boards (home page, command palette open) are the visual
target; the original export is `design/reference/unified-workspace-mockup.html`.
The logos on the tiles are placeholders (monograms).
