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

- **Certificate failure is an expected navigation failure, not successful loading.**
  WebView2 can emit failed NavigationCompleted before ServerCertificateErrorDetected.
  P1-14 explicitly cancels and reads back the action; keep rejection sticky through
  completion and match navigation IDs before accepting success. Its negative fixture
  accepts only its own expected failure, without changing shared fixture checks.
  Cancellation may retain the old document, so a source-change assertion needs a
  distinct recovery address. An initial already-blank view need not change source.
  See `docs/p1-14-security-display.md` and its native logs.
- **The local SslStream server could not use its ephemeral certificate key.**
  P1-14's initial helper failed with “No credentials are available in the security
  package.” Do not fix this by persisting keys outside the fixture or installing
  a trusted certificate. The installed Python standard library serves the same
  fresh untrusted certificate from files confined to the synthetic target folder.

- **An empty native Find.Start can leave its completion pending.** The P1-13
  fixture on this runtime completed a nonempty two-match search, but an empty
  term never completed. Atlas opens its own find input without calling Start;
  clearing calls Stop. Query IDs must change on both edits and bar reopen, since
  native cancellation cannot retract an event already queued to the UI. Moving
  logical GPUI focus alone is insufficient: transfer Windows focus from the page
  child to the chrome parent before typing. See `docs/p1-13-essentials.md`.
- **Do not build an example while its executable is running.** On Windows,
  the gate also links examples. Running the native fixture concurrently caused
  LNK1104 against its own executable. Serialize the native fixture and gate;
  do not kill another session's processes or relocate executables.

- **A consent click grants a replacement prompt.** Capture the displayed
  tab/request identity in each callback and give each prompt's buttons distinct
  element IDs. Reject a delayed click after navigation, tab change or a new
  request. Also disable WebView2 permission persistence with `SavesInProfile`
  false; otherwise a one-shot choice becomes durable outside Atlas's store.
  Source: P1-12 independent review, stale-action regression and native fixture
  in `docs/p1-12-permissions.md`.

- **The original lifecycle fixture sometimes times out during navigation.**
  Its fixture-navigation wait has failed before popups and during repeated
  cycles. P1-12's run passed, which does not establish a fix. Preserve its
  assertions and deadlines, record the individual suite result, and keep the
  original timeout separate from new slice failures. The owner explicitly
  authorized P1-12 through P1-14 despite this pre-existing intermittent failure.
  Sources: `docs/fixture-transport-timing-review.md` and `docs/p1-11-popups.md`.

- **A native fixture rejects popup focus before trying it.** Windows may
  deny `SetForegroundWindow` for a background process even though WebView2's
  `MoveFocus` succeeds. Keep foreground denial diagnostic and require the
  synthetic child to report `document.hasFocus()` after the production focus
  command. P1-11's 2026-10-12 run passed that assertion with foreground
  activation denied. This proves page focus, not GPUI visual acceptance.
  Sources: `docs/p1-11-popups.md` and Microsoft's
  [foreground activation rules](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow)
  (documented restriction and native observation).

- **A synthetic Windows HTTP server drops a connection before request bytes
  arrive.** An accepted socket inherits the listener's nonblocking mode;
  setting read/write timeouts does not make it blocking. Explicitly set each
  accepted stream to blocking mode before the bounded I/O. Otherwise an
  immediate `WouldBlock` can be mistaken for an idle timeout, and logging
  before the read can conceal the defect by changing timing. The close-page
  fixture regression proves `WouldBlock`/10035 before configuration and
  `TimedOut`/10060 after it. Source:
  `docs/parallel-browser-navigation-review-2026-10-10.md` (source and native
  socket test confirmed; not the original WebView2 breakpoint diagnosis).

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

- **A queued palette refocus can discard the Enter or Escape behind it.**
  The popup may remove its native window before the browser drains Ctrl L/K.
  Skip that obsolete popup-origin refocus without clearing its retained
  identity; the queued completion still needs that identity to pass the
  stale-popup guard. Keep main-window stale-handle recovery unchanged.
  Source: integrated-preview shortcut review and the app's routing regression
  (source-confirmed; native timing acceptance remains separate).

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

- **A retained directory or database can still be renamed despite excluding
  delete sharing.** Windows metadata-only opens do not enforce this sharing
  restriction. Retained directories need `FILE_LIST_DIRECTORY`, and retained
  existing primary files need `FILE_READ_DATA`; keep identity probes
  metadata-only so exclusive lifecycle/maintenance handles remain inspectable.
  Test empty directories and existing files as well as freshly created files,
  then verify rename succeeds after release. Source:
  `docs/missing-primary-startup-review.md`, native validation follow-up
  (both original rename assertions pass after correction, with 60 focused
  storage tests and the 436-test normal-desktop gate passing).

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

- **Windows PowerShell can replace Unicode in a native-program pipeline.**
  An edit piped to Python lost its middle-dot separator, so a replacement did
  not match and a P1-14 label regression failed. Use direct patches for Unicode
  text, or ASCII script literals with Unicode escapes; UTF-8 file reads alone
  do not control the shell's pipe encoding.
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
