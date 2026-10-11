# Traps to know

These are the mistakes that look fine, often pass the gate, and then fail
somewhere else. Read this before you start work and before you debug a build,
a test or a window. `AGENTS.md` holds the rules; this file holds the traps.

These traps come from research, the Phase 0 spikes and local implementation.
Each names its source and how sure it is. Update a trap when new evidence
confirms or disproves it. When a new trap costs you real time, add it in the same shape: one
bullet, symptom first, then the fix, then the source. Put the story of how
you found it in the commit message, not here.

## Page tabs: WebView2 through wry

- **A page closes its native window while its Atlas tab still looks open.**
  Published wry 0.57.0 destroys the child window directly on
  `WindowCloseRequested`. Atlas's approved two-file patch delegates this
  request to its session worker, which saves the logical close before native
  teardown. Keep the callback queue-only, handle duplicate/stale live IDs,
  and leave saved rows intact. Source: `docs/page-close-event-review.md`
  (source-confirmed; synthetic native delivery and deferred destruction
  passed). This does not establish the owner's breakpoint crash cause.

- **Browser shortcuts never reach Atlas while a page has focus.** Ctrl T,
  Ctrl W, Ctrl L and Ctrl K go to the page, and wry has no event for them
  (wry issue #451, open since 2021). Register WebView2's
  `AcceleratorKeyPressed` yourself on every page web view, through
  `WebViewExtWindows::controller()` and `webview2-com`. The handler runs
  synchronously and blocks the browser process, and cross-process COM calls
  inside it fail with `RPC_E_CANTCALLOUT_ININPUTSYNCCALL`, so it should only
  set `Handled` and post a message to Atlas's own event loop, ignoring
  auto-repeat through `PhysicalKeyStatus.WasKeyDown`. Plain typing never
  reaches the handler; Ctrl and Alt combinations, F-keys and Esc do.
  Source: `docs/research/08-gpui-and-wry-on-windows.md`, section 4 (high, from
  source and Microsoft's docs).
- **wry's default browser arguments switch SmartScreen off.** With no
  `with_additional_browser_args`, wry passes
  `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`, which
  removes phishing and malware protection from every tab. Always pass Atlas's
  own arguments, and never include `--user-agent` among them. Source: note
  08, section 5.1 (high, from source).
- **A custom user agent breaks Google instead of helping.** Toward Google's
  sites the default WebView2 presents itself as Google Chrome. Setting a user
  agent through the settings API (wry's `with_user_agent`) removes that and
  shows Google the "Microsoft Edge WebView2" brand. Leave the user agent
  alone. Source: `docs/research/07-google-sign-in-and-drm.md` (observed on
  runtime 154; high).
- **An IPC handler on a page web view adds `window.ipc`.** wry adds that
  global only when `with_ipc_handler` is set, and Google Docs and Sheets were
  reported to die on it because their own script declares `ipc`. Build page
  web views with neither an IPC handler nor an initialization script; then wry
  injects nothing. Source: note 07, "What this means", item 4 (one user's
  trace; medium) and note 08, section 5.1 (high).
- **Crash dumps go to Microsoft unless Atlas says otherwise.** wry builds its
  environment options internally and never sets
  `IsCustomCrashReportingEnabled`, so dumps follow Windows's diagnostic
  consent. Build the WebView2 environment yourself with `webview2-com`'s
  `CoreWebView2EnvironmentOptions` and pass it through `with_environment`.
  Source: note 08, section 5 (high).
- **Each tab silently gets its own environment.** Without
  `with_environment`, every `build` creates a new WebView2 environment.
  Create one environment once and pass it to every tab. A popup that becomes
  a tab must use the opener's environment and profile and must not have
  navigated, or `window.opener` breaks. Source: note 08, section 5 (high).
- **`TrySuspend` fails with `ERROR_INVALID_STATE`** if the web view is still
  visible. Call `set_visible(false)` first. Use either suspend and resume or
  the memory-usage level on a tab, not both. Source: note 08, section 5
  (high, from Microsoft's docs).
- **Child web views do not follow the window's size.** wry only resizes a
  web view it does not treat as a child, so the page card stays at its old
  size. Call `WebView::set_bounds` whenever the page card changes; bounds are
  logical units. Source: note 08, section 2.3 (medium, code-read only).

## The interface: GPUI on Windows

- **A hidden launch cannot establish visible-window readiness.** Fresh
  non-smoke Atlas processes started with `Start-Process -WindowStyle Hidden`
  remained alive without a targetable window in both execution contexts.
  The same frozen executable opened Home when launched normally for the
  owner's explicitly approved interactive test. Keep hidden launch for
  noninteractive checks, but use a visible launch for authorized window
  inspection. Source: continuation interactive-crash/desktop/visible launch
  records under `target/` (observed; not a breakpoint diagnosis).

- **A delayed Home snapshot reopens an already finished palette.** A new
  blank tab can reach the UI after its entry was submitted or dismissed.
  A boolean pending flag is insufficient: opening another saved page can
  rearm it for the old snapshot. Correlate each app-private Home action with
  its own committed acknowledgement, cancel superseded intent and consume
  matching no-op results too. Source: independent integrated-preview review
  of `ui/home_palette.rs` and the session worker (code-confirmed).

- **A smoke-test timeout on the GUI thread cannot bound a frozen GUI.** The
  earlier orderly timer also did nothing once shutdown began, so a missing
  worker-stop notification could hang the test. Keep an independent standard
  thread deadline for `--smoke` only, alongside orderly shutdown. A hard exit
  applies only to its freshly generated synthetic profile; it is not crash
  recovery or proof that a window is visible. Source: integrated-preview
  timeout review in `crates/atlas-app/src/main.rs` and `ui.rs` (code-confirmed).

- **GPUI draws above a WebView2 child window, not below it.** Note 08 read
  the code as the page covering GPUI; the Phase 0 spike found the opposite
  on this machine. Don't rely on either for layout: keep the page card's
  region free of GPUI content and put the palette and the sidebar peek in
  separate owned popup windows, which work. Source:
  `docs/research/09-phase-0-verdict.md` (observed in the spike; high).
- **GPUI's Windows backend repaints every window on every display refresh,**
  even when nothing changed, so an idle Atlas uses about 3.75% of a core.
  Patch the idle loop (D37) and measure idle CPU after any GPUI upgrade.
  Source: note 09 (measured; high).
- **Application Control can block locally built executables** (os error
  4551). Phase 0's debug builds were blocked and optimized builds ran. During
  P1-01, an optimized `xtask` was also blocked once; an unchanged retry passed
  the full gate without rebuilding or changing settings. The transient
  failure's cause is unproved. Use D38's profile, preserve failures, and never
  change Windows security settings to make a build run. Sources: note 09 and
  the P1-01 gate recorded in `docs/LOCAL-DEV.md` (observed; high). During
  P1-05, unchanged retries of optimized executables also remained blocked;
  do not assume retrying always resolves the policy.
- **Cargo leaves build helpers unoptimized even under a release profile.**
  D38's local profile therefore needs `build-override.opt-level = 3` as well
  as the application's release inheritance. Assertions and overflow checks
  still inherit the local profile. This corrects the configuration, but does
  not establish the cause of Application Control blocks or guarantee that
  Windows accepts every rebuilt executable. Sources: Cargo's profile defaults
  and P1-05's local build-helper investigation.
- **GPUI's popup window floats over every app.** `WindowKind::PopUp` is
  created `WS_EX_TOOLWINDOW | WS_EX_TOPMOST` and is not owned by the main
  window, so the palette would stay on top when Atlas loses focus. Set its
  owner with `SetWindowLongPtrW(GWLP_HWNDPARENT, main_hwnd)` and drop
  `WS_EX_TOPMOST` if ownership gives the stacking needed; whether GPUI
  tolerates the change is unverified. `WindowKind::Dialog` disables its
  parent, and `AnchoredPopup` is not implemented on Windows. Source: note 08,
  section 3.1 (high on what GPUI does; low on the fix).
- **GPUI's `Blurred` background is not the Windows 11 Acrylic.** It calls the
  undocumented `SetWindowCompositionAttribute` accent. For the transient
  Acrylic backdrop, create the popup `Transparent` and call
  `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE, 3)` on its window
  yourself. Blur is optional under D7. Source: note 08, section 3.2 (high).
- **Keys go to the page while GPUI still shows a caret.** Nothing in GPUI
  hands focus back to WebView2. After the palette closes, activate the main
  window, then call wry's `WebView::focus()`. Source: note 08, sections 2.3
  and 3.3 (medium, code-read only).
- **A caret version requirement on GPUI breaks the build.** Snapshot releases
  come weekly and may change the API; a `^` requirement has already moved apps
  onto a snapshot their components did not compile with. Pin with `=`.
  Source: note 08, section 1.2 (high).

## Local search

- **A NUL query can match padding beyond a field's text.** In the pinned
  frizbee 0.13.0 matcher, synthetic `\0` queries returned highlight indices
  equal to the field's byte length. Both the exhaustive and ASCII-filtered
  catalogue paths reproduced the invalid range. The approved three-file
  vendor correction now masks invalid ASCII lanes; both original regressions,
  public boundary tests and all eight available backend variants pass. Keep
  range validation and the regressions, including zero-score membership.
  Source: `docs/p1-05-frizbee-maintenance-review.md` and
  `target/continuation-frizbee-desktop-gate.log` (verified on this machine).
- **A valid frizbee match can have no highlight positions.** Its candidate
  filter and best-score traceback are separate; the latter can exhaust the
  typo budget before visiting a matching character. For an accepted field
  with an empty trace, retry only highlighting without that trace budget,
  retaining the original membership and score. Never use the retry to admit
  a rejected field. Zero-score and long greedy matches can still lack a
  trace; preserve them without inventing highlights. P1-05's synthetic
  long-near-match benchmark exposed this in the audited 0.13.0 source.
- **Different string wrapper types multiply frizbee's compiled kernels.**
  Pass `&str` consistently to the generic score, batch and highlighting APIs,
  including test references. Both `match_one` and `match_one_indices` accept
  generic wrappers; passing `&String` adds specializations alongside
  the library's internal `&str` path, across SIMD backends and matching
  modes. P1-05's first optimized build spent several minutes in code
  generation. Source inspection proves the duplicate instantiations; the
  compile-time improvement from normalizing calls is not yet measured. Two
  later score-reference tests repeated this trap: their optimized compiler
  run was stopped after more than ten CPU minutes and restarted with `&str`.
- **frizbee's highlight positions can point into UTF-8 continuation bytes.**
  They arrive in reverse byte order, not character order. Widen to complete
  Unicode scalars, sort and merge before slicing original display strings.
  P1-05 checks accented, emoji and distant multibyte positions against the
  audited 0.13.0 source; this does not promise grapheme-cluster boundaries.

## Storage

- **Read-only inspection can refuse a rollback before reporting corruption.**
  A primary accompanied by a rollback journal can return
  `SQLITE_READONLY_ROLLBACK` (776) during schema inspection. That error alone
  does not prove damage: do not turn ordinary read-only or I/O failures into
  permission to restore a backup. P1-04's synthetic sidecar-preservation
  tests exposed this before its recovery gate passed.
- **SQLite inspection and DELETE-capable Windows handles cannot overlap.**
  The pinned Windows VFS shares reads and writes, but not deletion. Validate
  with read-only guards, close them, reacquire movable handles and compare
  identities before renaming. Close movable handles before validating the
  installed copy. Source: the pinned SQLite Windows open path and P1-04's
  native recovery implementation.

- **SQLite's write-ahead log breaks in a synced folder.** In OneDrive or a
  network folder the `-wal` file separates from the database. Keep the
  database in the local, not roaming, app-data folder. Source:
  `docs/research/03-core-architecture.md`, "Speed and safety settings", and
  D13 in the plan (high).
- **A storage lease's full revalidation is not a cheap row check.** The
  Windows guard opens ancestor handles and enumerates registered sync roots
  on a dedicated WinRT thread. Do this when opening connections and at
  lifecycle boundaries, not for every tab read or save. Retained handles
  preserve the checked identities; later sync registration remains the
  documented limitation. Source: P1-03 source review and
  `docs/storage-path-preflight.md` (high on the work performed; no timing
  claim).
- **Holding sidecar handles can prevent SQLite's normal cleanup.** Refuse
  unsafe `atlas.db-wal`, `-shm` and `-journal` entries before opening SQLite,
  then release their inspection handles. Keep the directory/main-database
  lease through connection close, rather than holding sidecars without delete
  sharing. Source: P1-03 design review and SQLite's WAL lifecycle, linked in
  `docs/storage-path-preflight.md` (high on the lifecycle contract).
- **A native long-path check can pass while SQLite still refuses the open.**
  The MSVC build does not automatically add Windows' extended DOS prefix.
  The platform lease supplies it when the database or longest sidecar needs
  it, and every store connection selects the locking `win32-longpath` VFS
  (SQLite's file-access layer). That VFS also avoids the ordinary VFS's
  smaller pathname buffer. Keep ordinary DOS spelling for shorter paths.
  P1-03 also exposed SQLite 3.53.2's extended-path locking regression;
  P1-04's approved 3.53.4 source patch includes its upstream fix, without
  removing the need for the prefix or long-path VFS. Sources: P1-03's failing
  native/SQLite integration test and the source reviews recorded in
  `docs/dependency-audit.md` and `docs/p1-04-sqlite-maintenance-review.md`
  (high on source behavior; no performance remeasurement claimed).

## Repository and research

- **Files on disk have CRLF line endings; git stores LF.** This machine's
  system git config sets `core.autocrlf=true` and `.gitattributes` sets
  `* text=auto`, so `git ls-files --eol` shows `i/lf w/crlf` for most text
  files. A test that compares or hashes a fixture byte for byte, or reads one
  with `include_str!`, sees `\r\n` here and `\n` elsewhere. Normalise line
  endings before comparing or hashing, or give such fixtures `eol=lf` in
  `.gitattributes`. The same applies to D28's save check: both copies must be
  hashed with the same line endings. Source: observed on this machine on
  2026-10-09 (high).
- **The earlier research notes are partly out of date.** Notes 02 and 06 say
  WebView2 has no Widevine, which is wrong for the runtime on this machine,
  and note 03's layout and boundary assume Tauri, which D3 and D4 replaced.
  Read notes 07 and 08 and the plan's "What we found" before relying on an
  earlier note. Source: `docs/plans/Atlas v1 — Plan.md`, "What we found"
  (high).
