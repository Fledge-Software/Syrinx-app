# Architecture

This page is a map for someone opening the repository for the first time: what
each crate is for, and how the big pieces (pages, the interface, state,
storage, search and updates) fit together. It explains; it does not decide.
The authorities are
[`docs/architecture-decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/architecture-decisions.md),
which holds decisions D1–D49 with what was rejected and why, and
[`docs/decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/decisions.md),
the owner's dated decisions since the spec, which win wherever they disagree
with anything else. If this page and those files disagree, they are right
and this page is stale.

## The shape in one paragraph

Syrinx is one Windows process with one main window. Each web page is a
WebView2 child window placed inside Syrinx's "page card", the rounded area
right of the sidebar. Everything around the card, and the palette and
sidebar peek that float above it, is drawn by GPUI. Both are driven from
Rust, and Rust owns every piece of state: the tab list, saved pages,
history, settings and search. The interface only draws that state and
reports what the owner did. There is no JavaScript application and no
inter-process messaging layer between the interface and the core; the app
calls the core directly.

## The crates

Syrinx is a Cargo workspace under `crates/`. Each crate's first `//!` line
says why it exists; those lines are quoted here.

| Crate | What it is for |
|---|---|
| `syrinx-app` | "Opens the integrated browser and keeps its native workers alive through orderly shutdown." The `syrinx` executable: window, sidebar, page card, Home, palette and the wiring between them. Also builds `syrinx-setup`, the setup program (P1-37). |
| `syrinx-core` | "Owns Syrinx's application state independently of its interface and browser engine." The one generic tab model, saved pages and folders, the session, the closed-tab stack and the command table. |
| `syrinx-store` | "Persists Syrinx state through a checked storage location and a serialized writer." SQLite through rusqlite: migrations, settings, saved pages, history, sessions and snapshots. |
| `syrinx-search` | "Ranks local catalogue metadata without owning tab state or performing I/O." The in-memory catalogue the palette searches on every keystroke. |
| `syrinx-ui` | "Syrinx's design tokens as plain data, so the chrome draws every colour and size from one place." Palettes, accent, metrics and sidebar widths, with no toolkit dependency; `syrinx-app` converts them to GPUI colours. |
| `syrinx-platform` | "Confines Windows storage, page, single-instance, calendar, Edge, icon, disk, backdrop and setup services to the platform boundary." Everything WebView2 and Win32. |
| `syrinx-update` | "Local verification of signed update packages and latest statements, and the staging and install steps built on it." No network and no interface. Builds `syrinx-apply`, the helper that swaps in an update, and the release tool used by the weekly workflow. |
| `syrinx-update-net` | "Downloads Syrinx's update files from the one compiled-in host over WinHTTP, and only when the owner presses 'Check for updates'." |
| `xtask` | "Runs the local development gate and checks the workspace's dependency boundaries." `cargo xtask check` and `cargo xtask package`. |
| `fulltext-bench` | "Compares disposable synthetic full-text caches; no product crate uses this harness." The P1-23 bake-off between Tantivy and SQLite FTS5. |

The boundaries are enforced, not just intended: `cargo xtask check-deps`,
part of the gate, fails if a crate reaches past its layer. Only
`syrinx-app` and `syrinx-ui` may depend on GPUI (and today `syrinx-ui` uses
none of it), only `syrinx-platform` may depend on `wry`, `webview2-com` or
raw Win32, with narrow, listed exceptions: `syrinx-update` may call
Windows' cryptography through the `windows` crate, and `syrinx-update-net`
is the only crate allowed to use WinHTTP. As a result `syrinx-core`,
`syrinx-store` and `syrinx-search` are tested with plain `cargo test`, with
no window and no browser engine. That is what keeps the core model and the
search index testable, which the spec asks for.

The plugin crates the earlier plans describe (`syrinx-plugin-api`,
`crates/plugin-*`, `fledge-client`) do not exist; the plans were removed on
2026-10-16 and are read at `af3de53`. On 2026-10-15 the owner decided
plugins will be sandboxed WebAssembly in their own repositories instead.
The runtime is decided too: wasmtime with the component model (research
note 10, approved on 2026-10-21), hosted by `crates/syrinx-plugin-host`,
the only crate that will link wasmtime, with the `syrinx:plugin` WIT package
at `wit/v0_1/` (2026-10-24). Neither exists yet; #28 builds them. See
[Plugin system](Plugin-system).

Several crates (GPUI, wry, SQLite's wrapper and the fuzzy matcher frizbee)
are vendored under `vendor/` with small, owner-approved patches. Each patch
has a review in `docs/dev/vendor-reviews/` recording the exact diff and why
it was needed.

## How pages render

A **web view** is an embedded browser engine drawing one page. Syrinx uses
**WebView2**, the Chromium-based engine that Microsoft ships with Windows
(D3), in its Evergreen form, which Windows keeps up to date. Syrinx bundles
no Chromium. WebView2 already carries Google's Widevine module, so YouTube
and Disney+ play protected video with no DRM code in Syrinx.

Syrinx drives WebView2 through **wry**, a Rust library that creates and
places web views. Every tab is a wry child web view of the main window. All
tabs share one WebView2 environment, which Syrinx builds itself so it can
keep crash dumps local, keep SmartScreen (phishing and malware protection)
on, and keep WebView2's password saving off. Where wry has no API, Syrinx
calls WebView2 directly through `webview2-com`: browser shortcuts while a
page has focus, fullscreen video, crashed renderers and suspending hidden
tabs.

Page web views are left exactly as WebView2 makes them. Syrinx never sets a
custom user agent, attaches an IPC handler (a JavaScript-to-Rust message
channel) or injects scripts on them. Google treats an app that tampers
with its pages as a possible middleman and blocks sign-in, and an IPC
handler was reported to break Google Docs. These rules are in `AGENTS.md`
and their research is in
[Background 07](Background-07-google-sign-in-and-drm).

## The interface: GPUI

Syrinx's own interface is drawn with **GPUI**, the GPU-accelerated Rust
toolkit Zed's editor is built on (D4), with the `gpui-component` widget set.
Phase 0 compared it with a Tauri and Svelte build and GPUI won by far more
than D4's rule required; see
[Background 09](Background-09-phase-0-verdict). GPUI is pinned exactly,
because its snapshot releases change the API weekly.

Two Windows facts shape the layout. GPUI draws above the WebView2 child
window, so nothing of Syrinx's is painted inside the page card's area.
Overlays that must sit over a page, the palette and the sidebar peek, are
separate borderless popup windows owned by the main window. Syrinx also
carries a patch to GPUI's idle loop (D37), because unpatched GPUI on Windows
repaints every window on every display refresh and uses CPU while idle.

## Rust owns the state

The spec's rule is that Rust owns all state and logic and the interface is
thin. The **tab model** in `syrinx-core` is one generic model: a tab has a
kind (web, plugin view, note, file, diff, terminal) and a state, and saved
pages are separate records a live tab may be associated with. The sidebar and
the palette both read this one model, never per-feature code. Changes are
persisted as they happen, through a session worker, so a crash loses at most
the last uncommitted change. The one planned exception to "Rust owns state"
is the text editor for Notes and Forge: one host-provided CodeMirror 6
component, which Syrinx ships on its own editor origin (2026-10-21), owns
the live buffer while typing, and Rust saves only when both copies'
checksums match (D28).

## Storage

Durable state lives in one SQLite database at
`%LOCALAPPDATA%\Syrinx\data\syrinx.db`, opened through rusqlite with
write-ahead logging, one writer thread and readers (D13). It is never placed
in a synced folder such as OneDrive, because SQLite's write-ahead log
breaks there; `syrinx-platform` checks the folder before the store opens
it. Once a day, when idle, Syrinx writes a `VACUUM INTO` snapshot and keeps
seven. After a crash it checks the database, and it replaces a damaged
database with the newest verified snapshot only after the owner confirms
(D46). A small deletion record makes sure that restoring an older snapshot
cannot bring back history the owner cleared (D47).

## Search, in two tiers

- **Tier 1, in memory.** One row per titled thing (open tabs, saved pages,
  recent history, commands), ranked by the frizbee fuzzy matcher with
  frecency (frequency and recency) and an open-tab boost, on every
  keystroke. The budget is under 5 ms at the 95th percentile for 100,000
  rows (D48). The palette never fetches live search suggestions, so what the
  owner types stays on the machine (D9).
- **Tier 2, full text.** Page text kept for seven days (D18, D40) so the
  owner can find a page by what it said, excluding Google's sites, banking
  and payment sites, pages sent with `Cache-Control: no-store` and sites the
  owner excluded (D35). The engine is Tantivy: the P1-23 bake-off chose it
  over SQLite FTS5 and the owner confirmed it on 2026-10-24 (#52), pinned
  at 0.27.0. The adapter that indexes into it is #53, not built yet. The
  exclusion rules are in
  [`docs/page-text-indexing.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/page-text-indexing.md).

## Updates and releases

Syrinx makes no network call on its own. It checks for an update only when
the owner presses "Check for updates", and fetches fixed files from one
HTTPS host compiled into the executable (`syrinx-update-net`, over WinHTTP).
Before anything is installed, `syrinx-update` verifies a signed
latest-version statement and the package against public keys pinned in the
program. An update is installed only on restart, and the previous version
is kept and restored if the new one does not start. The format is in
[`docs/update-format.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/update-format.md)
and the delivery design in
[`docs/update-delivery.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/update-delivery.md).
Preview builds are made every Thursday by a GitHub Actions workflow; see
[`docs/releasing.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/releasing.md)
and, for the setup program, [`docs/installing.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/installing.md).

## Privacy, in brief

The usage report counts features, timings and error categories locally,
and is exported only when the owner chooses (D39). Crash dumps stay on the
machine. WebView2's password saving is off. The details are in
[`docs/usage-report-privacy.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/usage-report-privacy.md)
and the guardrails in `AGENTS.md`.
