# Traps to know

These are the mistakes that look fine, often pass the gate, and then fail
somewhere else. Read this before you start work and before you debug a build,
a test or a window. `AGENTS.md` holds the rules; this file holds the traps.

These traps come from research, the Phase 0 spikes and local implementation.
Each names its source and how sure it is. Update a trap when new evidence
confirms or disproves it. When a new trap costs you real time, add it in the same shape: one
bullet, symptom first, then the fix, then the source. Put the story of how
you found it in the commit message, not here. A `DW-n` name below is an
item of Plan 1's old deferred-work register, read at `af3de53`; open
deferred work now has a GitHub issue (`docs/dev/deferred-work.md`).

## Page tabs: WebView2 through wry

- **Under a restricted token, WebView2's GPU process stops on a breakpoint.**
  Run smoke checks and native fixtures from an ordinary desktop process, and
  do not read such a dump as a product crash. Started through a launcher that
  gives Syrinx this restricted token (the restricting SIDs in
  `crates/syrinx-platform/examples/restricted_token/repro.ps1`), every
  `--smoke` run left two `0x80000003` (`STATUS_BREAKPOINT`) dumps from
  `msedgewebview2.exe --type=gpu-process` at the same offset in its
  synthetic profile's `Crashpad\reports`, and exited 1 (5 of 5 runs, runtime
  155.0.4283.62). A normal launch (3 of 3) and a limited-user (LUA) token
  (3 of 3) stayed clean. With `--disable-gpu` (one run, through
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`) no dump appeared. Suspected, not
  shown: that the restricted agent executor behind the 2026-10-10 dialog
  gave Syrinx this token, and that the cause is the runtime's handling of
  it rather than Syrinx. Reproduce with that script (see
  `docs/dev/LOCAL-DEV.md`). Source: #59; observed for the scripted token on
  2026-10-18; the executor link and the cause are inferred.
- **A repeated response header shows only its first line.** A synthetic page
  sent `Cache-Control: max-age=0` and then `Cache-Control: no-store`; both
  `GetHeaders` and the full `GetIterator` of `WebResourceResponseReceived`
  reported only `max-age=0`, so Syrinx captured a `no-store` page (F2). Commas
  inside one line are split correctly. No WebView2 route Syrinx may use exposes
  the later line, and a request filter to see it is forbidden. The owner
  accepted this on 2026-10-12 as a known limit of D35's `no-store` rule
  (`docs/decisions.md`): do not read a missing `no-store` as proof that a
  page allows storing, and do not work around it. A report to Microsoft goes
  only with the owner's approval; none has been sent. Source:
  [`docs/p1-25-page-text.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-25-page-text.md), F2 (observed on this runtime).
- **A sign-in hop is not a redirect, and `IsUserInitiated` is not the
  owner's intent.** A bank or Google page that sends the tab on by script, a
  form or a link raises a fresh `NavigationStarting` with `IsRedirected`
  false, so a redirect-chain check misses it. `IsUserInitiated` follows
  Chromium's short-lived user activation: a click (or a posted Enter) on the
  bank page makes the next navigation "user-initiated". Recognise Syrinx's own
  navigations by the address it asked for, read `Source()` at the start as
  the document being left, carry the taint forward, and match a redirect's
  response to any hop of the navigation, since it can arrive after the next
  hop starts. Source: [`docs/p1-25-page-text.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-25-page-text.md), F21–F26 (observed).
- **The host cannot tell `pushState` from `replaceState`.** Both raise
  `SourceChanged` with `IsNewDocument` false and then `HistoryChanged`, with the
  same `CanGoBack` and `CanGoForward`, and whether a `pushState` adds an entry
  depends on a user gesture the host never sees. Chromium also skips, on Back
  and Forward, a page that moved on without a user gesture. Syrinx's per-tab
  back list (#126) therefore replaces the current row on a same-page change,
  hides pages left with `IsUserInitiated` false, and resets when a jump lands
  elsewhere; do not "fix" it with a DevTools history call (owner, 2026-10-28).
  Source: `crates/syrinx-platform/examples/navigation_history_spike.rs` and its
  results on #126 (observed on runtime 155, three runs).
- **Several `GoBack` calls in a row make one jump, not several loads.** The
  engine counts each call from the pending entry, so N back-to-back calls commit
  only the entry N steps away; the intermediate navigations start and are
  cancelled (`IsSuccess` false) without committing a document, though a
  `no-store` page may still get one request. Syrinx jumps this way
  (`BrowserCommand::GoToOffset`); do not step one call per committed load.
  Source: the same spike (observed on runtime 155, three runs).
- **An adopted popup's first document has no matching response event.** On
  this runtime the popup's page was judged `NoResponse`, so popups fail
  closed and are never read (F27). Do not "fix" this by assuming headers;
  find an observable response first. Source: [`docs/p1-25-page-text.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-25-page-text.md), F27
  (observed).
- **A reload or same-address visit raises no `SourceChanged`.** Only
  `NavigationStarting` and `NavigationCompleted` arrive, so code that learns the
  document address from `SourceChanged` sees no document after a reload. P1-25
  takes the navigation's last requested address on successful completion.
  Source: [`docs/p1-25-page-text.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-25-page-text.md), F14 (observed).
- **`DOMSnapshot.captureSnapshot` keeps `visibility: hidden` text and sends -1 for
  an empty attribute value.** Without the `visibility` computed style, hidden
  text is laid out and returned (F1); `contenteditable=""` arrives with value
  index -1, which a strict string lookup rejects as malformed (F10). Request
  only `visibility` and read -1 as the empty string. Source:
  [`docs/p1-25-page-text.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-25-page-text.md) (observed).

- **A recreated controller can reuse an old permission request number.** A
  delayed session-worker reply could then match a new prompt on the same live
  tab. P1-16 allocates request IDs across controller lifetimes and clears retired
  prompts on recovery. Keep both protections when changing controller ownership.
  Source: [`docs/p1-16-process-recovery.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-16-process-recovery.md) (code review and identity regression).

- **Certificate failure is an expected navigation failure, not successful loading.**
  WebView2 can emit failed NavigationCompleted before ServerCertificateErrorDetected.
  P1-14 explicitly cancels and reads back the action; keep rejection sticky through
  completion and match navigation IDs before accepting success. Its negative fixture
  accepts only its own expected failure, without changing shared fixture checks.
  Cancellation may retain the old document, so a source-change assertion needs a
  distinct recovery address. An initial already-blank view need not change source.
  See [`docs/p1-14-security-display.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-14-security-display.md) and its native logs.
- **The local SslStream server could not use its ephemeral certificate key.**
  P1-14's initial helper failed with “No credentials are available in the security
  package.” Do not fix this by persisting keys outside the fixture or installing
  a trusted certificate. The installed Python standard library serves the same
  fresh untrusted certificate from files confined to the synthetic target folder.

- **An empty native Find.Start can leave its completion pending.** The P1-13
  fixture on this runtime completed a nonempty two-match search, but an empty
  term never completed. Syrinx opens its own find input without calling Start;
  clearing calls Stop. Query IDs must change on both edits and bar reopen, since
  native cancellation cannot retract an event already queued to the UI. Moving
  logical GPUI focus alone is insufficient: transfer Windows focus from the page
  child to the chrome parent before typing. See [`docs/p1-13-essentials.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-13-essentials.md).
- **Do not build an example while its executable is running.** On Windows,
  the gate also links examples. Running the native fixture concurrently caused
  LNK1104 against its own executable. Serialize the native fixture and gate;
  do not kill another session's processes or relocate executables.

- **A consent click grants a replacement prompt.** Capture the displayed
  tab/request identity in each callback and give each prompt's buttons distinct
  element IDs. Reject a delayed click after navigation, tab change or a new
  request. Also disable WebView2 permission persistence with `SavesInProfile`
  false; otherwise a one-shot choice becomes durable outside Syrinx's store.
  Source: P1-12 independent review, stale-action regression and native fixture
  in [`docs/p1-12-permissions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-12-permissions.md).

- **The native fixture's lifecycle, permission or certificate suite sometimes timed out.**
  Diagnosed on 2026-10-12: its hidden close page was suspended mid-load (see
  the `TrySuspend` trap below). A passing run never established a fix; a
  deliberately held response did. Preserve the suite's assertions and
  deadlines. The permission and certificate suites' bare timeouts were
  diagnosed the same way on 2026-10-13: the old timer froze tabs 2003 and
  4000, which those suites never show, mid-load. A never-shown page is still
  suspended five seconds after its load completes, so show any fixture page
  whose script or events must arrive later than that. A refused or reset
  HTTPS connection now fails the certificate suite about 5 s after its
  `Failed` state, naming the attempt, the last connection state and the
  path of that suite's `server.log` (DW-64); read that log for a traceback
  or an early stop. A bare deadline during a rejection wait now means the
  attempt never reported `Failed` (under suspension it stayed
  `Https/Loading`), so suspect suspension first there. A failing run keeps
  every synthetic directory and lists them; a fully passing run deletes them (DW-65), so evidence exists
  only for failures. Sources: [`docs/p1-11-popups.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-11-popups.md),
  [`docs/fixture-transport-timing-review.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/fixture-transport-timing-review.md) and
  [`docs/fixture-timeout-diagnosis-plan.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/fixture-timeout-diagnosis-plan.md) (observed; high on the mechanism,
  inferred for the untraced original failures).

- **A native fixture rejects popup focus before trying it.** Windows may
  deny `SetForegroundWindow` for a background process even though WebView2's
  `MoveFocus` succeeds. Keep foreground denial diagnostic and require the
  synthetic child to report `document.hasFocus()` after the production focus
  command. P1-11's 2026-10-12 run passed that assertion with foreground
  activation denied. This proves page focus, not GPUI visual acceptance.
  Sources: [`docs/p1-11-popups.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-11-popups.md) and Microsoft's
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
  [`docs/parallel-browser-navigation-review-2026-10-10.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/parallel-browser-navigation-review-2026-10-10.md) (source and native
  socket test confirmed; not the original WebView2 breakpoint diagnosis).

- **A page closes its native window while its Syrinx tab still looks open.**
  Published wry 0.57.0 destroys the child window directly on
  `WindowCloseRequested`. Syrinx's approved two-file patch delegates this
  request to its session worker, which saves the logical close before native
  teardown. Keep the callback queue-only, handle duplicate/stale live IDs,
  and leave saved rows intact. Source: `docs/dev/vendor-reviews/page-close-event-review.md`
  (source-confirmed; synthetic native delivery and deferred destruction
  passed). This does not establish the owner's breakpoint crash cause.

- **Browser shortcuts never reach Syrinx while a page has focus.** Ctrl T,
  Ctrl W, Ctrl L and Ctrl K go to the page, and wry has no event for them
  (wry issue #451, open since 2021). Register WebView2's
  `AcceleratorKeyPressed` yourself on every page web view, through
  `WebViewExtWindows::controller()` and `webview2-com`. The handler runs
  synchronously and blocks the browser process, and cross-process COM calls
  inside it fail with `RPC_E_CANTCALLOUT_ININPUTSYNCCALL`, so it should only
  set `Handled` and post a message to Syrinx's own event loop, ignoring
  auto-repeat through `PhysicalKeyStatus.WasKeyDown`. Plain typing never
  reaches the handler; Ctrl and Alt combinations, F-keys and Esc do.
  Source: [`docs/research/08-gpui-and-wry-on-windows.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/08-gpui-and-wry-on-windows.md), section 4 (high, from
  source and Microsoft's docs).
- **wry's default browser arguments switch SmartScreen off.** With no
  `with_additional_browser_args`, wry passes
  `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`, which
  removes phishing and malware protection from every tab. Always pass Syrinx's
  own arguments, and never include `--user-agent` among them. Source: note
  08, section 5.1 (high, from source).
- **A custom user agent breaks Google instead of helping.** Toward Google's
  sites the default WebView2 presents itself as Google Chrome. Setting a user
  agent through the settings API (wry's `with_user_agent`) removes that and
  shows Google the "Microsoft Edge WebView2" brand. Leave the user agent
  alone. Source: [`docs/research/07-google-sign-in-and-drm.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/07-google-sign-in-and-drm.md) (observed on
  runtime 154; high).
- **An IPC handler on a page web view adds `window.ipc`.** wry adds that
  global only when `with_ipc_handler` is set, and Google Docs and Sheets were
  reported to die on it because their own script declares `ipc`. Build page
  web views with neither an IPC handler nor an initialization script; then wry
  injects nothing. Source: note 07, "What this means", item 4 (one user's
  trace; medium) and note 08, section 5.1 (high).
- **Crash dumps go to Microsoft unless Syrinx says otherwise.** wry builds its
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
- **`TrySuspend` succeeds on a page that is still loading, and the frozen
  page then reports nothing.** No `NavigationCompleted` or close request
  arrives until the page is resumed, and no error says why. Syrinx therefore
  starts a hidden page's suspension countdown only when its navigation
  completes (`browser/hidden_timer.rs`, owner decision 2026-10-12); keep it
  that way, and never time suspension from when a load starts. Separately, a
  hidden page's script timers run late: the close fixture's 100 ms timer
  fired 0.3–4.3 s late while hidden, against about 0.1 s once shown, so a
  native fixture that waits on page script should show the page once
  `Created` arrives. Source: the lifecycle-timeout diagnosis in
  [`docs/p1-11-popups.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-11-popups.md); a 6 s held response reproduced the recorded
  failure 2/2 under the old timer (observed; high).
- **`TrySuspend` fails with `ERROR_INVALID_STATE`** if the web view is still
  visible. Call `set_visible(false)` first. Use either suspend and resume or
  the memory-usage level on a tab, not both. Source: note 08, section 5
  (high, from Microsoft's docs).
- **Child web views do not follow the window's size.** wry only resizes a
  web view it does not treat as a child, so the page card stays at its old
  size. Call `WebView::set_bounds` whenever the page card changes; bounds are
  logical units. Source: note 08, section 2.3 (medium, code-read only).

## The interface: GPUI on Windows

- **The page loses the keys right after Syrinx focuses it.** GPUI's
  `Window::activate_window` does its Win32 work later, from a spawned task:
  `SetActiveWindow`, then `SetFocus` on the GPUI window, a synthetic Alt press
  and `SetForegroundWindow`. A page `Focus` sent in the same update reaches
  WebView2 first, so that `SetFocus` takes the keys back. Esc in the palette
  returned focus to the page 0 of 20 times; the harness found keyboard focus on
  the main window itself every time. With Syrinx already in front, activate
  natively and synchronously (`activate_if_foreground`) before sending the page
  `Focus`. With another application in front, never call `activate_window`:
  its synthetic Alt and `SetForegroundWindow` take the screen from the owner
  (owner decision, 2026-10-12). Defer the page's focus until the owner brings
  Syrinx back, through the main window's activation observer, and drop the
  deferral if the owner returns by clicking a chrome control: GPUI runs the
  activation callback from a task, after the click has focused the control.
  Source:
  `gpui-pre-windows-0.3.8/src/window.rs`, `activate`, and P1-33's runs
  `1791858770` (0/20) and `1791859013` (20/20 with only this change), recorded
  in [`docs/p1-33-palette-acceptance.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-33-palette-acceptance.md) (code-confirmed and measured).
- **A native resize or show inside a GPUI update loses the window's new size
  and first frame.** `SetWindowPos` and `ShowWindow` deliver `WM_SIZE` and
  `WM_SHOWWINDOW` synchronously; GPUI's callbacks then find the app borrowed,
  log the failure and drop the bounds change or the draw, consuming the frame
  demand. Move or show a kept GPUI window from a spawned task, outside any
  `update` closure, as DW-90's `show_palette` does. Hiding inside an update is
  safe: it only loses one modifier-state refresh. Source:
  `gpui-pre-0.3.8/src/window.rs` (`on_resize`, `on_request_frame` use
  `AsyncApp::update`, which is `try_borrow_mut`) and
  `gpui-pre-windows-0.3.8/src/events.rs` (code-confirmed).
- **A hidden GPUI window keeps waking for a focused input's caret.** The
  gpui-base input's caret blink re-arms a 500 ms timer that only a blur stops,
  and GPUI delivers blur listeners during a draw, which a hidden window never
  has. Hiding the palette with its input focused woke Syrinx twice a second.
  Drop the used input when hiding (dropping its entity cancels the blink task)
  and create a fresh, unfocused one; P1-33's harness counts the input's
  notifications while hidden. Source: gpui-base's `BlinkCursor` and GPUI's
  `on_blur` dispatch in `gpui-pre-0.3.8/src/window.rs` (code-confirmed;
  found in DW-90's review).
- **Opening a window from inside an entity's update panics if the new window's
  render updates that entity.** GPUI's `open_window` draws the window once before
  it returns, so the render runs synchronously inside the caller's lease and
  fails with "cannot update BrowserView while it is already being updated". The
  sidebar peek did this from its hover sampler; DW-90's harness hit it too, and a
  synthetic hover reproduced it before the fix. Open such a window from
  `cx.defer` (or a task) outside every update, and recheck that the request is
  still wanted when it runs. Source: `gpui-pre-0.3.8/src/app.rs`, lines
  1431–1455, and the fix in `ui/peek.rs` ([`docs/p1-17-peek.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-17-peek.md)).
- **A root element's mouse listener misses presses on occluding children and
  right after a key.** GPUI runs an element's `capture_any_mouse_down` only
  while its hitbox counts as hovered, which an `.occlude()` child (the
  toolbar) prevents, and `is_hovered` is false whenever the last input was a
  key. DW-62's first build therefore never saw a press on a toolbar button.
  To see every press in a window, register `Window::on_mouse_event` from a
  canvas's paint, as `controls::watch_pointer` does. Source:
  `gpui-pre-0.3.8/src/window.rs`, `HitboxId::is_hovered` (line 782), and
  DW-62's visual probe (observed).
- **A control revealed by GPUI's hover listener vanishes between press and
  release.** `on_hover` reports "not hovered" while the element has a pending
  mouse-down, so a press followed by a one-pixel move hides a control drawn
  only while the listener says hovered, and the mouse-up then clicks whatever
  is under it (a sidebar row, which selects instead of closing). Keep such a
  control in the tree and toggle its `visibility` from a `group_hover` style
  instead: `Visibility::Hidden` makes `Interactivity::paint` return before it
  paints the element, its mouse listeners and its accessibility node, so a
  hidden control can never take a click, while the group-hover wake registered
  just before that return still redraws it on the next hover. The compact
  sidebar's close badge (#128) is built this way. Source:
  `gpui-pre-0.3.8/src/elements/div.rs`, lines 3154–3172 (the hover listener
  requires no pending mouse-down) and 2533–2537 (the hidden return follows
  `paint_hover_group_handler`), read for PR #153 (code-confirmed; the owner's
  window check is pending).
- **A bounds observer fires on every activation change, not only on a move.**
  GPUI's activation callback calls `bounds_changed`, which runs every
  `observe_window_bounds` observer. The peek's watch closed the peek when it was
  clicked and undid the rearm after Alt Tab. Compare the window's actual
  bounds, viewport and scale with the last seen values before acting. Source:
  `gpui-pre-0.3.8/src/window.rs`, line 1953, and [`docs/p1-17-peek.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-17-peek.md)
  (code-confirmed and observed with posted activation).
- **A peek click closes the peek but the tab does not switch, and the session
  worker can then stop.** GPUI's `update_in` runs in the window that drew a view
  last, not the one the task was spawned in. The sidebar peek draws
  `BrowserView` too, so once the popup closed GPUI dropped the view's window and
  `update_in` failed until the main window drew again; the session event loop
  stopped on that failure, losing the click's tab switch (#10) and failing the
  worker's next send (#9). Update the browser view from tasks only through
  `ui::main_window::update_in_main`. `subscribe_in`, `observe_in` and
  `defer_in` resolve their window the same way: while the peek is open they get
  the popup, and once it closes they do nothing. Subscribe the browser view
  through `ui::main_window::subscribe_in_main` instead (#81). A unit test
  refuses all four under `src/ui` (only `main_window.rs` is exempt), and
  `cargo xtask palette-acceptance` clicks a peek row (`peek_click_*`). Source:
  `gpui-pre-0.3.8/src/app.rs`, `App::with_window` (line 2064) and the
  window-removal cleanup of `current_window_by_entity` (line 3049),
  `app/context.rs` lines 301-341, and the harness's peek step (observed
  2026-10-18).

- **A popup's input form can survive after its Enter handler loses its window.**
  `InputState` and `subscribe_in` are tied to the creating window. Do not render
  a retained Favorites form in a replacement peek or the main sidebar after
  that popup closes; dismiss the unfinished form at the window boundary.
  Source: P1-17 independent source review (source-confirmed; native acceptance
  remains in [`docs/p1-17-peek.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-17-peek.md)).
- **A queued peek dismissal can steal focus after Alt Tab.** A matching popup
  generation proves identity, not current activation. Check native foreground
  ownership again when applying the action, and close without refocusing when
  Syrinx has lost activation. Source: P1-17 source review and generation tests;
  the owner still verifies native timing.

- **A queued palette refocus can discard the Enter or Escape behind it.**
  The popup may remove its native window before the browser drains Ctrl L/K.
  Skip that obsolete popup-origin refocus without clearing its retained
  identity; the queued completion still needs that identity to pass the
  stale-popup guard. Keep main-window stale-handle recovery unchanged.
  Source: integrated-preview shortcut review and the app's routing regression
  (source-confirmed; native timing acceptance remains separate).

- **A hidden launch cannot establish visible-window readiness.** Fresh
  non-smoke Syrinx processes started with `Start-Process -WindowStyle Hidden`
  remained alive without a targetable window in both execution contexts.
  The same frozen executable opened Home when launched normally for the
  owner's explicitly approved interactive test. Keep hidden launch for
  noninteractive checks, but use a visible launch for authorized window
  inspection. Source: continuation interactive-crash/desktop/visible launch
  records under `target/` (observed; not a breakpoint diagnosis).

- **A visible test launch can pick up a real address.** A fresh profile
  opens Home with the palette focused and takes the foreground; within
  seconds of AN-02's launch the synthetic profile had opened a real address
  that no agent sent. The owner's typing landing in the palette is the likely
  route, a forwarded link the other; it was not diagnosed. Delete such a
  profile after the check, crop captures to the area being checked, and never
  keep a capture that shows a tab title or address. Source: AN-02's window
  check (observed 2026-10-14).
- **A UI Automation rectangle can lie outside the window.** A control
  scrolled out of a GPUI panel keeps its real position, which can be above
  the window; `SetCursorPos` clamps it to the screen edge and the click lands
  in another program even though Syrinx was in the foreground. Before a
  synthetic click, check that the point is inside Syrinx's window rectangle as
  well as that Syrinx owns the foreground, and check the foreground again
  before every capture. Source: AN-02's window check (observed 2026-10-14).

- **A delayed Home snapshot reopens an already finished palette.** A new
  blank tab can reach the UI after its entry was submitted or dismissed.
  A boolean pending flag is insufficient: opening another saved page can
  rearm it for the old snapshot. Correlate each app-private Home action with
  its own committed acknowledgement, cancel superseded intent and consume
  matching no-op results too. Source: independent integrated-preview review
  of `ui/home_palette.rs` and the session worker (code-confirmed).

- **A hidden non-smoke launch can wait forever on an invisible message
  box.** Since P1-34, start-up failures and dropped forwarded links show a
  Windows message box; only `--smoke` suppresses it. A `--test-data-dir`
  run started with `Start-Process -WindowStyle Hidden` then blocks on a box
  nobody can see. Use `--smoke` for unattended runs, or launch visibly.
  Source: P1-34/35 review of `syrinx_platform::notice` (code-confirmed).

- **A smoke-test timeout on the GUI thread cannot bound a frozen GUI.** The
  earlier orderly timer also did nothing once shutdown began, so a missing
  worker-stop notification could hang the test. Keep an independent standard
  thread deadline for `--smoke` only, alongside orderly shutdown. A hard exit
  applies only to its freshly generated synthetic profile; it is not crash
  recovery or proof that a window is visible. Source: integrated-preview
  timeout review in `crates/syrinx-app/src/main.rs` and `ui.rs` (code-confirmed).

- **An element with its own focus handle never becomes a Tab stop through
  `tab_index`.** GPUI applies `tab_index` only to handles it creates itself;
  with `track_focus(&handle)` the handle's own `tab_stop` decides, and
  `cx.focus_handle()` starts as `false`. Create such handles with
  `.tab_stop(true)`. The sidebar's resize edge was focusable but unreachable
  by Tab until P1-36. Source: `gpui-pre-0.3.8/src/elements/div.rs`, lines
  2251–2270 (source-confirmed).
- **A chrome control keeps GPUI focus, and its ring, while the page has the
  keys.** WebView2 takes Win32 focus without GPUI noticing. Clear GPUI focus
  to the window root when focus goes to the page: `focus_page` does, and the
  page's `GotFocus` event (`BrowserEvent::PageFocused`) covers clicks.
  Source: P1-36 (code-confirmed; owner visual check pending).
- **A page's `GotFocus` does not prove the page holds the keys.** Clearing
  GPUI focus on every `PageFocused` pushed the find bar's caret to the window
  root while the owner typed (#57, was DW-71). In the native fixture, two
  `PageFocused` events arrived during five find queries while `GetFocus` was
  the chrome window at every event and after every result. That WebView2
  raises them *because of* find is unconfirmed: one may be a late event from
  the suite's earlier page `Focus`. `page_took_focus` clears only when
  `chrome_has_keyboard_focus` is false; keep that check, and never infer from
  the event alone that the page holds the keys. `cargo xtask
  palette-acceptance` checks both directions in-process
  (`find_keeps_focus_on_stray_page_focus`, `find_ring_clears_on_real_page_focus`).
  Source: the essentials suite of `crates/syrinx-platform/examples/page_close`
  (focus staying on the chrome observed once, 2026-10-18; attribution to find
  inferred).
- **GPUI's accessibility tree can panic the `local` build, but only with a
  UI Automation client attached.** The `local` profile keeps debug
  assertions, and the tree builder panics if one `FocusHandle` is tracked by
  two elements in a frame (both report focus), if two siblings with roles
  share an element id, or on a stray active descendant. Nested regions with
  their own handles are safe: only the element whose handle *is* focused
  reports. The tree runs only while a client (a screen reader, some password
  managers) is attached, so the gate never builds it; after changing roles,
  ids or focus handles, rerun the UI Automation probe recorded in
  [`docs/p1-36-accessibility.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-36-accessibility.md). Source: `gpui-pre-0.3.8/src/window/a11y.rs`,
  lines 232–243, 411–423 and 530–542, and `elements/div.rs`, lines 2326–2335
  (source-confirmed; P1-36 probe survived).
- **A focused gpui-component text field is invisible to UI Automation, and
  a row cannot claim focus under it.** The element tracking the field's
  handle (`input-state`) has no role, so GPUI reports the window as focused
  and ignores any `aria_active_descendant` below. To give a field a focused
  node, track *the field's own handle* on a role-bearing ancestor, as the
  palette's `palette-query` does (DW-60): only the node with a role claims,
  keys still go to the field because it registers the handle last, and the
  container's copy must not be a Tab stop (twice in Tab order, Shift Tab
  from the field lands on itself). Setting the copy's `tab_stop` field does
  nothing: `track_focus` clones, and `FocusHandle::clone` rereads the flag
  shared by the focus id, which `tab_stop(false)` writes for every holder,
  so restore it to true before the field renders. Since DW-70 every text
  field goes through `crates/syrinx-app/src/text_field.rs` (`container` or
  `labelled`), which does all of this; draw a new field through it, never
  with a bare `Input::new`. Source:
  `gpui-base-0.7.1/src/input/base/state.rs`, line 4464,
  `gpui-pre-0.3.8/src/window/a11y.rs`, `set_focus`, and DW-60's probe in
  [`docs/p1-36-accessibility.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-36-accessibility.md) (source-confirmed and probed).
- **GPUI answers `WM_NCHITTEST` from the last pointer position it saw, not
  the point Windows asks about.** Windows sends the hit test before the move
  that would update GPUI, so a pointer arriving on ✕ from □ got □'s answer:
  the snap-layouts flyout on ✕ and "Minimize" on □ (#11). Syrinx's main
  window is subclassed in `syrinx_platform::caption` to answer the three
  caption buttons from the hit test's own point; keep the buttons square,
  as tall as the title bar and flush with the top-right corner, or update
  that geometry, and never draw anything clickable over that corner: a
  click there acts as the caption button beneath it. A probe that measures this must be per-monitor DPI aware:
  a DPI-unaware process reads scaled rectangles but sends raw `lparam`s.
  Source: `gpui-pre-0.3.8/src/window.rs`, `on_hit_test_window_control`
  (uses `mouse_hit_test`), and a `SendMessage` probe on 2026-10-18 that got
  the previously hovered button's code at every button before the fix and
  the right one after it (measured).
- **GPUI draws above a WebView2 child window, not below it.** Note 08 read
  the code as the page covering GPUI; the Phase 0 spike found the opposite
  on this machine. Don't rely on either for layout: keep the page card's
  region free of GPUI content and put the palette and the sidebar peek in
  separate owned popup windows, which work. Source:
  [`docs/research/09-phase-0-verdict.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/09-phase-0-verdict.md) (observed in the spike; high).
- **A chrome background behind the page card hides every page.** The tab
  gets its title and the status line says "Page loaded.", but the card stays
  empty. GPUI's DirectComposition target is topmost
  (`directx_renderer.rs`, `CreateTargetForHwnd(hwnd, true)`), so any pixel
  GPUI paints over the card covers the WebView2 child window, even a root
  `.bg()`. An `Opaque` window is also cleared to white under everything.
  Paint the window fill only around the card
  (`page_card::window_fill_layer`), keep the window's Mica backdrop for
  every palette, and after changing what the root or the page pane paints,
  run the app and look at a loaded page; the gate cannot see this. From
  P1-28/29 (cc9c496) until the fix, every build, v0.1.0 and v0.2.0
  included, hid every page. Source: the 2026-10-14 blank-page diagnosis,
  which found a visible, full-size `WRY_WEBVIEW` child under an empty card
  (observed; high).
- **GPUI's Windows backend repaints every window on every display refresh,**
  even when nothing changed, so an idle Syrinx uses about 3.75% of a core.
  Patch the idle loop (D37) and measure idle CPU after any GPUI upgrade.
  Source: note 09 (measured; high).
- **Smart App Control is off on the owner's machine since 2026-10-12.** The
  owner switched it off for development, so os error 4551 blocks should stop
  here. Older 4551 records stay valid history. Agents still never change
  Windows security settings themselves, and a clean or another machine may
  still block unsigned builds, so keep handling 4551 as below.
- **Application Control can block locally built executables** (os error
  4551). Phase 0's debug builds were blocked and optimized builds ran. During
  P1-01, an optimized `xtask` was also blocked once; an unchanged retry passed
  the full gate without rebuilding or changing settings. The transient
  failure's cause is unproved. Use D38's profile, preserve failures, and never
  change Windows security settings to make a build run. Sources: note 09 and
  the P1-01 gate recorded in `docs/dev/LOCAL-DEV.md` (observed; high). During
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
  window, so the palette would stay on top when Syrinx loses focus. Set its
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
- **A system backdrop goes flat whenever its window's frame is drawn
  inactive, including while Syrinx's own palette is open.** DWM draws Mica's
  and Acrylic's solid fallback on a window whose frame is inactive, and an
  owned popup taking activation sends its owner `WM_NCACTIVATE(FALSE)`; a
  non-activating popup (the peek) is never drawn active at all.
  `syrinx_platform::active_frame` rewrites only that message's `wParam`, so
  every installed window of the family is drawn active while any of them is
  active, and leaves `WM_ACTIVATE`, focus and the foreground alone. It relies
  on `WM_NCACTIVATE`'s `lParam` naming the other window, as Microsoft's page
  for the message documents (NULL when that window belongs to another
  application); its `SetActiveWindow` test checks that on each run. Call
  `install` on any new owned popup, or it counts as outside Syrinx. While
  another application is active the fallback stays: no documented DWM route
  keeps the material lit there. Source: `active_frame.rs` and its tests
  (2026-10-25); the owner's visual check is pending.
- **Keys go to the page while GPUI still shows a caret.** Nothing in GPUI
  hands focus back to WebView2. After the palette closes, activate the main
  window, then call wry's `WebView::focus()`. The activation must have
  finished first; GPUI's own `activate_window` has not, and it also forces the
  foreground (see the first trap in this section). Source: note 08, sections 2.3
  and 3.3 (medium, code-read only).
- **A caret version requirement on GPUI breaks the build.** Snapshot releases
  come weekly and may change the API; a `^` requirement has already moved apps
  onto a snapshot their components did not compile with. Pin with `=`.
  Source: note 08, section 1.2 (high).
- **An exe built with debug assertions starts only on the machine that built
  it.** Upstream GPUI then compiles its shaders at run time from
  `CARGO_MANIFEST_DIR`, so `v0.1.0` from D38's `local` profile panicked
  silently everywhere but the runner ("Recreating GPU state for DirectWrite:
  The system cannot find the path specified", exit code 101). The runner's
  own smoke run and screenshots passed because its source folder was there.
  The 2026-10-14 vendor patch embeds the shaders in every profile; keep it,
  and test a release zip on a machine other than the one that built it.
  Source: `vendor/gpui-pre-windows-0.3.8.provenance.md` (observed; high).

## Linux: WebKitGTK, X11 and POSIX

- **WebKitGTK's library dependencies do not guarantee a usable media pipeline.**
  At `a01b28d`, X11, lifecycle, popups, controls and permissions passed, but
  synthetic video reached `GStreamer element autoaudiosink not found` and
  WebKitWebProcess aborted. The machine had only GStreamer and base plugins.
  `autoaudiosink` belongs to the good set; require the good, bad and libav
  runtime packages in the installation guidance. Probe with
  `gst-inspect-1.0 autoaudiosink` when available; that is an element check,
  not proof of every decoder or audio device. Keep native media assertions
  intact and let the owner install missing system packages. Source:
  [coordinator receipt](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6683986953)
  and [GStreamer element documentation](https://gstreamer.freedesktop.org/documentation/autodetect/autoaudiosink.html)
  (native observation and upstream documentation, 2026-11-06; full rerun
  after plugin installation pending). Product detection and an actionable
  prerequisite message are proposed in #246; the installer warning alone
  cannot prevent a portable launch from reaching this engine failure.
- **A hidden-survivor resize failure can mean it moved, not that it stayed
  stale.** At `812b19f`, expected `(19, 29, 480, 300)` was the old survivor
  rectangle; actual `(7, 11, 320, 240)` was the active page's new rectangle.
  GTK resized every page unconditionally. Reuse Windows' shared `Placement`
  policy: leave hidden geometry alone and apply the current card before
  mapping on activation. Source: `browser_gtk/page_placement.rs`, Windows
  `browser/page.rs` and both native lifecycle fixtures; the coordinator's
  [3/3 reproduction](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6675466572)
  and the failing-then-passing display-free operation-order regression
  (observed and code-read, 2026-11-05; corrected native run pending).
- **A private Downloads variable set after GLib starts can still leave the
  owner's Downloads selected.** Prepare a mode-0700 config root containing
  an explicit `user-dirs.dirs` before any GLib/GTK initialization or threads,
  then read GLib's actual Downloads directory on `syrinx-gtk` before every
  fixture download. Refuse missing or different destinations; never accept
  HOME fallback. Source: `examples/linux/environment.rs` and its preparation
  and mismatch regressions (source-confirmed and display-free tested,
  2026-11-05; new download scenarios remain native pending).
- **A scratch runtime disconnects native audio from the desktop.** Keep the
  coordinator's real `XDG_RUNTIME_DIR` for native WebKit/audio tests while
  isolating data/cache/config. Only sandbox display-free Cargo commands use
  a fresh private runtime to avoid the sandbox's read-only desktop runtime.
  Do not move native audio into that scratch runtime to make tests look
  isolated. Source: Task 3 startup contract and the desktop socket paths
  (source-confirmed, 2026-11-05; new muted-audio scenario native pending).
- **An unresolved permission can outlive the navigation that requested it.**
  Take the pending map before denying native requests, so synchronous denial
  callbacks cannot reborrow it. Navigation emits completion while the page
  is live; close must retire the page before denial so stale callbacks remain
  invalid. Source: `browser_gtk/page_permissions.rs` and its ownership/event
  regressions (source-confirmed and display-free tested, 2026-11-05; expanded
  native Navigate/close cases pending).
- **A popup download may report before its Created acknowledgement.** The
  engine releases held download reports inside adoption, then the dispatcher
  emits Created. Accept either ordering but require both attribution and
  Created before declaring adoption complete. Source: `browser_gtk/engine.rs`,
  `browser/protocol.rs`, and `examples/linux/downloads/observation.rs`'s
  failing-then-passing ordering regression (source-confirmed, 2026-11-05;
  new popup download cases native pending).
- **The previous load's completion can falsely pass a Reload check.** Reaching
  an address through History does not drain its NavigationCompleted. Drain
  earlier FIFO callbacks before Reload and require that tab's new Loading
  followed by successful, noncancelled completion. Source:
  `examples/linux/history.rs`'s stale/other-tab/failure replays
  (source-confirmed and display-free tested, 2026-11-05; corrected native
  Reload assertion pending).
- **A calculated Find index alone does not prove the native selection moved.**
  The shared label is one-based and empty Find must complete with `0/-1`.
  Require native found/failed completion plus independent authored selected-span
  evidence for sequential direction and wrap checks. GTK drops the returned
  query and native index, so this does not prove overlapping-operation
  correlation; that pre-existing gap is #245. Source: `browser_gtk/page.rs`,
  `examples/linux/controls/find_observation.rs` and their RED/GREEN regressions
  (source-confirmed, 2026-11-05; native baseline, reversed-direction mutation
  and real Find-label inspection pending).
- **Native X11 focus alone does not establish window-manager activation.**
  At `cb44257`, three permissions runs failed the native-focus guard before
  Enter, and one earlier X11 probe failed popup-to-chrome transfer. Fixtures
  mapped unmanaged parents and used `XSetInputFocus`; their command barrier
  did not prove that Hyprland had activated them. Show a managed fixture,
  request `_NET_ACTIVE_WINDOW`, wait for WM acknowledgement and actual native
  focus, then send host Focus and await the page child. Keep the immediate
  pre-key guard and the product's foreground rule. Poll outside GTK's thread
  so its event loop can make progress. Do not change `follow_mouse` or add a
  global focus helper. Source: [the coordinator receipt](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6705935149),
  `examples/linux/permissions.rs`, `acceptance/window_focus.rs` and the
  [EWMH activation protocol](https://specifications.freedesktop.org/wm/latest-single/)
  (observed and source-confirmed, 2026-11-07; corrected native run pending).
- **A coordinator focus helper can break an otherwise valid native probe.**
  Run it only during palette acceptance, including the palette stage inside
  the full gate. Keep it off during every native fixture/probe: those use
  an owned unmanaged popup and test refusal to take focus. The coordinator's
  `dec469e` rerun passed page-close 3/3 with the helper off; helper interference
  must not be mistaken for a product focus failure. Source:
  [#221, comment 6673543416](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6673543416)
  (native observation, 2026-11-05). New native cases still require their rerun.

- **A raw `XSetInputFocus` takes the keys from another application, and on
  a minimised window it ends the process.** X11 grants focus to any client
  that asks, so a page shown while another program is in front took that
  program's typing, against the owner's 2026-10-12 foreground rule; and a
  window that is not viewable (minimised, or a child of one) answers with a
  BadMatch error, which GDK's process-wide X error handler treats as fatal.
  Check `x11::syrinx_in_foreground()` before any focus Syrinx starts itself
  (a click on the page is the owner's own act), focus through
  `Connection::set_focus`, which refuses a window that is not viewable, and
  wrap a focus on GTK's own connection in the display's error trap, as
  `page_keys.rs` does. Source: the #226 review; `x11.rs`'s tests cover a
  background Syrinx and an unmapped window under Xvfb (code-confirmed; the
  owner's minimise check is pending).
- **A private-connection request can fail silently while GDK holds an error
  trap.** `Connection::trapped` then reports success for a request that
  failed, so the caller acts on a window that is already gone; the next
  query fails cleanly. Xlib has one error handler for the whole process, and
  GDK's traps save and restore whichever is installed. So `ErrorTrap`
  (`linux/x11_errors.rs`) is installed once, on GTK's thread right after
  `gtk::init`, and nothing may call `XSetErrorHandler` again. While a GDK
  trap is pushed, private errors reach GDK's handler, which ignores errors
  on displays it does not own; that race remains. Moving the private
  connection to xcb or x11rb, whose errors come back per request, would
  remove it, but adds a dependency and needs the owner. Source: b56f7f3's
  Xvfb experiment on GTK 3.24.41 (observed 2026-11-03).

- **A measurement root can exit before its descendants.** Traversing only
  a still-live root misses orphaned children; waiting for the root alone
  also misses a child that ignores termination. The standalone measurement
  runner scopes itself as a child subreaper, retains process identities,
  reaps adopted children and uses bounded cleanup even after a GUI probe
  fails. Its prior process attribute is restored. Source: native subprocess
  regressions in `scripts/linux/test_measure_runner.py` and Linux
  `PR_SET_CHILD_SUBREAPER(2)` (observed, 2026-11-02).
- **Keep scratch runtime paths short.** Syrinx's Unix instance socket adds
  its own name to `XDG_RUNTIME_DIR`; Linux's `sockaddr_un.sun_path` is only
  108 bytes including the terminator. Placing runtime below a long benchmark
  results folder failed with "Syrinx's link channel failed" before the main
  window opened. The measurement runner uses a private, short
  `/tmp/sx-runtime-*` directory while retaining its results elsewhere. Source:
  `instance/pipe_unix.rs`, Linux `unix(7)` and the first software-rendered
  measurement-script trial (observed, 2026-11-02).
- **A download observer can keep a closed host's WebKit context alive.**
  The context's signal owns the registry, tracked views own their context,
  and clearing only `Engine.pages` leaves a cycle. Shutdown must release
  tracked views and held reports, take active downloads out of the registry,
  and cancel them outside its borrow; late starts are refused. Source:
  independent native context/registry weak-reference reproduction and
  `downloads_shutdown_tests.rs` (observed and regression tested, 2026-11-02).
- **A native test must use the same foreign-window host as a page.**
  A conventional GTK toplevel with WebKit hit an X11 MIT-SHM `BadAccess` in
  this restricted session; an unrealized view did not deliver location
  prompts. `permission_tests.rs` uses the production `Embed` host and proves
  both positive controls before accepting a missing handler call as policy
  enforcement. Source: the fixture's native trial runs (observed,
  2026-11-02). This does not claim that ordinary app windows fail to open.
  A subsequent `page_close` example using the ordinary `BrowserHost` also
  exited on MIT-SHM attach (`BadAccess`, request 130, minor 1). The coordinator
  then ran the draft outside the sandbox: page-close and cookie fixtures
  passed without MIT-SHM errors; history failed independently. This confirms
  the attach failure was environmental. Source: [#221, comment 6585434681](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6585434681)
  (observed, 2026-11-02). Run native acceptance outside this sandbox; do not
  disable MIT-SHM or alter the display, input method or engine sandbox.
- **A host load is not a WebKit user gesture.** `Page::navigate` uses
  `load_uri`, whose navigation action can report no gesture. Mapping that
  directly into `HistoryChange::NewDocument` hid all earlier rows in the
  shared back/forward list. Preserve one matching Syrinx request separately
  and consume it once; a later script move must stay page-initiated. Source:
  `browser_gtk/history_intent.rs`, the display-free five-load regression and
  the coordinator's failed native history run (code-read and observed,
  2026-11-02; #221). Do not change the shared list's hiding rule to mask it.
- **WebKit's public permission request omits the requesting frame's origin.**
  `WebKitUIClient.cpp` discards frame information for geolocation and exposes
  no requesting-origin getter through `WebKitUserMediaPermissionRequest`.
  Delegation enforcement is verified for geolocation and notifications
  only. Camera and microphone delegation are unverified on this machine:
  their positive controls had no usable device, so zero handler calls do
  not establish a policy block. The owner retains the delegation prompt for
  all four kinds and tracks device-backed iframe checks in #234.
  Prompts must say "or a site it embeds" and keep the top-level
  origin as the remembered-choice key. Never present that origin as a proven
  requesting frame. Source: [WebKitUIClient.cpp, WebKitGTK 2.54.1](https://github.com/WebKit/WebKit/blob/webkitgtk-2.54.1/Source/WebKit/UIProcess/API/glib/WebKitUIClient.cpp#L259),
  native loopback matrix in `linux-permission-delegation.md` (observed,
  2026-11-02), and `browser_gtk/permission_tests.rs`.
- **A camera-and-microphone request grants both when `allow()` is called.**
  Mapping it to the first matching device can reuse camera consent to grant
  a microphone too. Syrinx refuses the compound request because its prompt
  represents one permission only. Separate requests still have separate
  choices. Source: `browser_gtk/page.rs` and WebKit's
  `UserMediaPermissionRequest::allow` contract (code-read, 2026-11-02).
- **The print dialog runs a nested GTK loop.** Opening it while the command
  pump borrows `HOST` lets resize or close reenter that borrow. Defer it with
  a real idle source and a page-lifetime ticket. `MainContext::invoke`, used
  by `gtk_thread::post`, may execute immediately on the GTK thread and does
  not provide that deferral. Source: `PrintOperation::run_dialog`, GLib's
  `MainContext::invoke` contract and `page_tests.rs` (code-read and regression
  tested, 2026-11-02).
- **GTK allocations use logical units; X11 page bounds use physical pixels.**
  With inherited `GDK_SCALE=2`, using physical dimensions directly doubled
  the WebKit child inside its container. Convert through the widget's scale
  factor, rounding up partial pixels. Do not reset the owner's scale to hide
  it. Source: the native X11 window tree, 1236×1916 inside a 618×958 container
  before correction, and `page_tests.rs` (observed, 2026-11-02).
- **Unix profile names must retain their original bytes and case.** Windows'
  lowercase/backslash normalization aliases different Unix profiles, and
  lossy UTF-8 conversion aliases invalid byte sequences. Hash normalized
  absolute Unix path components as bytes. Source: `instance/names.rs` and
  its case, backslash and non-UTF-8 regression tests (code-read and tested,
  2026-11-02).
- **A restricted runtime directory can look like an instance-test failure.**
  The agent sandbox makes `/run/user/1000` read-only; ten instance tests
  failed there but all passed with a fresh mode-0700 `XDG_RUNTIME_DIR`.
  Keep `DISPLAY`, `DBUS_SESSION_BUS_ADDRESS` and the input-method environment
  unchanged alongside scratch data/cache/config. This is an environmental
  test setup, not a reason to change product code or assertions. Source:
  the baseline gate and coordinator's outside-sandbox check (observed,
  2026-11-02).
- **This sandbox has no GPU device.** Without `/dev/dri`, Mesa reports
  "Failed to query drm device" and uses software rendering. Its CPU, memory,
  startup and frame timings cannot represent the development machine.
  GDK frame-clock freeze/thaw critical messages appeared in both the gate's
  smoke run and an ordinary `cargo syrinx` run on the untouched branch;
  they are not confined to the smoke mode. Source: baseline native and gate
  logs and the coordinator's environment report (observed, 2026-11-02).
- **wry's GTK backend injects `window.ipc` into every page.** Its
  `InnerWebView::new_gtk` runs an initialization script defining
  `window.ipc` whether or not an IPC handler is set, which is exactly what
  the page rules forbid on Windows. Linux pages are therefore built with
  `webkit2gtk` directly (`browser_gtk/page.rs`), never through wry. Source:
  `vendor/wry-0.57.0/src/webkitgtk/mod.rs`, the `w.init("Object.defineProperty(window, 'ipc'…")`
  call after the handlers (code-confirmed, 2026-10-31).
- **A GTK page can sit inside Syrinx's window only through X11.** GTK 3 can
  wrap a foreign X11 window but not a Wayland surface, so each page is a
  GTK window whose GDK window is an X11 child of GPUI's window, as wry does
  it. GPUI is built with its X11 backend only and GDK is limited to X11
  (`gtk_thread.rs`), so under Hyprland Syrinx runs through XWayland. Source:
  wry's `create_container_x11_window` and `create_gtk_window` (code-read).
- **Native assertion failures need values, not only a generic label.**
  The 7f0899b coordinator run got past replaceState but its row-order error
  omitted the actual and expected rows, hiding whether an entry was renamed,
  hidden or duplicated. History assertions now include both values, the
  current/list snapshot and the recent synthetic event/command trace. Do not
  reverse expected rows to match WebKit's bulk GList ordering: this fixture
  uses signed nth_item offsets and the shared list's nearest-first API.
  Source: #221's coordinator run, the complete display-free history replay
  and WebKitGTK 2.54.1's WebKitBackForwardList/WebBackForwardList code
  (observed and code-read, 2026-11-04).
- **A history API change can happen before the document finishes loading.**
  `address_changed` ignored URI notifications while `is_loading` was true,
  including a post-commit replaceState/pushState during parsing. Nothing
  retried the report at `Finished`, so the shared list kept the old address.
  Defer that post-commit change until a successful finish; clear it at a new
  start/commit or failure. This does not report a provisional page as
  committed. Source: `browser_gtk/history_delivery.rs`, its display-free
  regression (failed before the fix), and the authored history fixture
  (code-read and tested, 2026-11-03; #221). The coordinator's `b5af570` run
  confirmed the timeout was fixed; the fragment-specific gap is below.
- **A host fragment request can finish without a document Finished signal.**
  `load_uri` to the same document produced Loading true, the fragment URI
  and Loading false, with no History report: delivery waited only for
  `load-changed::Finished`. Drain a deferred URI at Loading false only when
  no document load is active; real documents must still wait for their own
  success/failure. Source: [#221, comment 6640541799](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6640541799),
  WebKitGTK 2.54.1's `PageLoadState` and `WebPageProxy` same-document path,
  and the display-free history replay (observed and code-read, 2026-11-04).
- **A pending host URI can overwrite the document that is still finishing.**
  In the `e7ce7a1` native trace, requesting 44 while 43 finished produced
  SamePage 44 before NewDocument 44, so Forward later showed 42, 44, 44.
  WebKit's active URI includes pending API requests. Record the request
  before `load_uri`, and keep its URI out of the older document's finish;
  wait for its own document boundary or same-document settle. The rows are
  the shared Rust list's, not WebKit's bulk Forward getter. Source:
  [#221, comment 6641629095](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6641629095),
  WebKitGTK 2.54.1 `PageLoadState::activeURL`, and the exact display-free
  replay (observed and code-read, 2026-11-04).
- **A traversal's pending URI can complete the same jump twice.**
  At `885701d`, Forward exposed URI 43 as SamePage before its real
  BackOrForward 43 commit. The first landing consumed the pending jump;
  the second reset the shared list, leaving only Back row 43 after the
  next document's replaceState. Guard Back/Forward and Reload before their
  native API calls, just as `load_uri` is guarded. An older finish must not
  publish the new request's URI. Source: [#221, comment 6672868517](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6672868517),
  `/tmp/syrinx-b4-focus.GWs8EX/navigation_history_spike.log` and the exact
  display-free replay in `browser_gtk/page_tests.rs` (observed and tested,
  2026-11-05).
- **A link or script navigation's address is not the view's yet at `Started`.**
  When `load-changed` reports `Started` for a navigation the page began, the
  view's `uri` still names the document being left; the new address arrives
  in a `notify::uri` while the load is provisional. Taking the address at
  `Started` judged the old page's response missing (`NoResponse`) and its
  method unseen. `browser_gtk/page_text.rs` starts the chain at the first
  provisional address that differs from the committed one, and a reload's at
  commit. Syrinx's own `load_uri` sets the address at once. Source: a
  synthetic sign-in page that moved on by script (observed, WebKitGTK 2.54.1).
- **WebKitGTK ignores a page's document it cannot show.** Its default
  response policy downloads only an attachment and drops any other type it
  cannot display, so a link to a `.zip` served as `application/octet-stream`
  did nothing and no download started. Syrinx calls `download()` for such a
  main-frame response (`page.rs`, `download_unshowable`), on popups from their
  creation too, since a popup loads before the UI adopts it and its `uri` is
  still unset then. Source: WebKit's `webkitWebViewDecidePolicy` and a
  synthetic loopback download (observed, WebKitGTK 2.54.1).
- **GTK may be used from one thread only.** gtk-rs records the thread that
  initialised GTK and panics on use from any other, so every GTK call (pages,
  file dialogs, the start-up warning) goes through `gtk_thread::run` or
  `post` onto the one `syrinx-gtk` thread. Never call GTK from the GPUI
  thread. Source: gtk-rs's `assert_initialized_main_thread!` (code-read).
- **Linux reports a link where a folder was asked for as `ENOTDIR`, not
  `ELOOP`.** `open` with `O_DIRECTORY | O_NOFOLLOW` on a symbolic link fails
  with `ENOTDIR`, so a linked data folder looked like a wrong entry kind
  instead of a link. `native_unix::open_with` checks the name with `lstat`
  and reports a link as `ReparsePoint`. Source: `storage::tests_unix`'s
  `a_linked_data_folder_is_refused` (observed 2026-10-31).
- **Unix cannot hold a file against rename, write or delete.** Windows'
  no-sharing opens are what many storage tests assert; on Unix the guard
  instead revalidates retained identities (device and inode) and refuses a
  moved or replaced entry, and those tests are Windows-only. Do not try to
  emulate deny-sharing with advisory locks: only Syrinx would honour them.
  The control file and instance lock do use `flock`, which only Syrinx's
  own processes need to respect. Source: the storage and instance tests
  run on Linux (observed 2026-10-31).
- **Hyprland may keep a newly launched acceptance window in the background.**
  Focus-steal prevention did so in the foreground acceptance run at
  `732aafd`. The coordinator used `hyprctl focuswindow` only when no
  Syrinx-class window was active. Continuously forcing the main window
  would take keys from its popup and invalidate the check. Record this
  preparation; do not change the compositor's policy or make Syrinx steal
  focus. Hyprland's active application and X11's exact keyboard-focus window
  are different facts. Source: [#221, comment 6672563128](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6672563128)
  (observed, 2026-11-05).
- **A focus precondition expires when an acceptance check yields.**
  The `885701d` find check saw keys leave chrome, yielded to GPUI, then
  injected PageFocused after native focus had returned to chrome. Keeping
  the field focused was correct for that later ownership. Observe native
  child focus and the document's focus report, then deliver the test event
  in the same GPUI update; keep the ring-clearing assertion. Source: the
  `885701d` palette report linked in #221 and
  `ui/palette_acceptance.rs` (observed and code-read, 2026-11-05).
- **A managed test window may not receive focus on a real desktop.**
  Hyprland refused focus intermittently in the frame-ancestry gate unit at
  `885701d`. Test ownership with an unmapped tree and query its ancestors
  directly; exercise real focus in native fixtures already in front.
  Source: [#221, comment 6672868517](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6672868517)
  and `browser_gtk/x11.rs` (observed and code-read, 2026-11-05).
- **A per-module focus lock does not isolate the whole X11 unit suite.**
  The `fe5ea09` gate stopped at another managed-window focus setup, after
  1,054 tests passed. Browser tests also reset global focus on cleanup,
  while the acceptance unit independently focused raw windows. Keep unit
  roots unmapped and pass the observed foreground state into the private
  focus operations. Run real focus/visible-stacking probes explicitly in
  `page_close`, separately from parallel unit tests. Serialize intentional
  stale-window queries on GTK's thread, since GDK error traps replace a
  process-wide Xlib handler. Source: [#221, comment 6673181928](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6673181928),
  the workspace X11 test audit and `linux/x11_errors.rs` (managed-window
  failure observed; cross-suite interference and error-trap overlap
  code-read, 2026-11-05).
- **An adopted foreign GDK window does not deliver GTK toplevel focus.**
  GTK 3.24.52's X11 focus handler, including XI2, returns early for
  `GDK_WINDOW_FOREIGN`. The `732aafd` run had X11 focus inside the main
  window but document focus failed, including 0/20 Esc returns. Reconcile
  the foreign GTK host's focus/blur from the actual X11 ownership query;
  `grab_focus` alone cannot replace the missing toplevel event. Keep the
  document's own focus report as the acceptance check. Source: the same
  coordinator report, `gdkdevicemanager-core-x11.c` and
  `_gdk_x11_window_get_toplevel` at GTK 3.24.52 (observed and code-read,
  2026-11-05).
- **X11 gives a key to the window under the pointer when it lies inside the
  focused window.** With the chrome focused and the pointer over the page,
  the page's WebKit window took the find bar's typing; with the page focused
  and the pointer over the sidebar, its keys climbed past the container
  (which selected none) to GPUI. `browser_gtk/page_keys.rs` lets the page's
  windows select keys only while X11 focus is inside its container, and a
  click in the page focuses it, as WebView2 does. Do not forward keys with
  `XSendEvent` instead: fcitx's XIM drops synthetic key events, so forwarded
  typing vanished (it worked only with `XMODIFIERS` unset). GDK selects focus
  and key events through XInput2, so the core masks `XGetWindowAttributes`
  reports never show them, and a filter must match XI2 `GenericEvent`s too.
  Source: the 2026-10-31 Linux QA under XWayland with fcitx5 (observed).
- **GPUI's X11 backend ignores `show: false` and keeps a stale window
  origin.** `Window::new` maps every window it opens, so the kept palette sat
  visible at the screen's corner from start-up; `open_owned_popup` hides such
  popups. And `set_bounds` ignores the origin of a ConfigureNotify that also
  resizes, so after Hyprland moved and resized Syrinx in one step,
  `window.bounds()` kept the old origin and the palette opened beside the
  window. Popups place themselves from `owned_popup::main_window_bounds`,
  which reads the origin from the X server. Source:
  `gpui-pre-0.3.8/src/window.rs` (`map_window` in `Window::new`) and
  `gpui-pre-linux-0.3.8/src/linux/x11/window.rs`, `set_bounds`
  (code-confirmed and observed 2026-10-31).
- **A child process being started holds a copy of every open `flock`.** Until it
  execs, the child shares the parent's open file descriptions, so a lock just
  dropped by one thread still looked held to the next: the updater's
  `a_helper_stopped_after_the_swap_leaves_the_new_version_working` failed once
  in a full gate run, while another test of that program started a process.
  `ApplyLock::try_acquire` on Unix retries a refusal for about 100 ms; a real
  holder keeps the lock far longer. Source: the 2026-10-31 Linux gate (observed
  once; mechanism from `fork`/`flock` semantics, not traced).
- **A Unix socket read must also watch the stop signal.** A connected but
  silent same-user client held the link listener in a 2 s read, so closing
  Syrinx waited for it. `pipe_unix::read_message` reads in 100 ms slices and
  answers `CLOSING` once the stop is raised, as the Windows pipe ends every
  step on its stop event. Source: `instance::tests::a_silent_client_does_not_delay_shutdown`
  (observed 2026-10-31).

### Keep gate executables separate between worktrees

**Observed, 2026-11-02:** sharing `CARGO_TARGET_DIR` between worktrees selected
an `xtask` executable compiled in another worktree. `workspace_root` in
`crates/xtask/src/main.rs` uses compile-time `CARGO_MANIFEST_DIR`, so the
child commands checked that other source tree. Use a private target directory
for each worktree, and check the compiler source paths before recording a
branch's gate. An exit status alone does not identify which tree was checked.

## Update downloads: WinHTTP

- **A loopback test through WinHTTP's automatic proxy can leave the
  machine.** `WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY` follows Windows' proxy
  settings, and on this machine automatic detection is on, which can mean a
  WPAD lookup on the network before the request. `syrinx-update-net`'s
  test-only loopback endpoint opens its session with
  `WINHTTP_ACCESS_TYPE_NO_PROXY`; keep it so, and keep the production
  endpoint on the automatic proxy. Source: `crates/syrinx-update-net/src/endpoint.rs`
  and the machine's `DefaultConnectionSettings` flags (observed 2026-10-14).
- **WinHTTP reports a receive timeout about 3 s after its bound.** A 1 s
  bound ended a silent loopback response after about 4 s and a 5 s bound
  after about 8 s, with or without a proxy. A stall test must hold its
  server well past the bound and must not assert the bound itself; a held
  single-threaded server also delays the next connection, so answer each
  connection on its own thread. Source: P1-39c's
  `a_stalled_host_times_out` (observed on this machine).
- **An HTTPS request to a plain-HTTP server times out rather than failing
  the handshake** when the server waits for a full request head. The client
  sends its TLS ClientHello; the plain-HTTP server reads it as the start of
  a request and waits for a header end that never comes, while the client
  waits for a ServerHello. Treat `TimedOut` as a refusal there too, and
  assert that no plaintext request reached the server. Source: P1-39c's
  `a_secure_request_never_speaks_plain_http` (observed).

## Local search

- **Tantivy prefix matches can produce empty native snippets.** In 0.26.2 and
  0.27.0 (whose `phrase_prefix_query.rs` and `SnippetGenerator::create` are
  identical), `PhrasePrefixQuery::query_terms` omits the suffix term used
  for prefix matching. `SnippetGenerator::create` therefore cannot highlight that
  expansion by itself. P1-23 expands the body terms inside the query timer
  and rejects empty snippets for its matching body queries; otherwise the
  timing would compare different amounts of work. Source:
  [`docs/p1-23-fulltext-bakeoff.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-23-fulltext-bakeoff.md),
  continuation method and focused tests; the 0.27.0 recheck is a source
  comparison, recorded in `docs/dev/fulltext-audit.md`.
- **Tantivy deletion alone retains stored bodies in active segments.** D40
  needs segment merging and successful garbage collection after releasing
  old readers, including when only one segment has deletions. P1-23 scans
  stored rows and checks physical/live document counts after maintenance;
  hiding expired matches alone would not pass. Source:
  [`docs/p1-23-fulltext-bakeoff.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-23-fulltext-bakeoff.md), continuation retention checks.

- **A Tantivy `SegmentMeta` still in scope keeps its segment's files through
  garbage collection.** `searchable_segment_metas()` hands out tracked metas,
  and `garbage_collect_files` treats every tracked meta's files as living, so
  holding the list across a merge and its collection left the merged-away
  segment, with its deleted text, on disk; GC reported success and the
  searcher's live-segment counts looked clean. Drop the metas before
  collecting, and audit the folder for files of segments the searcher no
  longer has. Source: #53's blocked-cleanup test and the folder audit in
  `crates/syrinx-search/src/fulltext/tests/fixture.rs` (observed; high).

- **A NUL query can match padding beyond a field's text.** In the pinned
  frizbee 0.13.0 matcher, synthetic `\0` queries returned highlight indices
  equal to the field's byte length. Both the exhaustive and ASCII-filtered
  catalogue paths reproduced the invalid range. The approved three-file
  vendor correction now masks invalid ASCII lanes; both original regressions,
  public boundary tests and all eight available backend variants pass. Keep
  range validation and the regressions, including zero-score membership.
  Source: `docs/dev/vendor-reviews/p1-05-frizbee-maintenance-review.md` and
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

- **A new deletion-journal category makes a build that predates it refuse
  the journal and offer the clear-all-history repair.** D47's codec fails
  closed on a category byte it does not know, by design. A build between
  #90 (schema 7) and #121 opens the database normally, then meets a
  `SiteZoom` record in `syrinx.deletions`, reports the record as damaged and
  offers the repair; confirming it rewrites the journal as all-time history
  clears, so the zoom clears lose their replay protection. Builds before
  schema 7 never get that far: `FutureSchema` refuses the database first.
  When adding a category, expect a downgrade to the previous preview to meet
  that prompt, and warn of it in the merge commit's subject ("going back to
  an older preview shows the clear-data repair prompt"): the weekly release
  notes are `main`'s first-parent commit subjects and nothing else
  (`.github/scripts/release-notes.sh`), so no other text reaches them. This
  build names an unknown category as written by a newer Syrinx, in the
  journal message and the repair prompt alike. Source:
  `crates/syrinx-store/src/deletion.rs`, `decode_record`, and
  `crates/syrinx-app/src/session/history_repair.rs`, `repairable`
  (code-confirmed, #121).
- **A retained directory or database can still be renamed despite excluding
  delete sharing.** Windows metadata-only opens do not enforce this sharing
  restriction. Retained directories need `FILE_LIST_DIRECTORY`, and retained
  existing primary files need `FILE_READ_DATA`; keep identity probes
  metadata-only so exclusive lifecycle/maintenance handles remain inspectable.
  Test empty directories and existing files as well as freshly created files,
  then verify rename succeeds after release. Source:
  [`docs/missing-primary-startup-review.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/missing-primary-startup-review.md), native validation follow-up
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

- **`GetCurrentSyncRoots` crashes the process instead of failing, on Windows
  Server and on Windows 11 build 26300.** The sync-root factory's method
  pointer is invalid, so the storage guard died with
  `STATUS_ACCESS_VIOLATION` before any error could be handled, and every
  test that opens a store crashed its test binary with no message.
  `StorageProviderSyncRootManager::IsSupported()` crashes the same way, and
  an edition check (`IsOS(OS_ANYSERVER)`, the 2026-10-14 fix) misses client
  builds. `registered_roots` therefore reads the registrations from
  `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\SyncRootManager\<provider>\UserSyncRoots`
  and makes no WinRT call (owner decision, 2026-10-31); do not bring the
  WinRT lookup back. That layout is not a documented interface, so recheck
  it after a major Windows update. A registered root on a network or
  removable drive (or, on Linux, a FUSE or network mount) is skipped, not
  compared, so the database's own drive check is what keeps it out of such
  a folder. Source: weekly-release runs 4 and 5
  (cdb stacks in `GetCurrentSyncRoots` and `IsSupported`) and Windows 11 Pro
  26300 on 2026-10-31, where the fault event's offset resolved through the
  PDB to `GetCurrentSyncRoots` in two test binaries (observed).
- **SQLite's write-ahead log breaks in a synced folder.** In OneDrive or a
  network folder the `-wal` file separates from the database. Keep the
  database in the local, not roaming, app-data folder. Source:
  [`docs/research/03-core-architecture.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/03-core-architecture.md), "Speed and safety settings", and
  D13 in the plan (high).
- **A storage lease's full revalidation is not a cheap row check.** The
  Windows guard opens ancestor handles and enumerates every registered sync
  root in the registry. Do this when opening connections and at
  lifecycle boundaries, not for every tab read or save. Retained handles
  preserve the checked identities; later sync registration remains the
  documented limitation. Source: P1-03 source review and
  [`docs/storage-path-preflight.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/storage-path-preflight.md) (high on the work performed; no timing
  claim).
- **Holding sidecar handles can prevent SQLite's normal cleanup.** Refuse
  unsafe `syrinx.db-wal`, `-shm` and `-journal` entries before opening SQLite,
  then release their inspection handles. Keep the directory/main-database
  lease through connection close, rather than holding sidecars without delete
  sharing. Source: P1-03 design review and SQLite's WAL lifecycle, linked in
  [`docs/storage-path-preflight.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/storage-path-preflight.md) (high on the lifecycle contract).
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
  `docs/dev/dependency-audit.md` and `docs/dev/vendor-reviews/p1-04-sqlite-maintenance-review.md`
  (high on source behavior; no performance remeasurement claimed).

## Repository and research

- **A child test process's first report shares libtest's line.** A crash
  test that reruns its own binary with `--exact --nocapture` reads the
  child's reports from its output, but libtest prints `test <name> ... `
  without a line break first, so the child's first line arrives as
  `test <name> ... BEGIN`. A parent waiting for a line equal to `BEGIN`
  waits out its whole timeout. Match the end of the line, as
  `crates/syrinx-search/tests/fulltext_maintenance_crash.rs` does. Source:
  #53's maintenance crash tests (observed 2026-10-26).

- **Windows PowerShell can replace Unicode in a native-program pipeline.**
  An edit piped to Python lost its middle-dot separator, so a replacement did
  not match and a P1-14 label regression failed. Use direct patches for Unicode
  text, or ASCII script literals with Unicode escapes; UTF-8 file reads alone
  do not control the shell's pipe encoding.
- **A PowerShell double-quoted string silently drops Markdown backticks.**
  The backtick is PowerShell's escape character, so `` "`None`" `` becomes
  `None` with no error: a scripted replacement whose search text held a
  backticked doc comment matched nothing, and one whose replacement held
  them wrote doc comments without their code spans. Edit such text with the
  Edit tool, or use single-quoted PowerShell strings, and check the result.
  Source: AN-01's doc-comment edits (observed 2026-10-14).
- **`$PSScriptRoot` is empty in a `param` default when Windows PowerShell 5.1
  runs a script with `-File`.** It is set in the script body. 0.4.0's setup
  program ran `register-syrinx.ps1` that way, and the script stopped with "A
  folder path is required", so Syrinx was installed but not registered. Default a
  folder parameter in the body (`if (-not $X) { $X = $PSScriptRoot }`), never in
  `param(...)`. `xtask`'s packaging tests check the param blocks and run the
  script as setup does. Source: the owner's 0.4.0 install (observed 2026-10-16).
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
  earlier note. Source: [`docs/plans/Syrinx v1 — Plan.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/plans/Syrinx%20v1%20%E2%80%94%20Plan.md), "What we found"
  (high).
