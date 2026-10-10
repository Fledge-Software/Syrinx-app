---
title: "Atlas v1 — Forge — Plan (plan 4, outline)"
status: draft
created: 2026-10-09
subject: Atlas v1 plan 4 — the Forge code plugin (spec phase 5)
relates_to: "docs/plans/Atlas v1 — Plan.md (plan 1, and the v1-wide decisions D1–D36); docs/plans/Atlas v1 — Plugins and Fledge — Plan.md; docs/research/04-forge-and-notes.md"
issues_repo: not yet created
branch: claude/plan-atlas-v1
---

# Atlas v1 — Forge (outline)

> **Status: draft outline.** Detailed after the Phase 0 verdict (D16).

## Why this exists

The owner uses PyCharm for every language and wants it inside the browser
(`docs/spec.md`, "Forge"). PyCharm's workflow is the direction; v1 is
deliberately thin (D5).

## Scope (thin v1)

In the order note 04 recommends (`docs/research/04-forge-and-notes.md`,
"What this means for Atlas v1"):

1. **Buffer and highlighting:** a Rust document actor per file (ropey,
   Tree-sitter) and CodeMirror 6 in a local web view (D28); about 20
   compiled-in grammars listed in a Helix-style `languages.toml`; query files
   only from MIT, Apache or MPL sources, never Zed's GPL ones.
2. **Terminal:** portable-pty (ConPTY) and xterm.js, output coalesced per
   frame, with flow control.
3. **Git:** reads in process behind a `GitRead` trait; writes through the
   `git` command line, requiring Git for Windows (D29); diffs in
   `@codemirror/merge`.
4. **Language servers:** a Rust LSP client for servers already installed
   (D30).
5. **Run control** from Cargo, npm, just, make and pyproject (D31), then
   split editor groups and the context rail.

The project tree; back and forward between code locations; the Forge tab
hiding the sidebar by default (`design/README.md`, "Forge").

## Not in v1

The debugger (D31); downloading language servers (D30); interactive rebase,
a three-way merge editor, a minimap, semantic tokens; a native editor.

## Open threads

- A key-ownership table (Atlas, editor, terminal) before building: Ctrl W
  and Ctrl K are shell keys, and Ctrl K is PyCharm's commit key (realism
  check, objection 12).
- Keystroke-to-highlight latency over WebView2: if p95 exceeds one frame,
  highlight in the web view with web-tree-sitter (note 04, "Risks").
- Which Python task runners the owner uses (D31 assumes plain pyproject).
