# Syrinx: rules for agents

These are the rules for any agent working in this repository. `CLAUDE.md`
loads this file and `docs/dev/trapstoknow.md`; other agents should read both
before starting. The rules say how to work and how to write; the traps say
what has already been found to go wrong.

## Writing readable code

A person reads, reviews and changes everything you write. Write for them.

**Comments say what, briefly. Long explanations go elsewhere.**
- At the top of a file: one or two lines (a `//!` module comment in Rust)
  saying why the file exists. It is not a manual. Usage, settings and edge
  cases go next to the code they concern, or in `docs/`.
- Above each block of code: a short comment saying what the block does
  ("Refuse a database path inside a synced folder"). Do not narrate how the
  language works or restate the next line.
- A reason belongs in a comment only where the code cannot show it, such as
  a WebView2 quirk, a rule that keeps Google sign-in working, or a trap. Keep
  it to one or two lines, at that spot. Longer reasoning goes in the commit
  message, the issue or `docs/dev/trapstoknow.md`.

**Structure keeps the main path obvious.**
- Keep nesting shallow. Return early on guard conditions (invert the `if`,
  use `?` and `let ... else`), so the main path runs at the left margin. Code
  more than two levels deep is a function waiting to be extracted.
- Extract named functions for complex conditions and for each step of a long
  function, so the top-level function reads as a summary of what happens.
- Merge related checks into one place, for example every setting read at
  start-up in one `load_settings()`.
- Never duplicate logic. Two copies of a rule drift apart; write one
  function and call it from both places.
- Name things for what they hold or do. No one-letter names outside a tiny
  closure: `tab`, `webview` and `bookmark`, not `t`, `w` and `b`.

The scaffold now provides real examples in `crates/xtask/src/main.rs`:
`check_workspace` names each step, and `run_cargo` reports spawn and process
failures instead of swallowing them. Follow the same shape in product code.

## Layout

Syrinx is a Rust Cargo workspace on Windows 11. P1-01 provides `syrinx-app`
and `xtask`; P1-02 adds the window-independent `syrinx-core` model. Later
crates are added in their approved slice.
The layout proposed in research note 03 (wiki: `Background-03-core-architecture`,
"Crate layout") is the starting point, adjusted for the engine and interface
decisions made since (D3 and D4 in `docs/architecture-decisions.md`) and for
WASM plugins (owner, 2026-10-15 and 2026-10-21; research note 10), and the
scaffold settles it:

- Plugins are WebAssembly (WASM) components: Rust compiled to a sandboxed
  module that Syrinx loads at run time. Their contract is one versioned WIT
  package, `syrinx:plugin`, whose world is `plugin`, kept in this
  repository at `wit/v0_1/` (owner, 2026-10-24).
- `crates/syrinx-plugin-host` (owner, 2026-10-24) is the only crate that
  links wasmtime; it loads, links and runs plugins, enforces the owner's
  grants, and publishes the test host that plugin repositories run their
  contract tests against.
- `crates/syrinx-core` holds the tab model, session, palette and command
  registry, and stays free of wasmtime; `crates/syrinx-store` holds rusqlite
  and its migrations; `crates/syrinx-search` holds the in-memory catalogue
  (frizbee) and the full-text index (chosen by plan 1's FTS5/Tantivy
  comparison).
- The built-in plugins, Notes and later Forge, are not workspace crates.
  Each lives in its own repository made from the plugin template (#30) and
  ships as a signed package bundled in Syrinx's release. The Fledge plugin
  and its client are on hold (2026-10-20).
- `syrinx-app` and `syrinx-ui` draw the interface with the approved GPUI base;
  `syrinx-platform` owns WebView2 and Win32, with plugin views a separate
  type from page web views. Other product crates remain free of
  toolkit/web-view dependencies so the core stays testable without a window.
- Plugin tabs draw in HTML inside Syrinx's own plugin views; palette forms
  and Home widgets are declarative and drawn by GPUI (2026-10-21). Notes and
  Forge edit in one host-provided CodeMirror 6 component, which Syrinx ships
  in such a view (D28).
- `docs/` holds the spec, the owner's decisions, the v1 architecture
  decisions and the documents for the app and its operations. `docs/dev/`
  holds contributor material agents and tooling read: `LOCAL-DEV.md` (how to
  run the app and the gate on this machine), `trapstoknow.md`, the dependency
  audits, the deferred-work register and `vendor-reviews/`. The research
  notes are the wiki's Background pages; removed plans and records are linked
  from the wiki's History page.
- `design/` holds the design snapshot and the original mockup.

Phase 0's spike is throwaway code (D16). Keep it apart from the product
crates, and carry nothing from it into them without rewriting it to these
rules.

**The gate** is defined in `docs/dev/LOCAL-DEV.md`:

```
cargo xtask check
```

It runs the dependency check, formatting, Clippy with warnings denied, tests
and the application smoke run using D38's optimized `local` profile. Run it
on this machine before calling a change done.

## Read this first: what is the source of truth

The owner approved the four v1 plans on 2026-10-10; their work is now
tracked in GitHub issues. Read these in this order of authority:

1. **`docs/decisions.md`**, the owner's decisions since the spec. Where it
   and the spec disagree, it wins.
2. **`docs/spec.md`**, the owner's spec. Its "Decisions already made" are
   settled. Do not reopen them.
3. **`design/README.md`** and the live design canvas it links, for anything
   the owner sees. When the canvas and the snapshot in `design/canvas/`
   disagree, the canvas wins.
4. **`docs/architecture-decisions.md`**, the v1 decisions (Plan 1's
   "Decisions taken"). It argues the standing and later decisions with what
   was rejected and why, and they are settled. Where it disagrees with the
   three above, they win and the disagreement goes to a GitHub issue. Plans
   2–4 are also approved, with implementation queued in order through their
   epics (#25 plugin runtime, #35 Notes, #42 Forge; #32 Fledge is on hold).

- **Settled decisions stay settled.** If code or a task seems to need a
  different decision, stop and ask; do not quietly build around one.
- **Research notes are read for reasoning, never for current fact.** The
  notes, now the wiki's Background pages, are dated and track fast-moving
  projects (GPUI snapshots, wry, the WebView2 Evergreen runtime). Notes 07
  and 08 correct parts of the earlier notes, and note 03 assumes Tauri, which
  D3 and D4 replaced. Where a note disagrees with the decisions, they win.
- **The product is Syrinx; Atlas was only a working name.** The owner
  named the product Syrinx on 2026-10-12, and on 2026-10-17 asked that the
  docs and the wiki say Syrinx throughout, older records included. Write
  Syrinx everywhere. "Atlas" stays only where it is a real identifier: the
  old branch `claude/plan-atlas-v1`, the old install's folders and registry
  keys (`%LOCALAPPDATA%\Atlas\data`, `AtlasBrowser`), raw evidence logs,
  and vendored code, where GPUI's texture atlas is unrelated. Files retired
  on 2026-10-16, such as the plans, are read at
  [af3de53](https://github.com/Fledge-Software/Syrinx-app/tree/af3de53/docs/plans)
  and keep the old name there.
- **Anything marked ⚠️ or "unverified" is checked before anything is built on
  it.** Plan 1's "Still unproven" section
  ([at af3de53](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/plans/Syrinx%20v1%20%E2%80%94%20Plan.md))
  lists them; an open one belongs in a GitHub issue.

## Operator work

Some work needs the owner, not an agent:

- every Google sign-in test (D21 lists the paths: password with a phone
  prompt or code, a passkey, a Google Workspace account) and every Disney+
  or Hulu playback test, because they need the owner's own accounts. An agent
  never enters, stores or asks for the owner's credentials;
- checking whether the owner's Google Workspace admin console turns on
  device checks (an open thread, tracked in GitHub issues);
- anything on Fledge's side. The Fledge-side gaps are a list for the owner,
  not issues or pull requests in Fledge's repositories;
- installing software system-wide on the owner's machine, and registering
  Syrinx as a browser with Windows.

If the next task needs one of these, point at it and ask; do not work
around it.

Two items once on this list are done: the owner created the repository at
`Fledge-Software/Syrinx-app` (2026-10-13, D16) and made it public under
MPL-2.0, the licence chosen on 2026-10-13.

## Work order: follow the phases

**The owner wants development to follow the phases in order**, one at a
time:

- **Phase 0, the feasibility spike, came first and is done.** It ran
  locally on Windows 11 to prove what D6 lists: Google sign-in, YouTube,
  Disney+ and Hulu, the daily sites, live overlays with correct focus (D7),
  shortcuts while a page has focus, memory and start-up against Edge, and
  the GPUI against Tauri + Svelte comparison with the rule written in D4.
  Its verdict, go with GPUI on two conditions, is the wiki's Background 09
  (2026-10-10), and the owner gave the go-ahead by approving the four v1
  plans the same day.
- **Then the four v1 plans (D5)**, which follow the spec's phases 1 to 5:
  plan 1, the engine, browser core and command palette (spec phases 1 and 2);
  plan 2, the plugin runtime (spec phase 3, kept thin by D34); plan 3, Notes
  (spec phase 4); plan 4, Forge (spec phase 5).
- **Plugins go runtime, template, Notes, Forge (2026-10-20).** The runtime
  (#25) comes first, then the template (#30), then Notes (#35); Forge (#42)
  starts only once Notes is live as the owner's daily notes tool. The Fledge
  plugin waits for Fledge's API and the owner. The research note (#26) and
  the template may run alongside Plan 1; the plugin host (#28, #29) waits
  for Plan 1's exit. The owner accepts Plan 2 at #25's exit.
- **The local workflow is approved (D41).** GitHub is not required before
  implementation. Each approved slice is built on one branch in a local
  worktree and reviewed independently. Since 2026-10-12, a slice merges to
  `main` once the review approves it and the gate passes; see
  `docs/decisions.md`. Commits still need the owner's
  request. The owner-requested planning baseline is committed as `2c80273`.
  Since 2026-10-13 the repository is on GitHub at `Fledge-Software/Syrinx-app`:
  push only the branch your session was assigned, never `main` unless the
  owner asks (`docs/decisions.md`).

- **Always take the next piece of work from the earliest phase or plan that
  still has open work.** Within one, go in the plan's order unless its notes
  name a dependency that changes it.
- **Do not start later work while earlier work is open.** Out-of-order work
  is how foundations get skipped: a palette built before the tab model it
  reads, or a plugin built before the trait it implements.
- **Any exception to strict order is named in the plan or its epic.** For example, the
  real Fledge client is not part of plan 2; it waits for Fledge's Stage C
  (D24). Follow the plan's exception; do not invent one.
- **If the next task is blocked** (waiting on the owner, Fledge or a
  measurement), say so and **ask the owner before moving ahead.** Do not
  quietly skip forward.

## Guardrails: product-wide and load-bearing

These come from the spec, the owner's decisions and the research. Breaking
one breaks Google, the owner's privacy or the open-source plan; it is not a
style problem.

**Where state lives:**

- **Rust owns all state and logic; the interface is thin** (spec, S3). The
  one stated exception is CodeMirror, which owns the live buffer, selection
  and undo while typing; Rust stays the authority for what is on disk and
  saves only when both copies' checksums match (D28).
- **One generic tab model.** The sidebar and the palette read from it, never
  from per-feature code (spec, "Architecture direction").

**Page web views and Google** (research note 07, wiki:
`Background-07-google-sign-in-and-drm`):

- **Never set a custom user agent on a page web view**: not wry's
  `with_user_agent`, not the `--user-agent` browser argument, not a DevTools
  override. The default WebView2 presents itself to Google as Chrome; a
  custom one shows Google the WebView2 brand. A custom identity is tried only
  if Phase 0 shows the default fails, and then only with the owner (D22).
- **Never attach an IPC handler to a page web view.** It adds `window.ipc`,
  which was reported to break Google Docs and Sheets, and it hands untrusted
  pages a channel into Syrinx. Only Syrinx's own local web views (the editor
  and plugin views) talk to Rust.
- **The message handler and custom-protocol filter belong to the plugin-view
  type only.** They are never attached to a page web view, and no view of
  either kind gets an initialization script; the plugin-view slice under
  #25 adds a gate test that refuses all three in `browser/page.rs`. Plugin
  views use the same environment as pages but a separate InPrivate profile,
  a per-plugin `.invalid` origin served by Rust, a Content Security Policy
  the host sets, and no network of their own.
- **Never inject scripts, add request filters, rewrite headers or open a
  remote debugging port on Google's sites.** Google blocks sign-in for exactly
  that kind of middleman.
- **Page-text indexing honours its exclusions (D35).** Never index Google's
  sites, banking and payment sites, pages sent with `Cache-Control: no-store`
  or a site the owner excluded, and the extraction method must inject nothing
  on an excluded site.
- **Pass Syrinx's own browser arguments.** wry's defaults switch SmartScreen
  off (note 08, wiki: `Background-08-gpui-and-wry-on-windows`, 5.1); Syrinx
  keeps it on.
- Use the Evergreen WebView2 runtime only, and add no DRM code: Widevine
  lives inside the runtime.

**Plugins:**

- **A plugin depends only on the `syrinx:plugin` WIT world.** Built-in plugins
  use the same world, manifest, review and grants a third party will, so only
  owned, typed values cross it, identity is a plain id, and a plugin reaches
  Syrinx only through the world's imports (research note 10, section 2).
- **A plugin gets no capability by default.** Its manifest asks for each one
  with a reason; the owner grants it at install or first use and can revoke
  it. The host owns every risky service, such as files, terminals and git.
- **Running a program is full trust, and is called that.** The owner sees
  "Can run programs on your computer with your permissions". Third-party
  plugins get no full-trust capability in v1.
- **Syrinx fetches no plugin over the network in v1.** Built-ins are bundled
  in its signed release; any other plugin is installed from a folder the
  owner picks, and updates are owner-initiated.
- **Fledge sits behind a trait.** The Fledge plugin, on hold since 2026-10-20,
  calls Fledge only through the `fledge-client` trait, on the stand-in client
  until the real client is built after Fledge's Stage C (D24). Syrinx
  authenticates as an ordinary OAuth client with only the permissions the
  owner grants (D23).
- **Syrinx is a separate project from Fledge-App.** Never put Syrinx code or
  docs in a Fledge repository, and never open issues or pull requests there.

**Privacy and the owner's data:**

- **Only the approved local analytics scope (D39).** To help improve Syrinx,
  the owner approved aggregate feature/command counts, timings and error
  categories with explicit report export. Exclude account/device identifiers,
  URLs, searches, paths, document text and detailed event timelines. No
  automatic uploads or remote crash reports. A model supplies suggestions
  from a report the owner chooses to share; the owner reviews changes.
  Review the schema and implementation slices before collecting anything;
  real usage data stays out of development tools, fixtures and chats.
- **Disclose the runtime limitation.** The owner kept WebView2 and
  SmartScreen after reviewing their data collection. Local crash dumps do
  not disable every required runtime diagnostic. Do not promise complete
  anonymity for one person's exported usage report or change Windows settings.
- **Crash dumps stay local.** Syrinx builds its own WebView2 environment with
  `IsCustomCrashReportingEnabled` set, because wry never sets it and the
  default uploads dumps to Microsoft (note 08, section 5).
- **No live search suggestions.** While the owner types, the palette ranks
  only local tabs, bookmarks and history, so keystrokes stay on the machine
  (D9).
- **WebView2's password saving stays off** (D11).
- **Never put the database in a synced folder.** It lives in the local, not
  roaming, app-data folder, with a daily `VACUUM INTO` snapshot, seven kept
  (D13). SQLite's write-ahead log breaks in OneDrive and similar folders.
- **Page text expires after seven days and is excluded from those backups**
  (D40). Its cache/index must not restore expired content through replay or
  recovery. This is separate from the durable settings/bookmark/session store.
- The owner's history, page text, bookmarks, notes, cookies and Fledge data
  never go into logs, panic messages, test fixtures, commits, issues or a
  chat with an agent.

**Test data:**

- **Never use real personal data**, or a value copied from the owner's
  browser, accounts or files: not in a test, fixture, seed, log, issue,
  commit, prompt or chat.
- **Generate test data that cannot belong to anyone**: reserved domains
  (`example.com`, `.test`, `.invalid`), synthetic bookmark files and
  made-up notes, fresh on each run where it matters.
- **No credentials in tests.** No automated test signs in to Google or any
  real account (D36 rejected a sign-in canary for that reason). The Fledge
  plugin is tested against its stand-in client.

**Dependencies and licence:**

- **Syrinx is MPL-2.0 (owner, 2026-10-13); no GPL code.** Outside code comes in only
  under permissive licences (MIT, Apache-2.0, BSD) or as whole MPL-2.0 files
  kept under MPL (Plan 1's "Open threads"). Zed's editor and `languages`
  crate are GPL-3.0 and stay out.
- **Approved narrow licence exception, 2026-10-10:** `unicode-ident 1.0.26`
  may carry Unicode-3.0 alongside its MIT/Apache-2.0 terms. The owner approved
  it for the pinned Windows bindings' build dependency. This does not choose
  Syrinx's licence or approve unrelated packages; see `docs/dev/dependency-audit.md`.
- **Approved narrow licence exception, 2026-10-10:** `foldhash 0.2.0` may use
  Zlib for the pinned SQLite prepared-statement cache's dependency. The owner
  approved that package and version, not a general licence-policy expansion.
- **Approved narrow source patch, 2026-10-10:** P1-04 may vendor the audited
  `libsqlite3-sys 0.38.2` wrapper with SQLite 3.53.4 and matching bindings from
  upstream commit `901f9946efdaaa289e6b1c5bd56dc67f4b651e51`. Preserve the
  existing Rust dependency versions/features and verify the source identities
  as specified in `docs/dev/vendor-reviews/p1-04-sqlite-maintenance-review.md`. Unchanged upstream
  `TODO`/`FIXME` comments are permitted only inside
  `vendor/libsqlite3-sys-0.38.2`; Syrinx-authored work remains subject to the
  ordinary rule. This approval covers neither commits nor merges.
- **Approved narrow source patch, 2026-10-10:** wry remains exactly 0.57.0
  with the Windows close callback proposed in `docs/dev/vendor-reviews/page-close-event-review.md`.
  The owner approved that two-file dependency patch and its Syrinx close-event
  API so page-requested closure follows the durable tab lifecycle. Unchanged
  upstream `TODO`/`FIXME` comments and existing lint annotations are permitted
  only inside `vendor/wry-0.57.0`; no new Syrinx-authored suppression is allowed.
  Preserve all other archive files, dependency versions and features. This
  approval does not cover another dependency patch, commits, merges,
  publication or sending an upstream message; prepare the upstream diff locally.
- **Approved additional narrow source patch, 2026-10-10:** the owner explicitly
  approved applying the frozen wry failed-construction patch in the parallel
  investigation chat, including its documented limitations. The exact patch
  SHA-256 is `a87ca514149545576a25a39a390a444234d6db9159bcde751e7806aafc959042`;
  it changes `vendor/wry-0.57.0/src/webview2/mod.rs` and adds its private
  `construction.rs`. Preserve pins, features and all other vendor files.
  `docs/dev/vendor-reviews/wry-failed-construction-patch-review.md` records temporary native
  ownership, delayed parent attachment and limited structural cleanup
  diagnostics. Validation is serialized through the integration chat, with
  independent subagent review. Fault hooks stay in disposable test copies,
  never the release patch. This grants no additional dependency patch, COM
  teardown change, security change, commit, merge or publication.
- **Approved narrow source patch, 2026-10-10:** P1-05 may vendor the audited
  `frizbee 0.13.0` package with exactly the three-file padding correction in
  `docs/dev/vendor-reviews/p1-05-frizbee-maintenance-review.md`. Preserve the pin, features
  (`default-features=false`, `std` and `safe_read`), licence and other package
  files. Unchanged upstream `TODO`/`FIXME` comments and existing lint
  annotations are permitted only inside `vendor/frizbee-0.13.0`; no new
  Syrinx-authored suppression or deferred-work marker is allowed. Record the
  archive inventory, exact diff and modified hashes, and validate the unchanged
  dependency graph. This approval covers neither another dependency patch nor
  commits, merges, pushes, publication or an upstream message.
- **Approved narrow source patch, 2026-10-11:** P1-06 may vendor the pinned
  `gpui-pre-windows 0.3.8` package (Zed snapshot
  `279fe070bb389b79652e52065b2f001edcc0b11b`, Apache-2.0) with the D37 idle
  repaint patch described in `docs/dev/vendor-reviews/gpui-idle-patch-review.md`. Unchanged
  upstream `TODO`/`FIXME` comments, including those at
  `src/shaders.hlsl:490` and `:704`, and existing lint annotations are
  permitted only inside `vendor/gpui-pre-windows-0.3.8`; no new
  Syrinx-authored suppression or deferred-work marker is allowed. Preserve the
  exact pin, features, licence and all other package files; record the
  archive inventory, exact diff and modified hashes, and validate the
  unchanged dependency graph. This approval covers neither another dependency
  patch nor commits, merges, pushes, publication or an upstream message.
- **Approved narrow source patch, 2026-10-14:** the owner approved a second
  patch to `vendor/gpui-pre-windows-0.3.8`, after the first preview release
  would not start on any machine but the runner that built it. With debug
  assertions on, as in D38's `local` profile, upstream GPUI compiles its
  shaders at run time from the build machine's source folder. The patch
  changes only `build.rs` and `src/directx_renderer.rs` so that every profile
  embeds the precompiled shaders; building therefore needs the Windows SDK's
  `fxc.exe`. It is recorded in `vendor/gpui-pre-windows-0.3.8.provenance.md`
  with the same boundary as the approval above.
- **Approved dependency, 2026-10-11:** the owner approved adding Tantivy
  for P1-23's full-text bake-off, and for P1-24 if the bake-off chooses it.
  Pin the exact audited version and record its audit, including every
  transitive package's licence, in `docs/dev/fulltext-audit.md`. This
  approval accepts the lockfile growth Tantivy brings. It does not accept any
  transitive package outside the permissive licences listed above; stop and
  ask if one appears.
- **Approved narrow licence exception, 2026-10-21:** the wasmtime and
  Cranelift family (about 31 crates, listed in research note 10, section 1)
  may carry `Apache-2.0 WITH LLVM-exception` at the pinned `=48.0.x` versions
  and the Cranelift 0.135.x versions they require. The exception only adds
  permissions to Apache-2.0. This approves neither
  other packages under it nor a general licence-policy expansion; record the
  audit of every transitive crate before adding wasmtime.
- **Pin GPUI and wasmtime exactly with `=`.** GPUI's snapshot releases change
  its API weekly, and a caret requirement has already broken builds (note 08,
  1.2). wasmtime stays on its 48.x long-term-support line; a security patch
  within it is a routine pin bump that must pass the gate. A patched
  fork of wry or tao is allowed only if small, pinned and offered upstream
  (D12).
- **JavaScript packages follow the same audit (owner, 2026-10-24).**
  CodeMirror and every npm package it brings are audited for licence,
  maintenance and security in `docs/dev/codemirror-audit.md`, pinned exactly
  in a lockfile with integrity hashes, and shipped as a committed, built
  bundle. An `xtask` check refuses any lockfile package missing from the
  audit and compares the bundle's hash, so the gate needs no network. Syrinx
  never fetches script at run time or from a CDN.
- **Keys and tokens never go into the repository.** Fledge OAuth tokens and
  any other secret are read at run time from outside the repository.

**Scope:**

- **Windows 11 only in v1** (D2). No macOS or Linux code paths or testing.
- **Known limits are flagged, not hidden**: no Chrome extensions, and no
  password manager or ad blocking in v1.

## Traps that cost real time

**Read `docs/dev/trapstoknow.md` before you start work and before you debug a
build, a test or a window.** It lists the mistakes that look fine, often pass
the gate, and fail somewhere else. Add a new trap there, not here.

## Develop and test on this machine

Syrinx is a desktop app for one owner on Windows 11, so there is no server,
deploy or cloud preview. Build, run and test it on the owner's machine; the
scaffold writes how into `docs/dev/LOCAL-DEV.md`. Run the app and look at the
window after changing anything the owner sees: a passing gate does not prove
a page card resized, a popup stacked correctly or focus came back to the page.

## Where outstanding work goes

**In GitHub issues on `Fledge-Software/Syrinx-app`, and nowhere else.**
Open questions, deferred work and new slices each get an issue, written in
the Fledge format (an epic per plan or phase, its slices as sub-issues of
the epic; Syrinx uses no milestones, 2026-10-16). Labels follow the wiki's
How-we-track-work page; they are kept simple on purpose. The plans that
held this work before 2026-10-16 are read at af3de53, linked from the
wiki's History page.

A deferred-work marker (`TODO`, `FIXME`, `HACK`) in Syrinx-authored code is
allowed only when it names its GitHub issue, as in
`TODO(#28): bound command ranking`, and that issue is listed in
`docs/dev/deferred-work.md`. Open the issue and add the register row first;
the marker stays until that issue is fixed. `cargo xtask check` rejects
untagged or unregistered markers. The owner set this rule on 2026-10-11 and
switched it from `DW-n` tags to issue numbers on 2026-10-16. A marker never
excuses a failing test, a silenced lint or a swallowed error.

## Writing voice for commits, plans and anything published

**Commits, plans, reports, and later milestones, issues, pull requests and
comments are written to educate.** Assume the reader does not have the full
context: a new contributor once Syrinx is open source, or the owner six months
from now who has forgotten why.

- **Say why before what.** The reason a piece of work exists goes stale
  slowest and is the hardest thing to reconstruct later.
- **Define the terms and name the files.** Spell out jargon on first use
  (web view, accelerator, IPC) and point at real paths, decisions (D13) and
  plan sections so the reader can go look.
- **State what breaks if it is skipped**, and what "done" actually looks like.
- **Show the reasoning, including what was rejected.** A decision recorded
  without its alternatives gets re-litigated.
- **Write prose, not fragments.** Full sentences, no bare references.

The bar: someone who has never seen this codebase should be able to read it
and understand both the task and why it matters.

## Guardrails: ask before proceeding

Never do the following without explicit confirmation from the owner, even if
the task description seems to imply it or a script or tool makes it easy:

### Version control
- Committing when the owner has not asked (the v1 plan's own commits to its
  branch are the exception, D1)
- Adding another remote, pushing `main` or a branch other than the one your
  session was assigned, or opening a pull request unasked
- Force-pushing, or rewriting history (rebase, amend, filter-branch) that
  someone else may have
- Deleting branches or tags
- Merging any implementation branch without the owner's approval
- Once on GitHub: committing directly to a protected branch instead of
  opening a pull request, or merging your own pull request

### Destructive or irreversible operations
- `rm -rf`, bulk file deletion, or deleting anything you didn't create this
  session
- Deleting or overwriting the owner's Syrinx database, its snapshots, or the
  WebView2 user-data folder, which holds the owner's signed-in sessions
- Running a migration against the owner's daily database instead of a test
  copy
- Overwriting existing files with generated content instead of editing them

### Security and secrets
- Committing keys, tokens, passwords or `.env` files
- Adding a dependency without checking its maintenance, its security record
  and its licence
- Turning off SmartScreen, the web view sandbox or web security, granting
  site permissions automatically, or opening a remote debugging port "to make
  it work"
- Adding any network call Syrinx makes on its own (an update check, a
  download, a suggestion fetch)

### Environment and the owner's machine
- Installing software system-wide, or changing Windows settings or the
  registry (including registering Syrinx as a browser or the default browser)
- Changing CI or release scripts, once they exist
- Upgrading a major version of a core dependency, moving the GPUI, wry or
  `webview2-com` pins, or moving wasmtime off its 48.x line
- Changing `Cargo.lock` in ways that shift many transitive dependencies

### Product-specific
- Anything that touches a page web view's identity or contents (user agent,
  IPC handler, injected script, request filter); see the guardrails above
- Changing a decision in `docs/decisions.md`, the spec's "Decisions already
  made", `docs/architecture-decisions.md` or `design/README.md`
- Indexing, logging or exporting any of the owner's browsing data in a new
  way
- Calling Fledge's real API, or asking for more OAuth permissions than a
  task needs
- Adding GPL code, or code under a licence not listed above
- Making the repository or any of its content public

### Quality gates
- Skipping, deleting or weakening tests to make a build pass (marking a test
  `#[ignore]`, loosening an assertion, deleting a snapshot)
- Silencing clippy or the compiler (`#[allow(...)]`, lowering lint levels)
  instead of fixing the cause
- Merging with a failing gate
- Swallowing errors (`let _ =`, `.ok()`, a catch-all `unwrap_or_default`, a
  broad `catch_unwind`) instead of handling them

### Scope
- Refactors that touch more than a handful of unrelated files
- Changing public APIs other code depends on, above all the `syrinx:plugin`
  WIT world
- Renaming or moving files or directories that other tooling or docs may
  reference

## General behaviour
- Prefer the smallest diff that solves the problem.
- If a task requires one of the above, stop, explain what you want to do and
  why, and wait for a yes before doing it. Don't do a "dry run" that quietly
  performs the action.
- If instructions are ambiguous and the ambiguity affects something in the
  list above, ask rather than guessing.
- When you make a technical choice, explain the concept briefly, say why you
  chose it and name the main alternative; the owner is new to parts of this
  stack (spec, "How I want you to work").
- Make direct recommendations rather than lists of options. A product
  question the owner hasn't decided goes to the owner as a question with your
  recommendation; never settle it by picking a default.
- Always run the gate (or the relevant tests) before declaring a change done,
  and keep tests on the core model and the search index.
- State assumptions explicitly when you make them.
