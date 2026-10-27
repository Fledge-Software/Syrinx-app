# Owner decisions since the spec

Decisions the owner has made after `docs/spec.md` was written, newest first.
Each entry says what changed and why. The spec's "Decisions already made"
still stand unless an entry here says otherwise. Design decisions are
recorded separately, in `design/README.md`.

Until the product was named Syrinx on 2026-10-12 it was called Atlas. On
2026-10-17 the owner asked that every entry say Syrinx, older ones
included; "Atlas" remains only in real identifiers such as the old install's
folder names.

A `DW-n` name in an older entry is an item of Plan 1's deferred-work
register, read at
[af3de53](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/plans/Syrinx%20v1%20%E2%80%94%20Plan.md#deferred-work).
Since 2026-10-16 open deferred work is tracked by GitHub issue number
instead (`docs/dev/deferred-work.md`); where an item became an issue, the
entry names it.

## 2026-10-26

- **The plugin runtime starts now, before Plan 1's exit, with wasmtime
  pinned at exactly 48.0.7 (owner, 2026-10-26).** The owner's words: "Yes.
  Start the plugin runtime now, before Plan 1's exit (P1-40), with wasmtime
  pinned at exactly 48.0.7. Record this as my decision in
  docs/decisions.md and run #28 with the usual plan, review and build
  loop." The rest of this entry is the coordinator's record, and the owner
  may correct it.
  - **Why.** Notes PR #14 (Fledge-Software/Syrinx-Notes-Plugin) added a
    folder tree whose Owner check asked for a look in the real window, and
    nobody could do it: Syrinx has no plugin host, so it cannot load Notes.
    The owner moved that check to Notes #15, which waits for the host.
    Every Notes and Forge slice is in the same position. The runtime is
    what makes plugin work visible, so the owner chose not to let it wait
    for P1-40, the owner's one-week daily-driver acceptance.
  - **What changes.** The plugin host (#28 first, then #134 to #137 and
    Plan 2's Phase 2 slices) no longer waits for Plan 1's exit. The
    coordinator reads "the plugin runtime" as all of #25, Phase 2's window
    slices included; the owner may narrow this to Phase 1. This narrows
    the 2026-10-15 rule that no plugin work starts until Plan 1 closes, as
    2026-10-20 already did for #26 and #30. Plan 1's open work, P1-40
    included, stays open and keeps its own order. The plugin host still
    follows its own order inside #25, and the owner still accepts Plan 2
    at #25's exit. `AGENTS.md`'s work order, the in-repo wiki page
    `wiki/Plugin-system.md` and the bodies of #25, #27 and #28 are updated
    to match.
  - **The pin.** 2026-10-21's decision 1 pinned wasmtime to `=48.0.x`.
    The exact version is now `=48.0.7`, the version the template's WIT
    build ran. The owner's words fix wasmtime itself; the rest of the
    wasmtime family follows at the versions 48.0.7 requires. Whether
    `wasmtime-wasi` is a dependency at all is still open: it is the first
    open question in `docs/dev/vendor-reviews/wasmtime-wit-prebuild-check.md`,
    where option (b) drops it and Syrinx's host provides the WASI
    interfaces, and option (c) pins it at `=48.0.7`. The owner decides it
    before #28 adds the dependency. *(Answered later on 2026-10-26:
    option (b), so `wasmtime-wasi` is not added; see the next entry.)* #27's audit of every transitive crate,
    under the 2026-10-21 licence exception, is still done before wasmtime
    enters `Cargo.lock`. Moving to a later 48.0 security patch remains a
    routine pin bump that must pass the gate.
  - **What it costs.** Plugin-host code lands in Syrinx's own crates while
    Plan 1 is still being accepted, so the gate and the release carry
    wasmtime before the browser core is signed off. Research note 10, the
    WASM runtime note at
    `docs/dev/background-10-wasm-plugin-runtime-draft.md`, measured about
    17 MiB added on Linux; #28 records the build-time and size cost on
    Windows. Rejected: waiting for P1-40, which leaves every plugin slice
    unverifiable in the window until then.

- **The plugin host's first slice (#28): WASI, the licence check and the
  shape of `syrinx:plugin@0.1.0` (owner, 2026-10-26).** The owner's words:
  "I accept all the #28 recommendations as written: G1 option (b), G2, G3,
  G10, G9's eight items, G4, G5 and G7. I'll do O1 and O2 when you ask."
  The recommendations are those of #28's reviewed plan; the summary below
  is the coordinator's record of them, and the owner may correct it.
  - **G1: Syrinx's host provides WASI itself.** A plugin compiled from
    Rust for `wasm32-wasip2` imports sixteen WASI 0.2 interfaces (output,
    clocks, randomness and similar basics). Syrinx's plugin host
    implements them in its own code, about 300 lines, instead of using
    `wasmtime-wasi`. So the pin is `wasmtime =48.0.7` alone. Why:
    `wasmtime-wasi` 48.0.7 fails Syrinx's network-code check (D9), because
    `io-extras` asks `windows-sys` for WinSock, and it brings about eleven
    more crates and a `tokio` move. Rejected: option (c), pinning
    `wasmtime-wasi =48.0.7`, which would need a ninth network entry and a
    change to the headless network rule
    (`docs/dev/vendor-reviews/wasmtime-wit-prebuild-check.md`, open
    question 1).
  - **G2: the WASI interface files are copied in.** The 692 lines of WASI
    0.2 WIT that the host implements may be committed under
    `vendor/wasi-0.2.12-wit/`, under `Apache-2.0 WITH LLVM-exception`,
    with a provenance file. Rejected: registering the interfaces by hand
    without the files.
  - **G3: `async` is not compiled in.** The sentence of 2026-10-21's
    decision 1 that keeps wasmtime's `async` Cargo feature "because
    `wasmtime-wasi` requires it" no longer applies. Syrinx calls plugins
    synchronously and builds wasmtime without `async`.
  - **G10: three compiler crates fall under the licence exception.**
    `regalloc2` and `target-lexicon` (and `winx`, which option (b) no
    longer compiles) are covered by the 2026-10-21 exception at the
    versions wasmtime 48.0.7 requires, as `docs/dev/wasmtime-audit.md`
    reads it.
  - **G4: the gate checks licences.** #28 adds to `cargo xtask check` the
    plugin template's licence check: every package in `Cargo.lock` must
    carry an allowed licence or its exact approval: `unicode-ident
    1.0.26`, `foldhash 0.2.0`, the 18 ICU crates, `encoding_rs 0.8.42`,
    and the wasmtime family by exact name and version (29 lockfile entries
    under G1's option (b)). Until now only a manual audit protected them.
  - **G9: the shape of plugin interface 0.1.0 is approved.** It counts as
    frozen once #28's contract tests pass, and the template (#30) and
    Notes keep their own draft copies until #28's closing comment names
    the commit to copy:
    1. Within 0.1.x, only imports marked `@since` may be added; any other
       change starts a new version.
    2. There is no cancel export into a plugin's view code; Syrinx drops
       queued requests for a cancelled id before delivery.
    3. `forms.submit` receives a host-chosen request id, and a `pending`
       answer must return exactly that id.
    4. Forms gain `date` and `date-time` fields and an `email` or
       `web-address` text format. `forms.validate` can report its own
       failure, and the host never runs a command that has a form.
    5. Thirteen icons are added: file, code, git, diff, symbol, link, tag,
       calendar, mail, list, image, settings and error.
    6. `plugin-error` gains `cancelled`, which is never used where a write
       may have happened.
    7. A search entry may carry up to 16 keywords of up to 120 characters;
       an over-limit keyword is dropped and logged, not used to refuse the
       batch.
    8. The search-change kinds, the log levels and what a plugin is told
       when it starts stay as they are; anything new at start, such as a
       per-call time budget, comes later as a new `@since` service
       (planned for #135).

    The same approval writes two rules into the contract. The form rule:
    the host calls `forms.form-for` on preview, `forms.validate` as fields
    change and `forms.submit` on Enter, never `commands.run` for a command
    declared with a form, and `form-for` and `validate` have no side
    effects. The delivery order: the host never calls an export from
    inside an import; deferred calls run first in, first out, after the
    current export returns; for each view, `opened` comes before any
    `message` and `closed` comes last; and `events.deliver(r, ...)` follows
    the export that returned `pending(r)` or armed `r`.

    The new date fields and icons have no design art yet; they are drawn
    under `design/README.md` in #29 and #141.
  - **G5: plugin-view memory moves to #138.** 2026-10-21's decision 3 put
    it in #28's measurements, but no plugin view exists until #138, so
    #138 measures it.
  - **G7: the toolchain is pinned.** A `rust-toolchain.toml` pins Rust
    1.97.1 (CI's existing pin) with `wasm32-wasip2`, `rustfmt` and
    `clippy`. The CI `setup-rust` step installs the target, fetches the
    fixture lockfile and adds it to the cache key, and `LOCAL-DEV.md`
    records how rustup's auto-install behaves. Because the weekly release
    uses the same step, it builds on this toolchain too, with unchanged
    output; inside this repository the file overrides the machine's
    `stable` default.
  - **A correction to 2026-10-24's decision 5 (a note, not an owner
    decision).** The default stubs are
    written by hand in the guest, because the ones `wit-bindgen` generates
    trap (#27's pre-build check). The decision itself stands.
  - **Still the owner's to do.** O1: install `wasm32-wasip2` for 1.97.1
    and allow downloads of audited crates. O2: give the go-ahead for #28's
    measurement run on a quiet machine.

## 2026-10-25

- **The palette's first row is always what the owner typed (owner's own
  decision, 2026-10-25).** The owner typed `youtube` in the palette and
  found an unrelated open tab first and preselected, with the YouTube
  history below it and the web search in a footer. The owner's words: "why
  would anyone want to search youtube and get the recent. The thing the
  user is searching should obviously be the first option. Then you have
  other options underneath." Only the quoted words are the owner's; the
  rest is recorded by the coordinator and the owner may correct it.
  - **What changes, as recorded.** In the Everything view the first row is
    the typed text, and it is preselected, so plain Enter runs it: a search
    of the web for the text ("youtube — Search the web"), or, for a
    complete address, a visit to that address. The row is built locally
    and nothing is fetched while typing (D9). Open tabs, bookmarks and
    history follow in their sections, reached with Down. A row that holds
    the typed text only as letters scattered through it now ranks below
    every row holding it as a word or an address prefix; it stays listed.
    Where one section has both kinds, its scattered rows follow every word
    match under a second header of the same name (for example Open tabs,
    History, then Open tabs again). The `>`, `@` and `/` prefixes and the
    scope chips have no typed row and are unchanged.
  - **What it supersedes, as recorded.**
    - The choices of 2026-10-24 made in #127 and recorded in commit
      `305af76` rather than here: preselecting a confident local match
      below the search row instead of completing it inline. That match was
      a page whose address starts with the typed text, an open page's
      exact title, or a command's exact name. It also covered a typed bare
      host that only starts a known host: `cloud.exam` used to open a
      known `cloud.example.test`, and Enter now goes to
      `https://cloud.exam/`, which may not exist; the known page is one
      Down away. With the typed row always chosen, Enter in Everything no
      longer waits for the local results (#127 kept that wait, from P1-33,
      only because a confident match could still arrive). Without a typed
      row, in the chips, it still waits.
    - The pinned "Search the web" footer row under the results, which came
      from the design canvas (P1-30/31; `design/canvas/project/Palette.dc.html`)
      and which #127 kept. The footer row is gone, so in the chips and
      after a prefix the web search is no longer a click or a Tab stop.
      `design/README.md`'s "always offers Search the web (Alt Enter)" still
      holds: Alt Enter always searches the web, and the footer's hint line
      still names it.

- **Notes organises notes in folders; database-style views go to a separate,
  later plugin (owner's own decision, 2026-10-25).** The owner's words:
  "dont worry about databases for notes. We will put them in a different
  plugin. Just have folders that allow the user to organize notes."
  Only the quoted words are the owner's; everything below is recorded by
  the coordinator, and the owner may correct it.
  - **What changes, as recorded.** Database-style views (typed properties,
    table views and `.base` files) leave the Notes plugin for a separate
    plugin that comes later and is not scheduled, tracked as #167, *Epic:
    Databases plugin (typed properties and table views)*. Notes v1
    organises notes in folders instead. The coordinator's reading of
    "folders that allow the user to organize notes" is a folder tree in the
    Vault tab, creating, renaming and deleting folders, moving a note
    between folders with its links kept, and choosing the folder for a new
    note, built in #168 on the guarded rename of #38.
  - **What it supersedes, as recorded.** The spec's "database-style views"
    for Notes (`docs/spec.md`, § 4, which is kept verbatim; this entry
    overrides it) and decision N3 of the Notes epic (#35), "Bases in v1 is
    one table". #39 (the property-type metadata decision) and #40 (typed
    properties and one table view, formerly Notes 4) stay open as
    sub-issues of #167 instead of #35, each with a dated banner and its new
    parent, and Notes' acceptance (#41) no longer includes a table.
  - **What stays, as the coordinator reads it.** D27's Obsidian
    compatibility, wikilinks, backlinks and the outline panel. Properties
    stay in the files untouched, as plain frontmatter: Notes indexes them
    for search, never retypes or rewrites them, and reads and writes no
    `.base` file and no property-type metadata, so a vault edited in Syrinx
    still opens in Obsidian with its properties and bases as they were.
    D27's "Obsidian's property types" and "a subset of `.base`" then
    describe what the Databases plugin must keep compatible, not what
    Notes builds.
  - **Why, as the coordinator reads it.** The owner gave no reason beyond
    the quote. The table was the heaviest Notes slice and rested on an
    Obsidian file whose format is unverified (#39), and moving it out lets
    Notes reach daily use, and so unblock Forge (#42), sooner. Rejected:
    keeping the one-table view in Notes v1 (N3), which the owner chose
    against; dropping database views altogether, which the owner did not
    ask for.

## 2026-10-24

- **Syrinx's full-text index uses Tantivy, not SQLite's FTS5 (#52).** The
  quiet-machine bake-off of 2026-10-21 (results on #52) measured the broad
  corpus at a 95th-percentile query time of 2.3 ms for Tantivy against
  24.8 ms for FTS5. Expiring a week of pages took 2.2 s against about five
  minutes, and the index used 55 MB of disk after expiry against 408 MB.
  FTS5 used 130 to 180 MB less memory, but #52's rule preferred FTS5 only if
  the two were close, and they were not. The owner confirmed the rule's
  choice. #53, the production adapter, builds on Tantivy.
- **Tantivy moves to 0.27.0 before Syrinx ships it (#52).** The tested
  0.26.2 depends on a version of `lru` covered by RUSTSEC-2026-0253; 0.27.0
  depends on a fixed one. The owner approved moving the exact pin and the
  lockfile growth that comes with it. Every new package is audited in
  `docs/dev/fulltext-audit.md`, and the move stops for the owner if any
  licence falls outside MIT, Apache-2.0 or BSD. A confirming rerun of the
  comparison on 0.27.0, on a quiet machine the same day, matched 0.26.2's
  results (`docs/dev/fulltext-audit.md`).
- **Signing the weekly update files waits (#55, PR #119).** The owner is
  not setting up the signing key yet: it adds a monthly cost, and the
  previews have no users who need it. Weekly previews stay unsigned until
  the owner does `docs/owner-checklist.md`, item 1.
- **The Glass look and the Settings page passed the owner's window check
  (#93, #91).** The owner looked at both on `main` and found them better.

- **Dark Acrylic keeps a minimum tint, and downloads get Syrinx's own
  session list and flyout.** Three approvals, given together:
  - **(1) A dark theme under Acrylic never drops below a 12.5 % tint**
    (`DARK_ACRYLIC_TINT_FLOOR`, 32/255, in
    `crates/syrinx-ui/src/theme.rs`). The slider still runs 0–90 %, but
    Acrylic shows the bright desktop, and #124 checks a dark theme's text
    over Glass's bright wallpaper discs; over the teal one (`#23948A`)
    white text on Graphite's `#141519` reaches 4.5:1 only from that tint
    up. The owner named 12 %; the contrast maths put the exact least
    value at 32/255, so that is used. Implementation note, not part of
    the owner's decision: the window floor alone left the lighter
    translucent white surfaces (the selected row, fields, the search
    pill, plugin surfaces and Home's card while Home is Solid) under
    4.5:1 above about 60 %, so under dark Acrylic they keep at least a
    0.343 tint under them (`DARK_ACRYLIC_SURFACE_FLOOR`), the least at
    which white text on the selected row still reaches 4.5:1. This
    surface floor awaits the owner's look on Windows. Text is now
    readable at every step for every material, in both modes, with Home
    Solid or See-through; Mica, Mica Alt and
    light themes keep no window floor, and the Glass preset (50 %) and
    the Haze preset are unchanged. This replaces the implementation note
    below that a saved dark Acrylic theme was readable only up to 55 %.
    Rejected: letting the tint fall to nothing, with a warning above about
    55 %, because
    a choice the slider offers should not be one that fails readability;
    and text shadows, which would change every label's look and still
    give no measurable contrast guarantee.
  - **(2) Downloads (#143, #89): Syrinx keeps its own downloads list in
    memory, for the session only.** Nothing about a download is written
    to disk by Syrinx, and the list is cleared when Syrinx exits. It is
    fed by WebView2's `DownloadStarting` event, read from Rust on every
    page web view. That reads browser events only: it is not an IPC
    handler, an injected script or a request filter, so the rules for
    page web views and Google (`AGENTS.md`) still hold. It replaces
    WebView2's default download dialog, which each tab draws for itself.
    Rejected: a persisted history like Chrome's, which would keep a
    record of the owner's downloads past the session; and keeping the
    per-view default dialog, which belongs to one tab and does not look
    like Syrinx. The owner also approved (2026-10-24) setting `Handled`
    on `DownloadStarting`, so WebView2's own download flyout no longer
    appears, on the condition that SmartScreen still checks every
    download: a file it blocks shows in Syrinx's list as blocked with its
    reason, and Syrinx never offers a way to keep it.
    Implementation notes, not part of the owner's decision: the list
    keeps at most 100 downloads; when a new one would take it past that,
    the oldest finished ones are forgotten first, and an active download
    is never forgotten, so the list is longer than 100 only while more
    than that many are still running. A popup that asks to close while
    its download runs stays open, as a blank tab with the status "The
    popup closes once its download finishes.", until the download ends or
    the owner closes it, because nobody yet knows whether WebView2 keeps
    a download when its web view closes. The `page_close` fixture's
    downloads case 4 measures exactly that ("Closing a popup's web view
    mid-download kept/stopped the download"); its result on the owner's
    machine decides whether the deferral stays (`docs/owner-checklist.md`,
    item 7).
  - **(3) The downloads flyout matches the existing flyouts' style.**
    Rejected: a new design for it, which would add a look the canvas
    does not have.

- **Phase 2 is approved: the plugin runtime (#25) and Notes (#35) are
  re-cut for WASM plugins, and all thirteen recommendations of the
  independent review are accepted as stated.** #27 asked for Plan 2 to 4's
  slices to be re-cut after the 2026-10-15 move to WebAssembly (WASM)
  plugins and the 2026-10-21 approval of research note 10
  (`docs/dev/background-10-wasm-plugin-runtime-draft.md`). The re-cut
  keeps Plan 2's six slices and their evidence, narrows #28 and #29, adds
  the runtime slices note 10 needs (manifest and signed packages,
  lifecycle, grants, the compiled-code cache, the plugin view, plugin tabs,
  install from folder, search feeds and bundling) as new issues under #25,
  and rewrites #36 to #41 so each Notes slice has a Syrinx-app host half
  and a Notes-repository plugin half, with a table mapping every old exit
  criterion to its new form. An independent reviewer, who wrote none of the
  drafts, checked them against note 10 and these decisions, fixed the
  ownership and ordering faults, and left thirteen questions for the owner;
  each is recorded below with what was rejected. `AGENTS.md`'s layout and
  plugin guardrails are updated in the same change. This approval adds no
  dependency and changes no code; Plan 1's remaining work still comes first.

- **(1) The `wasm32-wasip2` target is approved for `rust-toolchain.toml`.**
  #28 builds its synthetic test plugin inside the gate, which needs the
  WASI 0.2 compile target. The slice that first needs it adds it; nothing
  changes the toolchain file before then. Rejected: committing a built
  `.wasm` file with a rebuild-and-compare check, because it relies on
  reproducible builds that note 10 left unconfirmed.

- **(2) "Plugin runtime 0", the pre-build checks, is folded into #27.**
  The wasmtime 48.0.x audit of every transitive crate and the check that
  the draft WIT parses and loads are done under #27, which stays open for
  them. Throwaway builds outside the repository count as paper work, as
  they did for #26, so this may run before Plan 1's exit; nothing under
  `crates/` and no build in this repository is allowed for it. Why: the
  2026-10-21 entry for decision 2 already calls the licence audit "#27's
  audit". Rejected: a new slice-0 issue, which would need its own
  exception to the phase order.

- **(3) The host crate is `crates/syrinx-plugin-host`.** It follows the
  existing `syrinx-*` names and says what it does, and it is the only crate
  that links wasmtime. Rejected: keeping the old `syrinx-plugin-api` name,
  which described a Rust trait that no longer exists.

- **(4) The WIT package lives at `wit/v0_1/` at the repository root.** It
  is the public contract the plugin template copies, so it is not buried in
  the host crate, and `xtask` parses it in the gate. Rejected: a folder
  inside `syrinx-plugin-host`.

- **(5) One world, with default guest stubs.** Every plugin targets the one
  `plugin` world; the guest crate generated by `wit-bindgen` supplies
  default stubs (for example `not-found` or empty results) so a plugin
  implements only what its manifest declares. #27's pre-build check
  confirms this works. *(Corrected on 2026-10-26: the stubs are
  written by hand, because the generated ones trap; see that date's
  entries.)* Rejected: one world per plugin role, which would
  multiply the version tracks to keep compatible.

- **(6) Notes' capability interfaces arrive with `@since(0.1.x)` in the
  slices that build them.** `documents`, `watch`, `drafts`, `editor` and
  `open-external` are added to the WIT package by #36 and #37, each marked
  with the minor version it appeared in, which wasmtime's linker treats as
  a compatible addition. Their paper shapes are reviewed in #27 first.
  Rejected: designing all of them in #28 before 0.1.0 is frozen, as the
  Notes draft and #98 had proposed, because fixing shapes before any code
  uses them risks freezing a wrong design.

- **(7) #28 publishes the test host as a library** in `syrinx-plugin-host`.
  Note 10 says Syrinx publishes a wasmtime test host for plugin
  repositories' contract tests, but no slice owned it. Each capability
  slice adds its synthetic version (an in-memory vault, scripted watch
  events, a scripted editor), and the template (#30) consumes it.
  Rejected: building it in the template, which would let the template's
  copy drift from the real host.

- **(8) Opening attachments is a narrow, optional, click-only capability**
  (#37, P3-09). The owner sees it as its own row on the review sheet,
  "Open files from your vault in their usual apps". It opens only types on
  an allow-list (PDF, images, office documents), only on the owner's
  click, after the host's containment checks. Why: opening a file in its
  default Windows app starts a process the sandbox cannot follow. Rejected:
  a deny list of executable types, which fails open for any type nobody
  thought of, and folding it into the folder grant silently.

- **(9) A link clicked in a plugin view opens only with `open-external`.**
  The plugin view (runtime 2b) turns a link to another origin into an
  "open in a tab" request, and the host honours it only when the plugin
  holds the `open-external` grant; otherwise it refuses it. Rejected:
  honouring every such request, which would give each plugin view an
  unreviewed way to open pages.

- **(10) #53 is amended before it is built.** The tier-2 full-text adapter
  keeps separate source namespaces and deletion rules: page text expires
  after seven days under D40, while note text is a rebuildable cache
  outside D13's snapshots with no seven-day expiry. Its stale reference to
  "Fledge 2 (#34) P2-10" is dropped, since plugin search feeds now belong
  to runtime 2e. Rejected: one deletion rule for every source, which would
  either delete notes' index after a week or keep page text too long.

- **(11) #36's catalogue is held in the plugin's memory and rebuilt from
  the files** at activation; #41 measures warm reopen on the large
  synthetic vault. The old criterion "index each note into SQLite" cannot
  be met by a WASM plugin, and Syrinx's database sits inside D13's
  snapshots, where note-derived content should not go. A host-managed
  rebuildable cache outside the snapshots is a later decision if the
  measurement says reopen is too slow.

- **(12) The JavaScript rule:** CodeMirror and every npm package it brings
  are audited in `docs/dev/codemirror-audit.md`, pinned exactly in a
  lockfile with integrity hashes, and shipped as a committed, built
  bundle. An `xtask` check refuses any lockfile package missing from the
  audit and compares the bundle's hash. Why: this keeps network access out
  of `cargo xtask check`. Rejected: building the bundle from npm inside the
  gate, which would make every gate run download code.

- **(13) Bundling the built-in plugins (runtime slice 4) sits outside
  #25's exit.** It needs Notes' first signed release, and the owner can
  install Notes from a folder before it lands, so it does not hold up the
  acceptance of Plan 2. Rejected: making it part of #25's exit, which would
  tie the runtime's acceptance to Notes' release.

- **The Home option stays, and PR #124 is reconciled with it.** The
  per-theme "Home: Solid / See-through" choice of 2026-10-22 is kept. PR
  #124 (the Glass preset) merged to `main` on 2026-10-23, and the merge
  of `main` into this branch reconciles it: its automatic thinning of the
  Haze preset's Home card is dropped in favour of that choice, so there is
  one way to make Home see-through rather than two that overlap. A saved
  Haze keeps Home Solid, so at its own 25 % its card is still the canvas's
  `rgba(10,11,14,0.5)`, as #124 drew it. Its Glass preset uses
  Transparency 50 %, which under the new 1 − Transparency/90 mapping gives
  a 0.44 tint, close to the canvas's 0.46; its tint test
  `the_window_tint_thins_as_transparency_rises` is updated to the new
  mapping. This settles the three points the 2026-10-22 entry left for
  #124's merge. Implementation note, not part of the owner's decision:
  #124 checks a dark theme under Acrylic over Glass's bright wallpaper
  discs, where a saved theme's text reached 4.5:1 only up to 55 % (the
  10 % row floor covers Mica, Mica Alt and light themes to 90 %); the
  dark-Acrylic floor approved above has since made it readable to 90 %.
  Rejected: keeping #124's automatic thinning beside the Home choice, which
  would make one preset ignore the owner's per-theme setting.

- **A saved folder whose title is blank restores as "Untitled folder" (#80,
  PR #99).** *Recorded by the coordinator from the owner's merge
  instruction of 2026-10-24; awaiting the owner's confirmation.* A saved
  session could carry a folder whose title is empty or only whitespace,
  although the model refuses such a name when the owner creates or renames
  a folder (2026-10-12, P1-21). On restore, Syrinx renames such a folder
  "Untitled folder" in memory, before the session is validated
  (`TabModel::from_snapshot` in `crates/syrinx-core/src/tabs/snapshot.rs`),
  and the new name reaches disk with the next session save. Rejected:
  refusing the session, because `syrinx-store` treats a session that fails
  validation on read as a damaged database, so one cosmetic field would
  cost the owner the whole session and send recovery looking at backups;
  and accepting the blank title as it is, which would leave the model
  holding a state it refuses to create. Implementation note, not part of
  the owner's decision: the rename is silent, because the loader has no
  way to report a repair; a status-line notice would be its own issue if
  the owner wants one.

- **"Home" is a display name only (#76, PR #100).** *Recorded by the
  coordinator from the owner's merge instruction of 2026-10-24; awaiting
  the owner's confirmation.* A saved blank page is shown as "Home"
  everywhere a saved page is shown (the sidebar row, its accessible name
  and drag ghost, the peek, the palette, the "Delete saved page" card and a
  Favorites drop), both while its tab is open and after it closes. Its
  stored title stays what the page reported, `about:blank`: the shown name
  comes from the rule an open tab already uses
  (`TabMetadata::display_title`, through `SavedPageSnapshot::display_title`
  in `crates/syrinx-core/src/tabs/snapshot.rs`). Rejected: "about:blank",
  which the sidebar showed while the palette and the page itself said
  Home; and storing "Home" as the saved title, which would rewrite stored
  rows where a display rule is enough, and could not be changed later
  without a migration. Implementation note, not part of the owner's
  decision: since #91, Ctrl D refuses Home, so only a row saved before
  that shows the name, and the owner has not yet looked at it in a window.

- **The restore-backup screen is drawn in Graphite, whatever theme is
  chosen (#51, PR #102).** *Recorded by the coordinator from the owner's
  merge instruction of 2026-10-24; awaiting the owner's confirmation.* When
  Syrinx offers to replace a damaged database with the newest verified
  backup (P1-04, D46), the confirmation screen
  (`crates/syrinx-app/src/ui/recovery.rs`) uses the default Graphite
  theme's colours and type. The owner's theme is stored in the database
  that has just failed to open, and Graphite is the default (2026-10-15).
  Rejected: reading or guessing the owner's theme, which the screen cannot
  do reliably when the database is damaged. Implementation notes, not part
  of the owner's decision: the screen shows the backup's date in the
  owner's local time, followed by UTC; its failure line uses Graphite's
  orange accent, because `syrinx-ui` has no error colour; its progress
  line is not yet announced to screen readers (#101); and its wording and
  its two choices still wait on the owner's own look, which keeps #51
  open.

- **The palette and the sidebar peek get the menus' Windows 11 rounded
  corners and shadow (#85, PR #103).** *Recorded by the coordinator from
  the owner's merge instruction of 2026-10-24; awaiting the owner's
  confirmation.* #72 gave the ☰ and ⋯ menus Windows 11 rounded corners and
  a system drop shadow. The command palette and the sidebar peek, which
  are popup windows of their own, now get the same, so Syrinx's three
  popups look like one family; one helper, `round_popup_corners` in
  `crates/syrinx-app/src/ui/owned_popup.rs`, serves all three. Rejected:
  keeping the palette and the peek square and shadowless, which left
  Syrinx's own popups inconsistent with each other. Implementation note,
  not part of the owner's decision: nobody has looked at it in a running
  window yet. The owner's eye check from PR #103 stands (Ctrl K over a Home
  tab, and the peek with the sidebar collapsed, in Graphite and Paper:
  clean corners, a soft shadow, no black corners and no double shadow),
  and `design/README.md` records the look once the owner confirms it.

## 2026-10-22

- **See-through themes really show what is behind the window.** On
  2026-10-20 the owner chose Acrylic at 90 % Transparency and saw almost no
  change (#93). Four things hid the material: Syrinx's own tint never fell
  below 10 % opacity, Windows was always asked for the dark backdrop, Home's
  card kept a 50 % dark fill, and the page card is opaque. The owner's
  decision:
  - **Acrylic shows the windows behind Syrinx, frosted.** The materials
    offered are Off (the old "None"), Mica, Mica Alt and Acrylic.
  - **The Transparency slider (0–90 %) lowers Syrinx's own tint to
    nothing.** Its opacity is now 1 − Transparency/90, so 90 % draws only
    Windows' material; the old 1 − Transparency/100 kept a 10 % floor.
    Mica keeps the same slider and mapping. A selected row alone keeps the
    old 10 % tint under it, because with no tint its light fill over a
    bright window drops text below 4.5:1.
  - **The backdrop is light or dark as the theme is**, instead of always
    dark. A light theme's text is checked over pale stand-in wallpapers to
    match the light material.
  - **Home's card can be see-through too**, as a per-theme choice ("Home:
    Solid / See-through", shown only with a material). See-through thins it
    by the same factor as the window tint. A choice was preferred over
    always thinning it, because Home holds the most text and a busy window
    behind it can make that harder to read.
  - **Web pages stay solid. The chrome the theme colours follows
    Transparency** (window background, title bar, sidebar and, when chosen,
    Home). The command palette, menus and the Customize panel stay opaque
    so their text stays readable over any window; the 2026-10-15 rule on
    opaque pages and palette stands, as does Reduce transparency.
  - **Saved themes keep working.** The Home choice is stored as
    `;home=see-through` only when on, so every saved theme reads and writes
    exactly as before.
  Rejected: a "Clear" material (an unblurred, fully transparent window),
  because text over a sharp background cannot be kept readable; and making
  the whole window, web pages included, see-through, because a transparent
  page background breaks sites that assume white and any pixel over the
  page card hides the page (`docs/dev/trapstoknow.md`). This is compatible
  with PR #124 (the Glass preset): both make the backdrop follow the theme
  with the same function, but #124's preset would need Transparency 50 %
  (a 0.44 tint) instead of 55 % to keep the canvas's 0.46 under the new
  mapping; its test `the_window_tint_thins_as_transparency_rises` expects
  the old 1 − Transparency/100 and must follow the new mapping; and its
  automatic thinning of Haze's card overlaps the Home choice. All three
  are settled in the 2026-10-24 entry.

## 2026-10-21

- **Syrinx remembers each site's zoom level (#90).** The owner said yes to
  storing per-site zoom, as Chrome, Edge and Firefox do. It is kept only in
  Syrinx's local database, keyed by site, and is never exported or sent
  anywhere. Without it, zoom resets on every new tab and restart, which is
  what #90 reported.
- **Settings opens from the settings button at the bottom of the sidebar
  (#91).** The spec already puts settings in the sidebar's bottom row; the
  owner confirmed that this button is the way into the Settings page, and
  that themes are reachable from it.
- **The Glass theme must look like the canvas's Glass mockups (#93).** The
  owner wants Glass and its Transparency setting to show real translucency
  and glass edges, as in `design/canvas/project/Main.dc.html`, not the
  near-opaque fill Syrinx draws today. The canvas is the target. In the
  owner's words, the Glass look belongs "in that theme selection", so Glass
  comes back as a preset beside Graphite and Paper. This replaces the
  2026-10-15 rule that only Graphite and Paper are presets, for Glass only;
  every other look is still built in the theme editor.
- **The cookie-flush fix keeps Google signed in (#96).** The owner checked
  that Google stays signed in after closing and reopening Syrinx with #111,
  so #111 closes #96.
- **The full-text bake-off runs tonight (#52).** The owner meant the
  2026-10-20 overnight run to include the quiet-machine comparison; it did
  not run. It runs on 2026-10-21, once the day's pull requests are
  merged, with every agent build stopped first.
- **Clearing "Cookies and site data" also clears saved zoom levels (#90).**
  This is what Chrome does. Zoom levels are site data, so the owner expects
  them to go with cookies.
- **Ctrl , opens Settings even while a web page has the keys (#91).** This
  follows Chrome and Edge. The cost is that a few web apps, such as
  github.dev, lose Ctrl , for their own settings.
- **Settings opens as its own tab (#91).** It does not replace the page
  the owner is on.
- **Ctrl D on Home saves nothing (#91).** Home is Syrinx's own page, so
  bookmarking it would save a blank page. It is refused, as it already is
  on the Settings tab.

- **The WASM plugin runtime research note (#26) is approved, with all
  fifteen of its decisions exactly as recommended.** The note answers the
  questions #26 set before #27 re-cuts Plan 2 to 4's slices: which
  WebAssembly (WASM) runtime Syrinx embeds, what crosses the plugin
  boundary and how it is versioned, which powers the host offers and how
  the owner grants them, how a plugin's interface is drawn, and how
  plugins are packaged, installed and updated. It currently lives at
  `docs/dev/background-10-wasm-plugin-runtime-draft.md`, because this
  session cannot push to the wiki; it moves to the wiki as
  `Background-10` when the owner publishes it, and like every Background
  page it is then read for reasoning, while the entries below are the
  decisions. The owner added two decisions of their own (the last two
  entries of this date). Each entry below names the note's section that
  argues it; the note's "Conflicts between the five reports" table records
  how its five source reports were reconciled. This approval lets the
  plugin host (#28, #29) start once Plan 1 closes, as the 2026-10-20 entry
  required; it does not itself add a dependency, which #27's audit and
  #28's slice still do. `AGENTS.md`'s plugin guardrails are updated by a
  separate proposal, not by these entries. *(On 2026-10-26 the owner let
  the host start before Plan 1 closes; see that date's entry.)*

- **Runtime and pin (decision 1): wasmtime with the component model,
  pinned exactly `=48.0.x`.** *(On 2026-10-26 the owner fixed the
  exact version at `=48.0.7`; see that date's entry.)* Syrinx embeds wasmtime with the component
  model and WIT (WebAssembly Interface Types, the typed interface
  language the owner chose on 2026-10-15), host bindings generated with
  `wasmtime::component::bindgen!`, pinned with `=` on the 48.x
  long-term-support line (supported to about 2028-08), with Cranelift, the
  component model and WASI 0.2, and with Winch, the pooling allocator,
  `component-model-async`, WASI 0.3 and `wasi-http` off. Syrinx calls
  plugins synchronously; wasmtime's plain `async` Cargo feature is still
  compiled in because `wasmtime-wasi` requires it, so the dependency audit
  covers the graph with it. *(Amended on 2026-10-26: `wasmtime-wasi`
  is not used and `async` is not compiled in; see that date's
  entries.)* Security patch releases within 48.0.x are
  routine pin bumps that pass the gate; a move to another major line still
  needs the owner. Why: it is the only runtime that meets the WIT decision
  with a support policy Syrinx can pin to; following wasmtime's monthly
  majors would mean an owner-approved pin move every two months. Rejected
  (note, section 1, "Why not wasmi or wasmer"): wasmi, which has no
  component model and would force the byte-buffer protocol the owner
  rejected; wasmer, which closed WASI Preview 2 as "not planned", has no
  component-model API, no published LTS or `SECURITY.md`, and brings
  licences outside Syrinx's list; and pinning 49.0.4, the documentation
  some reports cited, instead of the LTS line (section 1, "Reconciled").

- **Approved narrow licence exception (decision 2):** the wasmtime and
  Cranelift family (about 31 crates in the audited graph, listed in the
  note's section 1, "Licences of every transitive crate": the
  `cranelift-*`, `pulley-*`, `wasmtime*` and `wasmtime-internal-*` crates,
  `regalloc2`, `target-lexicon` and `winx`) may carry
  `Apache-2.0 WITH LLVM-exception`, at the pinned wasmtime 48.0.x versions
  and the Cranelift 0.135.x versions they require only. The exception only
  adds permissions to Apache-2.0, but `AGENTS.md` names permitted licences
  exactly, so it is approved by name. This does not approve another crate
  under that licence, a wasmtime line other than 48.0.x, or a general
  licence-policy expansion; #27's audit reads each crate's `LICENSE` file
  rather than crates.io metadata. `unicode-ident` stays at the approved
  1.0.26, and the existing `foldhash 0.2.0` and ICU approvals carry over
  unchanged; a version they do not cover needs a fresh approval.

- **Measurement (decision 3): #28 runs the Windows measurements, and
  plugin views are reported separately from D6's budget.** #28 measures
  runtime start-up, memory, compile time for a realistically sized guest
  and plugin-view memory on the owner's Windows machine, because every
  figure in the note is from Linux or unmeasured (section 1, "Cost against
  D6"; section 4, "Memory per extra web view"). Plugin views are reported
  separately, with the ten page tabs still within D6's 10%-of-Edge budget.
  Rejected: counting plugin views inside that budget, which would let a
  Notes tab push page tabs out of it. *(On 2026-10-26 plugin-view memory
  moved to #138; see that date's entries.)*

- **Interface rules (decision 4): one `syrinx:plugin@0.1.0` WIT package.**
  Typed records everywhere except two named JSON channels (view messages
  and a plugin's own settings); plain ids for anything that persists;
  resources only for live host capabilities such as a terminal; no WIT
  async in 0.1.0; static facts (commands, view kinds, feeds, widgets,
  capability requests) in the manifest, read without running code; pull
  search feeds whose cursor the host owns; and the host loads the current
  and previous minor below 1.0. Why: typed calls are checked when a plugin
  links, so a mismatch fails at load, and a host-owned cursor means no
  plugin code runs on a keystroke (D48). Rejected (section 2): JSON or
  byte payloads everywhere, Extism's model, which gives up generated
  bindings and load-time checks; typed everywhere with no JSON at all;
  listing commands through exports after activation; and a pushed
  `submit(batch)` feed, whose plugin cannot know whether the index
  committed.

- **Palette forms and widgets (decision 5): "draws its interface in HTML"
  means plugin tabs.** Palette forms stay declarative and are drawn by
  GPUI; Home widgets start declarative and `@unstable` until #31. Why: a
  web view inside the palette would add a renderer and a second focus
  path to its open path (section 4, "Palette forms and Home widgets stay
  declarative"). This reads the 2026-10-15 entry's "Form" bullet and
  settles #29's open question.

- **Plugin views (decisions 6 and 7): a separate plugin-view type, and the
  guardrail says where it applies.** A plugin's interface runs in its own
  Rust web-view type, in the same WebView2 environment as pages but one
  separate InPrivate profile, served by wry's custom protocol from the
  verified package at a per-plugin origin under `.invalid`, with a strict
  Content Security Policy (`default-src 'none'` with named sources) the
  host sets, no navigation away and no network from interfaces even with a
  network grant. It talks only to its own module, through a message bridge
  Rust validates. #27 states in words that the custom-protocol filter and
  the message handler are allowed on the plugin-view type only and remain
  forbidden on page web views and Google's sites. Rejected (section 4,
  "Environment and profile" and the conflicts table): the pages' default
  profile, which puts a plugin origin beside Google's cookies; a separate
  environment, which adds a browser process against D6; one profile per
  plugin; and the looser `default-src 'self'` policy.

- **Plugin search content (decision 8): each plugin's feeds get their own
  palette scope chip** (for example "Notes", shown only while that plugin
  is installed) as well as appearing under Everything, matching the spec's
  scope chips. It is `design/README.md` territory, so it is also recorded
  there, under "Decisions made after the spec".

- **Grants (decisions 9 and 10): nothing granted by default, and running
  programs is named as full trust.** A plugin's `syrinx-plugin.toml` asks
  for typed, scoped capabilities, each with a one-sentence reason; the
  owner reviews them on an install sheet, grants optional ones at first
  use and can revoke any; a fixed deny list, including the profile root,
  overrides every grant; an update that widens a capability is held
  inactive while the old version keeps running; and removing a plugin
  deletes its grants, storage, index contributions and drafts. The host
  owns every risky service once for all plugins (safe saves, watching,
  indexing, and later terminals, git, language servers and Tree-sitter).
  Running a program is worded "Can run programs on your computer with
  your permissions"; Forge adds a per-project trust question with a
  read-only restricted mode; third-party plugins cannot get full-trust
  capabilities until signing and provenance exist. The vault and grant
  model Notes needs (a folder the owner picks in a system picker, with
  Windows containment) follows from this. Why: it is the Chrome model the
  owner named, and a sandbox cannot follow a process it starts. Rejected
  (section 3, "The split between host and plugin" and "How the owner
  sees, grants and revokes"): raw WASI folder access and process access
  with each plugin building its own safe save, watcher and language
  client; Zed's generic `process:exec` alone; and pausing a plugin whose
  update widens a capability, which would break a daily tool.

- **Packages, loading and the cache (decisions 11 to 13).** A plugin
  package is a folder with a signed canonical file list in the shape of
  Syrinx's update format (`docs/update-format.md`), signed with a P-256
  key per plugin repository held as an Actions secret, trusted on first
  use with announced rotation, and with an unsigned developer mode off by
  default. Built-in packages are bundled in Syrinx's signed release (a
  release-workflow change, approved here), the owner can "Install plugin
  from folder…", and Syrinx fetches no plugin over the network in v1.
  Compiled native code may be cached in the local data folder
  (`%LOCALAPPDATA%\Syrinx\data\PluginCache\`), never shipped in a
  package, keyed by the `plugin.wasm` hash, wasmtime's compatibility hash
  and Syrinx's version, and read with `deserialize` only after a hash
  check. Why: it reuses reviewed update-format code, adds no archive
  parser and no network path, and avoids compiling a large guest at every
  activation. Rejected (section 5): an archive format parsed by Syrinx;
  fetching from GitHub releases, a second host whose downloads redirect;
  compiling built-ins into `syrinx.exe` (already rejected on 2026-10-15);
  `deserialize_file`, which maps a file a later edit could reach; and
  compiling at every activation.

- **Plugin repositories and the template's CI (decision 14).** Plugin
  repositories are public, so GitHub's artifact attestations work on any
  plan, and the template's `ci.yml` (build, lint, contract tests against
  a published test host on synthetic data, licence and reproducibility
  checks) and `release.yml` (on a version tag: sign, attest and publish)
  are approved as described in the note's section 5, "The template's CI
  (#30)", with every third-party action pinned by full commit SHA.
  Rejected: private repositories, whose attestations need GitHub
  Enterprise Cloud, leaving only the package signature and
  reproducibility.

- **Notes' editor and data (decision 15).** The Notes and Forge editor is
  a host-provided CodeMirror component with no plugin JavaScript inside it
  in v1, because D28's checksum-matched save is a property Syrinx must own
  (section 4, "The Notes and Forge editor"). Rejected: each plugin bundling
  its own CodeMirror configuration. Recovery drafts live in local app data
  outside D13's snapshots (section 3). Two points are approved as
  proposals that the Notes slices test before they are final, because the
  note proposes them without a full argument: ⚠️ drafts are deleted once
  the save they protect succeeds or after at most seven days (#37 tests
  it), and ⚠️ note text in the tier-2 index is a rebuildable cache outside
  the snapshots, with no seven-day expiry because the files are the truth
  (#36 tests it). Until those tests report, neither is settled.

- **The owner's acceptance of Plan 2 moves to epic #25's exit (owner's
  own decision; supersedes the 2026-10-20 implementation note).** The
  acceptance lived in #34, the Fledge "New demo lead" slice, and gated the
  start of Notes (#35). With Fledge on hold, #34 is blocked, so the gate
  now sits in the plugin runtime epic's exit criteria: Notes starts once
  the owner accepts Plan 2 at #25's exit. Rejected: leaving the gate in
  #34, which would hold Notes behind Fledge's API.

- **The plugin template repository is MPL-2.0 (owner's own decision),**
  matching Syrinx's licence (2026-10-13), so code moves between Syrinx and
  plugins made from the template under one licence.

## 2026-10-20

- **The WebView2 update notice is for the owner and testing only.** Installed
  builds (`cargo xtask package`, which the weekly preview ships) never show
  D36's notice or its "could not read the WebView2 version" message; they
  still remember the runtime version, so nothing is lost if the notice
  returns. Development and test builds keep it for now. This narrows D36,
  which assumed the owner was the only person running Syrinx.
- **Each weekly release lists the engines underneath it.** The notes name the
  GPUI, gpui-component, wry and webview2-com versions from `Cargo.lock`,
  marking any that changed since the previous preview, and the WebView2
  runtime the release was gated and screenshotted on.

- **The Fledge plugin is on hold until Fledge itself is ready (holds the
  Fledge half of D34).** D34 kept Plan 2 thin: the plugin runtime plus one
  Fledge action, "New demo lead", on a stand-in client (an in-memory copy
  of the Fledge client interface that talks to nothing), so the real client
  could replace it after Fledge's Stage C (D24). The owner decided in chat
  that the Fledge epic (#32) and its two issues (#33, #34) wait until
  Fledge's own API is ready and the owner says to resume. The reason is
  that Fledge's API is still moving: a stand-in written now would encode
  guesses about leads, money and identifiers that the real API may not
  keep, so the plugin and its tests would be built against a moving target
  and then rebuilt. Rejected: keeping Fledge as the second plugin and
  building it on the stand-in anyway, as D34 and the 2026-10-15 order
  assumed, because the stand-in would drift from the real API and its
  proof of the runtime would come from a workload the owner does not use
  yet. D24, D23 and D26 are unchanged and apply when the work resumes.
  Implementation note, not part of the owner's decision: #34 carried the
  owner's acceptance of Plan 2, which the Notes epic (#35) named as its
  entry gate; with Fledge held, the runtime epic's (#25) own exit is the
  natural place for that acceptance, and that is the owner's to confirm.
  *(Superseded on 2026-10-21: the owner confirmed it; the acceptance is
  now in #25's exit criteria. See that date's entry.)*

- **The plugin order is now runtime, template, Notes, Forge (replaces the
  2026-10-15 order).** The plugin runtime (#25: interface, host, palette
  forms and plugin tabs) comes first, then the plugin template (#30), then
  Notes (#35), then Forge (#42). Forge starts only after Notes is complete,
  tested and live as the owner's daily notes tool; the owner's go-ahead
  for Forge still applies. Notes goes first because the owner will use it
  every day, so it proves the runtime and the shared document service
  (the CodeMirror bridge and safe save of D28, built in #37) on a real
  workload before Forge adds its much heavier host needs: a pseudoterminal
  for the shell, git (D29), language servers (D30) and tree-sitter.
  Forge reuses that document service, so a Notes that has survived daily
  use is a better foundation than one that has only passed its tests.
  Rejected: starting Forge in parallel with Notes, which would build
  Forge on a document service and host that daily use has not yet tested,
  and would split attention across the two largest plugins at once; and
  the 2026-10-15 order (template, Fledge, Notes, Forge), because Fledge is
  now on hold (entry above).

- **Plugin research (#26) and the plugin template (#30) may start now,
  alongside Plan 1's remaining work (narrows 2026-10-15's "no plugin work
  starts until Plan 1 closes").** The WASM research note (#26, comparing
  WebAssembly runtimes, the interface definition and host capabilities)
  and the template repository's contents (#30) are paper and out-of-repo
  work: the research is reading and writing, and the template lives in its
  own repository, which the owner creates. Neither changes Syrinx's
  product crates, so neither can destabilise Plan 1's remaining slices.
  The plugin host (#28, #29) is different: it adds the interface and host
  to Syrinx's own crates, so it still waits for Plan 1 to close and, in
  addition, for the owner to approve the research note *(approved on
  2026-10-21; see that date's entries)*. *(Superseded on 2026-10-26: the
  plugin host no longer waits for Plan 1 to close. See that date's
  entry.)* Rejected: holding
  all plugin work until Plan 1 closes, which would leave the research and
  template idle although they cannot affect Plan 1; and starting the host
  now too, which would change product crates while Plan 1 is still being
  finished and before the runtime choice is approved. Implementation note,
  not part of the owner's decision: the template's "done" condition (a
  minimal plugin that loads in Syrinx, #25) can only be met once the host
  exists, so template work that starts now stops short of loading.

## 2026-10-18

- **Syrinx takes find, history and zoom keys even over a focused page.**
  The owner decided on #21 that Ctrl F, Ctrl H, Ctrl +, Ctrl − and Ctrl 0 go
  to Syrinx even while a web page has the keyboard focus, as in Chrome and
  Edge, because find, history and zoom are browser commands and not page
  commands; that the existing test asserting these keys reach the page first
  is changed to the new rule; and that Esc closes History. Before this, a
  focused page kept these keys, so Ctrl F opened WebView2's own find bar and
  Ctrl H did nothing (#21). This replaces the 2026-10-12 entry "Pages get
  find, history, print and zoom keys first (P1-13)" for find, history and
  zoom only; that entry's print rule (Ctrl P stays the page's) still
  stands. Implementation note, not part of the owner's decision: the page's
  own use of these keys (for example Google Docs' find and replace on
  Ctrl F and Ctrl H) is a consequence nobody has measured; it is an open
  point for the owner's hand checks.

## 2026-10-16

- **Planned work lives in GitHub issues; `docs/` holds only the app and its
  operations.** The plans, slice notes, handoffs, prompts and review records
  had become build history, and the owner wanted `docs/` to describe Syrinx
  rather than how it was built. Plans 2–4 became four epics with one issue
  per phase (#25 plugin runtime, #32 Fledge, #35 Notes, #42 Forge), each
  listing its approved slices, because the 2026-10-15 WASM decision means
  the slice boundaries are re-cut anyway (#27). Plan 1's remaining code
  work has its own epic. Plan 1's "Decisions taken" moved, unchanged, to
  `docs/architecture-decisions.md`; contributor material that agents and
  tooling read moved to `docs/dev/`; the research notes became the wiki's
  Background pages; the removed files stay readable at commit `af3de53`,
  linked from the wiki's History page. Rejected: copying developer docs only
  to the wiki (agents load `LOCAL-DEV.md` and the traps from disk, and the
  vendor-patch exceptions cite their reviews by path) and keeping the
  records in a `docs/archive/` folder.
- **Labels stay simple.** GitHub's default labels plus the area labels
  `plugin-runtime`, `fledge`, `notes` and `forge`, and `blocked`. Decision
  issues use `question`. Epics are titled "Epic: …"; there are no
  milestones. The owner chose this over the Fledge set (`epic`, `decision`,
  sizes, priorities) because Syrinx is now mostly tuning and feature
  additions; the set may grow when feature work needs it, but not before.
- **Deferred-work markers cite issues.** A marker is written `TODO(#n)`, and
  `cargo xtask check` requires `#n` in `docs/dev/deferred-work.md`. This
  replaces the `DW-n` register in Plan 1 (owner rule 2026-10-11, same
  intent). Rejected: keeping DW numbers in their own file, which would keep
  two numbering schemes.
- **Acceptance checks name the Graphite and Paper presets plus one custom
  theme made in the theme editor,** replacing "all five palettes" in the
  plugin plans, because only two presets ship since 2026-10-15 and a custom
  theme catches hard-coded colours.
- **The WASM research note (#26) may start before Plan 1 closes.** It is
  reading and writing, not code; the plugin interface, host and template
  still wait for Plan 1's exit, as decided on 2026-10-15. *(The template
  part is superseded on 2026-10-20: the template may also start now; the
  interface and host still wait. See that entry.)* *(The host's wait for
  Plan 1 ended on 2026-10-26; see that date's entry.)*

## 2026-10-15

- **Themes are built by the owner in an editor; only Graphite and Paper are
  presets (replaces "all five ship" in `design/README.md`).** The owner's
  window check that day passed Graphite and Paper. Tint, Haze and Glass
  did not match the design, and the owner judged that the design itself
  does not make transparency look good. The development-only theme lab
  showed what Windows can draw. The owner then chose an editor over
  picking new fixed palettes:
  - The default is Graphite with the orange accent, fully opaque.
  - Paper is the other preset.
  - Everything else is a custom theme: mode, background colour, solid or
    gradient, material (none, Mica, Mica Alt or Acrylic), transparency,
    glass edges, grain and accent.
  - Several themes can be saved by name.
  - Text is kept at 4.5:1 or better, pages and the command palette stay
    opaque, and Reduce transparency still turns materials off.
  - A previously chosen Haze, Glass or Tint becomes a saved custom theme of
    that name, so no look is lost.
  - The owner also approved merging the page-card fix first, with Graphite
    as the default.
  - See `docs/theme-editor.md`.

- **Plugins are built from a template repository, each in its own
  repository, as sandboxed WebAssembly loaded at run time; no plugin work
  starts until Plan 1 closes (changes D17 and D34).** The owner wants a
  plugin template made first, then Forge, Notes and Fledge each built from
  it, the way Obsidian's sample-plugin template and Zed's extensions work.
  Decided in chat:
  - *Repositories.* A template repository on GitHub (marked as a template)
    and one repository per plugin made from it, rather than the
    `crates/plugin-*` workspace crates the plan and `AGENTS.md` describe.
    The owner creates the repositories (operator work); agents prepare
    their contents. Rejected: a template folder inside this repository.
  - *Order.* After Plan 1 closes: the plugin interface and host first, then
    the template, then each plugin. Rejected: starting the interface and
    template now, ahead of Plan 1. *(Superseded on 2026-10-20: the plugin
    order is now runtime, template, Notes, Forge, with Fledge on hold, and
    the research note and the template may start before Plan 1 closes;
    the host still waits. See that date's entries.)* *(The host's wait for
    Plan 1 ended on 2026-10-26; see that date's entry.)*
  - *Form.* Each plugin is Rust compiled to WebAssembly (WASM) and run by
    Syrinx in a sandbox (the Zed model). It reaches files, git, processes
    and the network only through host functions its manifest asks for and
    the owner grants (the Chrome extension model), and draws its interface
    in HTML and JavaScript inside Syrinx's own local web view, as Notes and
    Forge already planned for CodeMirror (D28). Rust state stays in the
    plugin's WASM module and Syrinx's core, keeping S3. Rejected: compiling
    built-ins into `syrinx.exe` (every plugin release would need a Syrinx
    release, which defeats separate repositories), Obsidian-style
    JavaScript plugins (no sandbox, state outside Rust) and native DLLs (no
    sandbox, and Rust has no stable binary interface).
  - *What changes.* D17's sandbox is no longer deferred until a third
    party's plugin exists: the built-ins use it from the start. D34's
    "`Plugin` trait" becomes a WASM interface definition (WIT) and host;
    plan 2 stays thin otherwise. The owner chose this after being told the
    cost: Forge needs the host to provide real capabilities
    (terminals, git, language servers, tree-sitter), as Zed's host does.
  - *Before building.* A research note compares the WASM runtime and
    interface choices (for example wasmtime and the component model,
    licences and maintenance, host-capability design, start-up and memory
    cost against D6) and is reviewed with the owner before Plan 2's slice
    breakdown. `AGENTS.md`'s layout and plugin guardrails are updated in
    that slice breakdown, not before.

- **Each platform's build uses that platform's own web engine: WebView2 on
  Windows, WKWebView on macOS. Syrinx bundles no Chromium.** The owner
  asked, in chat, for Disney+, Hulu, YouTube and Google's services to work
  without shipping Chromium, because a bundled Chromium (CEF, Electron) adds
  hundreds of megabytes and a new release every four to eight weeks
  ([`docs/research/01-shell-and-engine.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/01-shell-and-engine.md)), and because the owner wants
  Syrinx's own settings, not Chrome's. Both engines come with the operating
  system, so Syrinx ships neither, and both play protected video through a
  DRM module the system supplies: WebView2 carries Google's Widevine (L3,
  observed in note 07, section 2.1, and Disney+ passed in Phase 0), and
  WKWebView carries Apple's FairPlay. Both are driven from Rust through wry,
  so the tab model, palette, store and plugins stay one code base; only
  `syrinx-platform`'s engine and window code is written once per platform.
  This does not change D2: v1 is still Windows 11 only, and the macOS build
  starts after v1, behind a macOS spike run on a Mac by the owner. ⚠️
  Unverified until that spike: that Disney+, Hulu, YouTube rentals and
  Google sign-in accept a WKWebView browser that is not Safari. Rejected:
  CEF on every platform (one engine, but bundled Chromium, Google's
  embedded-framework sign-in block names CEF, and Widevine needs Google's
  licence and signing); a Firefox (Gecko) fork (no embedding API, dropped on
  2026-10-09). A possible third build for Widevine where neither system
  engine provides it, chiefly Linux, is a proposal, not a decision:
  [`docs/proposals/widevine-third-build.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/proposals/widevine-third-build.md).

- **The usage switch's approved note is restored (AN-02).** With the report
  preview, Save report… and Copy report built, Customize's Usage report note
  again reads the approved sentence, "Nothing is sent anywhere. You can
  view, export or delete the counts at any time.", replacing the shortened
  note the owner chose on 2026-10-14 until AN-02 landed. This restores the
  approved proposal (`docs/usage-report-privacy.md`); it is
  recorded here because the 2026-10-14 entry promised it.

- **Ctrl W on a focused sidebar row closes that row's live page (DW-63).**
  DW-61 took the rows' small buttons out of the Tab order, which left
  "Unload page" (closing a saved page's live tab while keeping its saved
  row) with no key: the owner had to press Enter to open the page, then
  Ctrl W. The owner chose, in chat, Ctrl W while a row has focus: it unloads
  a live saved page and closes an open tab, bound in the row's key context
  so the window's Ctrl W is unchanged whenever the page or another control,
  not a row, has focus. On a folder row or a saved page with no live tab it
  does nothing rather than fall through and close the active page of a
  different row (the agent's choice, approved in review). The rejected
  alternative was Backspace, which no row uses but which does not mean
  "close" anywhere else in Syrinx. See DW-63 in the plan's ["Deferred work"
  register](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/plans/Syrinx%20v1%20%E2%80%94%20Plan.md#deferred-work) and [`docs/p1-36-accessibility.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-36-accessibility.md), "DW-63".

## 2026-10-14

- **The weekly release workflow signs updates; the owner keeps no release
  key on a device (P1-39d, DW-30, now #55; replaces the 2026-10-13 key
  arrangement).**
  The 2026-10-13 approval had the owner create two offline keys, a primary on
  a hardware key and a backup stored separately. Walking through P1-39d, the
  owner declined to keep a key on any device, because a lost or destroyed
  device would strand every installed copy, and asked why updates do not
  simply trust GitHub, where the weekly builds already appear. Trusting
  GitHub alone was rejected: HTTPS already stops a network man-in-the-middle,
  but the signature exists to stop someone who breaks into the update source
  (the website, Vercel, the site-push token), and GitHub Releases of the
  private repository would need a token that can read the source (the
  reason recorded on 2026-10-13). Keeping the owner-held keys was the other
  alternative. The owner chose, in chat: the primary release key is held as
  an encrypted GitHub Actions secret and `.github/workflows/weekly-release.yml`
  signs each release and re-signs `latest` every week, which also removes
  the 60-day manual re-signing; the backup key is an encrypted file kept in
  the owner's password manager, not on a device. Both public keys are still
  pinned in `PRODUCTION_KEYS`. The accepted trade-off: whoever takes over
  the owner's GitHub account or the workflow can sign updates, while a
  break-in to the website or Vercel alone still cannot. The key must still be
  created outside every agent session, and no agent sees the private key or
  the secret's value. Building this is an agent slice that changes the
  release workflow; see DW-30 in the plan's ["Deferred work" register](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/plans/Syrinx%20v1%20%E2%80%94%20Plan.md#deferred-work).

- **Sidebar rows get their own right-click menu (DW-63 follow-up, now
  #58).** When choosing Ctrl W for DW-63 (recorded under 2026-10-15), the
  owner also asked for a per-row right-click menu holding Unload page and
  the row's other actions (Close tab, Remove saved page, Rename and Remove
  folder), each showing its key, so every row action is reachable by mouse
  as well as by keyboard. Today a right-click anywhere in the sidebar opens
  only the general ⋯ menu. See DW-63 in the plan's ["Deferred work" register](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/plans/Syrinx%20v1%20%E2%80%94%20Plan.md#deferred-work).

- **Syrinx ships as a setup program that installs and updates itself
  (P1-37).** Release 0.2.0 shipped a zip holding `syrinx.exe` and PowerShell
  scripts. Copied out of that zip on its own, the exe ran, but nothing was
  installed or registered. A downloaded zip's scripts are also refused by
  Windows unless unblocked first. The owner's requirement: what Syrinx ships
  must stand alone, set up the folders it needs where they belong on first
  run, and update the existing install when a newer one is run.
  - The owner chose `syrinx-setup.exe`, written in Rust, over two
    alternatives. An Inno Setup wizard would need the tool on the build
    runner and a second script language. A WiX MSI is the heaviest to
    author and needs a licence check.
  - The setup program carries the release package and installs it per user
    into `%LOCALAPPDATA%\Programs\Syrinx`. It upgrades in place with the same
    swap P1-39b's helper uses, keeping one previous version, and never
    touches `%LOCALAPPDATA%\Syrinx\data`.
  - Its checkbox "Register Syrinx as a browser" is ticked by default, as the
    owner chose. It writes only the per-user keys P1-37 already lists and
    never sets the default browser.
  - The weekly release publishes the setup program as the download, beside
    the zip.
  - Every executable now links the C runtime statically
    (`.cargo/config.toml`). 0.2.0's `syrinx.exe` imported `VCRUNTIME140.dll`,
    so Windows would refuse to start it on a machine without the Visual C++
    redistributable.
  - This replaces the "portable zip plus scripts" recommendation as the way
    people install Syrinx. The scripts remain the manual route. See
    `docs/installing.md`, "The setup program".

- **The focus ring shows only after keyboard input, decided by Syrinx's own
  keyboard-or-pointer flag (DW-62).** P1-36 drew the ring on any focus, so
  a chrome button clicked with the mouse kept a ring until focus moved on.
  GPUI's built-in `focus_visible` cannot fix that: it judges keyboard use
  from the keys GPUI itself receives, so it never sees the F6 or Tab that
  leaves a web page through WebView2, and the ring would vanish exactly
  when a keyboard user leaves a page; it also counts a bare mouse move as
  pointer use. P1-36 recommended accepting the ring after clicks for v1,
  since that errs toward keyboard users; the other alternative was a
  vendored GPUI patch exposing a way to mark keyboard use, which would be a
  further change to the pinned toolkit. The owner chose, in chat, neither:
  Syrinx keeps its own flag, set to pointer by a mouse press in any Syrinx
  window and to keyboard by any key Syrinx's windows receive and by every
  key a focused page hands over (F6, Tab out of the page and the other
  shortcuts), and draws the ring only in keyboard mode. This needs no GPUI
  change and keeps the ring visible after F6 from a page. See
  [`docs/p1-36-accessibility.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-36-accessibility.md), "DW-62".

- **GPUI's existing Windows network and remote-call features are approved,
  pinned to the crates that ask for them (DW-68; narrows D9's check, not
  D9).** D9 keeps typing on the machine, and P1-39c made WinHTTP in
  `syrinx-update-net` the one exception (2026-10-13, question 8).
  `cargo xtask check-deps` checked only what Syrinx's own crates declare, so
  a third-party crate could switch on another networking namespace and Cargo
  would unify it into the build every crate shares. DW-68 now reads the
  features Cargo actually resolves on every `windows`, `windows-sys` and
  `winapi` version, and found that GPUI's own stack already turns some on:
  - `Win32_Networking_WinSock` on the pinned `windows 0.62.2`, requested by
    `gpui-pre` and `gpui-pre-windows` 0.3.8;
  - on `windows 0.57.0`, `sysinfo 0.31.4`'s IpHelper, Ndis, NetManagement
    and WinSock features and `Win32_System_Rpc` (used by its WMI sensor
    reading), reached through GPUI's `zed-scap`;
  - on `windows-sys 0.61.2`, WinSock from mio, polling, quinn-udp, rustix,
    socket2, uds_windows and zbus, and IpHelper from zbus.

  The owner approved exactly those eight entries, each with its requesting
  crates and versions, in `THIRD_PARTY_NETWORK_FEATURES`
  (`crates/xtask/src/dependency_policy/resolved_windows.rs`); RPC was
  approved as the eighth after a review widened the namespaces checked.
  Any new requester, version, feature or namespace fails the check, and so
  does an entry that no longer matches or lists a crate that stopped asking,
  so the list cannot outlive its reason.

  What is enforced, exactly: because `windows 0.62.2` is the build
  `syrinx-platform` and `syrinx-app` also use, its WinSock bindings compile
  in those crates. The source scan in `crates/xtask/src/network_sources.rs`
  refuses `WinSock`, `NetworkManagement`, any `WSA`-prefixed name (as a
  token) and the `ws2_32`, `wsock32`, `mswsock` and `iphlpapi` libraries
  (in any letter case, with or without `.dll`) in every Syrinx crate except
  the xtask tooling, and WinHTTP in every crate but the update download.
  Manifests are checked twice. The declaration rule refuses a Syrinx crate
  that asks `windows` or `windows-sys` for a networking feature. A Syrinx
  crate asking `winapi` for a network module is refused by the
  resolved-feature check instead, because no approved entry names it.
  These are text and manifest checks, not a sandbox: a feature check cannot
  see a raw `#[link]` or `windows-link` extern at all, and the source scan
  sees one only when it names one of those libraries.

  `winapi 0.3.9` also resolves `inaddr` and `in6addr` (requested by
  `ntapi 0.4.3`). These modules define only the IPv4 and IPv6 address
  structs and declare no functions, like std's `Ipv4Addr`, so they are not
  on winapi's network list. The owner deferred this classification; it was
  the agent's call, recorded here so it can be revisited.

  Rejected: patching GPUI's vendored manifest to drop WinSock, a new vendor
  patch that would not reach `sysinfo` through `zed-scap` and risks GPUI's
  own socket use; and closing DW-68 as a known limit, which would leave a
  third-party crate free to add networking unseen.

- **AN-01 starts before P1-39d, an exception to the plan's order.** The
  approved analytics proposal put AN-01 after P1-39. P1-39d needs the
  owner's release keys and update host, which are not ready, while AN-01's
  local collector needs neither, so the owner told the session to start
  AN-01 now rather than wait. Only the order changes: AN-02 still precedes
  P1-40.
- **AN-01's frozen command list is approved as built.** The proposal made the
  owner's review of the 20 built-in command counters plus `command.other`
  (listed in [`docs/an-01-local-analytics.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/an-01-local-analytics.md)) a done condition; the owner
  approved them unchanged. A new built-in command still needs a new schema
  version and the owner's approval.
- **The usage switch's note is shortened until AN-02.** The approved note
  says the counts can be viewed, exported or deleted, but viewing and
  exporting arrive only with AN-02. Until then Customize says "Nothing is
  sent anywhere. You can delete the counts at any time."; AN-02 restores
  the approved sentence when it adds the preview and export. Showing a
  promise the build cannot keep was the rejected alternative.

- **Where Windows cannot list sync roots, Syrinx treats the machine as
  having none (narrows D13).** D13 keeps the database out of OneDrive and
  similar synced folders; the storage guard
  (`crates/syrinx-platform/src/storage/native.rs`, `registered_roots`) asks
  Windows for its registered sync roots through
  `StorageProviderSyncRootManager::GetCurrentSyncRoots`. On the Windows
  Server machines GitHub uses to build releases, that call jumps to an
  invalid address and crashes the process (weekly-release run 4 captured
  the native stack), so the release gate could never pass there. The owner
  chose, over building releases on their own PC, to have the check cope
  without that API. The first attempt asked
  `StorageProviderSyncRootManager::IsSupported()`, Microsoft's documented
  check, first; run 5 showed that call crashes the same way. Syrinx
  therefore asks Windows whether it is a server edition (`IsOS(OS_ANYSERVER)`
  in shlwapi, which makes no WinRT call) and, if so, treats the machine as
  having no registered sync roots. On Windows 11, Syrinx's only supported
  system (D2), nothing changes. On Windows Server, Syrinx no longer refuses
  a synced folder; the rejected alternative was to fail closed there, which
  would keep every storage test failing on the release runner.

## 2026-10-13

- **P1-39 update delivery is approved as proposed.** The owner answered yes
  to all nine questions in `docs/update-delivery.md`.
  Syrinx checks for updates only when the owner presses "Check for updates"
  in Settings or the palette, never at start-up or in the background;
  start-up checks, a periodic check, BITS background transfers and
  downloading through a WebView2 page were rejected, because each makes
  network calls the owner did not ask for or leaves a timeline of launches
  in the host's logs. Updates come from fixed HTTPS files under
  `https://syrinxapp.dev/updates/v1/`, compiled into the executable with no
  setting to redirect it; GitHub Releases of the private repository were
  rejected because Syrinx would have to ship a token that can also read the
  source. "Install update from file…" uses the same checks with no network.
  The owner is willing to publish signed Syrinx builds at that address
  before the licence is chosen, once P1-39b is done (the proposal's
  recommendation was "yes, once P1-39b is done; your call"). Syrinx accepts
  a release only through a signed latest-version statement that expires at
  most 60 days after it was issued, which the owner re-signs at each release
  and between releases, so a frozen host can hold Syrinx back for at most
  that window. An update is downloaded only after the owner confirms its
  version and size, and installed only through Restart now or the owner's
  next quit, with the previous version kept and restored if the new one does
  not start. After relaunching for an update, Syrinx does not force itself
  to the front. The owner creates two offline release keys, a primary on a
  hardware key and a backup stored separately, outside the repository and
  outside every agent session; both public keys are pinned (DW-30, now #55),
  and a separate statement-only key was deferred. `cargo xtask check-deps`
  changes only as question 8 lists: `syrinx-app` and `syrinx-update-net` may
  depend on the update verifier; `syrinx-update-net` may use the `windows`
  crate with its own exact feature list; and that crate alone may use
  `Win32_Networking_WinHttp`, the only exception to D9's no-network rule.
  `reqwest` with `rustls` was rejected to keep a TLS stack and `tokio` out
  of Syrinx's own crates. P1-39 is split into P1-39a to P1-39d as the
  proposal's slice table defines, with no other renumbering. The release
  keys remain the owner's to create, so production updates wait on DW-30
  (now #55).

- **Syrinx is on GitHub at `Fledge-Software/Syrinx-app`.** The owner created
  the repository under the Fledge Software organisation and added it as
  `origin`, and confirmed on 2026-10-13 that the remote is theirs. This
  replaces "local only, no remote and no pushes" in D41 and the 2026-10-12
  entry below. `main` still changes only through the reviewed `--no-ff`
  merge once `cargo xtask check` passes. This entry does not make the
  repository public, choose its licence or move outstanding work from the
  plan's registers into GitHub issues; each of those is still the owner's
  decision. An agent pushes only the branch its session was assigned, and
  pushes `main` or opens a pull request only when the owner asks; `CLAUDE.md`
  and `AGENTS.md` say the same.

- **The Syrinx logo is the hawk emblem in `design/brand/syrinx-logo.svg`.**
  It is used for the app icon, taskbar and Start entries, installer,
  favicon and website.
  - **Icons sit on a dark tile.** Every icon is the hawk on a near-black
    rounded square. The hawk's head is cream (#FFFDF7) and disappears on
    light backgrounds such as light taskbars, the Paper theme and light
    browser tabs. A transparent hawk is used only where the background is
    known to be dark.
  - **The default accent matches the logo's orange, #FF612E**, replacing
    #FF8A65. It stays customisable, and must still meet the 4.5:1 text
    contrast rule in all five palettes, with a per-palette adjustment only
    where needed.
  - **A saved #FF8A65 now means the default.** When settings load, a stored
    accent equal to the former default #FF8A65 is treated as the default,
    now #FF612E, so people who kept the old default follow the logo. Any
    other stored colour stays exactly as chosen.
  - **Haze and Glass lighten an accent only when it actually fails.** On
    those see-through palettes the accent's text-and-mark colour is lifted
    to a lighter tone of its own hue only if it fails 4.5:1 as text or 3:1
    as a mark there. An accent that already passes, such as the green
    (#6FD3A8) and gold (#E8C16A) presets, keeps its own colour.
  - **The executable's icon is compiled by the Windows SDK's `rc.exe`** from
    a small build script, with no new dependency. The `embed-resource`
    crate was the rejected alternative. The rasters and `.ico` are
    generated once from the SVG and committed. Building the project must
    not require a rasterizer.

## 2026-10-12

- **The product is named Syrinx.** "Atlas" was a working name. It collides
  with OpenAI's ChatGPT Atlas, a browser in the same category, and the name
  is crowded across developer tools. The syrinx is a bird's vocal organ,
  which fits beside Fledge. The owner holds the domain `syrinxapp.dev`.
  Code, crates, the data folder and Windows registration names still say
  Atlas until a dedicated rename slice changes them together. That slice
  must also handle the data folder (`%LOCALAPPDATA%\Atlas\data`) before the
  owner's first real install (P1-37). New work should not spread the old
  name further than it must. *(Done since: the crates are `syrinx-*`, and
  the installed app's data folder is `%LOCALAPPDATA%\Syrinx\data`, as the
  2026-10-14 setup-program entry names it.)*
- **Finished work merges to main; slices are built in local worktrees.**
  This replaces D41's routing through the integration branch
  `codex/integrated-browser-preview`, which left main at P1-04 while
  reviewed work piled up elsewhere. Each slice is built on its own branch
  in a local worktree. It merges to `main` with `--no-ff` once an
  independent review approves it and `cargo xtask check` passes after the
  merge. The owner directed all finished work to main on 2026-10-12. The
  repository stays local, with no remote and no pushes. Commits still
  happen only within this approved slice workflow.

- **Syrinx never takes the foreground from another application.** When the
  owner has moved to another program, Syrinx does not bring itself to the
  front: no `SetForegroundWindow`, no synthetic Alt press and no GPUI
  `activate_window`. A page that should take the keys while Syrinx is behind
  waits, and takes them when the owner returns to Syrinx; Ctrl K while
  Syrinx is not in front leaves the palette closed rather than float a popup
  over the other program. The review of DW-90 found that the palette's Esc
  fallback called GPUI's `activate_window`, which forces the foreground with
  a synthetic Alt key. Keeping that fallback was rejected: focus returning a
  little sooner does not justify Syrinx stealing the screen from whatever the
  owner is doing.

  There are two exceptions, both following the owner's own action. Launching
  Syrinx brings it to the front (`cx.activate(true)` in
  `crates/syrinx-app/src/main.rs`). A link the owner clicks in another program
  while Syrinx is their browser also brings it forward
  (`ui/incoming_links.rs`): the forwarding instance grants the foreground
  through `AllowSetForegroundWindow` (`crates/syrinx-platform/src/instance/pipe.rs`). Every
  other activation, including Find in page, F6 pane cycling and the bookmark
  import preview, activates natively only while Syrinx is already in front.

- **Palette ranking is judged from key press to rows on screen (P1-33).**
  A keystroke's ranked rows must be presented within two display refresh
  intervals at the 95th percentile (33.3 ms at 60 Hz), with the maximum
  always reported. D48's 5 ms limit still applies to the search itself and
  stays with P1-05. Two intervals is the smallest budget that allows for the
  frame wait: a reply that lands just after a frame must wait for the next.
  Judging only the search core was rejected because it hides the frame wait
  and the reply's trip back to the window.

- **"Opens within one frame" is judged at the 95th percentile (P1-33).**
  The palette's first presented frame must follow Ctrl K within one refresh
  interval at p95, with every sample and the maximum reported. This follows
  D48: a single Windows scheduling stall should not decide the verdict, and
  hiding the slow samples would conceal real responsiveness. Requiring every
  opening within one frame was rejected for that reason.

- **The palette popup is created once and then shown and hidden (DW-90).**
  P1-33 measured about half a second from Ctrl K to the palette's first
  frame. The cause is provisionally attributed to creating a new native
  window for every opening; the owner's foreground runs will confirm it. The
  owner approved keeping one owned, non-topmost popup hidden between uses.
  It is a separate slice after P1-33 merges, rerun through P1-33's harness.
  Keeping a new window per opening was rejected, because it cannot meet the
  one-frame target.

- **Page text skips every exit from an excluded site unless Syrinx started it
  (P1-25, "skip all exits").** A page reached by leaving an excluded document
  (Google, a bank or payment site, or a site the owner excluded) is never
  read, unless Syrinx itself started that navigation from the address bar,
  the palette, a bookmark, a saved page, a tile or Home. Links, page
  scripts, auto-submitting forms, popups, back, forward and reload all count
  as leaving. The taint carries on: a page reached that way passes it to the
  next page it leads to, until Syrinx navigates the tab, so a sign-in
  callback that moves on by script is not read either. A popup opened from
  an excluded or tainted page starts tainted. This replaces the
  user-initiated test in the entry below: WebView2's `IsUserInitiated`
  follows Chromium's short-lived user activation, so a "Continue" click on a
  bank page counted as the owner's own navigation and its next page was
  read. Keeping the user-initiated test was rejected for that reason. The
  form-`POST` rule below stands.

- **Page text skips sign-in hops (P1-25).** *(Its user-initiated test was
  replaced the same day by "skip all exits" above; the `POST` rule stands.)* A page reached by a navigation
  that an excluded site (Google, a bank or payment site, or a site the owner
  excluded) started by itself, through its own script or an auto-submitting
  form, is not read. Neither is any page produced by a form submission (a
  `POST`) on any site, including one that redirects to an ordinary page
  afterwards. A link the owner follows out of an excluded site still leads to
  a readable page if that page is otherwise eligible. Sign-in flows carry
  tokens and personal details from the excluded site to the next page this
  way, which the redirect rule alone did not catch. Syrinx tells these apart
  from WebView2's own `IsUserInitiated` flag and the request method it
  reports, with no request filter and no injected script. Reading such pages
  and relying only on the exclusion lists was rejected.

- **A split `no-store` header is an accepted limit (P1-25, F2).** When a site
  sends `Cache-Control` as several separate header lines, WebView2 shows
  Syrinx only the first, so a `no-store` on a later line is missed and the
  page can be read. The owner accepted this as a known limit rather than
  adding a request filter, which the Google guardrails forbid. A report to
  Microsoft is possible but is sent only with the owner's approval; nothing
  has been sent.

- **Page text uses the browser's `visibility` style (P1-25).** The snapshot
  asks WebView2 for each node's computed `visibility`, so text a page hides
  with `visibility: hidden` is left out, as it is for `display: none`. This
  departs from the proposal's empty style list; the owner approved it after
  the F1 test showed hidden text being kept.

- **A background tab finishes loading before it is put to sleep (P1-15).**
  The five-second countdown before a hidden page is suspended now starts
  when its navigation completes, not when it starts, so a slow page opened in
  the background is fully loaded when the owner switches to it. Suspending
  mid-load froze such pages half-loaded and was found as the cause of an
  intermittent native-test timeout. Keeping the earlier timing, which saves
  a little more memory, was rejected. A page that never finishes still
  sleeps after a bounded wait.

- **History is kept for 90 days (DW-4).** Older visits are deleted
  automatically through the same path as Clear browsing data, so they stay
  deleted after a backup is restored (D47). Ninety days matches Chrome: long
  enough for the palette to recall a page, short enough to keep the database
  small. The owner rejected keeping history forever, a year and 30 days.

- **The installed app keeps its data in `%LOCALAPPDATA%\Atlas\data` (P1-37).**
  *(Since the rename slice the folder is `%LOCALAPPDATA%\Syrinx\data`; see
  the naming entry above and the 2026-10-14 setup-program entry.)*
  Development and preview builds keep using `SyrinxProductPreview`, so test
  runs never mix with the owner's real browsing data. Nothing is moved or
  deleted between them; the installed app starts empty and bookmarks are
  imported again. Keeping the preview folder was rejected for that reason.

- **Page text is read with one DevTools snapshot, never a script (P1-25).**
  The owner approved `docs/page-text-indexing.md` as
  recommended. Syrinx captures an eligible page's visible text with a single
  `DOMSnapshot.captureSnapshot` call and reads only response headers, so
  nothing runs inside any page. Google's own sites (YouTube included), banks
  and payment sites, pages with password or card fields, pages not proven
  cacheable and redirect chains through an excluded site are never read. If
  testing shows a site can detect the capture, the work stops and returns to
  the owner. This keeps D35's no-injection rule absolute.

- **Local usage counting is approved, off until switched on (AN-01/02).**
  The owner approved `docs/usage-report-privacy.md` as
  recommended: the fixed version 1 schema, turning it off deletes the counts,
  30-day periods with two kept, never backed up or restored, and export only
  as a JSON file the owner saves or copies. AN-01 follows P1-39 and AN-02
  precedes P1-40. This implements D39 without uploads or identifiers.

- **Pages get find, history, print and zoom keys first (P1-13).** When a web
  page has focus, Ctrl F, Ctrl H, Ctrl P and the zoom keys go to the page
  first, as in Chrome. If the page doesn't use them, the engine's own find,
  print and zoom apply. Syrinx's own find and history stay reachable from
  Ctrl K and the toolbar. Grabbing them first broke Google Docs' in-document
  find and its Ctrl H find and replace. Ctrl T, W, L, K and Tab stay Syrinx's.
  *(Replaced on 2026-10-18 for find, history and zoom: see that entry.)*

- **Smart App Control is off for development.** Windows Application Control
  repeatedly blocked locally built, unsigned executables (os error 4551),
  breaking gate runs at random. The owner turned Smart App Control off on
  their machine. This was the owner's own action; agents still never change
  Windows security settings. Syrinx stays unsigned for now (D38), and a fresh
  Windows install may still block it.

- **The five palettes are approved (P1-07).** The owner reviewed the
  `--palette-sample` window against the design and approved it. The D44
  drag thresholds stay proposed until they can be tried in a draggable
  sidebar, because a static sample can't show how a drag feels.

- **Folder removal keeps saved pages by default (P1-21).** Removing a folder
  moves its contents up one level unless the owner explicitly chooses to
  delete them, and deleting contents asks for confirmation first. That way,
  tidying folders can never silently lose saved pages. Empty folder names
  are refused.
- **Bookmark import defaults (P1-22).** Exported toolbar folders import as
  ordinary folders, not as Favorites (D45 Favorites stay a deliberate
  choice). Only UTF-8 files are accepted; anything else is refused with a
  request to re-export, rather than guessing the encoding. The parser's
  default limits (32 MiB, 50,000 entries, 32 levels of nesting) stand.
- **`syrinx-core` may depend on `url` (DW-1).** So that import and navigation
  share one address rule instead of two copies, the core crate may take the
  `url` crate as its first outside dependency when the import step is built.
- **The recorded contrast shortfalls are accepted for now (P1-07).** Sixteen
  colour pairs in the canvas palettes miss WCAG minimums, mostly muted text
  and worst in Glass. The owner accepted them for now rather than changing
  the canvas. `syrinx-ui`'s test keeps the list fixed, so any new shortfall
  fails the gate.

## 2026-10-11

- **Plan 1 slices may run in parallel.** To speed up delivery, the owner
  approved working independent Plan 1 slices at the same time, each on its
  own branch with independent review. Plans 2–4 still wait for Plan 1's
  acceptance.
- **Deferred-work markers are allowed when tracked.** `TODO`, `FIXME` and
  `HACK` may appear in Syrinx code if they carry a `DW-n` tag listed in the
  plan's ["Deferred work" register](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/plans/Syrinx%20v1%20%E2%80%94%20Plan.md#deferred-work); each becomes an issue once on GitHub.
  `cargo xtask check` enforces this.
- **Two dependency approvals.** Vendoring `gpui-pre-windows 0.3.8` for the
  D37 idle patch, with its unchanged upstream TODO comments, and adding
  Tantivy for the full-text bake-off. `AGENTS.md` records both, with their
  limits.

## 2026-10-10

- **Build the integrated browser next (D49).** After finding that the
  morning's product libraries had no browser window, the owner explicitly
  approved bringing integration forward: connect the existing tab model,
  saved-page storage, backups and search to GPUI vertical tabs, WebView2
  navigation and Ctrl K. This replaces strict slice order for this preview,
  without dropping remaining features or performance requirements. The
  owner still approves commits and merges. The target is daily use of the
  full planned browser before Monday, 2026-10-12; that date is a goal, not a
  claim that incomplete or untested features are ready.
- **Local Syrinx must run unsigned for now (D38 clarification).** The owner
  has no signing certificate and explicitly requires development to continue
  without one. Signing remains deferred; it is not an application prerequisite.
  Keep Windows security settings unchanged and verify the actual application
  executable. A successful unsigned launch does not guarantee that Windows
  accepts every rebuilt application or development tool.
- **Search's 5 ms target uses the 95th percentile (D48).** The owner chose
  requiring 95% of repeated searches to finish within 5 ms, with the worst
  sample also reported. This accounts for occasional Windows scheduling
  interruptions without hiding them. Measure the complete 100,000-row
  ranking, scope-count and highlight path, not only the underlying matcher.
- **History clearing survives backup restoration (D47).** To keep deleted
  history from returning while preserving older recovery points, the owner
  chose a local deletion record containing only cleared date ranges and data
  categories, never URLs or page contents. Restoration must honor that
  record. This replaces the alternative of discarding all older backups
  whenever history is cleared.
- **Restoring a damaged database requires confirmation (D46).** Because an
  older backup can lose newer state, Syrinx offers the newest verified backup
  with its date and asks the owner before replacing the current database.
  Damaged files remain preserved locally. The owner chose confirmation rather
  than automatic restoration.
- **The top square tiles are Favorites (D45).** They provide quick access
  to websites, plugin pages and other supported Syrinx destinations, separate
  from inline saved pages and folders. A plugin Favorite uses the normal
  plugin open/focus lifecycle. Before that plugin exists, its tile keeps the
  not-yet-implemented indication rather than pretending to open it.
- **The sidebar can be resized by dragging (D44).** To give the page more
  room without losing navigation, dragging smoothly changes the sidebar
  from wide with labels, to favicon-only, to hidden; dragging back restores
  it. The existing hidden-sidebar hover peek and Ctrl Alt S remain. The
  owner's screenshot supplies direction, not a new image asset in the
  repository. Exact width thresholds and compact-folder controls will be
  reviewed with the later UI slice.
- **Pin means Save, with saved pages and folders inline (D43).** To keep
  useful destinations beside ordinary tabs, saving creates a persistent
  sidebar row. Closing it unloads only its live page; opening an unloaded
  row uses its saved address, while opening an already-live row focuses it.
  Live navigation does not rewrite the saved title or address. Remove or
  Unpin removes the saved row and leaves an open page as an ordinary tab.
  Saved rows precede ordinary tabs; saving appends to the saved group and
  removing an open saved page puts its tab at the start of ordinary tabs.
  HTML bookmark import preserves folder hierarchy in this same model and
  leaves imported pages unloaded. This replaces D42's temporary pin flag,
  while preserving its web-tab close/history rules. The owner approved
  the revised 40-slice map: flat saved-page lifecycle now, folder management
  and import in their existing later slices.
- **Tab transitions follow visible order and keep Syrinx open (D42,
  clarified by D43).** Closing the active page selects the next open page
  to its right, otherwise its left, skipping unloaded saved rows. Closing
  the last live page shows Home and leaves Syrinx open. New ordinary web tabs
  append and become active. Reordering stays within saved or ordinary groups.
  Keep the 20 most recent web-tab closures, including saved-page instances,
  not 100. Reopen selects the restored ordinary tab with its metadata and
  position, without live page or form contents. For a saved-page closure,
  reopen focuses its existing live instance or opens its fixed saved target
  if the row still exists. If the row was removed, history may reopen the
  page as ordinary but never recreates the saved record. Plugins reopen from
  their permanent rows and do not enter this history.
- **Keep the current WebView2 engine and SmartScreen.** The owner confirmed
  the engine works after reviewing Microsoft's required runtime diagnostics
  and SmartScreen collection. Disclose those runtime behaviors separately
  from Syrinx's own analytics; the custom-crash setting still keeps crash
  dumps local (D39).
- **Syrinx may measure its own use locally to guide improvements.** This
  replaces the earlier blanket "no telemetry" rule. The owner chose local
  aggregate counters and explicit report export: feature usage, command
  counts, timings and error categories, without account/device identifiers,
  URLs, searches, file paths, document text or a detailed event timeline.
  There are no automatic uploads. A model may suggest changes from a report
  the owner chooses to share; the owner reviews product changes, and Syrinx
  does not adapt its interface automatically. With one v1 user, removing
  identifiers cannot guarantee that a report's author is unknown by context.
  Collection and export need a reviewed schema and implementation breakdown
  before they are built; this is not permission to read or share real usage
  data during development (D39).
- **Seven days applies to retained page text, not just search visibility.**
  Keep the expiring page-text cache and its index outside Syrinx's seven daily
  durable-state backups. Remove expired content from active storage and
  prevent restart, replay or restore from reintroducing it. This covers
  Syrinx-managed retention, not forensic erasure or owner-managed backups
  (D40, clarifying D13/D18/D35).
- **The four plans and plan 1's 40-slice breakdown are approved.** The owner
  also approved local implementation: one branch per slice, independent
  review and the owner's approval before every merge. This supersedes D16's
  GitHub-before-implementation gate. The repository remains local with no
  remote or pushes; commits still require the owner's request. Preserve the
  planning baseline with an explicitly requested documentation commit before
  scaffolding. Implementation remains in phase order (D41).

## 2026-10-09

These entries depart from `docs/spec.md`. Each is argued in
`docs/architecture-decisions.md`, decision number in brackets.

- **v1 runs on Windows 11 only** (D2). No macOS or Linux work or testing in
  v1, so the Phase 0 verdict is for Windows, not per OS.
- **Pages render in WebView2, driven from Rust through wry, without Tauri**
  (D3). Servo is added later as a per-tab engine.
- **Syrinx's interface is drawn in Rust with GPUI, not Svelte** (D4), unless
  Phase 0's comparison with a Tauri + Svelte build trips the rule written
  in the plan.
- **Notes and Forge edit in CodeMirror 6 inside a local web view** (D28).
  CodeMirror owns the live buffer and undo while typing; Rust owns disk,
  saves, parsing, git and the language-server client.
- **Forge writes through the git command line, not libgit2** (D29), with
  fast reads in process. It requires Git for Windows.
- **Phase 0 runs locally before anything else is planned in detail** (D16).
- **Import is the browser's bookmarks export, not a CSV.** Edge, Chrome and
  Firefox export bookmarks only as the standard HTML bookmarks file, so
  Syrinx imports that file. v1 imports no history and no passwords; password
  import arrives with a password manager after v1. This replaces "Import is a
  CSV file only" below. Recorded in `docs/architecture-decisions.md` (D10,
  D14, D15).
- **Performance is the deciding goal for the engine and UI.** The owner wants
  Syrinx to be as efficient as possible: low memory per tab, fast start, low
  typing and scrolling latency, low idle CPU. TypeScript and Svelte are
  allowed. Syrinx does not have to be written entirely in Rust, but all state
  and logic still live in Rust, as the spec says.
- **The browser engine is open again.** The spec's direction (Tauri 2 with OS
  web views) is one option, alongside Servo, Chromium through CEF, and
  hybrids. The research in [`docs/research/`](https://github.com/Fledge-Software/Syrinx-app/tree/af3de53/docs/research) compares them. The choice is the
  first decision of the v1 plan and is made with the owner, not by default.
  The rule "embed an existing engine, don't build one" still holds.
- **Import is a CSV file only for v1.** Syrinx doesn't read other browsers'
  profiles. What the CSV holds (bookmarks, history or both) is still to be
  confirmed. *(Replaced the same day: see the bookmarks-export entry above.)*
- **Signing, SmartScreen and the Microsoft Store wait.** They come back only if
  Syrinx is productised. The Store's rule that browsers use the Chromium or
  Gecko engine does not constrain the engine choice now.
- **Syrinx will probably be open source, released on GitHub.** That brings a
  public repository and a licence choice when the repository moves to GitHub.
  It stays local until the owner says otherwise.
- **The next session asks the owner questions.** Planning v1 is a
  conversation: the engine, the UI approach, the v1 scope and anything else
  the owner hasn't decided are put to the owner in question rounds, never
  settled by picking a default.
- **A hidden sidebar peeks over the page on hover, everywhere in Syrinx.** See
  `design/README.md`.
- **A plugin is a tab that stays in the Plugins list when closed.** See
  `design/README.md`.
