# Next session: plan and build Atlas v1 with /suprpas

This file is the hand-off from the 2026-10-09 design and research session. The
first part is for the owner: how to start. The second part, from **Brief**
down, is the braindump that `/suprpas` reads.

## How to start (for the owner)

1. **Put the work on `main`.** Today's spec, design and research are on the
   branch `design/spec-and-palettes`. Workers branch from the default branch,
   so fast-forward it first:

   ```bash
   git switch main && git merge --ff-only design/spec-and-palettes
   ```

2. **Turn on the context guard.** `/suprpas` asks for this. In PowerShell,
   before starting Claude Code in this folder:

   ```bash
   $env:CLAUDE_AUTOCOMPACT_PCT_OVERRIDE = '50'
   ```

3. **Start a fresh session in `C:\Users\<user>\Documents\GitHub\Atlas-app`**
   and paste:

   ```
   /suprpas Read docs/next-session-prompt.md from the "Brief" heading down and treat it as the braindump. Plan Atlas v1 with me, and question me before you decide anything I haven't.
   ```

4. **What it will ask of you:**
   - rounds of questions while it plans (expect many; that's intended);
   - four approvals: the plan is ready, the breakdown and its waves, a design
     direction for each design issue, and each wave's merges;
   - **a GitHub repository before any issue is published.** `/suprpas` tracks
     work as GitHub issues and pull requests. Planning runs entirely locally;
     it stops before publishing and asks you to create the repository. `gh` is
     signed in here as `ZanderOfficial`; git commits as `BuyItLandon`. Which
     account owns the repository is your choice.

---

## Brief

### What Atlas is

Atlas (working name) is a browser-first workspace app: a desktop web browser
with a vertical-tab sidebar, where code (Forge), notes (Notes) and the
owner's business software (Fledge) run as built-in plugins that open as tabs,
with one command palette and one shared search across all of it. The full
spec is `docs/spec.md`. Read these, in this order, before anything else:

1. `docs/decisions.md`: the owner's decisions since the spec. **Where it and
   the spec disagree, it wins.**
2. `docs/spec.md`: the product. Its "Decisions already made" still stand.
3. `design/README.md`: every design decision, including the five base
   palettes, the sidebar peek, plugin tabs and Forge. The visual target is the
   design canvas at https://claude.ai/artifact/Cog1vuMS5rfYFmHLYPKj9f (private
   to the owner); `design/canvas/project/` is a snapshot of its source. The
   palette tokens are the `tokens()` method in
   `design/canvas/project/Main.dc.html`.
4. `docs/research/` (grep these; don't read them whole after the first
   pass). Every claim has a source and a confidence:
   - `01-shell-and-engine.md`: Tauri 2 multi-webview per OS, overlays,
     browser plumbing, CEF and Servo fallbacks, a Phase 0 checklist.
   - `02-browser-v1-scope.md`: what a daily-driver browser needs at v1
     (Must / Should / Later).
   - `02a-tauri-wry-facts.md`: Tauri 2.12.2 and wry 0.57.0 capabilities
     and blockers.
   - `02b-default-browser-import-signing.md`: default-browser registration,
     import formats, signing. Signing and the Store are deferred.
   - `03-core-architecture.md`: the plugin trait, the Rust-to-UI boundary,
     search (frizbee in memory, tantivy for full text), storage, testing,
     workspace layout.
   - `04-forge-and-notes.md`: CodeMirror 6, Tree-sitter, LSP, git, terminal,
     run control, Notes' editor and database view.
   - `05-fledge-api.md`: what Fledge's API offers Atlas today (nothing stable
     yet), the auth route, a client trait sketch, and Fledge-side work.
   - `06-engine-and-ui-performance.md`: engines and UI toolkits compared on
     performance and site compatibility, with a recommendation and a spike
     checklist with go/no-go thresholds.

There is **no code yet**. The repository is local (no `origin`).

### How to work with me

- **Question me.** Planning is a conversation. Use `/planissues`'
  question rounds (`grilling`) throughout phase P, and put every undecided
  item to me rather than picking a default. The list under "Questions for the
  owner" below is the minimum. Ask in rounds of a few at a time, most
  consequential first.
- **Recommend, and explain.** For each choice: what the concept is, in a
  sentence or two (I'm new to web view embedding and plugin architectures);
  what you recommend and why; and the main alternative. Then let me decide.
- **Performance decides.** I want Atlas as efficient as possible: low
  memory per tab, fast start, low typing and scrolling latency, low idle CPU.
  TypeScript and Svelte are allowed; all state and logic stay in Rust.
- **Use the research, don't redo it.** `/planissues` step 3 still runs its
  researchers. Point them at the gaps and the conflicts listed below, and have
  them re-check anything a decision hinges on. Treat the notes as dated
  2026-10-09.

### Decisions you may not reopen

From the spec:
- one workspace, no profiles or work/dev modes;
- the browser engine is embedded, never built from scratch;
- Rust owns all state and logic, and the UI layer is thin;
- no graph view;
- the command palette is central.

From `docs/decisions.md`:
- performance is the deciding goal;
- import is a CSV file only;
- signing, SmartScreen and the Microsoft Store wait until productisation;
- Atlas will probably be open source with releases on GitHub.

From `design/README.md`: every decision listed there. Changing one needs my
yes.

Known limits to keep stating, not hiding: no Chrome extensions, and no
password manager or ad blocking in v1.

### The first decision: engine and UI

This decides most of what follows, so settle it with me first, then confirm it
with a spike.

| Option | What it is | What the research says |
|---|---|---|
| A | GPUI (Zed's Rust UI toolkit) for the chrome, plus the system web view (WebView2 on Windows) driven from Rust | Recommended by `06`: fastest start, no extra renderer process, native editor speed. Risks: GPUI's crates.io release is a year old, and its accessibility support is months old. |
| B | Tauri 2 with a Svelte 5 chrome, on the same web views | The spec's plan, and `06`'s fallback. `01`: workable on Windows and macOS with platform code; not on Linux on stock Tauri. Maintainers have said they have no capacity for multi-webview bugs. |
| C | Chromium through CEF | A contingency for Linux or for a site WebView2 can't load. Hundreds of MB, a Chromium update every 4–8 weeks, unsandboxed on Windows. |
| D | Servo | Not ready for daily use: Google sign-in, YouTube, Drive and GitHub fail. Revisit later. |

Conflicts between the notes, to resolve with me and then test:
- **Overlays.** Nothing can paint over a native web view.
  - `06` expects the palette and the sidebar peek to be popup windows,
    possibly with real Acrylic blur over the page.
  - `01` recommends a frozen screenshot of the page under the palette, and a
    small opaque web view raised for the peek. It warns that WebView2 runtime
    154 reportedly paints transparent web views opaque.

  Test both in the spike.
- **Editors.** `04` recommends CodeMirror 6 in a web view for both Forge and
  Notes. `06` says a web-view editor costs speed and leans native. In
  option A, that means Forge and Notes either run as local web-view tabs
  inside a GPUI window, or get native editors (much more work, per `04`).

Then **Phase 0 is a gate**. The spike follows the checklists in `01` and
`06`:
- Google sign-in first;
- then the 15 sites listed in `06`, in order, which come mostly from my Quick
  access tiles;
- memory, start-up and latency measured against Edge and against a control
  build of the other option;
- both overlay approaches;
- shortcuts while a page has focus.

It ends with a written per-OS verdict and a go/no-go against thresholds I
confirm (`06` proposes memory within 10% of Edge and 13 of 15 sites
passing). After the spike, stop and ask me. If the result changes the plan,
replan; don't build on a plan you know is wrong.

### What v1 means (propose this; confirm with me)

v1 is Atlas as my daily browser on Windows, with the spec's sections 1–4 in
place:
- the window, sidebar and page card;
- the home page;
- the command palette, including inline plugin actions;
- the three built-in plugins.

The Must items from `02-browser-v1-scope.md` are in it too. macOS and Linux are
questions below.

That is too big for one Super-Pass. **Split it into four plans** (planissues
step 4), outline all four in this session, and build the first:

1. **Engine, browser core and palette** (spec phases 0–2):
   - the scaffold, the spike, the tab model, the window and sidebar;
   - the home page, history, bookmarks and session restore;
   - the palette with tab, bookmark and history search;
   - the five palettes as design tokens;
   - CSV import.
2. **Plugin runtime and Fledge** (phase 3). The Fledge client starts as a
   stub behind a trait, as the spec says. `05` explains why: no stable API
   exists, and a real client waits for Fledge's Stage C.
3. **Notes** (phase 4).
4. **Forge** (phase 5).

Foundations for plan 1, the W0 wave:
- **the scaffold:** workspace layout per `03`, check commands, a first
  passing test, `DOCS/LOCAL-DEV.md`;
- **the Phase 0 spike and its verdict;**
- **the core tab model crate**, with tests;
- **storage:** rusqlite and migrations;
- **the search crate:** the in-memory tier first;
- **the design tokens.** Their design direction is already chosen: offer the
  canvas as the direction at the gate.

### Work the research says the plan must include

These are easy to miss. Each should land in a plan section or an issue:
- **Shortcuts are caught natively** and routed to Rust. While a page has
  focus, WebView2 never passes Ctrl T, Ctrl L, Ctrl Tab or Ctrl Shift T to
  the UI.
- **A permission handler from day one.** Without one, wry on macOS grants
  camera and microphone to every site.
- **Plumbing the engine doesn't provide:**
  - find in page (on macOS);
  - favicons and context menus;
  - fullscreen video;
  - back and forward;
  - crash recovery;
  - session restore. A restored tab gets its address back but not its
    back/forward history.
- **Memory levers:** one shared web view environment, suspended background
  tabs, and tab web views built without Tauri's injected IPC scripts.
- **Privacy:** no telemetry, local crash logs only. WebView2 uploads crash
  dumps to Microsoft unless Atlas turns that off.
- **Search:**
  - the palette ranks an in-memory catalogue with frizbee on every keystroke;
  - full text uses tantivy, after a one-day bake-off against SQLite FTS5 on
    my own notes.
- **Fledge:** the "New lead" form doesn't map onto Nest yet (no Company
  field; `capture_lead` takes no estimated value). The Fledge-side work in
  `05` (scopes, a lead-capture route, real token checks in Connect and Desk,
  a change feed) is a list for me. It is not issues in Fledge's repositories.

### Questions for the owner (the minimum)

Platforms and engine:
- **Engine and UI:** option A, B or something else? Do I accept GPUI's
  churn and young accessibility for a faster native chrome?
- **Platforms:** is v1 Windows only? Is Linux needed in v1 (if so, CEF is the
  plan there)? Is macOS a v1 platform, and does a Mac exist to test on?
- **Google:** do I sign into Google in my daily browser? If Google blocks the
  embedded web view, which do I prefer: sign in through another browser,
  accept the gap, or present a different user agent (against Google's
  policy)?
- **DRM:** do I use Widevine-only services (Spotify's web player, Netflix)?
  WebView2 can't play them.
- **The spike:** are the go/no-go thresholds right? Is a frozen page under the
  palette acceptable (a video's picture stops while its sound continues)?
- **Forks:** is a small patched fork of wry or tao acceptable?
- **macOS translucency:** should Haze and Glass be on by default on macOS if
  the spike confirms their GPU cost?

Browser behaviour:
- **Pinning shortcut:** Ctrl S pins the sidebar in the design, but Forge needs
  Ctrl S for save. Move pinning to Ctrl Alt S, as Zen does?
- **Default search:** which engine does "Search the web" use? Should the
  palette fetch suggestions as I type (which sends keystrokes to it)?
- **Saved logins:** keep WebView2's own password saving off, or turn it on as
  a stopgap?
- **The Passwords button:** drop it from v1 (the design recommends this)?
- **CSV import:** what does the CSV hold: bookmarks, history or both? From
  which tool's export?

Data and search:
- **Third-party plugins:** will plugins ever come from other people? If not,
  the WASM host may never be needed.
- **Page text:** should Atlas index the text of pages I visit?
- **Fledge data:** copy it into the local index, or search it live?
- **Synced folders:** will the data folder ever sit in OneDrive? WAL breaks
  there.
- **Search stores:** two stores (SQLite plus tantivy), or FTS5's single file if
  the bake-off is close?
- **Snapshots:** how many days of database snapshots should Atlas keep?

Forge:
- **Git:** may Forge use the `git` command line for writes, with libgit2 or
  gix only for reads? This departs from the spec, because libgit2 can't read
  Git's new reftable format and won't run hooks. May it require Git for
  Windows?
- **Language servers:** only those already installed in v1? Automatic
  downloads later, or never?
- **Debugger:** confirm it is out of v1.
- **Run control:** keep it (an addition to the spec)? Which Python task
  runners do I use?

Notes:
- **Obsidian:** should a vault stay usable in Obsidian?
- **The editor:** native, or a local web-view tab (this follows from the
  engine and UI decision)?
- **IME:** which scripts must it support?

Fledge:
- **The client:** is Atlas an ordinary third-party OAuth client (recommended)?
- **Data channel:** a REST surface or MCP?
- **Rules:** Atlas drafts mail but never sends it; is that right?
- **Leads:** where does a lead's Company live?
- **Timing:** when is Stage C?

The project:
- **Licence:** which one, before the repository goes public? It decides
  whether MPL-2.0 code such as Helix's queries is acceptable. GPL-3.0 code from
  Zed stays out either way.
- **GitHub:** which account or organisation owns the repository, and should it
  start private?

### Repository, tracker and conventions for this Super-Pass

- **Plans live in this repository**, in `docs/plans/`, following
  `/planissues`' rule for projects outside Fledge. The Super-Pass ledger lives
  in the plan file there. Atlas is not a Fledge product, so don't attach
  Fledge-Dev-Docs for the plan.
- **No GitHub yet.** `CLAUDE.md` says this repository is local only, with no
  pushes, and commits only when I ask. Phase P respects that. At the end of
  the breakdown gate, before publishing anything, stop and ask me to create
  the GitHub repository. Give me the exact `gh repo create` command for my
  answer to the account and visibility questions. Once `origin` exists, the
  scaffold's PR updates `CLAUDE.md`'s working agreements to match, and from
  then on the Super-Pass rules govern branches, PRs and merges.
- **The Fledge repositories are read-only** for this whole Super-Pass. Never
  call Fledge's Supabase or any production URL.
- **The skills** are installed in `~/.claude/skills`, `suprpas` included.
  Don't install them into this repository's working tree, and never commit
  `.claude/`.
- **The machine** (checked 2026-10-09):
  - Windows 11 build 26300;
  - Rust 1.97.1, Node 24.15 and pnpm 10.34;
  - a Cargo build with a build script built and ran without Smart App Control
    blocking it;
  - if a build or the app is blocked later, don't change the policy; ask me.
- **Writing voice:** issues, commits and PRs explain why before what, name the
  files, and are readable by someone with no context.
