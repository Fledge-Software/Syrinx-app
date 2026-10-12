---
title: "Atlas v1 — Plan (plan 1: engine, browser core and palette)"
status: active
created: 2026-10-09
subject: Atlas v1 plan 1 — the engine, the browser core and the command palette; also holds the v1-wide decisions that plans 2–4 share
relates_to: "docs/spec.md; docs/decisions.md; design/README.md; docs/research/; sibling plans: Atlas v1 — Plugins and Fledge, Atlas v1 — Notes, Atlas v1 — Forge"
issues_repo: not yet created (the repository is local; the owner creates it before anything is published)
branch: claude/plan-atlas-v1
---

# Atlas v1

> **Status: active.** The owner approved all four plans, this plan's 40-slice
> breakdown and the local implementation/review workflow on 2026-10-10 (D41).
> The same day's D43–D45 amendment preserves all 40 slice IDs and their order.
> Plans 2–4 now also have approved 12/16/21 slice maps; predecessor exits remain.
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
`docs/research/` provide the reasoning for work built in order. The engine
decision reopened on 2026-10-09 is settled. P1-01's scaffold, P1-02's tab
model and P1-03's storage are merged. P1-04's SQLite maintenance prerequisite
and complete backup/recovery slice passed the 138-test local gate and
independent source reviews. P1-04 is committed and locally merged as `7710c06`.
D49's integrated browser preview now has a window, native pages and a palette,
but interactive acceptance, the reported WebView2 crash and the remaining
browser requirements are unresolved; see `docs/implementation-status.md`.

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

**Confirmed by the owner's handoff on 2026-10-10:** v1 is for the owner on
Windows 11, build 26300, with agents assisting. The repository remains local.

**What this plan assumes, until the owner confirms (⚠️):**
- ⚠️ Success means Atlas is the owner's default browser for daily use, with
  memory and start-up close to Edge's.
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

The findings below record the pre-spike reasoning. For measured results and
the owner's completed tests, `docs/research/09-phase-0-verdict.md` supersedes
their untested-runtime statements; outstanding checks are listed below.
⚠️ Historical comparisons with rejected engines, service resolution caps,
DRM security levels and third-party project timelines have not been
re-established by the readiness review. They are not current guarantees or
permission to reopen D3/D4. The current evidence is in
`docs/plans/Atlas v1 — Readiness and implementation breakdown.md`.

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
| D49 | What should be runnable next? | Bring the integrated product browser forward: existing tabs, saved-page storage, backups and search connected to a GPUI window, vertical tabs, WebView2 navigation and Ctrl K. Keep remaining features and performance requirements tracked; commit/merge approvals still apply. | Continuing strict order before exposing any morning work in a window: the owner explicitly chose a usable integrated preview after rejecting the old spike. Calling the preview full v1: its unfinished requirements remain visible. | 2026-10-10 |
| D48 | How is the 5 ms tier 1 search target measured? | Require 95% of repeated complete 100,000-row searches to finish within 5 ms; report the worst sample too. Measure ranking, scope counts and returned highlights together. | Requiring every measured sample below 5 ms: occasional Windows scheduling interruptions can dominate a maximum. Hiding maxima or measuring the matcher alone: conceals the actual responsiveness. | 2026-10-10 |
| D47 | How does explicit history clearing survive restoring an older backup? | Keep a local deletion record containing only cleared date ranges and data categories, never URLs or page contents. Restoration honors it, preserving older recovery points without reintroducing cleared history. | Discarding all older backups whenever history is cleared: loses their recovery depth. Restoring deleted history: undoes the owner's clearing request. | 2026-10-10 |
| D46 | May Atlas restore a damaged database automatically? | Offer the newest verified backup with its date and ask the owner before replacing the current database. Preserve damaged files locally. | Automatic restoration: the owner wants to confirm a replacement that may lose newer state. | 2026-10-10 |
| D45 | What are the top square sidebar tiles? | Favorites, separate from inline saved pages/folders. They may target websites, plugin pages and other supported Atlas destinations. Plugin targets use the normal plugin lifecycle; until implemented, show the existing unavailable indication. | Website-only tiles exclude the owner's plugin destinations. Treating tiles as saved-page records conflates two navigation surfaces. An early plugin host or fake successful opening skips plan 2. | 2026-10-10 |
| D44 | Can the sidebar adapt to the space the owner wants? | Add smooth width dragging from wide with labels to favicon-only to hidden, with reverse dragging restoring it. Preserve hidden-sidebar hover peek and Ctrl Alt S. The owner's screenshot supplies direction; do not copy it into the repository. UI thresholds and compact-folder affordances remain for later review. | A fixed-width sidebar cannot provide the requested space control. Replacing hover peek or the shortcut removes settled navigation. Guessing thresholds before the UI prototype hides an undecided interaction. | 2026-10-10 |
| D43 | How do saved pages fit among tabs? | Pin means Save: persistent inline rows and folders. Closing unloads only the live page; opening focuses an existing instance or loads the saved address. Live navigation leaves saved title/address unchanged. Remove/Unpin leaves a live page ordinary. Saved rows come first; saving appends to their group and removing an open saved page puts it at the start of ordinary tabs. HTML import preserves folders and starts unloaded. The owner approved the revised 40-slice map, with a flat foundation now and full folders/import later. | A separate bookmark destination misses the requested inline workflow. A temporary pin flag would require replacing P1-09's behavior later. Opening every imported page wastes memory. Updating saved addresses on navigation changes the destination without an explicit edit. D42's original live-pin behavior is superseded; its web-tab close/history rules remain. | 2026-10-10 |
| D42 | How do tabs close, reorder and reopen predictably? | Closing the active page selects the next open page to its right, otherwise its left, skipping unloaded saved rows. Closing the last live page shows Home and keeps Atlas open. New ordinary web tabs append and become active. Reorder stays within saved/ordinary groups. Keep the 20 most recent web-tab closures, including saved-page instances. Ordinary reopen restores metadata/position. Under D43, saved reopen focuses its live instance or loads its fixed saved target if the row survives; if removed, history may reopen an ordinary page but never recreates the saved record. Plugins are excluded. No live page or form contents are retained. | Most-recently-used selection: the owner chose visible adjacency. Closing Atlas with its last page loses Home. A 100-entry stack: the owner chose 20. Retaining live pages/forms exceeds metadata-only reopen. Plugin reopen is already available through permanent rows. Restoring a deleted saved row would undo an explicit removal. | 2026-10-10; clarified by D43 |
| D41 | May implementation proceed while the repository stays local? | Yes. The four plans and plan 1's 40-slice breakdown are approved. One local branch per slice, independent review, and the owner's approval before every merge replace D16's GitHub prerequisite. Commits still require the owner's request; first preserve the planning baseline with an explicitly requested documentation commit. No remote or pushes. | Waiting for GitHub: the owner wants to continue locally. Treating workflow approval as permission for commits or merges: those remain separate owner controls. | 2026-10-10 |
| D40 | Does the seven-day page-text limit include Atlas backups? | Yes. Keep page text and its index in an expiring cache outside D13's seven daily durable-state snapshots. Expiry applies to active storage and must survive restart, replay and restore. This is an Atlas-managed retention policy, not forensic erasure. | Only hiding older text from search while keeping it in snapshots: exceeds the retention the owner chose. | 2026-10-10 |
| D39 | Keep WebView2 despite its diagnostics, and may Atlas have analytics? | Keep WebView2 and SmartScreen and disclose the runtime's collection; crash dumps stay local. Replace the blanket no-analytics rule with local aggregate feature/command counts, timings and error categories, explicitly exported by the owner. No account/device IDs, URLs, searches, paths, document text or detailed event timeline; no automatic uploads. A model may suggest changes from a report the owner chooses to share, and the owner reviews changes. Review the concrete schema, retention/control behavior and added implementation slices before collection. | No analytics: the owner wants evidence to improve Atlas. Automatic uploads and an automatically adapting interface: the owner selected local counters, explicit export and reviewed suggestions. Removing IDs as an absolute anonymity guarantee: one person's report can remain identifiable by context. | 2026-10-10 |
| D38 | How do development builds run, given Smart App Control blocks unsigned debug builds? | A release-style profile with debug info for the gate and dev runs; no change to Windows settings. The owner requires local Atlas to run unsigned for now; no certificate prerequisite. Verify each app build because this profile grants no policy exemption. | The owner adjusting Smart App Control: changes a security setting for convenience (`docs/research/09-phase-0-verdict.md`). Making a certificate mandatory: the owner has none and signing remains deferred. | 2026-10-10 |
| D37 | May Atlas carry a patched GPUI? | Yes, on D12's rules: small, `=`-pinned, offered upstream, dropped once merged. First use: GPUI's Windows idle loop, which repaints every window each frame (idle CPU 3.75% of a core against the 0.5% bar). If the patch can't reach 0.5%, GPUI against Tauri returns to the owner | Switching to Tauri now: gives up about 0.9 s of start-up and 60–150 MB. Accepting high idle CPU: battery and heat for a browser left open all day | 2026-10-10 |
| D36 | Does Atlas watch for Google sign-in breaking? | It shows a one-line notice when the WebView2 runtime version changes, so the owner watches for sign-in trouble, and offers to open the current Google page in Edge in one click if sign-in breaks. No automatic sign-in test | An automatic canary that signs in: credentials in a test. Nothing: a silent break would surprise the owner mid-work | 2026-10-09 |
| D35 | How is page text indexed (D18) without breaking Google? | Indexed locally for 7 days, but never on Google's sites, banking and payment sites, or pages sent with `Cache-Control: no-store`; the owner can add sites to the exclusion list in settings. The extraction method is chosen in plan 1 and must inject nothing on excluded sites | Dropping it for v1: the owner wants recall by content. Indexing everything: breaks the rules that keep Google working (`docs/research/07-google-sign-in-and-drm.md`) and keeps a 7-day record of banking pages in the database and its snapshots | 2026-10-09 |
| D34 | How big is plan 2 in v1? | Thin: the plugin runtime, the `Plugin` trait, the stand-in Fledge client and the palette's inline "New lead" action. D19's keep-local switches and D20's full Fledge client move to the real-client work after Stage C | Building D19 and D20 in full on the stand-in: design and settings on an API that doesn't exist yet and will change, against D5's "deliberately thin" | 2026-10-09 |
| D32 | Which input methods must the editors be tested with? | English and Latin scripts only; no IME test matrix in v1 | Chinese, Japanese or Korean IME testing: not used by the owner | 2026-10-09 |
| D31 | Run control and debugger in Forge v1? | Run control in (an addition to the spec): Forge reads tasks from Cargo, npm scripts, pyproject, just and make and runs them in a terminal. Debugger out of v1. On 2026-10-10 the owner confirmed standard installed Python entry points plus terminal commands are enough; runner-specific uv, Poetry, tox and nox adapters wait for a stated need. `[project.scripts]` values are entry-point references, not shell commands (see plan 4) | Both out: loses the PyCharm workflow the owner relies on. Debugger in: Zed's took 8 months and over 25,000 lines (`docs/research/04-forge-and-notes.md`). Guessing common Python task tables: adds adapters the owner has not requested | 2026-10-09; Python scope confirmed 2026-10-10 |
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
| D16 | When does building start, and how is work tracked while Atlas is local? | Phase 0 first, locally, then stop for the owner. The original post-spike GitHub prerequisite and outline-only restriction are superseded by D41: all four plans and plan 1's local slice map are approved, and implementation may use local branches with independent review and owner-approved merges. GitHub creation remains the owner's action when wanted. *Earlier wording required a repository before issue breakdown; the 2026-10-10 approval explicitly changes that gate.* | Breaking all four plans into implementation issues before Phase 0: its results could invalidate them. Requiring a repository now: the owner approved local implementation and deferred publication. | 2026-10-09; revised 2026-10-10 |
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

The owner-requested documentation baseline is committed as `d79fc29` on
`claude/plan-atlas-v1`. P1-01 passed its gate and independent review, then
was committed and merged with the owner's approval as `ed19d6d`: a minimal
Cargo workspace, runnable console entry point and dependency-boundary gate.
The scaffold branch remains at that commit. P1-02 passed its gate and
independent review on `codex/p1-02-tab-model`, then was committed and merged
into `claude/plan-atlas-v1` as `892d3bc` with the owner's approval.
There is no browser UI yet. P1-03 storage is implemented on
`codex/p1-03-storage`, based on the P1-02 completion record `9a6ae35`.
Its 77-test local gate and independent reviews passed, and the owner then
requested its commit and approved its local merge, completed as `c4a6631`
in `claude/plan-atlas-v1`. For P1-04, the owner approved confirmed restoration
(D46), a local history-deletion record (D47), and the pinned SQLite 3.53.4
maintenance patch. The patch passed the full 78-test gate; its source audit
and hashes are recorded in `docs/p1-04-sqlite-maintenance-review.md` and
the vendor provenance. The complete P1-04 backup/recovery implementation
subsequently passed the 138-test gate and independent source reviews;
`docs/p1-04-recovery-preflight.md` records the design and result. With the owner's
explicit approval, `7710c06` was committed and fast-forward merged locally.
The throwaway Phase 0 spikes remain on `spike/gpui` and `spike/tauri`; their
code has not been copied into the product. The current branch also holds
the spec, owner decisions, design snapshot (`design/canvas/project/`, with
the five palettes' tokens in the
`tokens()` method of `design/canvas/project/Main.dc.html` and one board per
palette in `Theme*.dc.html`), the original mockup
(`design/reference/unified-workspace-mockup.html`) and research notes 01–09.
The repository is local, with no remote. A reader grepping the research will
find Tauri and Svelte everywhere: notes 01–05 were written when the spec's
Tauri plan stood, and D3 and D4 replaced it.

## The design

This section covers plan 1: the engine, the browser core and the palette.
Plans 2–4 have approved detailed plans in their own files (see "Sibling plans").

The Phase 0 GPUI manifest pins `gpui-pre =0.3.8`,
`gpui-pre-platform =0.3.8`, `gpui-component =0.7.1`, `wry =0.57.0`,
`webview2-com =0.39.1` and direct `windows =0.62.2`. These are the existing
baseline, not permission to upgrade. Audit the resolved dependency graph
before adding it to the product; exact versions alone do not prove licence
or security suitability.

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

One generic model, as the spec asks (`docs/spec.md`, "Core model"). A live
tab has an id, a kind (web, plugin view, note, file, diff, terminal), a title,
address, favicon, order and optional badge. D43 replaces a temporary pinned
flag with an optional association to an independently identified saved page.
A saved page has its own stable title, address and position even when it has
no live tab. Plan 1 builds only web behavior, but the kind enum is complete
so plugins slot in.
The sidebar and the palette read this model, never per-feature code. Persist
changes as they happen rather than only on exit. Restore the last committed
session after an application crash; queued changes may not have committed.
With WAL and `synchronous=NORMAL`, a power loss or operating-system crash
can roll back recent commits. This is not a zero-loss guarantee
([SQLite durability settings](https://www.sqlite.org/pragma.html#pragma_synchronous)).

The owner's D42/D43 rules make transitions follow the visible list. Closing
the active page selects the next open page to its right, otherwise its left;
unloaded saved rows are skipped. Closing the last live page shows Home
without exiting Atlas. Saved entries precede ordinary tabs. Saving appends
to the saved group; Remove/Unpin puts any remaining live page at the start
of ordinary tabs. Reorder stays within each group. New ordinary web tabs
append and become active. The in-session stack keeps the 20 most recent
web-tab closures, including saved-page instances, but never retains live
pages or form contents. Ordinary reopen restores metadata and position.

A saved row survives closing its page. Opening it focuses its existing
instance or creates a page at the saved address; navigation cannot overwrite
its saved title/address. For a saved-page history entry, reopen uses this
same open/focus path if the saved row survives. If the row was removed,
history may reopen an ordinary page from the closure metadata but must not
resurrect the saved record. Plugins do not enter the closed-tab stack.
P1-02 implements only flat saved-entry lifecycle and its association with
live tabs. P1-03 persists both; P1-21 adds full inline folder
management, and P1-22 imports HTML bookmarks into that same model unloaded.
The sidebar is a projection of these records, not a second source of state.

The following are implementation safeguards, not additional owner choices.
Opening a new live instance assigns a fresh tab identity so an old page's
late events cannot affect its replacement; saved-page identity survives
unloading. Clamp an ordinary reopen position to the current ordinary group
length; a surviving saved entry keeps its own position. With no live pages the
model has no active tab, even if saved rows remain; the later shell shows
Home for that state. Explicit foreground/background opening modes keep the
API usable for later callers; ordinary new-tab actions use foreground mode.
P1-02 does not build a runtime
plugin registry early; that remains plan 2 work.

### Pages

- **One shared WebView2 environment,** built by Atlas through `webview2-com`
  so it can keep crash dumps local (`IsCustomCrashReportingEnabled`) and
  replace wry's default browser arguments, which switch SmartScreen off
  (`docs/research/08-gpui-and-wry-on-windows.md`). Set options on the supplied
  environment itself; a later wry builder argument does not reconfigure it.
  Explicitly disable password autosave (D11). This does not erase existing
  passwords. Check that the Evergreen Runtime and required interfaces exist;
  explain a missing runtime without downloading or installing it automatically.
- **Each tab is a wry child web view** of the GPUI window, with no IPC
  handler and no injected script, no custom user agent, and nothing
  injected on Google's sites (the rules under "What we found").
- **Raw WebView2 calls** through `WebViewExtWindows` for shortcuts
  (`AcceleratorKeyPressed`), fullscreen video
  (`ContainsFullScreenElementChanged`), process failure (`ProcessFailed`)
  and hidden tabs (`TrySuspend` after a delay). A main-frame renderer failure
  can offer Reload; loss of the shared browser process requires recreating
  all affected views. Deduplicate recovery events and keep the core tab state.
  Hide before suspending, check the completion's success value, and reject a
  late completion for a tab that has since become active. Do not combine
  suspension with a memory-target policy on the same view.
- **Popups become tabs** through wry's new-window handler, in the same
  environment and profile. The target has not navigated before adoption
  through the new-window result; merely opening the requested URL does not
  preserve `window.opener`.
- **A permission handler from day one,** with WebView2's own prompt and its
  per-site memory. Keep clipboard auto-grant disabled; never infer a
  requesting origin from whichever tab happens to be active.
- **Lean on WebView2's built-in UI** for find (Ctrl F), downloads (Ctrl J),
  printing, the right-click menu and notifications, and replace each only if
  it looks wrong in the page card (`docs/research/02-browser-v1-scope.md`,
  "What this means", item 4).
  Route Ctrl J explicitly to `OpenDefaultDownloadDialog`; test notification
  delivery and saved permission choices on this runtime. Include the scope's
  existing Musts: zoom, a truthful HTTPS indicator and certificate warnings.
  wry 0.57 disables zoom hotkeys by default; explicitly enable
  `with_hotkeys_zoom(true)` and test Ctrl plus/minus/0. Per-site persistence
  is not promised. Preserve native certificate-error handling; never choose
  `ALWAYS_ALLOW`. ⚠️ A complete successful-page security-state API has not
  been established in the pinned bindings, so the indicator can describe
  scheme and observed errors, not promise a verified secure connection.
  Never bypass a certificate error or treat an HTTPS URL alone as proof of
  a secure loaded page.

One environment does not guarantee one renderer process. D28's editor
resource aim must be measured; Chromium decides process sharing and isolation
([WebView2 process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model)).

### Shortcuts

Ordinary GPUI key handlers do not receive keys while a page has focus,
so Atlas uses WebView2's `AcceleratorKeyPressed` on **every** web view it
creates (pages now; the CodeMirror and terminal web views in plans 3 and 4).
The handler is synchronous and blocks the browser process, so it only marks
the key handled and posts a message to the GPUI thread, filtering repeats
where appropriate. One command table in
`atlas-core` maps keys to commands; the palette and menus read from it. The
sidebar pin key is Ctrl Alt S (D8/D44); it is distinct from saving a page.

Phase 2 supplies a minimal owned address-entry popup for Ctrl L and Ctrl K,
and in-session reopen-closed tabs for Ctrl Shift T. Phase 4 extends that same
popup with ranked search, scopes and previews; it does not build a second
overlay. This keeps the Phase 2 shortcut gate executable before full search
UI exists. Phase 3 adds durable session recovery to the earlier tab behavior.

### Overlays

The palette (Ctrl K) and the sidebar peek are borderless GPUI popup windows
above the page, owned by the main window through `GWLP_HWNDPARENT` (GPUI's
popups are otherwise topmost over every app), near-opaque, with the
`DWMSBT_TRANSIENTWINDOW` Acrylic backdrop as a bonus. The peek takes no focus;
the palette does, and Esc returns it to the page with wry's `focus()`.
Set ownership on the popup, not on the child page window; remove topmost
behavior and retest Alt Tab, minimize, restore and closing the main window.
A successful backdrop API call does not prove visible blur (Phase 0).
Hovering the left edge is detected by a few-pixel strip of chrome or by
polling the cursor, because a pointer over a page never reaches GPUI. They
must work over every kind of tab (D7).

### Storage

rusqlite (bundled) with `rusqlite_migration`, WAL, `synchronous=NORMAL`,
one writer thread and two readers, in `%LOCALAPPDATA%\Atlas\data\atlas.db`
(never a synced folder), with a daily `VACUUM INTO` snapshot taken when idle
and 7 kept (D13), and a clean-shutdown flag that triggers a `quick_check`
after a crash (`docs/research/03-core-architecture.md`, "Storage").
Write each snapshot to a fresh destination; only a completed, validated
snapshot enters the seven-copy rotation. An interrupted `VACUUM INTO` can
leave unusable output ([SQLite snapshot behavior](https://www.sqlite.org/lang_vacuum.html)).
Test recovery on disposable databases. The interaction between the seven
snapshots and D18's seven-day page-text retention is settled by D40: the
page-text cache and index stay outside these backups, with at most seven
days retained. ⚠️ Prove expiry and restart/replay/restore behavior before
enabling capture.
The owner must confirm restoration before Atlas replaces the current
database with the newest verified backup, shown with its date; damaged files
remain preserved locally (D46). Keep a local deletion record containing only
cleared date ranges and data categories, never URLs or page contents, so
restoring an older snapshot cannot reintroduce explicitly cleared history
(D47) while preserving older recovery points. The proposed engineering design keeps
that journal outside restored state and applies it to a staged recovery
copy. P1-04 now defines the content-free format and refusal/replay contract;
P1-27 must persist the journal and generation barrier and connect actual
clearing participants before history clearing ships. No history collection
or history tables belong in P1-04.
The owner-approved SQLite 3.53.4 patch now supplies the reviewed damaged-input
fixes and passed the full 78-test gate. The P1-04 backup and recovery design is in
`docs/p1-04-recovery-preflight.md`: an external clean-shutdown record finalized
after all database connections close, guarded snapshot publication, an
interruption-aware restore protocol and the deletion-journal boundary.
Lifecycle ownership, snapshots, rotation, confirmed recovery with interruption
resume and the deletion-log contract are implemented. The complete 138-test
gate and independent source reviews passed; the owner approved its commit and merge.

P1-03's Windows directory guard belongs in `atlas-platform`, even though
the browser engine is introduced later. The store accepts a directory lease
through its own interface; it never depends on the platform crate. This
filesystem prerequisite enforces D13 without weakening the headless boundary.
`docs/storage-path-preflight.md` records registered sync-root detection,
handle lifetimes, refusal tests and the limits of that check. No Windows API
proves that an unregistered program never copies a folder. The core snapshot
format preserves full-width IDs, saved/live order and allocation watermarks;
closed history is not persisted. SQLite encodes IDs as eight-byte values,
since its signed integer range cannot represent all valid model IDs.

### Search

- **Tier 1, in memory:** one row per titled thing (open tabs, bookmarks,
  recent history, commands), ranked by `frizbee` with frecency and an
  open-tab boost on every keystroke, with the scope chips counted in the
  same pass. Budget: under 5 ms at the 95th percentile for 100,000 rows,
  with the worst sample also reported (D48).
- **Tier 2, full text:** page text for 7 days (D18, D35), excluding Google's
  sites, banking and payment sites and `no-store` pages. The store (tantivy
  or SQLite FTS5) is chosen by a one-day bake-off, here in plan 1 because
  page text needs it; the extraction method must inject nothing on excluded
  sites.
  ⚠️ Decide eligibility before any extraction, including redirects and frame
  boundaries. A response-header observation is not permission to install a
  request filter. Prove `no-store` handling and exclusion coverage with
  synthetic sites before asking the owner to enable capture on daily browsing.

### The palette

Opens with Ctrl K over any tab. Scope chips with counts (Everything, Tabs,
Bookmarks, History; plugin actions arrive in plan 2), matched characters
highlighted, a preview of the selected row, keyboard-first (arrows, Enter,
Esc). It is also the address bar: it always offers "Search the web" (Alt
Enter) on Google, with no live suggestions (D9).

### Browser memory

History comes from navigation and title events. D43's inline saved pages
and folders are the bookmark source of truth; HTML import (D14) populates
them without loading pages. Ctrl Shift T uses D42's web closed-tab stack,
including saved-page closures through D43's open/focus rules.
Session restore preserves live tabs' addresses/titles/order and
their saved-page associations; it restores saved rows without loading every
saved destination. WebView2 back/forward lists are not promised. No history
or password import is added (D10, D15).

### The home page and the design

The new-tab page, sidebar, page card, palette and Customize panel follow the
design canvas (`design/README.md`); its tokens become `atlas-ui`'s five
palettes and the accent. The design direction is already chosen: the canvas
is offered as the direction at the design gate. Haze uses Mica; Glass uses
Acrylic plus Atlas's styling; both fall back to Graphite when "Reduce
transparency" is on. Pages stay opaque.

D43–D45 amend the older snapshot: inline saved pages/folders share the
sidebar with ordinary tabs, while the separate top square tiles are named
Favorites and can target supported websites, plugin pages or other Atlas
destinations. Plugin Favorites use the normal plugin lifecycle once it
exists; before then they stay visibly unavailable. The home page's Quick
access remains its own surface.

D44 adds smooth sidebar dragging through wide, favicon-only and hidden
states, and reverse dragging restores it. Persistent sidebar width changes
resize the page card; the existing hover peek overlays it without resizing.
Ctrl Alt S remains available. P1-07 reviews the interaction thresholds;
P1-08–P1-10 apply width states, row rendering and focus, with the owned live
peek delivered by P1-17. P1-29 persists width/presentation. The owner's
screenshot supplies direction without becoming a repository asset.

### Privacy and shipping

The owner kept WebView2 and SmartScreen after reviewing their collection
(D39). Disclose the runtime behavior: the custom-crash flag covers crash
submission, not every required diagnostic. Crash logs and dumps stay local
([Microsoft's privacy description](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/data-privacy)).

To guide improvements, D39 replaces the blanket no-analytics rule with
local aggregate counters and explicit report export. Count Atlas feature
use, built-in commands, timings and error categories without identifiers,
URLs, searches, paths, text or an event timeline. Use fixed metric names and
coarse timing buckets; never collect content merely to strip it out later.
No upload service or model call is authorized. The owner chooses whether to
share a report for suggestions, and reviews product changes. A single-user
report is not guaranteed anonymous merely because it contains no account ID
([NIST's de-identification guidance](https://csrc.nist.gov/pubs/sp/800/188/final)).

⚠️ Before analytics implementation, present the bounded schema, synthetic
example report, enable/reset/retention behavior, export preview and performance
budget for the owner to approve. The added analytics slices are a proposed
amendment outside the already approved 40-row map; they do not block the
scaffold. Tests remain synthetic, and no real usage is read or exported here.

Without
Tauri, Atlas builds what Tauri gave for free (realism check, objection 6):
an updater that verifies ed25519/minisign signatures, single-instance
forwarding of links opened from other apps, default-browser registration,
and an installer. Update-file signatures are distinct from Windows code
signing, which waits (S8). Unsigned builds can be blocked, not merely warned
about; D38 records a locally working profile, not a security-policy exemption.
The scaffold must run both its application and test executables under the
unchanged Windows policy. Atlas notices WebView2 runtime changes (D36).
⚠️ The update host, check cadence and installation/restart behavior need
owner approval before any updater network call is implemented. Verify the
signature before installation, and test rejection using synthetic packages.

## Sequencing

Each phase becomes a milestone. Phase 0 runs locally before the issues for
phases 1–5 are broken down (D16).

**Planning exception, owner, 2026-10-10:** finish the detailed drafts for
plans 2–4 locally before moving toward implementation. The owner is not
publishing the repository yet. This permits the planning work below to be
described in advance; implementation still proceeds through plan 1, then
plans 2, 3 and 4, one phase at a time. It does not authorize a remote,
published issues or an implementation shortcut around an unfinished gate.

**Local implementation approved, owner, 2026-10-10 (D41):** the four plans
and plan 1's 40-slice map are approved. Use local branches, independent review
and owner-approved merges without waiting for GitHub. Commits remain by
explicit request; first preserve the planning baseline. Existing technical
and operator gates still apply. Analytics under D39 receives its own bounded
amendment before implementation, without moving ahead of the foundations.

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
- **Also delivers (Phase 0 verdict):** the GPUI idle-loop patch (D37) with
  idle CPU re-measured under 0.5% of a core, and start-to-first-page
  re-measured the same way for GPUI and Tauri.
- **Order:** scaffold, tab model, storage, tier 1 search, GPUI idle patch,
  design tokens. Complete one review before the next. The idle measurement
  uses the retained throwaway harness with fresh synthetic profiles; it does
  not require building the Phase 2 product shell early or copying spike code
  into the product. Repeat the measurement on the product shell in Phase 2.
- **Exit criteria:** `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings` and `cargo test` pass using
  D38's profile as documented by the scaffold, with assertions and overflow
  checks retained; idle CPU under 0.5% with 8 tabs suspended; the tab model, migrations and ranking are covered by
  tests; tier 1 meets D48's 5 ms at the 95th percentile for 100,000 rows in
  `xtask bench`, with the maximum reported; the five
  palettes render a sample screen matching the canvas.

### Phase 2 — The browser shell

- **Why now:** the first build Atlas can browse with.
- **Delivers:** the window, sidebar and page card with D44 width states;
  ordinary web tabs and flat inline saved rows (create, switch, close/unload,
  reopen, reorder, save/unpin); navigation and the address; the shortcut router;
  in-session reopen-closed tabs and minimal address entry for Ctrl L/K;
  popups as tabs; fullscreen; crashed-tab recovery; suspend; permissions;
  WebView2's find, downloads, print and context menu; zoom; HTTPS/certificate
  status; the sidebar peek. The Musts come from research note 02; its old
  engine capability claims are not accepted without the current checks.
- **Exit criteria:** the owner has tested what Phase 0 left untested (Hulu,
  Google sign-in with a password and phone prompt, staying signed in after a
  restart, and the 13 other daily sites, at least 13 passing); the owner can
  browse their daily sites for a day in Atlas with no crash. With a page
  focused, Ctrl T/W/L/K, Ctrl Tab/Shift Tab, Ctrl Shift T, Ctrl Alt S and the
  delivered navigation/find/print/download/zoom keys work. Palette result
  selection and scope keys are tested once Phase 4 adds them. Synthetic tests separately cover
  renderer and shared-browser-process failure, permission persistence,
  certificate errors and suspend/resume races. Product-shell idle performance
  still meets D37 after integration.

### Phase 3 — Browser memory and the home page

- **Why now:** without history, bookmarks and restore, Atlas isn't a daily
  browser.
- **Delivers:** history, page-text indexing with exclusions (after the
  tier 2 bake-off), full inline saved-page/folder management and unloaded
  HTML import, the closed-tab stack, session restore, the home page with
  Quick access, separate sidebar Favorites, and Customize settings for
  palette/accent and sidebar width/presentation; clearing controls cover both
  WebView2 data and Atlas history/search copies, with partial failures shown.
  WebView2 clearing applies to the shared profile, not only the active tab.
  Keep its controller alive and await completion; an aborted operation or
  missing callback cannot be reported as success. Never clear by deleting
  the owner's user-data directory.
- **Exit criteria:** quitting and relaunching restores the committed live
  session and saved rows without opening every saved destination. The owner's
  exported bookmarks import into inline folders in an owner-operated check;
  automated imports use synthetic exports and create no live pages. Excluded
  sites leave no page text in the database or index; clearing data and expiry
  cannot reintroduce it via replay or restore. Retention follows the owner's
  clarification.

### Phase 4 — The command palette

- **Why now:** the palette is central (S5), and it needs the memory of
  phase 3 to search.
- **Delivers:** the palette over any tab, scope chips, highlighted matches,
  preview, Search the web.
- **Exit criteria:** opening the palette paints within one frame of Ctrl K;
  keystroke ranking meets D48's 5 ms at the 95th percentile for 100,000 rows,
  with the maximum reported; the overlay tests from
  Phase 0 pass in the real app.

### Phase 5 — Daily driver

- **Why now:** to make Atlas the owner's default browser.
- **Delivers:** the signed updater, single-instance link forwarding,
  default-browser registration, an installer, the WebView2 runtime notice,
  and an accessibility pass (F6 between panes, NVDA over the chrome).
- **Exit criteria:** Atlas is set as the default browser and used as the
  owner's only browser for a week, with memory and start-up within D6's
  thresholds.
- **Proposed addition after D39:** local aggregate counters and explicit
  report preview/export, split into AN-01 and AN-02 in the readiness report.
  The owner approved the feature direction, but its concrete schema/control
  proposal and added slice map still need approval before analytics work.
  The proposal places them after P1-39 and before the week of use (P1-40).

## What is deliberately not built

In plan 1: Chrome extensions (an Atlas scope choice, even though current
WebView2 has optional extension APIs); a password manager,
password import and ad blocking; macOS and Linux; history import; automatic
analytics uploads and remote crash reports;
Servo; picture-in-picture, split view, multiple windows, a site-settings
page and save-page-as (`docs/research/02-browser-v1-scope.md`, "Later");
code signing (S8); a frozen-page overlay (D7); live search suggestions
(D9). In v1 as a whole: the WebAssembly plugin host (D17); the debugger
(D31); downloading language servers (D30); Fledge beyond the plugin runtime
and "New lead" until Stage C (D34); a graph view (S4).

## Outlined later features

**Mobile sync over Tailscale or Headscale, with optional cloud sync later.**
The owner requested this direction on 2026-10-10 so a future mobile Atlas
can exchange selected data with desktop without an Atlas-hosted sync
server. Setup must offer an easy in-app installation and pairing walkthrough,
reuse existing Tailscale accounts, and support existing Headscale networks.
The [mobile sync outline](<Atlas later — Mobile sync outline.md>) records
the setup flow, proposed data scope, common sync engine, offline and mobile
limits, encrypted cloud alternative, acceptance criteria and open questions.

This is deferred until the future mobile phase after the four v1 plans.
It changes neither D2's Windows-only v1 scope nor the current slice order.
The outline records the feature direction; implementation needs a reviewed
mobile plan. Tailscale still uses coordination and sometimes relay servers,
and Headscale requires a user-operated coordination server, so this must
not be described as eliminating all server infrastructure.

## ⚠️ Still unproven

- ⚠️ **Remaining Google checks:** the password-and-phone-prompt path and
  staying signed in after restart await the owner in Phase 2. Passkey and
  Workspace sign-in passed in Phase 0. Future Evergreen behavior is not
  guaranteed; D36 watches runtime changes.
- **Live popups passed in both spikes**, including stacking and focus
  return. Repeat those checks in the production shell; the throwaway
  implementation is evidence, not reusable product code.
- ⚠️ **Hulu and the 13 other daily sites** remain owner tests in Phase 2.
  YouTube and Disney+ passed. Playback does not establish an unmeasured
  resolution or DRM-security-level guarantee.
- **GPUI cleared D4's chrome startup and memory comparison**, but idle CPU
  failed. Phase 1 must prove D37's patch reaches the 0.5% bar and remeasure
  start-to-first-page using the same endpoint in both candidates.
- ⚠️ **NVDA accessibility on Windows 11:** the Phase 0 verdict does not
  establish a completed screen-reader test. Verify the product shell in
  Phase 5; do not mark this passed from the toolkit's stated support.
- ⚠️ A way to read page text that injects nothing on excluded sites (D35).
  Chosen in Phase 3.
- ⚠️ **Tier 1 search has not met D48.** P1-05's first complete synthetic
  100,000-row benchmark passed correctness checks but missed the 5 ms p95
  limit in 25 of 26 cases; its worst sample was 383.737 ms. The measurements
  and ongoing profiling are in `docs/p1-05-search.md`. Upstream matcher
  microbenchmarks do not establish Atlas's complete-path responsiveness.
  One-frame palette paint and the real product's D6 memory/startup comparisons
  remain unmeasured.
- ⚠️ The private live design canvas has not been compared with the local
  snapshot in this review. Resolve that at the design-token gate using
  owner-authorized access; no integration app is authorized by this review.

## Sibling plans

Expanded locally after the Phase 0 verdict, at the owner's request on
2026-10-10. Each records its own product answers, ordered implementation
phases, acceptance criteria and remaining technical checks. The owner
approved all four on 2026-10-10; their implementation remains queued after
this plan, in order:

- `docs/plans/Atlas v1 — Plugins and Fledge — Plan.md` (plan 2)
- `docs/plans/Atlas v1 — Notes — Plan.md` (plan 3)
- `docs/plans/Atlas v1 — Forge — Plan.md` (plan 4)

## Open threads

The minimum list from the brief, grouped. Each is cleared as it is answered.

- **Owner's target date:** on 2026-10-10 the owner requested the full planned
  browser, including the built-in plugins, usable for their daily workflow
  before Monday, 2026-10-12. This is the delivery target, not evidence of
  completion. D49 subsequently approves bringing the integrated browser
  forward; checks and commit/merge approvals still apply. Report tested
  progress and concrete blockers against that target.
- **P1-05 acceptance:** retain D48 while profiling the failed search benchmark.
  Windows Application Control also blocks the rebuilt gate runner and store
  test executable (error 4551), including unchanged retries. Build helpers
  now use D38's optimized local route; no Windows setting was changed. The
  actual unsigned console app still launches. The owner requires unsigned
  development and has no certificate; signing is not a prerequisite. The
  CPU-specific diagnostic also missed D48 in 25 of 26 cases with identical
  result checksums, so changing CPU flags is not the performance fix. The
  slice cannot be presented as complete or merged with either gate unresolved.
- **Sidebar UI details (D44):** the wide/compact/hidden progression,
  reversible drag, hover peek and Ctrl Alt S are approved. Review exact
  widths, transition thresholds and compact-folder controls with the owner
  at the relevant UI slice; do not infer pixel values from the screenshot.
- **Google Workspace device checks:** Workspace sign-in passed in Phase 0,
  but that does not establish the admin configuration or all protected
  resources. Only the owner/admin checks those settings. D21's extension
  warning is a possible deployment constraint, not proof this account has it.
- **Analytics design (D39):** local counters, explicit export and reviewed
  model suggestions are approved. The concrete schema, enable/reset controls,
  retention, export preview and performance budget still need a proposal
  before collector implementation. No automatic upload or model integration
  is authorized, and actual usage stays out of development artifacts.
- **Retention implementation (D40):** the owner chose seven days with page
  text excluded from daily backups. Prove expiry across cache/index and
  restart/replay/restore; do not treat search invisibility as deletion.
- **Historical wording:** `design/README.md` and the spec explain the lack
  of an Extensions button using an obsolete engine limitation. The approved
  v1 scope still excludes extensions. D28's renderer count is a resource
  intention rather than an API guarantee; D29's rejected-library and
  future-Git claims remain dated research until verified for an actual pin.
  This review does not change the recorded decisions or the design source.
- **Shipping:** update delivery settings, signing/installation feasibility
  and browser registration need their Phase 5 owner gates. D37's upstream
  patch offer also requires approval for the actual public submission while
  the repository is local; prepare it for review without publishing it.
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
    Atlas is a local project; D41 removes the repository prerequisite.
    Which account or organisation it is created under is decided when the
    owner is ready to create the remote.
  - **Current local workflow, owner, 2026-10-10:** D41 explicitly approves
    the plans, 40-slice map and local branch/review/owner-merge workflow.
    Readiness does not need another approval. The owner requested the
    documentation-baseline commit, now `d79fc29`, then explicitly approved
    committing P1-01 and merging it into `claude/plan-atlas-v1` after its gate
    and independent review passed. That merge is complete at `ed19d6d`.
    P1-02 also passed its gate and independent review; the owner requested
    its commit and approved its local merge, completed as `892d3bc`.
    Later commits and merges require their own approval; no remote or push
    is authorized.

## Deferred work

Atlas-authored code may carry a deferred-work marker (`TODO`, `FIXME` or
`HACK`) only when it is tagged with a row from this register, written as
`TODO(DW-1): what is left`. The owner allowed this on 2026-10-11 so that
known gaps can stay next to the code they concern, as long as each one is
tracked here and comes back as a fix. `cargo xtask check` refuses an
untagged marker or a tag missing from this table. Once Atlas is on GitHub,
each row gets an issue and the issue number goes in the last column. When
the work is done, delete the marker and move the row's status to Done.

Add a row before writing the marker. Say why the work is deferred and which
slice it belongs to, so whoever comes back to it knows when it is due.

| ID | Deferred work and why | Owning slice | Status | Issue |
| --- | --- | --- | --- | --- |
| DW-1 | Unify the bookmark parser's address check with navigation's `web_address` (`crates/atlas-app/src/navigation.rs`) through one shared function in `atlas-core`; the owner approved `atlas-core` using `url` for this on 2026-10-12. Until then the parser uses a stricter syntactic copy, and the import commit step must re-check every address with navigation's rule and report failures as skipped. P1-22 is not complete until this is done. | P1-22 | Open | |
| DW-2 | Persist the sidebar's settled width state (wide, favicon-only or hidden) across restart. P1-08's drag works, but `atlas-store` has no settings table, so the state lives in `BrowserView` memory and resets to wide on restart; adding a settings table is a schema migration that belongs with P1-29's appearance settings. The marker is where the sidebar settles in `crates/atlas-app/src/ui/sidebar_view.rs`; see `docs/p1-08-sidebar-states.md`. Done at the 2026-10-12 integration merge: a drag release or key step that changes the state saves `sidebar.width_state` through P1-29's settings table, and start-up restores it, keeping wide when the value is missing or unreadable. | P1-29 | Done | |
| DW-3 | Add the Ctrl Alt S shortcut that pins the sidebar visible (D8/D44). P1-08's sidebar edge already steps between wide, favicon-only and hidden with the Left and Right keys when focused, but the global shortcut and its interaction with hover peek belong to P1-17, which owns peek and sidebar pinning. No code marker yet; P1-17 picks it up with the peek popup. | P1-17 | Open | |
| DW-4 | Bound browsing history before D48's 100,000-row scale. P1-20 keeps every visit and loads every remembered address into the tier-1 catalogue at start-up (`Worker::restore` in `crates/atlas-app/src/session.rs`), so both the database and start-up cost grow without limit. Add a retention policy (an owner decision on age or count) and pruning that removes visits, then addresses left without visits, and keeps the catalogue in step, using the same participant as D47 clearing (`Store::apply_history_deletions`). Deferred because P1-20 only had to make history durable and searchable, and pruning shares its machinery with clearing. P1-27 (2026-10-12) built that machinery but left the policy open: the choice of maximum age or row count, and its value, is the owner's (recommended: 90 days). Once chosen, pruning is a range clear through `Store::apply_history_deletions` at start-up and daily, with the catalogue following as `crates/atlas-app/src/session/deletions.rs` does. Done on `claude/p1-27-clear-data` (2026-10-12) after the owner chose 90 days: start-up expires visits older than 90 days before the catalogue loads, and the idle check repeats it at most once a day. Each expiry is a journaled D47 range clear `[0, end)`, so a restored backup loses the expired visits too, and it removes at most 2,000 visits, continuing a larger backlog on later idle checks. The cutoff never passes 90 days before the newest stored visit and moves at most two days past the last applied cutoff per run, so a wrong clock, even with visits recorded under it, cannot expire recent browsing in one step; a start within a day of the last expiry skips it. See `docs/p1-27-clear-data.md`, "History retention". | P1-27 follow-up | Done | |

## Issue map

Approved by the owner on 2026-10-10 (D41). The following table is copied from
`docs/plans/Atlas v1 — Readiness and implementation breakdown.md`; P1 labels
identify local slices, not published GitHub issues. Each phase is a future
milestone, with one branch, independent review and owner-approved merge per
slice. Q1–Q4 are resolved by D39–D41; the planning baseline is preserved in
`d79fc29`. Analytics additions are proposed separately, outside this approved
map, and do not change the foundation order.

The owner also approved the D43–D45 amendment on 2026-10-10: flat saved-page
lifecycle in P1-02, persistence in P1-03, width-aware shell/unified rows,
later folders/import, Favorites and accessible compact/hidden states. Both
copies below and in the readiness report use the same 40 IDs and order.
This changes the approved deliverables, not the one-slice-at-a-time workflow.

The proposed AN-01/AN-02 amendment is described in the readiness report's
"Proposed analytics amendment (D39)" section. It is not part of these 40
approved rows until the owner approves the added slice boundaries.

### Phase 1 milestone: foundations

| ID | Why this slice exists; deliverable and done condition | Main files / decisions | Blocked by |
|---|---|---|---|
| P1-01 | A reproducible local build prevents later work resting on an unusable toolchain. Create the minimal Cargo workspace, dependency-boundary check, passing test and runnable app; document and execute the D38 gate. Audit maintenance, security and licences for the resolved graph before adding dependencies, here and in every later slice. Record the approved local workflow. Do not define the future Plugin trait early. | `Cargo.toml`, `crates/xtask`, `docs/LOCAL-DEV.md`, `CLAUDE.md`, `AGENTS.md`; D16/D38. | Q3/Q4, a preserved planning baseline and the dependency pre-add audit. |
| P1-02 | One model prevents sidebar/palette disagreement and replacing temporary pins later. Implement live tab identity/kinds, a flat saved-page lifecycle with independent saved identity and live association, selection/order, save/unpin, unload/open-or-focus and a 20-entry closure stack for ordinary and saved web instances. Test D42/D43 without a window; folders and plugin registration remain later work. | `crates/atlas-core`; S3, D42/D43. | P1-01. |
| P1-03 | Saved destinations must survive independently of live pages. Open a local unsynced store, migrate it and persist/reload flat saved entries plus live-tab snapshots and their associations through a serialized writer. Prove rollback, unloaded-entry preservation and errors on disposable databases. | `crates/atlas-store`, core persistence boundary, filesystem-only `atlas-platform` directory guard; D13/D43. | P1-02; Q2 for durable/cache separation. |
| P1-04 | A bad backup must not replace a good one. Add idle daily snapshots, seven-copy rotation, clean-shutdown checking and an owner-confirmed recovery path; interruption leaves the last valid copy usable. Define and test the local deletion-journal contract so later history clearing survives restoring older snapshots. | `crates/atlas-store`, filesystem lifecycle in `atlas-platform`; D13/D46/D47. | P1-03; approved SQLite maintenance prerequisite, now verified. |
| P1-05 | The palette needs fast local ranking before UI work. Add the tier 1 catalogue, boosts, counts and match positions with incremental updates and a reproducible 100,000-row benchmark. | `crates/atlas-search`, `crates/xtask`; D9/D48. | P1-04. |
| P1-06 | Continuous repainting defeats the performance goal. Implement the small pinned GPUI idle patch, add focused regression evidence and remeasure eight suspended tabs and equivalent first-page startup in both throwaway candidates using fresh synthetic profiles. Prepare a reviewable upstream patch. | Pinned GPUI patch and measurement report; D37/D12. | P1-05; passing measurements. Public offer requires separate owner approval. |
| P1-07 | Shared tokens and width states keep the shell consistent. Render the five palettes, accent and transparency fallback with wide, favicon-only, hidden and peek sidebar samples. Review D44 drag thresholds with the owner and inspect contrast against the authorized design direction. | `crates/atlas-ui`; S10/D44, `design/README.md`. | P1-06; live-canvas comparison or owner-approved snapshot, with D43–D45 amendments. |

### Phase 2 milestone: browser shell

| ID | Why this slice exists; deliverable and done condition | Main files / decisions | Blocked by |
|---|---|---|---|
| P1-08 | Browsing needs a safe engine boundary that follows sidebar size. Create the shared environment and one page in a GPUI card; wire smooth drag transitions through wide, favicon-only and hidden states, including reverse drag, resize and DPI changes. Verify page guardrails, runtime detection and approved privacy disclosure; retain native permission prompts and disable clipboard auto-grant. | `crates/atlas-platform`, `crates/atlas-app`; D3/D11/D44, Q1. | P1-07; Q1. |
| P1-09 | One sidebar must represent saved rows and live pages without duplicates. Wire flat saved and ordinary rows to D42/D43 creation, save/unpin, unload/open-or-focus, ordering and close/reopen. Render wide/compact states, retain hidden-state selection, and prove repeated lifecycle changes create no duplicate or leaked views. Full folders remain P1-21. | `atlas-core`, `atlas-app`, `atlas-ui`; D42–D44. | P1-08. |
| P1-10 | Navigation must work from a focused page in every sidebar width state. Wire live address/title updates without rewriting saved targets, back/forward/reload, the owned address-entry popup and native shortcut router. Ctrl L/K opens entry; Ctrl T/W/Tab/Shift T follows D42/D43. Test focus through drag/hidden transitions; Phase 4 extends this same popup. | `atlas-core`, `atlas-platform`, `atlas-app`; D8/D9/D42–D44. | P1-09. |
| P1-11 | OAuth popups require an opener relationship. Adopt new-window requests as tabs without previous navigation; synthetic opener/focus tests pass. | `atlas-platform`, `atlas-app`; page rules. | P1-10. |
| P1-12 | Sites need usable permission prompts without automatic grants. Integrate engine permission and notification UI; test decisions and persistence using disposable profiles. | `atlas-platform`; privacy guardrails. | P1-11. |
| P1-13 | Browser essentials must be reachable. Wire find, downloads, print, context menu and zoom controls, including explicit Ctrl J routing and focused-page tests. | `atlas-platform`, command registry; note 02 Musts. | P1-12. |
| P1-14 | The address display must not misstate security. Implement a truthful scheme/status display and native certificate-error behavior; verify invalid certificates never silently succeed. | `atlas-platform`, `atlas-ui`; note 02 Musts. | P1-13; supported native security-state approach. |
| P1-15 | Streaming and long-lived tabs need correct lifecycle handling. Add fullscreen and hidden-tab suspension with late-callback guards; test video layout and resume. | `atlas-platform`, `atlas-app`; D6. | P1-14. |
| P1-16 | Losing one process must not lose the session model. Recover renderer and shared-browser-process failures distinctly; synthetic fault checks prove no duplicate recovery or missing tabs. | `atlas-platform`, `atlas-app`; crash recovery. | P1-15. |
| P1-17 | Dragging the sidebar hidden must preserve access over live pages. Add the owned hover-peek popup and Ctrl Alt S sidebar pinning, integrated with D44 width states, without resizing the page during peek. Test focus, stacking, Alt Tab and window lifecycle; move navigation/address controls to the title bar while hidden. | `atlas-ui`, `atlas-platform`, `atlas-app`; D7/D8/D44, `design/README.md`. | P1-16. |
| P1-18 | The Phase 0 conditions must hold in the product. Record shell idle/startup checks and the owner's remaining site/sign-in tests plus one day of browsing. This is an evidence/report slice; failures return to their owning implementation slice. | Phase 2 acceptance report; D6/D21/D22/D37. | P1-17; owner account tests and day of use. |

### Phase 3 milestone: browser memory and home

| ID | Why this slice exists; deliverable and done condition | Main files / decisions | Blocked by |
|---|---|---|---|
| P1-19 | Session recovery must not load every saved destination. Restore committed live-tab addresses/titles/order and saved associations, preserving saved rows that were unloaded. Retain D42's in-session reopen behavior and prove crash/restart recovery on disposable data. Back/forward lists are not promised. | `atlas-core`, `atlas-store`, `atlas-app`; D13/D42/D43. | P1-18. |
| P1-20 | Navigation must become searchable memory. Persist history events and update tier 1 incrementally, with a usable history list and no duplicate entries from stale events. | `atlas-store`, `atlas-search`, `atlas-ui`; D10. | P1-19. |
| P1-21 | Saved destinations need inline organization without a second bookmark model. Extend the flat D43 foundation with folder and saved-page creation, editing, moves, ordering and removal through core/UI/store/search. Review compact-folder controls, and prove lifecycle, hierarchy and restart consistency. | `atlas-core`, `atlas-store`, `atlas-search`, `atlas-ui`; D43/D44. | P1-20. |
| P1-22 | Imported bookmarks must join the same inline saved-page model without opening every page. Parse bounded HTML exports, preview/confirm and preserve folder hierarchy/order as unloaded saved entries. Test synthetic Chrome/Edge/Firefox-shaped exports, nested folders and malformed input; read no browser profile. | Saved-page import and UI; D14/D15/D43. | P1-21. |
| P1-23 | Full-text selection needs evidence. Run the FTS5/Tantivy bake-off on one synthetic corpus, including update/delete/expiry and recovery costs. Deliver the benchmark and reasoned choice for review before production integration. | Benchmark harness/report; D18. | P1-22; candidate dependency audits. |
| P1-24 | Search needs a tested implementation of the chosen store. Implement the `FullText` adapter and durable recovery against synthetic documents; interrupted writes, deletions and replay preserve the intended results. | `atlas-search`; reviewed P1-23 choice. | P1-23. |
| P1-25 | Capture must be safe before real browsing can enable it. Prepare the concrete page-access method for owner approval before any guarded page API use; then prove eligibility/extraction on synthetic pages, redirects, frames and `no-store`. Deliver evidence and the production recommendation. | Synthetic feasibility harness/report; D18/D35. | P1-24; owner approval of the concrete page-access method before prototype use. |
| P1-26 | Page recall needs bounded, expiring data. Integrate the proven extraction path with the page-text cache/index, disable setting, expiry and restart/restore rules; excluded content never enters storage. | `atlas-platform`, `atlas-search`, cache store; D18/D35, Q2. | P1-25 and review of its evidence; Q2. |
| P1-27 | Clearing browsing data must include Atlas's copies. Add explicit kind/time choices and coordinated WebView2/history/search clearing; partial failures remain visible, and replay/restore does not silently resurrect cleared entries. | `atlas-platform`, `atlas-store`, `atlas-search`, settings UI. | P1-26; retention-aware backup/recovery design. |
| P1-28 | Quick destinations need one truthful action path. Build the canvas home page, editable Quick access tiles and separate square sidebar Favorites, with durable ordering/labels. Favorites can target websites, plugin pages or other supported Atlas destinations. Plugin targets retain their unavailable indication until the normal plugin lifecycle exists; add no early host. | `atlas-app`, `atlas-ui`, `atlas-store`; D45, spec home/sidebar. | P1-27. |
| P1-29 | Appearance and sidebar space should remain as chosen. Persist Customize palette/accent, Reduce transparency and sidebar width/presentation state. Inspect all five palettes and wide/compact/hidden restoration across restart without changing hover-peek behavior. | `atlas-ui`, settings; S10/D44. | P1-28. |

### Phase 4 milestone: command palette

| ID | Why this slice exists; deliverable and done condition | Main files / decisions | Blocked by |
|---|---|---|---|
| P1-30 | Ctrl K needs one command source and predictable focus. Extend P1-10's live owned entry popup into the full command palette with registry-backed rows, keyboard selection, Esc and focus restoration. Do not create a second overlay mechanism. | `atlas-core`, `atlas-ui`, `atlas-platform`; S5/D7. | P1-29. |
| P1-31 | Search and sidebar must act on the same destinations. Add scope counts, highlights and bounded previews across tier 1/tier 2. Shared stable targets open/focus saved rows, live tabs and supported Favorite destinations through the same actions; stale results cannot replace newer queries or duplicate a saved row's open instance. | `atlas-search`, `atlas-core`, palette UI; D43/D45. | P1-30. |
| P1-32 | The palette is also the address bar. Add explicit URL navigation, command prefixes and Google Search the web via Alt Enter; verify typing sends no suggestion requests. | Command parsing and palette actions; D9. | P1-31. |
| P1-33 | Central UI speed must be measured after composition. Verify one-frame opening, complete ranking latency and live-overlay focus/stacking regressions. Record methodology and failures without hiding slow cases. | Palette performance/acceptance report; D6/D7. | P1-32; measured budgets pass. |

### Phase 5 milestone: daily driver

| ID | Why this slice exists; deliverable and done condition | Main files / decisions | Blocked by |
|---|---|---|---|
| P1-34 | External links must reach one running Atlas. Implement single-instance forwarding with validated URL arguments and startup-race tests. | `atlas-platform`, `atlas-app`; external links. | P1-33. |
| P1-35 | Evergreen changes can affect sign-in. Store the last observed runtime version, show D36's notice on change and provide the explicit open-in-Edge action; no sign-in canary. | `atlas-store`, `atlas-app`, `atlas-ui`; D22/D36. | P1-34. |
| P1-36 | Compact or hidden chrome must remain usable without dragging or reading an icon alone. Implement F6 traversal, accessible names/states and keyboard access across wide/compact/hidden/peek modes, inline folders and Favorites. Record an NVDA pass across the chrome. | `atlas-ui`, `atlas-platform`; accessibility Musts, D43–D45. | P1-35; owner authorization if software installation is required. |
| P1-37 | Installation must be reproducible without secretly changing Windows. Prepare and validate the installer and registration path; owner performs/approves installation and default-browser selection. Preserve data on upgrade. | Packaging, installation documentation; S8. | P1-36; packaging/policy feasibility and owner operator actions. |
| P1-38 | Updates must reject tampering. Implement local signed-package verification and failure handling first, using synthetic packages and an independently pinned verification key. | Updater core/tests; signed updates. | P1-37; reviewed format and key-handling design. |
| P1-39 | Delivery needs explicit network and restart behavior. Connect the verifier to the owner-approved host/check/install policy; prove failed download, invalid signature and interrupted install preserve the working version. | Updater delivery/UI. | P1-38; owner approval of host, network calls and install/restart behavior. |
| P1-40 | “Daily driver” needs evidence from use. Record one week as the owner's default browser and the product's D6 memory/startup results; reconcile failures before closing plan 1. | Final acceptance report; D6. | P1-39; owner default-browser action and week of use. |

## Super-Pass ledger

started: 2026-10-09   code repo: local Atlas-app (D41; no remote)   plan branch: claude/plan-atlas-v1
| wave | job | route | model | worker result | branch @ sha | review | merged |
|---|---|---|---|---|---|---|---|
| Phase 0 | GPUI candidate spike | spike (worktree) | opus | partial: built, 50/50 automated checks; idle CPU 3.75% fails 0.5% bar; owner tests pending | spike/gpui @ bfac1c5 | n/a | never (throwaway) |
| Phase 0 | Tauri + Svelte control spike | spike (worktree) | opus | done: 25/25 in both overlay modes; idle CPU 0.54% | spike/tauri @ 2579d86 | n/a | never (throwaway) |
| Phase 1 | P1-01: scaffold and architecture gate | local slice | Codex | Full gate passed: dependency check, fmt, Clippy, 7 tests and console app launch; optimized executable was blocked once, then passed unchanged | codex/p1-01-scaffold @ ed19d6d; base d79fc29 | independent review passed after restoring default Cargo test selection so future doctests run | merged into claude/plan-atlas-v1 as ed19d6d with owner approval on 2026-10-10 |
| Phase 1 | P1-02: generic tab and flat saved-page model | local slice | Codex | Full gate passed: dependency check, fmt, Clippy, 24 model tests, 7 architecture tests and console app launch; D42/D43 lifecycle implemented without UI or storage | codex/p1-02-tab-model @ 892d3bc; base ed19d6d | independent source/test review and documentation/map audit passed with no findings | merged into claude/plan-atlas-v1 as 892d3bc with owner approval on 2026-10-10 |
| Phase 1 | P1-03: durable saved pages and session records | local slice | Codex | Implemented: 77-test local gate passed; guarded Windows location, atomic saved/live snapshots and migrations; narrow licence allowances approved | codex/p1-03-storage @ c4a6631; base 9a6ae35 | independent core/store, native and documentation reviews passed; long-path regression fixed and retested | merged into claude/plan-atlas-v1 as c4a6631 with owner approval on 2026-10-10 |
| Phase 1 | P1-04: Backups and confirmed recovery | local slice, complete | Codex | Approved SQLite patch, lifecycle ownership, idle daily snapshots, seven-copy rotation, confirmed/resumable recovery and deletion contract; full 138-test gate passed | codex/p1-04-backups; 7710c06 | independent SQLite, store, native backup/recovery, deletion-contract and documentation reviews passed | owner-approved commit and fast-forward merge into claude/plan-atlas-v1 |
| Phase 1 | P1-05: Local search catalogue | local slice, in progress | Codex | Approved frizbee padding correction applied and independently reviewed; the latest full desktop gate passes 347 workspace tests, including 153 search and five public matcher regressions. Test-only phase and restore diagnostics identify candidate filtering costs and verify one-build restore correctness. All eight available matcher backend variants pass. The unchanged production benchmark preserves 1,040 exact results but only 9/26 cases meet D48. Earlier restricted-context smoke failure remains recorded. The range index remains test-only. | codex/integrated-browser-preview; based on 33dade0, preserving P1-05 work | normal-desktop full gate passes; retain restricted failed-smoke evidence, then complete performance acceptance and production-loading review; see docs/p1-05-search.md | not complete, committed or merged; D48 acceptance remains open |
gates: 2026-10-09 owner approved starting Phase 0 locally
gates: 2026-10-10 Phase 0 verdict written: go with GPUI on two conditions (docs/research/09-phase-0-verdict.md)
gates: 2026-10-10 owner approved proceeding into plan 1; readiness and the issue breakdown still require separate approval
gates: 2026-10-10 owner requested detailed local plans 2–4 and deferred repository publication; implementation remains in plan order
gates: 2026-10-10 owner approved four plans, the 40-slice map and local branch/review/owner-approved merge workflow (D41); retained WebView2, approved local counters with explicit export and reviewed suggestions (D39), and excluded page text from backups (D40)
gates: 2026-10-10 owner requested the eight-file documentation-baseline commit; preserved as d79fc29 before branching for P1-01
gates: 2026-10-10 owner explicitly requested the P1-01 commit and approved its local merge into claude/plan-atlas-v1; this authorization covers P1-01 only
gates: 2026-10-10 P1-01 merged as ed19d6d; P1-02 started on codex/p1-02-tab-model with owner-approved D42 transition rules
gates: 2026-10-10 owner approved D43–D45 and the revised 40-slice map; P1-02 includes flat saved entries, while folders, sidebar UI and Favorites remain in their approved later slices
gates: 2026-10-10 P1-02 passed the full local gate (31 tests and console launch), independent code review and documentation/map audit
gates: 2026-10-10 owner explicitly requested the P1-02 commit and approved its local merge into claude/plan-atlas-v1; this authorization covers P1-02 and its documentation only
gates: 2026-10-10 P1-02 committed as 892d3bc and fast-forward merged into claude/plan-atlas-v1; completion documentation records the resulting state
gates: 2026-10-10 P1-03 core snapshots passed the full local gate with 37 tests before platform dependencies were introduced; owner approved Unicode-3.0 for unicode-ident 1.0.26 and Zlib for foldhash 0.2.0; these allowances do not select Atlas's own licence
gates: 2026-10-10 final P1-03 cargo xtask check passed all 77 tests, formatting, Clippy, dependency boundaries and console launch; independent reviews passed; no production database or Windows setting was changed
gates: 2026-10-10 owner requested the P1-03 commit and approved its local merge; c4a6631 was fast-forward merged into claude/plan-atlas-v1; completion documentation records the resulting state
gates: 2026-10-10 owner chose confirmation before restoring a damaged database (D46) and a local deletion record that prevents restored backups from resurrecting cleared history (D47)
gates: 2026-10-10 owner approved the pinned SQLite 3.53.4 source patch and vendor-comment exception; source identities and unchanged dependency graph verified; full cargo xtask check passed 78 tests and console launch
gates: 2026-10-10 owner explicitly requested the P1-04 commit and approved its local merge; 7710c06 was fast-forward merged into claude/plan-atlas-v1 after the 138-test gate and independent reviews; upstream vendor whitespace preserved with all 17 source hashes verified
gates: 2026-10-10 owner approved the page-close API and pinned wry vendor exception; its implementation passed independent review, 99 app/platform tests, dependency checks, workspace Clippy, scoped formatting, an actual synthetic native close fixture and the rebuilt standard app's desktop smoke check. The final workspace formatting recheck found unrelated search-profiler edits; see docs/page-close-event-review.md. No commit, merge or upstream publication is authorized by this patch approval.
current: D49 integrated browser preview remains in progress with uncommitted work preserved. The current atlas-app.exe and preserved atlas-app-parallel-preview.exe share SHA-256 3ca0bf045fc0826e7300a647307845733fb5249b085f52feeadbebf0abd4c276. Two owner-requested parallel sessions used subagents for navigation reliability and WebView2 construction ownership. Reviewed navigation/popup fixes, saved-row lifecycle regressions and narrowed ASCII projection are integrated. The owner separately approved the exact wry construction patch; its source and provenance reviews pass. The latest full gate passes dependency checks, formatting, Clippy and 301 tests (including 41 app and 160 search unit tests), then Windows blocks the public matcher-test executable with error 4551 before execution. The full gate remains failed. Separate rebuilt app smoke and patched native lifecycle runs pass. Fault-injection validation is being prepared in disposable copies and has not run. The latest benchmark preserves all 1,040 exact results but passes only 10/26 D48 cases; the range index remains test-only. The owner stopped Computer Use during first inspection of the new synthetic preview, which was left open; no further UI automation followed. Popup typing/navigation and the original WebView2 crash remain unresolved. Plans 2–4 retain their approved 12/16/21 slice boundaries and predecessor exits. No commit, merge, push or security change occurred. See docs/handoff-integrated-preview-2026-10-10.md for current evidence, parallel-session ownership and required checks.

## Iteration log

- **2026-10-10** — With the owner's explicit commit and merge approval,
  committed P1-04 as `7710c06` and fast-forward merged it into
  `claude/plan-atlas-v1`. This also preserves the existing `8cc0e50`
  mobile-sync outline without starting that later work. No product source
  changed after the passing 138-test gate. P1-05 tier 1 search is next;
  its eventual commit and merge need separate owner approval.

- **2026-10-10** — Completed P1-04 implementation and the full 138-test local
  gate. Exclusive lifecycle ownership, idle daily validated snapshots,
  seven-copy rotation and confirmed recovery now work on disposable Windows
  stores. Known interruptions resume only after renewed confirmation for
  the same source. The deletion-log codec defines the later history-clear
  boundary; persistence and actual replay participants remain P1-27 work.
  Independent source reviews passed. No P1-04 commit or merge is authorized.
  The owner reconfirmed strict slice order despite limited remaining usage;
  the console scaffold is not yet a browser UI.

- **2026-10-10** — Applied the owner-approved SQLite maintenance patch for
  P1-04. Seventeen vendored files retain thirteen original archive inputs
  and four exact upstream replacements. Rust versions and features remain
  unchanged; only libsqlite3-sys's source changes in the lockfile. The full
  gate passed 78 tests, including a linked-engine identity check. Recorded
  the source provenance and recovery design. No P1-04 commit or merge is
  authorized, and the backup/recovery implementation remains next.

- **2026-10-10** — Began P1-04 preflight after the P1-03 merge. The owner
  chose confirmation before database restoration (D46) and a local deletion
  record that restored backups must honor (D47). Recorded these choices in
  both issue-map copies. Filesystem replacement, journal replay and backup
  publication remain engineering proposals; corruption processing awaits
  the SQLite source patch review. No P1-04 code or production data was changed.

- **2026-10-10** — With the owner's explicit approval, committed P1-03 as
  `c4a6631` and fast-forward merged it into `claude/plan-atlas-v1`. Recorded
  the result in the plan, local-development notes and next-session handoff.
  No product source changed after the passing 77-test gate. P1-04 backups and
  recovery is next; no remote or push was added.

- **2026-10-10** — Completed P1-03 implementation and verification on
  `codex/p1-03-storage`. The store atomically preserves saved/live records,
  selection, full-width identities and revisions; failures roll back and
  stale writers cannot replace newer state. The native lease refuses unsafe
  locations while preserving SQLite cleanup. Independent review removed
  repeated Windows discovery from ordinary reads/saves and moved existing
  record validation before persistent configuration. Runtime testing found
  and corrected MSVC SQLite long-path handling, with three boundary cases
  retained in the final 77-test passing gate. The pinned SQLite long-path
  performance caveat remains documented in `docs/dependency-audit.md`.
  This verification preceded the owner-approved commit and merge recorded
  above. P1-04 has not started.

- **2026-10-10** — Started P1-03 on `codex/p1-03-storage`. Added the core
  snapshot/restore boundary with structural validation and six new tests;
  the full 37-test gate passed before introducing platform dependencies.
  Audited the downloaded SQLite/Windows graph separately from product
  compilation. The owner approved Unicode-3.0 for `unicode-ident 1.0.26`
  and Zlib for the statement cache's `foldhash 0.2.0`.
  Windows path validation is a filesystem-only prerequisite within P1-03,
  documented in `docs/storage-path-preflight.md`, not early browser work.
- **2026-10-10** — With the owner's explicit commit and merge approval,
  committed P1-02 as `892d3bc` and fast-forward merged it into
  `claude/plan-atlas-v1`. Updated the development record and next-session
  handoff to start P1-03 storage. No product source changed after the passing
  gate and review; the repository remains local.
- **2026-10-10** — Implemented D42/D43 in the dependency-free
  `crates/atlas-core`: distinct saved/live identities, one sidebar projection,
  fixed saved destinations, unload/open-or-focus, unpin and bounded history.
  The full local gate passed 24 model tests and seven architecture tests,
  with formatting, Clippy and the console launch. Independent code and
  documentation reviews found no issues; both maps contain the same 40 rows.
  P1-02 remains uncommitted pending the owner's commit and merge approval.
- **2026-10-10** — The owner approved Pin=Save with persistent inline rows
  and folders (D43), reversible sidebar width dragging (D44), and separate
  Favorites for supported Atlas destinations (D45). Updated both copies of
  the approved map without adding or reordering its 40 slices. Clarified
  that D42's 20-entry history includes saved-page web closures and resolves
  them through surviving saved rows without recreating removed records.
  P1-02 remains in progress; no later slice is implemented by this amendment.
- **2026-10-10** — Recorded D42 before implementing tab transitions: visible
  right-then-left selection, Home after the last close, pinned-first groups
  with normal closure, new web tabs appended and activated, and 20 recent
  web-tab closures with metadata-only reopen. Documented the fresh-identity
  and position-clamping safeguards separately from the owner's choices.
  Verified P1-01's merge at `ed19d6d` and started P1-02 on
  `codex/p1-02-tab-model`; its gate and review are pending.
- **2026-10-10** — Preserved the approved documentation in `d79fc29` at the
  owner's request, then implemented P1-01 on `codex/p1-01-scaffold`. Added
  the minimal workspace, dependency-boundary check and seven synthetic tests,
  audited the development-only JSON dependency graph and documented D38's
  optimized gate in `docs/LOCAL-DEV.md`. The full gate and console app launch
  passed. Windows blocked the optimized check tool once, then accepted the
  unchanged executable; recorded that observation without claiming its
  cause or changing security settings. Independent review caught a gate
  command that would skip future documentation tests; restored Cargo's
  default test selection, reran the full gate and obtained a clean review.
  The owner then explicitly approved committing this slice and merging it
  locally into `claude/plan-atlas-v1`; that integration is `ed19d6d`.
- **2026-10-10** — Recorded the owner's readiness and local-workflow approval
  (D41), seven-day retention including backup exclusion (D40), and revised
  analytics direction (D39). The owner chose local counters and explicit
  report export, with model suggestions reviewed by the owner. Marked all
  plans active and populated this Issue map from the approved 40 slices.
  Collector details and added slices remain a separate design amendment;
  no collection, upload, commit or implementation started from this approval.
- **2026-10-10** — Readiness review corrected durability guarantees,
  WebView2 process and popup behavior, extension capability, build-policy
  assumptions and missing browser Musts. Recorded exact spike pins and
  implementation evidence gates. Microsoft runtime diagnostics and the
  interaction of backups with retention were put to the owner. A separate
  local report holds the proposed review-sized breakdown; the Issue map
  remains unapproved, and no product code or remote was created.
- **2026-10-10** — Completed detailed drafts of plans 2–4 after four owner
  question rounds. The sibling plans record their accepted demonstration,
  vault, save, attachment, project, shortcut, Git and language-tool scope.
  D31's Python assumption is resolved by the owner's entry-points-and-terminal
  answer. Independent cross-plan review checked the host boundary, shared
  editor ownership and search checkpoint/restart behavior. Technical checks
  and measured acceptance remain prerequisites, not claimed results.
- **2026-10-10** — The owner deferred repository publication and asked to
  finish the outlines for Plugins and Fledge, Notes and Forge locally,
  asking questions where needed. This is an explicit planning exception to
  the immediate repository-creation step recorded earlier. Implementation
  order, readiness approval and publication permissions remain separate.
- **2026-10-10** — The owner approved proceeding into plan 1 after the
  handoff review. Repository creation remains the owner's next step (D16);
  the local repository still has no remote. This approves continuing the
  handoff sequence, not marking this draft ready or publishing issues. The
  readiness check and the issue breakdown each return to the owner for
  approval before implementation begins.
- **2026-10-10** — Phase 0 verdict (`docs/research/09-phase-0-verdict.md`):
  go with GPUI on two conditions. The owner tested Google sign-in by passkey
  and with the Workspace account, YouTube and Disney+, all passing; Hulu,
  the password path, staying signed in and the other sites move to Phase 2's
  exit criteria. The owner allowed a patched GPUI for the idle loop (D37)
  and release-style dev builds (D38).
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
