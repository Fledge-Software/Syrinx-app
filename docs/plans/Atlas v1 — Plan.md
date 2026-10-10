---
title: "Atlas v1 — Plan"
status: draft
created: 2026-10-09
subject: Atlas v1, the owner's daily browser on Windows, with the command palette and the Forge, Notes and Fledge plugins
relates_to: "docs/spec.md; docs/decisions.md; design/README.md; docs/research/"
issues_repo: not yet created (the repository is local; the owner creates it before anything is published)
branch: claude/plan-atlas-v1
---

# Atlas v1

> **Status: draft.** Not a source of truth until it is marked `active`.
> Where it disagrees with `docs/decisions.md`, `docs/spec.md` or
> `design/README.md`, those win, and the disagreement belongs under Open
> threads.

## Why this exists

The owner works across a browser, an IDE (PyCharm), a notes app and their own
business software, Fledge. Each holds part of the context, and none can
search the others. Atlas puts all four in one window: a browser first, with
code, notes and Fledge as built-in plugins that open as tabs, and one command
palette and one search over everything (`docs/spec.md`, "What this is").

The spec's phased plan (`docs/spec.md`, "Phased plan") and the research in
`docs/research/` are done; there is no code yet. This plan turns them into
work that can be built, in order, starting with the engine decision that the
owner reopened on 2026-10-09 (`docs/decisions.md`).

If nothing is done, the research goes stale. It is dated 2026-10-09 and
tracks fast-moving projects (Tauri's unstable multi-webview, GPUI snapshot
releases, the WebView2 Evergreen runtime).

## The idea, as first described

The braindump is the "Brief" section of `docs/next-session-prompt.md` as
committed in `e5ecfaf` (2026-10-09). It is kept there rather than copied here,
because it is long; that commit is the unchanged original. In short:

- **Atlas** is a browser-first workspace: a desktop browser with a
  vertical-tab sidebar, where Forge (code), Notes and Fledge run as built-in
  plugins that open as tabs, with one command palette and one search.
- **Performance decides.** Low memory per tab, fast start, low typing and
  scrolling latency, low idle CPU. TypeScript and Svelte are allowed; all
  state and logic stay in Rust.
- **The first decision is the engine and UI:** A, GPUI chrome plus the system
  web view (recommended by research note 06); B, Tauri 2 with a Svelte chrome
  (the spec's plan, 06's fallback); C, Chromium through CEF (a contingency);
  D, Servo (not ready). Settle it with the owner, then confirm it with a spike.
- **Phase 0 is a gate:** Google sign-in first, then 15 daily sites, memory,
  start-up and latency against Edge and a control build, both overlay
  approaches, and shortcuts while a page has focus. It ends with a per-OS
  verdict and a go/no-go against thresholds the owner confirms.
- **v1 (proposed)** is Atlas as the owner's daily browser on Windows with spec
  sections 1–4 and the Must items of `docs/research/02-browser-v1-scope.md`,
  split into four plans: (1) engine, browser core and palette; (2) plugin
  runtime and Fledge; (3) Notes; (4) Forge. Outline all four; build the first.
- **Work the research says must be planned:** native shortcut routing, a
  permission handler from day one, the plumbing the engine lacks, memory
  levers, privacy defaults, the two-tier search, and the Fledge-side gaps
  (which are a list for the owner, not issues in Fledge's repositories).

## Our understanding (2026-10-09)

**What the owner said:**
- The outcome is one app that replaces a separate browser, IDE and notes app
  for the owner, with one search and one source of context (`docs/spec.md`).
- It is a browser first. The default experience on launch is browsing.
- Performance is the deciding goal (`docs/decisions.md`).
- Standing decisions are listed under "Decisions taken" and are not reopened.

**What this plan assumes, until the owner confirms (⚠️):**
- ⚠️ The only user for v1 is the owner, on their Windows 11 machine (build
  26300). Open source comes later.
- ⚠️ Success means Atlas is the owner's default browser for daily use, with
  memory and start-up close to Edge's.
- ⚠️ The work is done by the owner with Claude Code agents; there is no team.
- ⚠️ The 15 sites in `docs/research/06-engine-and-ui-performance.md` (Phase 0
  checklist, item 4) are the owner's real daily set.

**Facts the owner gave (2026-10-09):**
- They stay signed into Google every day as part of their normal workflow
  (Gmail, Calendar, Drive, YouTube). Google sign-in is therefore a must, not a
  nice-to-have.
- They watch YouTube, and sometimes Disney+ and Hulu, in the browser.
- They have no Mac.

**Size:** this is four plans sharing one braindump, as the brief proposes.
The split is confirmed at step 4, after research.

## What we found

Research notes 01 to 06 in `docs/research/` cover the engine, browser scope,
Tauri and wry facts, import and signing, the core architecture, Forge and
Notes, the Fledge API and performance. The findings that shape this plan are
filled in after the first question rounds and the gap research (step 3).

## Is it realistic?

To be written after research. See Open threads.

## Decisions taken

Standing decisions, recorded so they are not re-litigated:

| # | Decision | Chosen | Source | Date |
|---|---|---|---|---|
| S1 | Profiles or modes | One workspace; no profiles or work/dev modes | `docs/spec.md`, "Decisions already made" | 2026-10-09 |
| S2 | Engine | Embed an existing engine; never build one | `docs/spec.md`; `docs/decisions.md` | 2026-10-09 |
| S3 | Where state and logic live | Rust owns all state and logic; the UI layer is thin | `docs/spec.md` | 2026-10-09 |
| S4 | Graph view | Excluded | `docs/spec.md` | 2026-10-09 |
| S5 | Command palette | Central | `docs/spec.md` | 2026-10-09 |
| S6 | Deciding goal for engine and UI | Performance | `docs/decisions.md` | 2026-10-09 |
| S7 | Import | ~~A CSV file only~~ Replaced by D14 with the owner's yes: Atlas doesn't read other browsers' profiles; it imports a file the owner exports | `docs/decisions.md` | 2026-10-09 |
| S8 | Signing, SmartScreen, Microsoft Store | Deferred until productisation | `docs/decisions.md` | 2026-10-09 |
| S9 | Distribution | Probably open source, released on GitHub | `docs/decisions.md` | 2026-10-09 |
| S10 | Design | Every decision in `design/README.md` stands; changing one needs the owner's yes | `design/README.md` | 2026-10-09 |

Known limits, stated rather than hidden: no Chrome extensions; no password
manager and no ad blocking in v1.

Decisions made in this plan:

| # | Decision | Chosen | Rejected, and why | Date |
|---|---|---|---|---|
| D15 | Does v1 import passwords? | No. There is no password manager in v1, so imported passwords would have nowhere safe to live and nothing to fill them in. The owner signs in once per site and stays signed in through cookies. Password import comes with a password manager after v1 | A small password manager in v1 (encrypted store, autofill, Passwords button back): sizeable and security-sensitive, against the spec's scope. Importing into WebView2's own store: no known import API, and it would turn saving on against D11 | 2026-10-09 |
| D14 | What does the bookmark import read? | The standard HTML bookmarks file that Edge, Chrome and Firefox export (`<!DOCTYPE NETSCAPE-Bookmark-file-1>`, `docs/research/02b-default-browser-import-signing.md`). Replaces "a CSV file only" (S7) with the owner's yes; `docs/decisions.md` updated | CSV only: no browser exports bookmarks as CSV, so the owner would have to convert by hand. Both: a second format to build and test for no gain | 2026-10-09 |
| D13 | How many database snapshots, and may the data folder be synced? | A daily `VACUUM INTO` snapshot taken when idle, 7 kept, in the local (not roaming, not synced) app-data folder; Atlas never puts its database in OneDrive or another synced folder | 30 days: more disk for little gain. A synced folder: SQLite's write-ahead log breaks there (`docs/research/03-core-architecture.md`, owner question 4) | 2026-10-09 |
| D12 | May Atlas carry a patched fork of wry or tao? | Yes, if small: pinned, a handful of patches, each also offered upstream, dropped once upstream merges it | No forks: gaps in shortcut routing or the overlay raise would have to wait or be worked around (`docs/research/01-shell-and-engine.md`, owner question 4) | 2026-10-09 |
| D11 | The Passwords button, and WebView2's own password saving | No Passwords button in v1, and WebView2's built-in password saving stays off; sites keep the owner signed in through their own cookies | Saving on as a stopgap: passwords would sit in WebView2's profile with no Atlas UI to see or manage them | 2026-10-09 |
| D10 | Does the import bring in history? | No: history starts empty and fills as the owner browses | Importing history: makes the palette useful on day one, but the owner doesn't want it | 2026-10-09 |
| D9 | Which engine does "Search the web" use, and does the palette fetch suggestions as you type? | Google, with no live suggestions: while typing, the palette ranks only local tabs, bookmarks and history, so keystrokes stay on the machine and results never wait on the network. Suggestions may become a setting later | Google with live suggestions: sends every keystroke to Google and adds network latency to the palette. Another engine: the owner lives in Google | 2026-10-09 |
| D8 | What pins the sidebar? | Ctrl Alt S, as Zen does; Ctrl S stays free to save in Forge and Notes. The owner approved changing this design decision; `design/README.md` updated | Ctrl S, as Arc does: collides with save in Forge and Notes (`docs/research/02-browser-v1-scope.md`, owner question 3) | 2026-10-09 |
| D7 | If a live overlay window over the page fails, is a frozen snapshot of the page under the palette acceptable? | No. The palette and the sidebar peek must stay live over the page. The spike tries a borderless GPUI popup window with Acrylic first; if it fails, the next step is WebView2 visual hosting, not a freeze frame | Freeze frame (`docs/research/01-shell-and-engine.md`, recommendation): robust, but a playing video's picture would pause under the palette while its sound continues | 2026-10-09 |
| D6 | What must Phase 0 prove (go/no-go)? | Hard musts: Google sign-in, YouTube (1080p and fullscreen), Disney+ and Hulu play. Also: at least 13 of the other daily sites pass; the palette and the sidebar peek draw live over a page with correct focus; Ctrl T, W, L and K work while a page has focus; memory with 10 tabs within 10% of Edge's; idle CPU near zero; the GPUI chrome paints before WebView2 finishes starting | Stricter (match Edge, every site): sets a bar no embedded engine is known to meet. Looser (only sign-in and overlays gate): lets a slow or broken base through to every later wave | 2026-10-09 |
| D5 | How much is v1? | Four plans, each plugin's v1 deliberately thin: (1) engine, browser core and palette; (2) plugin runtime and Fledge on a stand-in client; (3) Notes; (4) Forge (editor, project tree, terminal, basic git, installed language servers; PyCharm is the direction, not the v1 bar). This Super-Pass builds plan 1, which ends at a browser the owner can use daily | Four plans at the spec's full depth: Forge alone would hold v1 back for a very long time (Zed's debugger took 8 months; `docs/research/04-forge-and-notes.md`). Plan 1 only, plugins in v2: drops the plugins that are Atlas's reason to exist | 2026-10-09 |
| D4 | How is Atlas's own interface (the chrome) drawn? | Rust with GPUI, Zed's GPU-drawn toolkit, pinned to a snapshot. Phase 0 measures it against a Tauri 2 + Svelte control build of the same sidebar and palette before the choice is final | Tauri 2 + Svelte (the spec's plan): the chrome can't paint until WebView2 starts, it adds a renderer and a JS heap, and puts TypeScript in the UI layer, against the owner's wish for Rust (`docs/research/06-engine-and-ui-performance.md`) | 2026-10-09 |
| D3 | Which engine renders pages in v1? | The system WebView2 (Edge's Chromium, part of Windows), driven from Rust; everything Atlas draws and decides stays Rust. Servo is added later as a per-tab engine once it handles the owner's sites, re-checked about every six months | Servo only: fails Google sign-in, YouTube, Drive and GitHub, has no DRM, downloads, PDF or printing, and runs unsandboxed by default (`docs/research/06-engine-and-ui-performance.md`, "Short answer"). Servo and WebView2 per tab from v1: two engines' plumbing for one developer. CEF: still embedded Chromium for Google, no Widevine without a Google licence, hundreds of MB and a release every 4–8 weeks. A Firefox (Gecko) fork like Zen: no desktop embedding API, so a full fork rebased every Firefox release; the owner dropped it | 2026-10-09 |
| D2 | Which platforms does v1 run on? | Windows 11 only. macOS is neither built nor tested in v1 (the owner has no Mac and doesn't want Mac testing). ⚠️ Linux is assumed out too: the owner didn't name it, and wry's Linux child web views are X11-only (`docs/research/06-engine-and-ui-performance.md`, "Defer") | macOS in v1: no machine to test on, and it needs far more built (find, downloads, certificate warnings, mute; `docs/research/02-browser-v1-scope.md`). Linux in v1: it would make CEF the plan there | 2026-10-09 |
| D1 | May the plan be committed while it is being planned? (`CLAUDE.md` says commits only when the owner asks) | Yes: commit as we go, to the local branch `claude/plan-atlas-v1`; no pushes, since there is no remote | Leaving the plan uncommitted until the end: a compaction or a closed session would lose the only record of the decisions | 2026-10-09 |

## What exists today

No code. The repository holds the spec, the owner's decisions, the design
snapshot (`design/canvas/project/`, palette tokens in the `tokens()` method of
`design/canvas/project/Main.dc.html`), the original mockup
(`design/reference/unified-workspace-mockup.html`) and research notes 01–06.
The repository is local, with no remote.

## The design

To be written (step 5).

## Sequencing

To be written (step 5).

## What is deliberately not built

To be written (step 5).

## ⚠️ Still unproven

To be written (step 5).

## Open threads

The minimum list from the brief, grouped. Each is cleared as it is answered.

- **Platforms and engine:** what to do if
  Google blocks sign-in in WebView2 (research 07 running); whether Disney+ and
  Hulu play in WebView2 (research 07 running).
- **Data and search:** third-party plugins ever; indexing page text; Fledge
  data copied or searched live; synced data folder; one store or two; snapshot
  retention.
- **Forge:** git CLI for writes; requiring Git for Windows; language servers
  installed only, downloads later or never; debugger out of v1; run control and
  Python task runners.
- **Notes:** Obsidian compatibility; native or web-view editor; IME scripts.
- **Fledge:** third-party OAuth client; REST or MCP; drafts never sent; where a
  lead's Company lives; when Stage C happens.
- **The project:**
  - **Licence: undecided (owner, 2026-10-09).** Blocks going public. Until it
    is chosen, Atlas takes in outside code only under permissive licences
    (MIT, Apache-2.0, BSD) or as whole MPL-2.0 files kept under MPL, and no
    GPL code (the brief already keeps Zed's GPL-3.0 code out). Decided by
    the owner before the repository goes public.
  - **GitHub:** the owner's only account is ZanderOfficial. The repository
    will probably move to the Fledge Software organisation eventually; for now
    Atlas is a local project. How the Super-Pass tracks and reviews work while
    Atlas is local is put to the owner (blocks the breakdown gate, not
    planning).

## Issue map

Written when the owner approves the breakdown.

| Plan section | Title | Labels | Blocked by | Epic | Milestone | Issue |
|---|---|---|---|---|---|---|

## Super-Pass ledger

Empty until phase B starts.

## Iteration log

- **2026-10-09** — Import settled: Atlas reads the browser's HTML bookmarks
  export, replacing "CSV only" with the owner's yes (D14, `docs/decisions.md`
  updated), and v1 imports no passwords (D15).
- **2026-10-09** — Round 3: no history import (D10); no Passwords button and
  WebView2 password saving off (D11); small upstreamed forks of wry or tao
  allowed (D12); 7 daily snapshots, never in a synced folder (D13). The
  owner's import answer (bookmarks and passwords as separate files from
  another browser) raised two conflicts, put back to the owner.
- **2026-10-09** — Round 2: the spike's go/no-go bar (D6), overlays must stay
  live with no freeze-frame fallback (D7), the sidebar pins with Ctrl Alt S and
  `design/README.md` now says so (D8), and "Search the web" uses Google
  with no live suggestions (D9).
- **2026-10-09** — Chrome drawn in Rust with GPUI, to be confirmed by the
  spike's numbers against a Tauri + Svelte control (D4). v1 is four plans
  with thin plugin v1s; this Super-Pass builds plan 1 (D5).
- **2026-10-09** — Engine settled (D3). The owner first asked about a Firefox
  fork like Zen, then about a Rust engine with no reliance on another app.
  Servo was laid out against the owner's needs (Google daily, YouTube,
  Disney+, Hulu) and fails both hard requirements today, so the owner chose
  WebView2 now and Servo later as a per-tab engine. The Firefox-fork research
  was stopped unfinished at the owner's word; no note was written.
- **2026-10-09** — Round 1 answers: v1 is Windows only, with no macOS work or
  testing (D2). The owner uses Google daily and streams YouTube, Disney+ and
  Hulu; a researcher was sent to check Google sign-in and DRM in WebView2
  as of today (`docs/research/07-google-sign-in-and-drm.md`). Q1 (engine and
  UI) and Q5 (realism and the four-plan split) are still open.
- **2026-10-09** — Plan file created from the brief in
  `docs/next-session-prompt.md`. Research notes 01–06 read (summaries,
  recommendations and owner questions). First question round sent (engine
  and UI, platforms, Google sign-in, DRM, realism and the four-plan split,
  committing). The owner allowed committing as we go (D1); the rest of the
  round is open.

## Provenance

- `docs/next-session-prompt.md` at `e5ecfaf` (the braindump)
- `docs/spec.md`, `docs/decisions.md`, `design/README.md`
- `docs/research/01-shell-and-engine.md` … `06-engine-and-ui-performance.md`
- The planning conversation of 2026-10-09 (`/suprpas`)
