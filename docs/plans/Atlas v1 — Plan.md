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
Notes, the Fledge API and performance. Notes 07 and 08 were written for this
plan on 2026-10-09 and correct parts of the earlier notes.

**WebView2 is a better fit than the earlier notes said**
(`docs/research/07-google-sign-in-and-drm.md`):
- **Widevine is there.** The WebView2 runtime on the owner's machine (154)
  ships Google's Widevine module, identical to Edge's, at the software-only
  level (L3), and played a Widevine test stream with no setting. Notes 02 and
  06 say WebView2 has no Widevine; that is wrong for this runtime. Disney+ and
  Hulu accept Firefox and are Widevine partners, so Widevine should be
  enough, capped at 720p in a browser (Hulu: 480p unless HDCP is verified).
  Ordinary YouTube uses no DRM at all. PlayReady is advertised but failed to
  start on this machine, in Edge too. (High that Widevine plays here; medium
  for Disney+ and Hulu until tried with the owner's accounts.)
- **Google sign-in is untested, not known blocked.** Google's written rules
  name CEF and Embedded Internet Explorer, never WebView2; only Microsoft's
  docs say Google blocks WebView2. On runtime 154 the engine presents itself
  to Google's sites as Google Chrome (and as Edge plus a "WebView2" brand
  everywhere else), and Google's sign-in page loads normally. The step where
  the 2021 block appeared (after entering an email) was not tried, because no
  credentials were used. (Medium.) If the default fails, there is no
  sanctioned fallback.
- **Rules that keep Google working:** Atlas must never set a custom user
  agent on page web views (that removes the Chrome presentation toward Google
  and exposes the WebView2 brand), must not attach an IPC handler to page web
  views (wry's injected `window.ipc` was reported to break Google Docs and
  Sheets), and must not inject scripts, rewrite requests or open a remote
  debugging port on Google's sites.

**GPUI with wry works, with platform code** (`docs/research/08-gpui-and-wry-on-windows.md`):
- GPUI has no current crates.io release under its own name (`gpui` 0.2.2,
  2025-10-22). Apps outside Zed pin Longbridge's weekly `gpui-pre` snapshot
  (0.3.8, 2026-10-05) exactly, through `gpui-kit`/`gpui-component` 0.7.1
  (Apache-2.0), or depend on Zed's git repository. Longbridge is also moving
  to a second fork, `gpui-fast`.
- GPUI exposes the window's raw handle, so wry can build WebView2 tabs as
  child windows of a GPUI window.
- GPUI's popup windows on Windows float above every app and use the old
  Acrylic call, so Atlas sets the owner window and the
  `DWMSBT_TRANSIENTWINDOW` backdrop itself through raw Win32 calls, and hands
  focus back to the page with wry's `focus()`.
- wry 0.57 covers the shared WebView2 environment, popups that keep
  `window.opener`, permissions, the memory level and downloads. Shortcuts
  (`AcceleratorKeyPressed`), fullscreen video, crashed tabs (`ProcessFailed`)
  and `TrySuspend` need raw WebView2 calls through `webview2-com`. Atlas
  builds its own environment to keep crash dumps local, and overrides wry's
  default browser arguments, which switch SmartScreen off.
- **The fallback for overlays is not ready.** WebView2 visual hosting in wry
  is an open pull request, and Longbridge's Windows work on it is
  incomplete. With frozen snapshots ruled out (D7), a failed popup spike is a
  real schedule risk.
- Runtime behaviour (popup stacking, Acrylic under a GPUI popup, focus
  hand-off, GPUI with a screen reader) was read from source, not run; the
  spike tests it.

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
| D32 | Which input methods must the editors be tested with? | English and Latin scripts only; no IME test matrix in v1 | Chinese, Japanese or Korean IME testing: not used by the owner | 2026-10-09 |
| D31 | Run control and debugger in Forge v1? | Run control in (an addition to the spec): Forge reads tasks from Cargo, npm scripts, pyproject, just and make and runs them in a terminal. Debugger out of v1. ⚠️ The owner didn't name Python task runners, so v1 reads plain `[project.scripts]` and the common pyproject task tables | Both out: loses the PyCharm workflow the owner relies on. Debugger in: Zed's took 8 months and over 25,000 lines (`docs/research/04-forge-and-notes.md`) | 2026-10-09 |
| D30 | Where do Forge's language servers come from? | Only servers already installed (on PATH or set in settings) in v1; automatic download and update after v1 | Never downloading: the owner wants it later. Downloading in v1: network access, updates and more work | 2026-10-09 |
| D29 | How does Forge do git? | The `git` command line for every write and network operation (commit with hooks, checkout, merge, push, fetch with the owner's credential helpers); fast reads (status, diff, log, blame) in process in Rust behind a `GitRead` trait. Requires Git for Windows. Departs from the spec's "use libgit2" with the owner's yes | libgit2 for everything: can't read reftable repositories (Git 3's default) and won't run hooks; Zed removed it on 2026-06-02 (`docs/research/04-forge-and-notes.md`). Bundling a git: a bigger download and Atlas's own update duty | 2026-10-09 |
| D28 | Native editors or CodeMirror for Notes and Forge in v1? | CodeMirror 6 in a small local web view inside the tab, for both. Rust owns files, parsing (Tree-sitter, pulldown-cmark), git and language servers. One WebView2 renderer is used only while a Notes or Forge tab is open. A native editor may replace it later | Native GPUI editors: selection, input methods and accessibility to build, no native rich-markdown editor to start from, and Zed's editor is GPL-3.0 (`docs/research/04-forge-and-notes.md`). Native for Forge only: still the bulk of that cost | 2026-10-09 |
| D27 | Does a Notes vault stay usable in Obsidian? | Yes: plain markdown files, wikilinks, Obsidian's property types, and a subset of its `.base` database-view format | Free to diverge: the owner would lose the way out to Obsidian | 2026-10-09 |
| D26 | Where does a lead's Company live? | Already decided on the Fledge side and being built, in Nest; Fledge's modules now share one database. The stand-in client models Company on the lead, and the real client reads it from Nest | — (decided in Fledge, not here) | 2026-10-09 |
| D25 | Which channel does Atlas use for Fledge data? | A small, structured, versioned REST API on the same OAuth tokens, for leads, mail, calendar and docs (Fledge-side routes, on the owner's list) | Fledge's MCP server: returns prose for a model, has no leads tool, and would need structured output added (`docs/research/05-fledge-api.md`) | 2026-10-09 |
| D24 | When can the real Fledge client be built? | After Fledge's Stage C, which the owner says comes after the Fledge redesign; no date. Plan 2 ships the Fledge plugin on the stand-in client behind the trait, and the real client is its own later work, started when Stage C lands | Targeting the real client in plan 2: nothing it needs is deployed (`docs/research/05-fledge-api.md`, "Short answer") | 2026-10-09 |
| D23 | How does Atlas authenticate to Fledge? | As an ordinary OAuth client through Fledge's consent screen (OAuth 2.1, PKCE, loopback redirect), with only the permissions the owner grants | A pre-registered first-party client: Fledge would have to build a separate trust path (`docs/research/05-fledge-api.md`, owner questions) | 2026-10-09 |
| D22 | If the owner's real Google sign-in fails in Phase 0? | Stop and decide with the evidence: where it failed, and the options then (Google in Edge, presenting Atlas differently against Google's policy, or rethinking the engine) | Deciding now: a no-go on the engine, or settling for Google in Edge, before knowing how it fails | 2026-10-09 |
| D21 | Which Google sign-in paths must Phase 0 prove? | All the owner uses: password with a phone prompt or code, a passkey (Windows Hello or phone), and a Google Workspace account. ⚠️ If the Workspace admin has device checks (Context-Aware Access or Endpoint Verification), they need a browser extension Atlas can't run (`docs/research/07-google-sign-in-and-drm.md`, new questions) | — | 2026-10-09 |
| D20 | What may Atlas do in Fledge? | Everything a person can: Atlas is meant to become Fledge's desktop app, and these foundations are what make that possible. Sending mail and invites from Atlas needs Fledge-side consent scopes that don't exist today (`docs/research/05-fledge-api.md`, "Work needed on the Fledge side"); that is a list for the owner, not issues in Fledge's repositories | Draft only (Fledge's current rule for outside apps): the owner wants Atlas to be Fledge's full client | 2026-10-09 |
| D19 | Is Fledge data copied into Atlas's local index, or searched live? | The owner decides per kind of data. Each plugin has its own settings, and Fledge's has a keep-local switch for each kind (docs, canvases, tables, leads, mail, calendar and so on): on, that kind is copied into the local index; off, it is searched live | All copied: stores everything on the machine whether wanted or not. All live: every keystroke waits on the network | 2026-10-09 |
| D18 | Does Atlas index the text of visited pages? | Yes, locally, for up to 7 days of history, with a setting to turn it off | Title and address only: the owner wants recall by content | 2026-10-09 |
| D17 | Will Atlas run other people's plugins? | Later, not in v1. Built-ins use the same plugin API a third party would; the WebAssembly sandbox is built only once someone else's plugin exists | Never: closes a door the owner wants open. In v1: a sandbox, permissions and an installer, a large addition | 2026-10-09 |
| D16 | How does the build phase track and review work while Atlas is local? | Plan now, build later. All four plans are planned locally in this session, up to an approved breakdown written into each Issue map with empty issue numbers; nothing is built, Phase 0 included, until the owner creates a repository (only account: ZanderOfficial; probably the Fledge Software organisation later) | A private repository on ZanderOfficial now: the owner wants Atlas local for now. A fully local build: the Super-Pass's review and PR skills are built for GitHub, so it would run a reduced version of them | 2026-10-09 |
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

- **Google Workspace device checks:** does the owner's Workspace admin
  console turn on Context-Aware Access or Endpoint Verification? Whoever
  administers that Workspace can check; Phase 0 will show it anyway.
- **Data and search:** one store or two (SQLite FTS5 or tantivy), settled by
  the bake-off before Notes.
- **The project:**
  - **Licence: undecided (owner, 2026-10-09).** Blocks going public. Until it
    is chosen, Atlas takes in outside code only under permissive licences
    (MIT, Apache-2.0, BSD) or as whole MPL-2.0 files kept under MPL, and no
    GPL code (the brief already keeps Zed's GPL-3.0 code out). Decided by
    the owner before the repository goes public.
  - **GitHub:** the owner's only account is ZanderOfficial. The repository
    will probably move to the Fledge Software organisation eventually; for now
    Atlas is a local project, and building waits for a repository (D16).
    Which account or organisation it is created under is decided when the
    owner is ready to build.

## Issue map

Written when the owner approves the breakdown.

| Plan section | Title | Labels | Blocked by | Epic | Milestone | Issue |
|---|---|---|---|---|---|---|

## Super-Pass ledger

Empty until phase B starts.

## Iteration log

- **2026-10-09** — Forge round: git command line for writes with in-process
  reads, needing Git for Windows (D29, departs from the spec with the owner's
  yes); installed language servers only, downloads after v1 (D30); run
  control in, debugger out (D31); no IME testing (D32). The discussion
  frontier is empty; next is the realism check and writing the plans.
- **2026-10-09** — Fledge and Notes round: Fledge data over a small
  structured REST API (D25); a lead's Company is already being built in Nest
  (D26); Notes vaults stay Obsidian-compatible (D27); Notes and Forge edit
  in CodeMirror 6 inside a local web view for v1 (D28).
- **2026-10-09** — Google and Fledge round: Phase 0 proves every Google
  sign-in path the owner uses (D21) and stops for a decision if one fails
  (D22); Atlas is an ordinary OAuth client of Fledge (D23); the real Fledge
  client waits for Stage C, after the Fledge redesign (D24).
- **2026-10-09** — Research 07 and 08 reported (summarised under "What we
  found"). WebView2 ships Widevine, correcting notes 02 and 06; Google's
  sign-in page loads in WebView2 and the full sign-in is a Phase 0 test; GPUI
  and wry work together with raw Win32 and WebView2 calls, and the overlay
  fallback is not ready. Plan 2 round: no third-party plugins in v1 (D17);
  page text indexed locally for 7 days (D18); Fledge data local or live per
  kind, by switch (D19); Atlas may do everything in Fledge and becomes its
  desktop app (D20).
- **2026-10-09** — Licence left undecided until Atlas goes public. Atlas
  stays local: plan all four plans now, build once a repository exists (D16).
  A researcher was sent on GPUI and wry on Windows
  (`docs/research/08-gpui-and-wry-on-windows.md`).
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
