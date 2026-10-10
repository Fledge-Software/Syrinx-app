---
title: "Atlas v1 — Notes — Plan (plan 3, outline)"
status: draft
created: 2026-10-09
subject: Atlas v1 plan 3 — the Notes plugin (spec phase 4)
relates_to: "docs/plans/Atlas v1 — Plan.md (plan 1, and the v1-wide decisions D1–D36); docs/plans/Atlas v1 — Plugins and Fledge — Plan.md; docs/research/04-forge-and-notes.md"
issues_repo: not yet created
branch: claude/plan-atlas-v1
---

# Atlas v1 — Notes (outline)

> **Status: draft outline.** Detailed after the Phase 0 verdict (D16).

## Why this exists

The owner wants notes in the same window and the same search as the browser
and code (`docs/spec.md`, "Notes"): roughly Obsidian crossed with Notion,
with no graph view (S4).

## Scope (thin v1, D5)

- **A vault is a folder of `.md` files**, Obsidian-compatible (D27):
  wikilinks, Obsidian's property types, a subset of its `.base` format.
- **Rust indexer:** pulldown-cmark and serde-saphyr into SQLite tables for
  notes, headings, links and properties, feeding both search tiers; every
  change Atlas makes is a byte-range patch.
- **The editor:** CodeMirror 6 with live preview in a local web view (D28),
  with Atlas's shortcut routing on that web view.
- **Backlinks, rename with link rewrite, the outline panel,** outside-edit
  handling (debounced watcher, conflict banner when dirty).
- **One table view** over a folder or tag: frontmatter columns, one sort,
  AND filters, inline edits written back losslessly.

## Not in v1

Formulas, relations, rollups, kanban and calendar views, block references,
embeds, any WYSIWYG mode, a graph view (`docs/research/04-forge-and-notes.md`,
"Notes"). No IME testing (D32).

## Open threads

- The full-text store is chosen in plan 1's tier 2 bake-off; confirm it on
  the owner's notes.
- Where the owner's existing notes live today, and whether Atlas opens that
  folder as the vault.
