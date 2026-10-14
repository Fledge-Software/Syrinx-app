# Plan 2 prompt: plan the customisable Home page and widgets

**Plan 2 work. Start only when plan 2 (the plugin runtime) begins** (owner,
2026-10-14); it is linked from `docs/plans/Syrinx v1 — Plugins and Fledge — Plan.md`
and the Plan 2 section of `docs/plans/Syrinx v1 — Plans 2–4 slice proposals.md`. This
prompt records what the owner asked for and decided so the work starts from
those decisions instead of rediscovering them. Nothing here is approved for
implementation yet: the first deliverable is a plan, reviewed by the owner.

## Why this matters

Home (the new-tab page) is the first thing the owner sees in every new tab.
Today it is fixed: a header, the search pill, a Quick access grid and a strip
of plugin cards (`crates/syrinx-app/src/ui/home.rs`). The owner wants it to be
their own workspace, the way Obsidian's panes are: choose what is on it,
arrange it, size it, and keep that arrangement. Widgets make Home useful at a
glance (today's weather, the Fledge leads waiting, the note last edited)
without opening a tab for each.

## What the owner asked for (2026-10-14)

1. **One set of tiles for tabs and plugins.** The owner picks a set of tabs
   (sites) and plugins, shown inline together, replacing today's separate
   Quick access grid and plugin strip.
2. **Widgets.** Built-in widgets and widgets that plugins provide. Installing
   a plugin gives the owner its widgets to place on Home.
3. **Split-pane layout, saved.** Home is divided into sections the owner can
   split, resize by dragging dividers and move by dragging, like Obsidian's
   panes. The layout persists.
4. **Update the design canvas's Home boards** (`design/README.md` links the
   live canvas) to show the new Home before code changes.

## Decisions already taken (do not reopen)

- **Layout model: split panes** (owner chose this over a snap-to-grid
  dashboard and a fixed stack). A section can be split left/right or
  top/bottom, its dividers dragged, and a section dragged onto another to
  move it. Saved as one layout in the local store (D13 rules apply).
- **Weather comes from a plugin, never from Syrinx.** Syrinx makes no network
  request of its own for widgets (AGENTS.md, "Adding any network call Syrinx
  makes on its own"). A developer builds a weather plugin, as Notes, Forge and
  Fledge are built, and it brings its own data.
- **Plugin widgets are part of plan 2's plugin design, not earlier.** How an
  author builds a widget is undecided. The owner's direction: plugins come
  from a template repository, are submitted for review, and approved ones are
  added to the Plugins list. Record this under the plan's "Open threads"; do
  not design the widget API before the Plugin trait work.

## Open questions to settle with the owner in the plan

- Widget format for plugins: declarative shapes Syrinx draws in its own style
  (stat, list, chart, card, small form), a sandboxed local web view per
  widget, or both. Only owned, serialisable values may cross the Plugin trait
  (AGENTS.md, "Plugins"), which favours declarative.
- Which built-in widgets ship first (clock/date, open tabs, recent history,
  saved pages; Notes and Fledge widgets once those plugins exist; Fledge only
  through its stand-in client, D24).
- Whether the search pill stays fixed at the top or becomes a movable section.
- Reset-to-default, and whether layouts are per window or global (there is
  one workspace and no profiles, per the spec).
- Keyboard access for splitting, resizing and moving sections (P1-36's rules).
- Review of the template repository and the Plugins list: who reviews, and
  how a reviewed plugin reaches the list. This touches publishing and needs
  the owner.

## Reading list

- `docs/decisions.md`, `docs/spec.md` (Home page, Plugin API), the plan 2 file
  in `docs/plans/`, and `design/README.md`.
- `crates/syrinx-app/src/ui/home.rs`, `ui/tiles.rs`, `ui/tile_editor.rs` and
  `crates/syrinx-core/src/destinations.rs` (Quick access and Favorites today).
- `docs/trapstoknow.md`, section "The interface: GPUI on Windows": dragging,
  focus and popups have known traps.

## Deliverable

A plan section (or a new plan file, if plan 2's breakdown says so) with the
decisions above, the open questions answered by the owner, and a slice
breakdown. No code, no commits and no canvas edits until the owner approves
the plan.
