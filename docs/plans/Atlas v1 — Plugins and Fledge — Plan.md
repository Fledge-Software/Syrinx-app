---
title: "Atlas v1 — Plugins and Fledge — Plan (plan 2, outline)"
status: draft
created: 2026-10-09
subject: Atlas v1 plan 2 — the plugin runtime and the Fledge plugin on a stand-in client (spec phase 3)
relates_to: "docs/plans/Atlas v1 — Plan.md (plan 1, and the v1-wide decisions D1–D36); docs/research/03-core-architecture.md; docs/research/05-fledge-api.md"
issues_repo: not yet created
branch: claude/plan-atlas-v1
---

# Atlas v1 — Plugins and Fledge (outline)

> **Status: draft outline.** Detailed after the Phase 0 verdict (D16). The
> decisions it rests on are in plan 1's "Decisions taken".

## Why this exists

Atlas's reason to exist is that code, notes and Fledge live inside the
browser and share one palette and one search (`docs/spec.md`, "What this
is"). That needs a plugin runtime first; Fledge is the first plugin because
its inline palette action ("New lead") proves the runtime end to end without
an editor.

## Scope (D34: thin)

- **The plugin runtime:** `atlas-plugin-api` with the `Plugin` trait,
  manifest, `ViewSpec` (component, form, list), `Outcome` and `IndexFeed`
  (`docs/research/03-core-architecture.md`, "The plugin trait"); built-ins
  registered in a plain list; calls on async tasks behind a timeout and
  `catch_unwind`; per-plugin settings; a plugin row in the sidebar that stays
  in the Plugins list when closed (`design/README.md`).
- **Palette integration:** plugin commands as a scope chip; `FormSpec`
  rendered in the preview pane by `atlas-ui`; Tab between fields, Enter
  creates, Ctrl Enter creates and opens (`design/README.md`).
- **The Fledge plugin on a stand-in:** a `FledgeClient` trait in a
  `fledge-client` crate with a stub implementation; "New lead" with Name,
  Company, Source and Est. value (Company modelled as Nest will hold it,
  D26).

## Later, with the real client (after Stage C, D24)

OAuth 2.1 as an ordinary client (D23); the structured REST API (D25); the
per-kind keep-local switches (D19); Atlas as Fledge's full desktop app
(D20). The Fledge-side work in `docs/research/05-fledge-api.md` ("Work
needed on the Fledge side") is a list for the owner, not issues here.

## Phases (to detail)

1. The runtime and a fake plugin under test.
2. Plugin commands and forms in the palette.
3. The Fledge plugin on the stand-in client.

## Open threads

- Third-party plugins come later (D17); confirm the trait stays WASM-ready.
- Whether plan 1's toolkit decision (D4) changes how plugin views are drawn.
