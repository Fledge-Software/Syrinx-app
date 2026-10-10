# Traps to know

These are the mistakes that look fine, often pass the gate, and then fail
somewhere else. Read this before you start work and before you debug a build,
a test or a window. `AGENTS.md` holds the rules; this file holds the traps.

Atlas has no code yet, so every trap below comes from the research notes or
from this machine rather than from time already lost. Each names its source
and how sure it is. When the Phase 0 spike confirms or disproves one, update
it. When a new trap costs you real time, add it in the same shape: one
bullet, symptom first, then the fix, then the source. Put the story of how
you found it in the commit message, not here.

## Page tabs: WebView2 through wry

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
- **Smart App Control blocks unsigned debug builds on this machine** ("An
  Application Control policy has blocked this file", os error 4551); release
  builds run. Build with the release-style dev profile (D38) and never change
  the policy. Source: note 09 (observed by both spikes; high).
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

## Storage

- **SQLite's write-ahead log breaks in a synced folder.** In OneDrive or a
  network folder the `-wal` file separates from the database. Keep the
  database in the local, not roaming, app-data folder. Source:
  `docs/research/03-core-architecture.md`, "Speed and safety settings", and
  D13 in the plan (high).

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
