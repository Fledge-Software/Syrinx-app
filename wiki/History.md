# History

This page tells the story of the project so far, for a reader who wants to
know how Syrinx got to where it is, and links every document that was
retired from the repository on 2026-10-16. Nothing was lost: each retired
file is still in git history, and the links below point at the last commit
that had it, `af3de53`.

## Timeline

**The spec and the research (2026-10-09).** The owner wrote the product spec
for a browser-first workspace then called Atlas: an Arc-style browser with
a vertical-tab sidebar and a central command palette, with code, notes and
the owner's business software, Fledge, as built-in plugins. The spec
proposed Tauri 2 with a Svelte interface and asked for a feasibility spike
before anything else. The same day, research notes 01 to 08 (now the
Background pages of this wiki) examined the shell, the browser's v1 scope,
the core architecture, the plugins, Fledge's API, engine and interface
performance, Google sign-in and DRM, and GPUI and wry on Windows. Four v1
plans were drafted from them: Plan 1 for the engine, browser core and
palette, Plan 2 for the plugin runtime and Fledge, Plan 3 for Notes and
Plan 4 for Forge.

**Phase 0 (2026-10-09 to 2026-10-10).** Two throwaway spikes were built side
by side, one drawing the interface with GPUI and one with Tauri and Svelte,
both rendering pages in WebView2 through wry. The verdict
([Background 09](Background-09-phase-0-verdict)) was "go, with GPUI": Google
sign-in with a passkey and with a Workspace account, YouTube and Disney+ all
passed, and GPUI beat the Tauri control by far more than decision D4's rule
required. Two conditions came with it: GPUI's idle CPU had to be fixed,
which a small pinned patch later did (D37), and the musts Phase 0 did not
test moved into Plan 1's exit criteria. On 2026-10-10 the owner approved all
four plans and Plan 1's 40-slice breakdown, and agents began building the
slices one branch at a time, each reviewed independently, with the owner
approving every commit and merge.

**Plan 1 (from 2026-10-10).** The slices, P1-01 to P1-40, built the
foundations first: the workspace and gate, the window-independent tab
model, storage with snapshots and confirmed recovery, the in-memory search
catalogue and the GPUI idle patch. Then came the browser shell (popups,
permissions, find and zoom, the security display, suspension and
fullscreen, process recovery, the sidebar peek), browser memory (session
restore, history, folders, bookmark import, clearing data, Home and
Favorites), the command palette and address bar, and the work towards
daily use (one running instance, accessibility, the setup program and
owner-pressed updates). Reviewed work first collected on an integration
branch; on 2026-10-12 the owner directed every finished slice to merge to
`main` once its review and the gate passed. Plan 1 is still open: its last
slice is the owner's one-week daily-driver acceptance.

**Atlas becomes Syrinx (2026-10-12).** "Atlas" collided with OpenAI's
ChatGPT Atlas, a browser in the same category. The owner named the product
Syrinx, after a bird's vocal organ, which fits beside Fledge. Records
written before then keep the old name.

**GitHub and the licence (2026-10-13).** The repository moved to GitHub
as `Fledge-Software/Syrinx-app`, Syrinx was licensed under MPL-2.0, and the
owner asked for a weekly preview release so that people following the
project get a predictable build. The same day the owner approved the
update-delivery design: Syrinx checks for updates only when the owner asks.

**Preview releases (from 2026-10-14).** The first preview, v0.1.0, started
only on the machine that built it, because GPUI compiled its shaders at run
time from the build machine's source folder; a second approved GPUI patch
embedded them. A later diagnosis found that a background painted under the
page card had hidden every page since the Home and settings slices, v0.1.0
and v0.2.0 included. Release 0.2.0 shipped as a zip, which led to the
decision that Syrinx ships as a setup program that installs and updates
itself (P1-37). By 2026-10-16 five previews, v0.1.0 to v0.5.0, had been
published.

**Plugins and engines (2026-10-15).** The owner decided that plugins will be
sandboxed WebAssembly, each in its own repository made from a template,
starting only after Plan 1 closes ([Plugin system](Plugin-system)), and that
each platform's build uses its own system web engine with no bundled
Chromium. The owner also replaced the five fixed themes with Graphite,
Paper and a theme editor.

**Work moves to issues (2026-10-16).** The plans' remaining work, their open
threads and the deferred-work register became GitHub issues, organised as
epics with sub-issues. `docs/` was cut down to what describes the app and
its operation, contributor material moved to `docs/dev/`, and the research,
ideas and history moved to this wiki. Plan 1's "Decisions taken" table
became `docs/architecture-decisions.md`. The documents below were removed
from the repository at that point.

## Retired documents

Every file removed on 2026-10-16, linked at `af3de53`. They are records of
their time: read them for reasoning and evidence, not for current fact.
Where one of them disagrees with `docs/decisions.md` or
`docs/architecture-decisions.md`, those win.

### Research notes

The eleven research notes now live in this wiki as the Background pages; these links are the originals.

- [`docs/research/01-shell-and-engine.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/01-shell-and-engine.md): Whether Atlas could run on Tauri 2 child webviews; now [Background 01](Background-01-shell-and-engine).
- [`docs/research/02-browser-v1-scope.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/02-browser-v1-scope.md): What a daily browser needs at v1 and what can wait; now [Background 02](Background-02-browser-v1-scope).
- [`docs/research/02a-tauri-wry-facts.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/02a-tauri-wry-facts.md): What a Tauri 2 and wry tab can and cannot do; now [Background 02a](Background-02a-tauri-wry-facts).
- [`docs/research/02b-default-browser-import-signing.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/02b-default-browser-import-signing.md): Default-browser registration, bookmark import and code signing; now [Background 02b](Background-02b-default-browser-import-signing).
- [`docs/research/03-core-architecture.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/03-core-architecture.md): Plugins, the Rust and interface boundary, search, storage, testing and crate layout; now [Background 03](Background-03-core-architecture).
- [`docs/research/04-forge-and-notes.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/04-forge-and-notes.md): Editor, syntax, language servers, git, terminal and notes for the two plugins; now [Background 04](Background-04-forge-and-notes).
- [`docs/research/05-fledge-api.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/05-fledge-api.md): Fledge's API surface as the Fledge plugin would see it; now [Background 05](Background-05-fledge-api).
- [`docs/research/06-engine-and-ui-performance.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/06-engine-and-ui-performance.md): Which engine and interface toolkit Atlas should be built on; now [Background 06](Background-06-engine-and-ui-performance).
- [`docs/research/07-google-sign-in-and-drm.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/07-google-sign-in-and-drm.md): Google sign-in and protected video in WebView2; now [Background 07](Background-07-google-sign-in-and-drm).
- [`docs/research/08-gpui-and-wry-on-windows.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/08-gpui-and-wry-on-windows.md): The GPUI and wry facts the Phase 0 spike relied on; now [Background 08](Background-08-gpui-and-wry-on-windows).
- [`docs/research/09-phase-0-verdict.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/09-phase-0-verdict.md): The Phase 0 verdict: go, with GPUI, on two conditions; now [Background 09](Background-09-phase-0-verdict).

### Plans

The four v1 plans the owner approved on 2026-10-10, their readiness review and slice proposals. Their remaining work is now in issues, and Plan 1's "Decisions taken" table is now `docs/architecture-decisions.md`.

- [`docs/plans/Syrinx v1 — Plan.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/plans/Syrinx%20v1%20%E2%80%94%20Plan.md): Plan 1: the engine, browser core and command palette, with the v1-wide decisions D1–D49, the 40-slice issue map and the deferred-work register.
- [`docs/plans/Syrinx v1 — Plugins and Fledge — Plan.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/plans/Syrinx%20v1%20%E2%80%94%20Plugins%20and%20Fledge%20%E2%80%94%20Plan.md): Plan 2: the plugin runtime and the Fledge plugin on a stand-in client (spec phase 3).
- [`docs/plans/Syrinx v1 — Notes — Plan.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/plans/Syrinx%20v1%20%E2%80%94%20Notes%20%E2%80%94%20Plan.md): Plan 3: the Notes plugin (spec phase 4).
- [`docs/plans/Syrinx v1 — Forge — Plan.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/plans/Syrinx%20v1%20%E2%80%94%20Forge%20%E2%80%94%20Plan.md): Plan 4: the Forge code plugin (spec phase 5).
- [`docs/plans/Syrinx v1 — Readiness and implementation breakdown.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/plans/Syrinx%20v1%20%E2%80%94%20Readiness%20and%20implementation%20breakdown.md): The approved readiness review and Plan 1's 40-slice local breakdown, with the analytics amendment.
- [`docs/plans/Syrinx v1 — Plans 2–4 slice proposals.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/plans/Syrinx%20v1%20%E2%80%94%20Plans%202%E2%80%934%20slice%20proposals.md): The slice breakdowns for Plans 2 to 4 and their common gates, which the WASM plugin decision will re-cut.
- [`docs/plans/Syrinx later — Mobile sync outline.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/plans/Syrinx%20later%20%E2%80%94%20Mobile%20sync%20outline.md): The deferred outline for device sync over Tailscale or Headscale; now [Ideas: Mobile sync](Ideas-Mobile-sync).

### Proposals

- [`docs/proposals/widevine-third-build.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/proposals/widevine-third-build.md): A proposal, not approved, for a Widevine build where no system engine provides it; now [Ideas: Widevine third build](Ideas-Widevine-third-build).

### Status ledgers, hand-offs and session prompts

Notes written so one agent session could pick up where another stopped. The issues replace them.

- [`docs/implementation-status.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/implementation-status.md): The implementation and evidence ledger: each slice's state, merge commit and latest gate against all four plans.
- [`docs/handoff-2026-10-10.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/handoff-2026-10-10.md): Hand-off prompt to continue Atlas from the Phase 0 verdict.
- [`docs/handoff-integrated-preview-2026-10-10.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/handoff-integrated-preview-2026-10-10.md): Checkpoint for bringing the integrated browser forward under D49 without losing approved foundations.
- [`docs/handoff-p1-02-2026-10-10.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/handoff-p1-02-2026-10-10.md): P1-02 complete: resume at P1-03.
- [`docs/handoff-p1-03-2026-10-10.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/handoff-p1-03-2026-10-10.md): Hand-off after the P1-03 storage merge.
- [`docs/handoff-p1-04-2026-10-10.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/handoff-p1-04-2026-10-10.md): Checkpoint for continuing P1-04: storage lifecycle, backups and confirmed recovery.
- [`docs/handoff-p1-05-2026-10-10.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/handoff-p1-05-2026-10-10.md): Checkpoint for continuing the P1-05 local search slice.
- [`docs/next-session-prompt.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/next-session-prompt.md): The agent half of the 2026-10-14 evening hand-off; the owner's half became `docs/owner-checklist.md`.
- [`docs/prompts/astra-common-rules.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/prompts/astra-common-rules.md): Rules every parallel "Astra" agent session followed so sessions did not collide.
- [`docs/prompts/astra-1-p1-05-search-acceptance.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/prompts/astra-1-p1-05-search-acceptance.md): Astra task 1: finish P1-05 by meeting D48's search budget, or prove why the design cannot.
- [`docs/prompts/astra-2-p1-11-to-p1-14-browser-essentials.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/prompts/astra-2-p1-11-to-p1-14-browser-essentials.md): Astra task 2: browser essentials, P1-11 to P1-14.
- [`docs/prompts/astra-3-p1-23-fulltext-bakeoff.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/prompts/astra-3-p1-23-fulltext-bakeoff.md): Astra task 3: choose the full-text engine with evidence (P1-23).
- [`docs/prompts/home-layout-and-widgets.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/prompts/home-layout-and-widgets.md): Prompt for planning the customisable Home page and widgets once Plan 2 begins; now issue #31.

### Plan 1 slice notes

Each slice of Plan 1 kept a note of what was built, how it was verified and what the owner still had to check. The lasting findings from them are in `docs/dev/trapstoknow.md`.

- [`docs/storage-path-preflight.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/storage-path-preflight.md): P1-03: refusing a database folder inside a synced folder, and the limits of that check.
- [`docs/p1-04-recovery-preflight.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-04-recovery-preflight.md): P1-04: backups that stay usable after an interruption, guarded restore and the deletion journal.
- [`docs/p1-05-search.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-05-search.md): P1-05: measuring the in-memory search catalogue before connecting the palette.
- [`docs/p1-07-design-tokens.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-07-design-tokens.md): P1-07: design tokens, so every view draws colours and sizes from one place.
- [`docs/p1-08-sidebar-states.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-08-sidebar-states.md): P1-08 and P1-09: sidebar width states and folder rows.
- [`docs/p1-09-21-folder-ui.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-09-21-folder-ui.md): P1-09 and P1-21: folder editing in the sidebar and remembered folder expansion.
- [`docs/p1-11-popups.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-11-popups.md): P1-11: popups opened as tabs that keep `window.opener`, and the fixture-timeout diagnosis.
- [`docs/p1-12-permissions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-12-permissions.md): P1-12: an explicit permission bar, and site consent that is not persisted by WebView2 behind Syrinx's back.
- [`docs/p1-13-essentials.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-13-essentials.md): P1-13: find, downloads, zoom and other browser essentials reachable without taking keys from pages.
- [`docs/p1-14-security-display.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-14-security-display.md): P1-14: truthful connection labels and certificate rejection.
- [`docs/p1-15-suspension-fullscreen.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-15-suspension-fullscreen.md): P1-15: suspending hidden pages and fullscreen video.
- [`docs/p1-16-process-recovery.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-16-process-recovery.md): P1-16: recovering from a failed renderer or browser process without replacing the session.
- [`docs/p1-17-peek.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-17-peek.md): P1-17: the sidebar peek that reaches a hidden sidebar over a live page.
- [`docs/p1-19-20-restore-history.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-19-20-restore-history.md): P1-19 and P1-20: session restore and history.
- [`docs/p1-21-folders-core.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-21-folders-core.md): P1-21: the headless core and storage for saved-page folders.
- [`docs/p1-22-import-parser.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-22-import-parser.md): P1-22: the HTML bookmark import parser.
- [`docs/p1-23-fulltext-bakeoff.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-23-fulltext-bakeoff.md): P1-23: the Tantivy against SQLite FTS5 bake-off, its retention checks and its unfinished performance runs.
- [`docs/p1-25-page-text.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-25-page-text.md): P1-25: synthetic feasibility of reading page text within D35's exclusions; its findings F1–F27.
- [`docs/p1-27-clear-data.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-27-clear-data.md): P1-27: clearing browsing data.
- [`docs/p1-28-29-home-settings.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-28-29-home-settings.md): P1-28 and P1-29: the Home page, Favorites and saved appearance.
- [`docs/p1-30-31-palette.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-30-31-palette.md): P1-30 and P1-31: the command palette.
- [`docs/p1-32-address-bar.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-32-address-bar.md): P1-32: the palette as the address bar.
- [`docs/p1-33-palette-acceptance.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-33-palette-acceptance.md): P1-33: palette acceptance measured from key press to pixels.
- [`docs/p1-34-35-instance-runtime.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-34-35-instance-runtime.md): P1-34 and P1-35: a single running Syrinx, and the WebView2 runtime-change notice.
- [`docs/p1-36-accessibility.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-36-accessibility.md): P1-36: keyboard navigation and screen-reader accessibility, with the UI Automation probe.
- [`docs/an-01-local-analytics.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/an-01-local-analytics.md): AN-01: local usage counting.
- [`docs/an-02-report-export.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/an-02-report-export.md): AN-02: the usage report preview and explicit export.
- [`docs/integrated-browser-preview.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/integrated-browser-preview.md): Making the existing product work usable in one browser window (the integrated preview under D49).

### Reviews and investigations

One-off reviews and diagnoses. Where one produced a lasting rule, that rule is now a trap in `docs/dev/trapstoknow.md`.

- [`docs/parallel-browser-navigation-review-2026-10-10.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/parallel-browser-navigation-review-2026-10-10.md): Browser navigation reliability review, including the nonblocking-socket fixture bug.
- [`docs/parallel-browser-usability-review-2026-10-10.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/parallel-browser-usability-review-2026-10-10.md): Source review of browser usability before plugin work.
- [`docs/parallel-focus-review-2026-10-10.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/parallel-focus-review-2026-10-10.md): Review of palette focus ordering and keyboard focus returning to the page.
- [`docs/fixture-timeout-diagnosis-plan.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/fixture-timeout-diagnosis-plan.md): Diagnosis of the permission and certificate fixture timeouts: hidden pages suspended mid-load.
- [`docs/fixture-transport-timing-review.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/fixture-transport-timing-review.md): Test-socket lifetime around the native fixture's final log.
- [`docs/smoke-send-failure-review.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/smoke-send-failure-review.md): Making the smoke run fail on a worker transport error even when workers stop cleanly.
- [`docs/missing-primary-startup-review.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/missing-primary-startup-review.md): Guarded start-up when the primary database is missing.
- [`docs/wry-creation-lifetime-review.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/wry-creation-lifetime-review.md): Review of failed native page creation that preceded the approved wry failed-construction patch.
- [`docs/browser-parent-host-review.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/browser-parent-host-review.md): Proposal to give wry a parent window owned by the browser thread.
- [`docs/webview2-breakpoint-investigation.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/webview2-breakpoint-investigation.md): Plan to reproduce the reported WebView2 breakpoint dialog before choosing a fix.
### Moved, not removed

These files were renamed or moved in the same restructure and are still in
the repository: `docs/LOCAL-DEV.md` and `docs/trapstoknow.md` moved to
`docs/dev/`; `docs/p1-37-installer-design.md` became `docs/installing.md`;
`docs/p1-38-update-format.md` became `docs/update-format.md`;
`docs/proposals/p1-39-update-delivery.md` became `docs/update-delivery.md`;
`docs/proposals/an-01-02-local-analytics.md` became
`docs/usage-report-privacy.md`; `docs/proposals/p1-25-page-text-extraction.md`
became `docs/page-text-indexing.md`; `docs/owner-next-steps.md` became
`docs/owner-checklist.md`; `docs/p1-23-fulltext-audit.md` became
`docs/dev/fulltext-audit.md`; and the vendored-dependency reviews, with the
P1-06 evidence in `docs/evidence/p1-06/`, moved to `docs/dev/vendor-reviews/`.
