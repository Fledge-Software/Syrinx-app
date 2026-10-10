---
title: "Atlas v1 — Plan (plan 1: engine, browser core and palette)"
status: draft
created: 2026-10-09
subject: Atlas v1 plan 1 — the engine, the browser core and the command palette; also holds the v1-wide decisions D1–D36 that plans 2–4 share
relates_to: "docs/spec.md; docs/decisions.md; design/README.md; docs/research/; sibling plans: Atlas v1 — Plugins and Fledge, Atlas v1 — Notes, Atlas v1 — Forge"
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

**Verdict: realistic with changes**, from a fresh skeptic on 2026-10-09 who
saw none of the discussion. The engine choice is sound. Its twelve
objections, and how each was answered:

1. **GPUI's case is weaker once the editors are web views (D28).** Answered:
   D4 now stands on a Phase 0 comparison with a rule written in advance.
2. **Live overlays have no tested fallback.** Answered: D7 gates on a live,
   near-opaque popup; blur is optional; failure stops for a decision.
3. **Departures from the spec aren't in `docs/decisions.md`, and planning
   all four plans before Phase 0 contradicts `CLAUDE.md`.** Answered: the
   departures are written into `docs/decisions.md`; D16 now runs Phase 0
   locally first, with plans 2–4 as outlines. `CLAUDE.md` was
   updated with the owner's yes.
4. **Google sign-in rests on undocumented behaviour that can change with any
   runtime update.** Answered in part: D22 covers breaks after launch. D36 adds a
   notice on runtime changes.
5. **The Phase 0 thresholds can't be measured as written.** To answer: a
   measurement protocol is written before the spike (plan 1, Phase 0).
6. **Dropping Tauri drops its updater, single-instance link handling and
   installer.** To answer: each becomes a plan 1 item.
7. **The GPUI base is unchosen and churns weekly.** To answer: choosing and
   `=`-pinning one base, with an upgrade budget and a list of missing
   widgets, is a Phase 0 output.
8. **"Rust owns all state" doesn't hold inside CodeMirror.** Answered: D28
   states the exception and keeps saves and the language-server client in
   Rust.
9. **Indexing page text (D18) collides with the rules that keep Google
   working, and with privacy.** Answered: D35's exclusions.
10. **The Tauri control build has no decision rule.** Answered: D4's rule;
    the control's page tabs use wry directly so the numbers compare.
11. **Plan 2 outgrew "thin".** Answered: D34.
12. **Shortcuts must work in the editor and terminal web views too, and some
    keys clash with shell and PyCharm keys.** To answer: Phase 0 tests a
    CodeMirror and an xterm.js tab; a key-ownership table comes before
    plan 4.

Cost: about $0 a month to run (no servers; signing deferred). Effort is
dominated by the GPUI chrome and Forge; plan 1 alone is the largest plan.

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
| D36 | Does Atlas watch for Google sign-in breaking? | It shows a one-line notice when the WebView2 runtime version changes, so the owner watches for sign-in trouble, and offers to open the current Google page in Edge in one click if sign-in breaks. No automatic sign-in test | An automatic canary that signs in: credentials in a test. Nothing: a silent break would surprise the owner mid-work | 2026-10-09 |
| D35 | How is page text indexed (D18) without breaking Google? | Indexed locally for 7 days, but never on Google's sites, banking and payment sites, or pages sent with `Cache-Control: no-store`; the owner can add sites to the exclusion list in settings. The extraction method is chosen in plan 1 and must inject nothing on excluded sites | Dropping it for v1: the owner wants recall by content. Indexing everything: breaks the rules that keep Google working (`docs/research/07-google-sign-in-and-drm.md`) and keeps a 7-day record of banking pages in the database and its snapshots | 2026-10-09 |
| D34 | How big is plan 2 in v1? | Thin: the plugin runtime, the `Plugin` trait, the stand-in Fledge client and the palette's inline "New lead" action. D19's keep-local switches and D20's full Fledge client move to the real-client work after Stage C | Building D19 and D20 in full on the stand-in: design and settings on an API that doesn't exist yet and will change, against D5's "deliberately thin" | 2026-10-09 |
| D32 | Which input methods must the editors be tested with? | English and Latin scripts only; no IME test matrix in v1 | Chinese, Japanese or Korean IME testing: not used by the owner | 2026-10-09 |
| D31 | Run control and debugger in Forge v1? | Run control in (an addition to the spec): Forge reads tasks from Cargo, npm scripts, pyproject, just and make and runs them in a terminal. Debugger out of v1. ⚠️ The owner didn't name Python task runners, so v1 reads plain `[project.scripts]` and the common pyproject task tables | Both out: loses the PyCharm workflow the owner relies on. Debugger in: Zed's took 8 months and over 25,000 lines (`docs/research/04-forge-and-notes.md`) | 2026-10-09 |
| D30 | Where do Forge's language servers come from? | Only servers already installed (on PATH or set in settings) in v1; automatic download and update after v1 | Never downloading: the owner wants it later. Downloading in v1: network access, updates and more work | 2026-10-09 |
| D29 | How does Forge do git? | The `git` command line for every write and network operation (commit with hooks, checkout, merge, push, fetch with the owner's credential helpers); fast reads (status, diff, log, blame) in process in Rust behind a `GitRead` trait. Requires Git for Windows. Departs from the spec's "use libgit2" with the owner's yes | libgit2 for everything: can't read reftable repositories (Git 3's default) and won't run hooks; Zed removed it on 2026-06-02 (`docs/research/04-forge-and-notes.md`). Bundling a git: a bigger download and Atlas's own update duty | 2026-10-09 |
| D28 | Native editors or CodeMirror for Notes and Forge in v1? | CodeMirror 6 in a small local web view inside the tab, for both. Rust owns files, parsing (Tree-sitter, pulldown-cmark), git and the language-server client (which stays in Rust). One WebView2 renderer is used only while a Notes or Forge tab is open. **Exception to S3, stated:** CodeMirror owns the live buffer, selection and undo while typing; Rust is the authority for what is on disk and writes a save only after the two copies' checksums match. A native editor may replace it later | Native GPUI editors: selection, input methods and accessibility to build, no native rich-markdown editor to start from, and Zed's editor is GPL-3.0 (`docs/research/04-forge-and-notes.md`). Native for Forge only: still the bulk of that cost | 2026-10-09 |
| D27 | Does a Notes vault stay usable in Obsidian? | Yes: plain markdown files, wikilinks, Obsidian's property types, and a subset of its `.base` database-view format | Free to diverge: the owner would lose the way out to Obsidian | 2026-10-09 |
| D26 | Where does a lead's Company live? | Already decided on the Fledge side and being built, in Nest; Fledge's modules now share one database. The stand-in client models Company on the lead, and the real client reads it from Nest | — (decided in Fledge, not here) | 2026-10-09 |
| D25 | Which channel does Atlas use for Fledge data? | A small, structured, versioned REST API on the same OAuth tokens, for leads, mail, calendar and docs (Fledge-side routes, on the owner's list) | Fledge's MCP server: returns prose for a model, has no leads tool, and would need structured output added (`docs/research/05-fledge-api.md`) | 2026-10-09 |
| D24 | When can the real Fledge client be built? | After Fledge's Stage C, which the owner says comes after the Fledge redesign; no date. Plan 2 ships the Fledge plugin on the stand-in client behind the trait, and the real client is its own later work, started when Stage C lands | Targeting the real client in plan 2: nothing it needs is deployed (`docs/research/05-fledge-api.md`, "Short answer") | 2026-10-09 |
| D23 | How does Atlas authenticate to Fledge? | As an ordinary OAuth client through Fledge's consent screen (OAuth 2.1, PKCE, loopback redirect), with only the permissions the owner grants | A pre-registered first-party client: Fledge would have to build a separate trust path (`docs/research/05-fledge-api.md`, owner questions) | 2026-10-09 |
| D22 | If the owner's Google sign-in fails, in Phase 0 or later? | Stop and decide with the evidence: where it failed, and the options then (Google in Edge, presenting Atlas differently against Google's policy, or rethinking the engine). Because WebView2's Chrome presentation toward Google is undocumented and Evergreen updates silently, the same rule covers a break after launch | Deciding now: a no-go on the engine, or settling for Google in Edge, before knowing how it fails | 2026-10-09 |
| D21 | Which Google sign-in paths must Phase 0 prove? | All the owner uses: password with a phone prompt or code, a passkey (Windows Hello or phone), and a Google Workspace account. ⚠️ If the Workspace admin has device checks (Context-Aware Access or Endpoint Verification), they need a browser extension Atlas can't run (`docs/research/07-google-sign-in-and-drm.md`, new questions) | — | 2026-10-09 |
| D20 | What may Atlas do in Fledge? | Everything a person can, in the long run: Atlas is meant to become Fledge's desktop app. **Not in v1** (D34): it is built with the real client after Stage C. Sending mail and invites needs Fledge-side consent scopes that don't exist today (`docs/research/05-fledge-api.md`, "Work needed on the Fledge side"); that is a list for the owner, not issues in Fledge's repositories | Draft only (Fledge's current rule for outside apps): the owner wants Atlas to be Fledge's full client | 2026-10-09 |
| D19 | Is Fledge data copied into Atlas's local index, or searched live? | The owner decides per kind of data: Fledge's plugin settings get a keep-local switch for each kind (docs, canvases, tables, leads, mail, calendar and so on). **Not in v1** (D34): built with the real client after Stage C | All copied: stores everything whether wanted or not. All live: every keystroke waits on the network | 2026-10-09 |
| D18 | Does Atlas index the text of visited pages? | Yes, locally, for up to 7 days of history, with a setting to turn it off | Title and address only: the owner wants recall by content | 2026-10-09 |
| D17 | Will Atlas run other people's plugins? | Later, not in v1. Built-ins use the same plugin API a third party would; the WebAssembly sandbox is built only once someone else's plugin exists | Never: closes a door the owner wants open. In v1: a sandbox, permissions and an installer, a large addition | 2026-10-09 |
| D16 | When does building start, and how is work tracked while Atlas is local? | Phase 0 first, locally: plan 1 is written in full and plans 2–4 stay outlines; the Phase 0 spike (throwaway code) runs in this local repository without GitHub, then stops and reports, as `CLAUDE.md` asks. Issues are broken down after its verdict, once the owner creates a repository (only account: ZanderOfficial; probably the Fledge Software organisation later). *Revised after the realism check; was "plan all four plans to an approved breakdown, build nothing until a repository exists"* | Breaking all four plans into issues before Phase 0: its results (GPUI or Tauri, whether overlays work) could invalidate them. A private repository now: the owner wants Atlas local for now | 2026-10-09 |
| D15 | Does v1 import passwords? | No. There is no password manager in v1, so imported passwords would have nowhere safe to live and nothing to fill them in. The owner signs in once per site and stays signed in through cookies. Password import comes with a password manager after v1 | A small password manager in v1 (encrypted store, autofill, Passwords button back): sizeable and security-sensitive, against the spec's scope. Importing into WebView2's own store: no known import API, and it would turn saving on against D11 | 2026-10-09 |
| D14 | What does the bookmark import read? | The standard HTML bookmarks file that Edge, Chrome and Firefox export (`<!DOCTYPE NETSCAPE-Bookmark-file-1>`, `docs/research/02b-default-browser-import-signing.md`). Replaces "a CSV file only" (S7) with the owner's yes; `docs/decisions.md` updated | CSV only: no browser exports bookmarks as CSV, so the owner would have to convert by hand. Both: a second format to build and test for no gain | 2026-10-09 |
| D13 | How many database snapshots, and may the data folder be synced? | A daily `VACUUM INTO` snapshot taken when idle, 7 kept, in the local (not roaming, not synced) app-data folder; Atlas never puts its database in OneDrive or another synced folder | 30 days: more disk for little gain. A synced folder: SQLite's write-ahead log breaks there (`docs/research/03-core-architecture.md`, owner question 4) | 2026-10-09 |
| D12 | May Atlas carry a patched fork of wry or tao? | Yes, if small: pinned, a handful of patches, each also offered upstream, dropped once upstream merges it | No forks: gaps in shortcut routing or the overlay raise would have to wait or be worked around (`docs/research/01-shell-and-engine.md`, owner question 4) | 2026-10-09 |
| D11 | The Passwords button, and WebView2's own password saving | No Passwords button in v1, and WebView2's built-in password saving stays off; sites keep the owner signed in through their own cookies | Saving on as a stopgap: passwords would sit in WebView2's profile with no Atlas UI to see or manage them | 2026-10-09 |
| D10 | Does the import bring in history? | No: history starts empty and fills as the owner browses | Importing history: makes the palette useful on day one, but the owner doesn't want it | 2026-10-09 |
| D9 | Which engine does "Search the web" use, and does the palette fetch suggestions as you type? | Google, with no live suggestions: while typing, the palette ranks only local tabs, bookmarks and history, so keystrokes stay on the machine and results never wait on the network. Suggestions may become a setting later | Google with live suggestions: sends every keystroke to Google and adds network latency to the palette. Another engine: the owner lives in Google | 2026-10-09 |
| D8 | What pins the sidebar? | Ctrl Alt S, as Zen does; Ctrl S stays free to save in Forge and Notes. The owner approved changing this design decision; `design/README.md` updated | Ctrl S, as Arc does: collides with save in Forge and Notes (`docs/research/02-browser-v1-scope.md`, owner question 3) | 2026-10-09 |
| D7 | What must the overlays achieve, and is a frozen page under the palette acceptable? | The palette and the sidebar peek must stay live, over web pages and over the editor web views. Phase 0 gates on an owned, live, near-opaque popup window with correct focus; the Acrylic blur is a bonus, not a gate. If that fails, stop and decide (WebView2 visual hosting, or a palette that shrinks the page). No freeze frame. *Revised after the realism check; blur was a gate* | Freeze frame (`docs/research/01-shell-and-engine.md`, recommendation): a playing video's picture would pause under the palette. Blur as a gate: a cosmetic effect deciding go/no-go, when the design already keeps the palette near-opaque (`design/README.md`) | 2026-10-09 |
| D6 | What must Phase 0 prove (go/no-go)? | Hard musts: Google sign-in, YouTube (1080p and fullscreen), Disney+ and Hulu play. Also: at least 13 of the other daily sites pass; the palette and the sidebar peek draw live over a page with correct focus; Ctrl T, W, L and K work while a page has focus; memory with 10 tabs within 10% of Edge's; idle CPU near zero; the GPUI chrome paints before WebView2 finishes starting | Stricter (match Edge, every site): sets a bar no embedded engine is known to meet. Looser (only sign-in and overlays gate): lets a slow or broken base through to every later wave | 2026-10-09 |
| D5 | How much is v1? | Four plans, each plugin's v1 deliberately thin: (1) engine, browser core and palette; (2) plugin runtime and Fledge on a stand-in client; (3) Notes; (4) Forge (editor, project tree, terminal, basic git, installed language servers; PyCharm is the direction, not the v1 bar). This Super-Pass builds plan 1, which ends at a browser the owner can use daily | Four plans at the spec's full depth: Forge alone would hold v1 back for a very long time (Zed's debugger took 8 months; `docs/research/04-forge-and-notes.md`). Plan 1 only, plugins in v2: drops the plugins that are Atlas's reason to exist | 2026-10-09 |
| D4 | How is Atlas's own interface (the chrome) drawn? | Rust with GPUI, pinned to one snapshot base chosen in Phase 0. Phase 0 compares "GPUI chrome + a CodeMirror tab" with "Svelte chrome + a CodeMirror tab" (both with page tabs through wry directly), against a rule written before the spike: switch to Tauri 2 + Svelte if GPUI saves less than about 50 MB and 150 ms at start, or if the overlays work only in Tauri. *Revised after the realism check, because D28 moved the editors into web views* | Tauri 2 + Svelte now: gives up GPUI's start-time and memory lead before measuring it. GPUI with no comparison: commits to a pre-1.0 toolkit without evidence | 2026-10-09 |
| D3 | Which engine renders pages in v1? | The system WebView2 (Edge's Chromium, part of Windows), driven from Rust; everything Atlas draws and decides stays Rust. Servo is added later as a per-tab engine once it handles the owner's sites, re-checked about every six months | Servo only: fails Google sign-in, YouTube, Drive and GitHub, has no DRM, downloads, PDF or printing, and runs unsandboxed by default (`docs/research/06-engine-and-ui-performance.md`, "Short answer"). Servo and WebView2 per tab from v1: two engines' plumbing for one developer. CEF: still embedded Chromium for Google, no Widevine without a Google licence, hundreds of MB and a release every 4–8 weeks. A Firefox (Gecko) fork like Zen: no desktop embedding API, so a full fork rebased every Firefox release; the owner dropped it | 2026-10-09 |
| D2 | Which platforms does v1 run on? | Windows 11 only. macOS is neither built nor tested in v1 (the owner has no Mac and doesn't want Mac testing). ⚠️ Linux is assumed out too: the owner didn't name it, and wry's Linux child web views are X11-only (`docs/research/06-engine-and-ui-performance.md`, "Defer") | macOS in v1: no machine to test on, and it needs far more built (find, downloads, certificate warnings, mute; `docs/research/02-browser-v1-scope.md`). Linux in v1: it would make CEF the plan there | 2026-10-09 |
| D1 | May the plan be committed while it is being planned? (`CLAUDE.md` says commits only when the owner asks) | Yes: commit as we go, to the local branch `claude/plan-atlas-v1`; no pushes, since there is no remote | Leaving the plan uncommitted until the end: a compaction or a closed session would lose the only record of the decisions | 2026-10-09 |

## What exists today

No code. The repository holds the spec, the owner's decisions, the design
snapshot (`design/canvas/project/`, with the five palettes' tokens in the
`tokens()` method of `design/canvas/project/Main.dc.html` and one board per
palette in `Theme*.dc.html`), the original mockup
(`design/reference/unified-workspace-mockup.html`) and research notes 01–08.
The repository is local, with no remote. A reader grepping the research will
find Tauri and Svelte everywhere: notes 01–05 were written when the spec's
Tauri plan stood, and D3 and D4 replaced it.

## The design

This section covers plan 1: the engine, the browser core and the palette.
Plans 2–4 are outlined in their own files (see "Sibling plans").

### The parts

The crate layout follows note 03's "Rust-toolkit variant", so the toolkit
decision (D4) stays reversible: only `atlas-app` and `atlas-ui` know GPUI,
and only `atlas-platform` knows WebView2.

```
Atlas-app/
  Cargo.toml          virtual workspace: members = ["crates/*"], resolver 3, shared lints
  crates/
    atlas-plugin-api/ the Plugin trait and its data types; depends on no Atlas crate (used from plan 2)
    atlas-core/       tab model, session, closed-tab stack, command registry, shortcut table, palette logic
    atlas-store/      rusqlite: migrations, settings, tiles, bookmarks, history, sessions, snapshots
    atlas-search/     tier 1 catalogue (frizbee + frecency); tier 2 full text (behind a FullText trait)
    atlas-platform/   everything WebView2 and Win32: the shared environment, page web views,
                      shortcuts, fullscreen, crash, suspend, popups' owner and backdrop
    atlas-ui/         GPUI widgets and the design tokens (five palettes, accent)
    atlas-app/        the binary: window, sidebar, page card, home page, palette, wiring
    xtask/            check-deps (only atlas-app/atlas-ui may depend on GPUI; only
                      atlas-platform on wry or webview2-com), bench
```

`atlas-core`, `atlas-store` and `atlas-search` depend on neither GPUI nor
WebView2, so they are tested with plain `cargo test`. `atlas-app` calls the
core directly: with a Rust chrome there is no IPC boundary in plan 1.

### The tab model

One generic model, as the spec asks (`docs/spec.md`, "Core model"). A tab
has an id, a kind (web, plugin view, note, file, diff, terminal), a title, an
address, a favicon, a pinned flag, an order and an optional badge. Plan 1
builds only web tabs, but the kind enum is complete so plugins slot in.
The sidebar and the palette read this model, never per-feature code. Every
change to it is written to SQLite as it happens, so a crash loses nothing
(`docs/research/02-browser-v1-scope.md`, "Crash recovery of the app").

### Pages

- **One shared WebView2 environment,** built by Atlas through `webview2-com`
  so it can keep crash dumps local (`IsCustomCrashReportingEnabled`) and
  replace wry's default browser arguments, which switch SmartScreen off
  (`docs/research/08-gpui-and-wry-on-windows.md`).
- **Each tab is a wry child web view** of the GPUI window, with no IPC
  handler and no injected script, no custom user agent, and nothing
  injected on Google's sites (the rules under "What we found").
- **Raw WebView2 calls** through `WebViewExtWindows` for shortcuts
  (`AcceleratorKeyPressed`), fullscreen video
  (`ContainsFullScreenElementChanged`), crashed tabs (`ProcessFailed`, a
  "sad tab" with Reload) and hidden tabs (`TrySuspend` after a delay).
- **Popups become tabs** through wry's new-window handler, in the same
  environment and profile, so `window.opener` survives OAuth sign-ins.
- **A permission handler from day one,** with WebView2's own prompt and its
  per-site memory.
- **Lean on WebView2's built-in UI** for find (Ctrl F), downloads (Ctrl J),
  printing, the right-click menu and notifications, and replace each only if
  it looks wrong in the page card (`docs/research/02-browser-v1-scope.md`,
  "What this means", item 4).

### Shortcuts

WebView2 never hands browser shortcuts to the host while a page has focus,
so Atlas catches them with `AcceleratorKeyPressed` on **every** web view it
creates (pages now; the CodeMirror and terminal web views in plans 3 and 4).
The handler is synchronous and blocks the browser process, so it only marks
the key handled and posts a message to the GPUI thread. One command table in
`atlas-core` maps keys to commands; the palette and menus read from it. The
pin key is Ctrl Alt S (D8).

### Overlays

The palette (Ctrl K) and the sidebar peek are borderless GPUI popup windows
above the page, owned by the main window through `GWLP_HWNDPARENT` (GPUI's
popups are otherwise topmost over every app), near-opaque, with the
`DWMSBT_TRANSIENTWINDOW` Acrylic backdrop as a bonus. The peek takes no focus;
the palette does, and Esc returns it to the page with wry's `focus()`.
Hovering the left edge is detected by a few-pixel strip of chrome or by
polling the cursor, because a pointer over a page never reaches GPUI. They
must work over every kind of tab (D7).

### Storage

rusqlite (bundled) with `rusqlite_migration`, WAL, `synchronous=NORMAL`,
one writer thread and two readers, in `%LOCALAPPDATA%\Atlas\data\atlas.db`
(never a synced folder), with a daily `VACUUM INTO` snapshot taken when idle
and 7 kept (D13), and a clean-shutdown flag that triggers a `quick_check`
after a crash (`docs/research/03-core-architecture.md`, "Storage").

### Search

- **Tier 1, in memory:** one row per titled thing (open tabs, bookmarks,
  recent history, commands), ranked by `frizbee` with frecency and an
  open-tab boost on every keystroke, with the scope chips counted in the
  same pass. Budget: under 5 ms at 100,000 rows.
- **Tier 2, full text:** page text for 7 days (D18, D35), excluding Google's
  sites, banking and payment sites and `no-store` pages. The store (tantivy
  or SQLite FTS5) is chosen by a one-day bake-off, here in plan 1 because
  page text needs it; the extraction method must inject nothing on excluded
  sites.

### The palette

Opens with Ctrl K over any tab. Scope chips with counts (Everything, Tabs,
Bookmarks, History; plugin actions arrive in plan 2), matched characters
highlighted, a preview of the selected row, keyboard-first (arrows, Enter,
Esc). It is also the address bar: it always offers "Search the web" (Alt
Enter) on Google, with no live suggestions (D9).

### Browser memory

History from navigation and title events; bookmarks with folders, imported
from the browser's HTML bookmarks file (D14); a closed-tab stack (Ctrl
Shift T); session restore of each tab's address, title, pin and order
(WebView2 can't restore back/forward lists); no history or password import
(D10, D15).

### The home page and the design

The new-tab page, sidebar, page card, palette and Customize panel follow the
design canvas (`design/README.md`); its tokens become `atlas-ui`'s five
palettes and the accent. The design direction is already chosen: the canvas
is offered as the direction at the design gate. Haze uses Mica; Glass uses
Acrylic plus Atlas's styling; both fall back to Graphite when "Reduce
transparency" is on. Pages stay opaque.

### Privacy and shipping

No telemetry; crash logs and dumps local. SmartScreen stays on. Without
Tauri, Atlas builds what Tauri gave for free (realism check, objection 6):
an updater that verifies ed25519/minisign signatures, single-instance
forwarding of links opened from other apps, default-browser registration,
and an installer. Code signing waits (S8), so installs show SmartScreen's
warning. Atlas notices WebView2 runtime changes (D36).

## Sequencing

Each phase becomes a milestone. Phase 0 runs locally before the issues for
phases 1–5 are broken down (D16).

### Phase 0 — Feasibility spike (throwaway, local)

- **Why now:** every later phase stands on the engine, the toolkit and the
  overlays, and three of them rest on behaviour nobody has run.
- **Delivers:** a throwaway GPUI + wry app and a Tauri + Svelte control (page
  tabs through wry in both, and one CodeMirror tab in each), run against
  note 06's checklist with these additions:
  - a **measurement protocol written before any number is taken:** private
    working set of all Atlas or Edge processes; Edge on a fresh profile
    with sleeping tabs at the same timeout as Atlas's suspend; the median of
    5 runs; idle CPU under 0.5% of one core over 5 minutes; the sites
    counted as N of 13 with "pass", "partial" and "fail" defined;
  - the owner's real Google sign-in on every path they use (D21), and
    YouTube, Disney+ and Hulu with their accounts;
  - shortcuts in a page, a CodeMirror and an xterm.js tab;
  - the popup overlays over a page and over the CodeMirror tab;
  - the GPUI base chosen and `=`-pinned (`gpui-pre` or `gpui-fast`), with an
    upgrade budget and a list of the chrome widgets gpui-component lacks.
- **Exit criteria:** a written Windows verdict
  (`docs/research/09-phase-0-verdict.md`) with go/no-go against D6, D4's
  switching rule applied, and the owner told what worked, what didn't and
  what is recommended. Work stops there, as `CLAUDE.md` asks. The spike's
  code is never kept.

### Phase 1 — Foundations (W0)

- **Why now:** everything else is built on them.
- **Delivers:** the scaffold (workspace, `xtask check-deps`, check commands,
  a first passing test, `docs/LOCAL-DEV.md`); the tab model crate with tests;
  storage with migrations and snapshots; the tier 1 search crate with
  benchmarks; the design tokens in `atlas-ui`.
- **Exit criteria:** `cargo fmt --check`, `cargo clippy -D warnings` and
  `cargo test` pass; the tab model, migrations and ranking are covered by
  tests; tier 1 meets 5 ms at 100,000 rows in `xtask bench`; the five
  palettes render a sample screen matching the canvas.

### Phase 2 — The browser shell

- **Why now:** the first build Atlas can browse with.
- **Delivers:** the window, sidebar and page card; web tabs (create, switch,
  close, reorder, pin); navigation and the address; the shortcut router;
  popups as tabs; fullscreen; crashed-tab recovery; suspend; permissions;
  WebView2's find, downloads, print and context menu; the sidebar peek.
- **Exit criteria:** the owner can browse their daily sites for a day in
  Atlas with no crash; every shortcut in the table works with a page focused.

### Phase 3 — Browser memory and the home page

- **Why now:** without history, bookmarks and restore, Atlas isn't a daily
  browser.
- **Delivers:** history, page-text indexing with exclusions (after the
  tier 2 bake-off), bookmarks with HTML import, the closed-tab stack,
  session restore, the home page with Quick access tiles, and the Customize
  panel (palette and accent).
- **Exit criteria:** quitting and relaunching restores every tab; the owner's
  exported bookmarks import correctly; excluded sites leave no page text in
  the database.

### Phase 4 — The command palette

- **Why now:** the palette is central (S5), and it needs the memory of
  phase 3 to search.
- **Delivers:** the palette over any tab, scope chips, highlighted matches,
  preview, Search the web.
- **Exit criteria:** opening the palette paints within one frame of Ctrl K;
  each keystroke ranks within 5 ms at 100,000 rows; the overlay tests from
  Phase 0 pass in the real app.

### Phase 5 — Daily driver

- **Why now:** to make Atlas the owner's default browser.
- **Delivers:** the signed updater, single-instance link forwarding,
  default-browser registration, an installer, the WebView2 runtime notice,
  and an accessibility pass (F6 between panes, NVDA over the chrome).
- **Exit criteria:** Atlas is set as the default browser and used as the
  owner's only browser for a week, with memory and start-up within D6's
  thresholds.

## What is deliberately not built

In plan 1: Chrome extensions (WebView2 can't run them); a password manager,
password import and ad blocking; macOS and Linux; history import; telemetry;
Servo; picture-in-picture, split view, multiple windows, a site-settings
page and save-page-as (`docs/research/02-browser-v1-scope.md`, "Later");
code signing (S8); a frozen-page overlay (D7); live search suggestions
(D9). In v1 as a whole: the WebAssembly plugin host (D17); the debugger
(D31); downloading language servers (D30); Fledge beyond the plugin runtime
and "New lead" until Stage C (D34); a graph view (S4).

## ⚠️ Still unproven

- ⚠️ Google sign-in end to end in WebView2, on every path the owner uses;
  and that WebView2 keeps presenting as Chrome to Google. Proven by Phase 0;
  watched by D36.
- ⚠️ A GPUI popup, re-owned with raw Win32, stacks and takes focus correctly
  over a child WebView2. Proven by Phase 0; no ready fallback.
- ⚠️ Disney+ and Hulu play with Widevine L3 in an Atlas tab. Proven by
  Phase 0 with the owner's accounts.
- ⚠️ GPUI's chrome saves enough start time and memory to justify itself
  over Svelte (D4's rule). Proven by Phase 0.
- ⚠️ GPUI's AccessKit works with NVDA on Windows 11. Checked in Phase 0,
  required in Phase 5.
- ⚠️ A way to read page text that injects nothing on excluded sites (D35).
  Chosen in Phase 3.

## Sibling plans

Outlined now; each gets its own question rounds and full detail after the
Phase 0 verdict (D16):

- `docs/plans/Atlas v1 — Plugins and Fledge — Plan.md` (plan 2)
- `docs/plans/Atlas v1 — Notes — Plan.md` (plan 3)
- `docs/plans/Atlas v1 — Forge — Plan.md` (plan 4)

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

started: 2026-10-09   code repo: none yet (local, D16)   plan branch: claude/plan-atlas-v1
| wave | job | route | model | worker result | branch @ sha | review | merged |
|---|---|---|---|---|---|---|---|
| Phase 0 | GPUI candidate spike | spike (worktree) | opus | partial: built, 50/50 automated checks; idle CPU 3.75% fails 0.5% bar; owner tests pending | spike/gpui @ bfac1c5 | n/a | never (throwaway) |
| Phase 0 | Tauri + Svelte control spike | spike (worktree) | opus | done: 25/25 in both overlay modes; idle CPU 0.54% | spike/tauri @ 2579d86 | n/a | never (throwaway) |
gates: 2026-10-09 owner approved starting Phase 0 locally
current: Phase 0, both spikes building   next: collect both RESULTS.md, run the owner's manual tests, write docs/research/09-phase-0-verdict.md, stop and report

## Iteration log

- **2026-10-10** — Owner's manual tests on the GPUI spike, first report:
  Google sign-in, YouTube and Disney+ all work. Not yet reported: which
  sign-in paths were tried, Hulu, the other daily sites, and staying signed
  in after a restart. Both spikes' automated results are in the ledger; D4's
  comparison favours GPUI on start-up (chrome on screen 0.52 s against
  1.44 s) and memory (27 MB against 175 MB with no tabs; 273 MB against
  335 MB with one), provided idle CPU is fixed (GPUI 3.75% of a core against
  the 0.5% bar; the control 0.54%).
- **2026-10-09** — The owner approved starting Phase 0 locally. Two
  throwaway spikes started in worktrees: the GPUI candidate and the Tauri +
  Svelte control. `CLAUDE.md` and `AGENTS.md` adapted from fledge-template
  at the owner's request, with `docs/trapstoknow.md`.
- **2026-10-09** — Split into four plans as decided in D5 (step 4). This file
  is plan 1 and keeps the v1-wide decisions; plans 2–4 are outlines. Plan 1's
  design, phases 0–5 with exit criteria, what is not built and what is
  unproven were written (step 5).
- **2026-10-09** — Page text is indexed with exclusions for Google, banking
  and no-store pages (D35); Atlas notices WebView2 runtime changes and offers
  Edge for Google if sign-in breaks (D36); `CLAUDE.md` updated with the
  owner's yes to describe WebView2, wry and GPUI and to point at
  `docs/decisions.md` and this plan.
- **2026-10-09** — Realism check (step 3): "realistic with changes", twelve
  objections, summarised under "Is it realistic?". Owner's answers: keep
  GPUI but let Phase 0 decide against a written rule (D4 revised); overlays
  gate on a live popup, blur optional (D7 revised); Phase 0 runs locally
  first, plans 2–4 stay outlines until its verdict (D16 revised); plan 2 is
  thin, with D19 and D20 moved after Stage C (D34). D22 now covers Google
  breaking after launch; D28 states its exception to "Rust owns all state".
  The departures from the spec were written into `docs/decisions.md`.
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
