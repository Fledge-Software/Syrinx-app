> **Research note dated 2026-10-09.** Read for reasoning, not current fact; later decisions in [`docs/decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/decisions.md) and [`docs/architecture-decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/architecture-decisions.md) win.
>
> This note corrects parts of notes 01, 02a and 06. The Phase 0 spike later found one of its readings reversed on this machine: GPUI draws above a WebView2 child window, not below it (see [note 09](Background-09-phase-0-verdict) and docs/dev/trapstoknow.md). Notes 07 and 08 correct parts of the earlier notes, and note 03 assumes Tauri. The product was called Atlas until 2026-10-12; read Atlas here as Syrinx.
>
> Moved from `docs/research/08-gpui-and-wry-on-windows.md` ([original at af3de53](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/08-gpui-and-wry-on-windows.md)) on 2026-10-16.

# GPUI and wry on Windows: what the Phase 0 spike can rely on

**Date:** 2026-10-09

**Status:** Research note 08. It checks the GPUI and wry facts that the Phase 0 spike and the first build issues depend on. It builds on [`06-engine-and-ui-performance.md`](Background-06-engine-and-ui-performance) (sections 4 and 5, and the Phase 0 checklist), [`02a-tauri-wry-facts.md`](Background-02a-tauri-wry-facts) and [`01-shell-and-engine.md`](Background-01-shell-and-engine), and does not repeat them. Source code was read from shallow clones of `zed-industries/zed` (main, 2026-10-09), `longbridge/gpui-kit` (main, 2026-10-09) and `tauri-apps/wry` at tag `wry-v0.57.0`. Nothing was built or run, so every runtime behaviour below is read from code or docs, not observed.

**Terms.** A **child HWND** is an operating-system window placed inside another window. **Windowed hosting** is WebView2's default mode, where it draws into its own child HWND. **Visual hosting** is the mode where WebView2 draws into a DirectComposition visual that the app stacks itself (the `CompositionController` API). **DWM** is the Windows desktop compositor. A **snapshot crate** is a copy of Zed's code that a third party publishes to crates.io under another name.

## Short answer

Yes, with three corrections to what the earlier notes assumed.

1. **GPUI is not on crates.io in a usable form.** `gpui` 0.2.2 (2025-10-22) is still the latest release under that name. Apps outside Zed depend on Longbridge's weekly snapshot (`gpui-pre` 0.3.8, 2026-10-05) through `gpui-kit`/`gpui-component` 0.7.1, or on a git dependency on Zed. Longbridge's repo is now called `gpui-kit`; `gpui-component` is one crate in it. The kit has since moved to a second Longbridge fork, `gpui-fast`.
2. **The "borderless owned popup" plan needs raw Win32 work.** GPUI's `WindowKind::PopUp` on Windows creates a top-most tool window with no owner, so it floats above other applications too. `WindowBackgroundAppearance::Blurred` uses the old `SetWindowCompositionAttribute` Acrylic, not `DWMSBT_TRANSIENTWINDOW`. Atlas gets the popup's HWND through `raw-window-handle` and sets ownership, the backdrop and activation itself. Both are a few `windows`-crate calls, but neither has been shown to work in GPUI.
3. **wry covers the tab basics but not the browser-grade events.** wry 0.57 handles new-window requests (with the opener, so `window.opener` works), permissions, a shared environment and memory level. It does not expose accelerator keys, fullscreen, process failure, suspend or crash-report settings. Atlas reaches those through `WebViewExtWindows` (`controller()`, `environment()`, `webview()`) and the `webview2-com` crate, and builds its own environment so it can set crash reporting.

The existing `gpui-webview` crate does not help on Windows: its README says the WebView covers GPUI overlays there. Nothing found contradicts the architecture, so there is no no-go. The riskiest unknowns are the popup's focus handoff with the WebView and Acrylic on a GPUI popup. Both are Phase 0 spike items.

## 1. GPUI release state

### 1.1 crates.io (high)

- `gpui`: latest 0.2.2, published 2025-10-22, Apache-2.0, repository `zed-industries/zed`. Releases 0.2.0 to 0.2.2 all came in October 2025; nothing since. The crate has 344,070 downloads.
- `gpui-pre`: "Zed's GPU-accelerated UI framework (gpui-pre snapshot of zed@279fe07)", 0.3.8, published 2026-10-05. It has been published weekly (0.3.5 on 2026-09-14, 0.3.6 on 09-21, 0.3.7 on 09-28). `gpui-pre-platform`, `-web`, `-macros` and `-reqwest-client` follow the same numbers. These are Longbridge's snapshot crates of Zed's own code.
- `gpui-fast` 0.1.6 (2026-10-09), repository `longbridge/gpui-fast`: "GPUI extracted from the Zed monorepo as a standalone workspace, with the layout engine reworked". Apache-2.0.

Note 06 says "used via snapshot crates"; that is confirmed, and 06's "a year old" release date is correct.

### 1.2 How apps outside Zed depend on it (high)

- **`gpui-kit` is the real name.** The repository `github.com/longbridge/gpui-kit` was `longbridge/gpui-component`. Its README calls it "GPUI Kit" and says "`gpui-kit` pins the matching GPUI release and re-exports GPUI, base, component, and assets, so a Rust application lists a single dependency". The crates are `gpui-kit` 0.7.1, `gpui-base`, `gpui-component` 0.7.1 (211,395 downloads) and `gpui-wry`/`gpui-webview` (see 2.2). Note 06's "gpui-kit" is therefore right as the project name, and `gpui-component` is the styled-components crate inside it.
- **Licence:** crates are Apache-2.0. The repository root has `LICENSE-APACHE` and a separate `LICENSE-DOCS.md`; GitHub's licence detector reports "Other".
- **Version pinning:** the workspace `Cargo.toml` pins exactly: `gpui = { package = "gpui-pre", version = "=0.3.8" }`, with a comment that a caret requirement once moved applications onto a snapshot that `gpui-component` did not compile with (issue #3156), and that "any snapshot may change GPUI's API". A CI script fails when a pin is not exact. It also lists `gpui-fast` 0.1.3 as the optional alternative backend.
- **Release rhythm:** gpui-kit 0.6.2 (2026-09-18), 0.6.4, 0.6.6 (09-21), 0.7.0 (09-28), 0.7.1 (10-05). It is weekly, and one version (0.6.5) was yanked.
- **The other route** is a git dependency on `zed-industries/zed`, which is what the Longbridge issue examples used earlier (`gpui = { git = ... }`). Zed's crates have no semver promise.

### 1.3 Windows support (high)

- Zed shipped on Windows with a stable release on 2025-10-15, called "a fully supported platform" with a full-time Windows team; it renders with DirectX 11 and DirectWrite. The Windows backend is now its own crate, `gpui_windows`, in Zed's repository.
- The crate has `accesskit` and `accesskit_windows` dependencies, and the window code calls `adapter.update_window_focus_state` on activation.

### 1.4 AccessKit (high on status, low on quality)

- Zed PR #56065 "gpui: Accesskit support" was closed on 2026-05-27, and `gpui` and `gpui_windows` both depend on `accesskit` today. This agrees with note 06 ("since May 2026").
- `gpui-kit`'s README claims "AccessKit roles, names, states, relationships, and actions are built into the interaction layer and covered by tests". That is a vendor claim. No screen-reader test of a real app was found (low).

## 2. A child HWND inside a GPUI window

### 2.1 The raw handle (high)

- GPUI's `Window` implements `raw_window_handle::HasWindowHandle` and `HasDisplayHandle` (`crates/gpui/src/window.rs`, line 7572). On Windows the platform window returns a `Win32WindowHandle` built from its `hwnd` (`gpui_windows/src/window.rs`, line 595).
- So `wry::WebViewBuilder::build_as_child(&*window)` compiles against GPUI's window with no patches. This is exactly what `gpui-webview` does on Windows (`crates/webview/src/lib.rs`, line 131).
- Version note: wry 0.57 depends on `raw-window-handle` 0.6, the same major version GPUI uses. The Longbridge crate uses a fork, `lb-wry` 0.53.3, instead of upstream wry. Atlas should use upstream wry 0.57 directly and write its own thin glue (see the recommendation below).

### 2.2 Existing crates (high)

- `gpui-wry` was renamed `gpui-webview`; the last `gpui-wry` is 0.7.1 (2026-10-05, 5,140 downloads, Apache-2.0). The README says it is "still a experimental with limited features".
- Its platform table says that on Windows (WebView2) GPUI overlays above the page are "Not yet; the WebView covers the overlays". It advises "use the webview in a separate window or a Popup layer when overlapping GPUI content is required". That is the same approach Atlas chose.
- Longbridge's open PR #2626 (opened 2026-07-30) says the macOS overlay path is validated, and that "Windows CompositionController and Linux hosting are documented follow-ups". A later comment says a Windows checkpoint (commit `4e560a95`) "creates a GPUI native-surface portal, passes its IDCompositionVisual to the wry CompositionController", with "Pointer/focus/IME forwarding and device-loss rebinding" incomplete and Windows validation still needed. It depends on a Zed PR (#61945, "Render deferred overlays above native child views"), which was closed without merging on 2026-09-27. So visual hosting inside GPUI is being attempted by someone else but is not a finished fallback.
- Zed itself has no WebView: PR #54433 (a wry-backed `WebView` element) was closed unmerged on 2026-04-23.

### 2.3 Z-order, resize, DPI and focus (medium; code-read only)

- **Z-order.** wry creates a container child HWND (`WS_CHILD | WS_CLIPCHILDREN`, window class `WRY_WEBVIEW`) and moves it to `HWND_TOP`. Windowed WebView2 therefore draws over everything GPUI paints in the same window. Note 06 already records this; the Longbridge README confirms it for GPUI.
- **Resize.** For child webviews wry does not subclass the parent window (the subclass only attaches when `!is_child`, `webview2/mod.rs` line 605). Atlas must call `WebView::set_bounds` itself whenever the page card changes size. Bounds take logical units, which wry converts with the parent's DPI (`util::hwnd_dpi`).
- **Focus.** Issue longbridge/gpui-kit#1787 (macOS, closed) shows the failure mode: after the WebView loads, keyboard input goes to the page while GPUI still shows a caret. A maintainer called it "the known limitation of the Wry use in GPUI". On Windows, GPUI's `handle_activate_msg` raises AccessKit focus events but there is no code that tells WebView2 to give focus back; Atlas must call `WebView::focus()` and `WebView::focus_parent()` itself. This is untested here (low).
- **No Windows bug reports.** A search of `longbridge/gpui-kit` issues for Windows plus webview found only crashes in the demo ("RefCell already borrowed", closed) and the overlay PR above. No report of resize or DPI bugs was found. Absence of reports is weak evidence (low).

## 3. GPUI popup windows on Windows

### 3.1 What GPUI creates (high, from source)

`gpui_windows/src/window.rs`, lines 492 to 514:

- `WindowKind::PopUp` creates the window with `WS_EX_TOOLWINDOW | WS_EX_TOPMOST` and style 0. It is **top-most for the whole desktop**, not owned by or tied to the main window, and it has no taskbar button. GPUI's own doc comment says "use sparingly".
- Atlas can hide the non-client area: when `titlebar` is `None` or `appears_transparent`, `hide_title_bar` is true and `WM_NCCALCSIZE` removes the frame.
- `WindowKind::Dialog` takes the active window as parent but disables that parent (modal), which is wrong for a palette over a live page.
- `WindowKind::AnchoredPopup` returns `PopupNotSupportedError` on Windows ("Native popups are not implemented on Windows yet").
- `WindowOptions { focus: false }` shows the window with `SW_SHOWNOACTIVATE`, which suits the hover-peek sidebar. There is no `WS_EX_NOACTIVATE` option.
- Atlas gets the popup's HWND from `raw-window-handle` as in 2.1, then calls `SetWindowLongPtrW(GWLP_HWNDPARENT, main_hwnd)` to make it owned (an owned top-most window still stays above its owner) and can drop `WS_EX_TOPMOST` if ownership alone gives the stacking it wants. Whether GPUI tolerates that change after creation is unverified (low).

### 3.2 Backdrop (high on what GPUI does, medium on Atlas's fix)

- `WindowBackgroundAppearance` has `Opaque`, `Transparent`, `Blurred`, `MicaBackdrop` and `MicaAltBackdrop` (`crates/gpui/src/platform.rs`, line 2766).
- On Windows `MicaBackdrop` calls `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE, 2)` (main window) and `MicaAltBackdrop` uses 4 (tabbed window); both are skipped below build 22621. **`Blurred` does not use the system backdrop**: it calls the undocumented `SetWindowCompositionAttribute` with accent state 4 (the older Acrylic-blur-behind effect) from `user32.dll` (`window.rs`, lines 926 to 940 and 1636 to 1680).
- GPUI has no `DWMSBT_TRANSIENTWINDOW` (value 3) option. Atlas calls `DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &3, 4)` on the popup's HWND itself, after creating it with `WindowBackgroundAppearance::Transparent`. Microsoft documents value 3 as `DWMSBT_TRANSIENTWINDOW`, "Acrylic" for transient windows (cited in note 06).
- GPUI never calls `DwmExtendFrameIntoClientArea`. Zed's Mica works without it, so GPUI's composition swap chain evidently is enough, but whether Acrylic shows under a GPUI popup's alpha swap chain is something only the spike can confirm.
- Windows 11 build 26300 is above the 22621 minimum.

### 3.3 Focus handoff (medium; code-read only)

- To the popup: show it with `focus: true`; GPUI answers `WM_MOUSEACTIVATE` with `MA_ACTIVATE` (`events.rs`, line 96), so clicks activate it.
- Back to the page: close or hide the popup, then call wry's `WebView::focus()`, which is `ICoreWebView2Controller::MoveFocus(PROGRAMMATIC)` (`webview2/mod.rs`, line 1583). If Windows first gives focus to the GPUI main window, `WebView::focus_parent()` (a `SetFocus` on the parent) exists too. Atlas should activate the main HWND, then call `focus()`.
- The sequence "palette opens over a focused page, Esc closes, typing resumes in the page" is already in 06's spike checklist; the code path above is what to test.

## 4. Keyboard shortcuts while a page has focus (high)

- **wry does not expose `AcceleratorKeyPressed`.** A search of the wry 0.57.0 source for `AcceleratorKey` finds nothing. The only related setting is `with_browser_accelerator_keys(bool)`, which maps to `ICoreWebView2Settings3::SetAreBrowserAcceleratorKeysEnabled` and turns WebView2's built-in browser keys (F5, Ctrl+P and so on) on or off. wry issue #451 ("CustomMenuItem accelerators don't trigger on_menu_event on Windows") is still open (updated 2025-12-17).
- **Atlas must register the handler itself:** `WebViewExtWindows::controller()` returns the `ICoreWebView2Controller`, and `webview2-com` 0.39 provides `AcceleratorKeyPressedEventHandler` for `add_AcceleratorKeyPressed`.
- **Which keys reach the host.** Per Microsoft: the event fires "when an accelerator key or key combo is pressed or released while the WebView is focused". A key counts as an accelerator if Ctrl or Alt is held, or if the key does not map to a character. Shift is never an accelerator and Escape always is. Plain typing never reaches the host; Ctrl+K, Ctrl+T, Ctrl+W, Ctrl+L, F-keys and Esc do.
- **Constraints.** In windowed mode the handler runs synchronously and blocks the browser process until it returns or calls `Handled(true)`; outgoing cross-process COM calls fail with `RPC_E_CANTCALLOUT_ININPUTSYNCCALL`. So the handler should only set `Handled` and post a message to Atlas's own event loop. It must ignore auto-repeat using `PhysicalKeyStatus.WasKeyDown`. Microsoft's sample does exactly this.

## 5. wry on Windows for a multi-tab browser

Everything here is for wry 0.57.0 and `webview2-com` 0.39.1 (2026-03-11) unless stated.

| Need | In wry? | Detail |
|---|---|---|
| One shared environment | Yes, by passing it in | `WebViewBuilderExtWindows::with_environment(ICoreWebView2Environment)`. Without it each `build` runs `CreateCoreWebView2EnvironmentWithOptions` again (`create_environment`, line 289). `WebContext::new(data_directory)` only holds the user-data folder path. Its docs say webviews with different environment options must use different data directories. Atlas should create one environment once and pass it to every tab. |
| Profiles | Yes | `with_profile_name`; the docs say webviews with different profile names in one environment have isolated cookies and storage. |
| Popups become tabs, keep `window.opener` | Yes | `with_new_window_req_handler(|uri, features| ...)` receives `NewWindowFeatures` with `opener.webview` and `opener.environment`. Returning `NewWindowResponse::Create { webview: ICoreWebView2 }` calls `SetNewWindow` and `SetHandled(true)`. The handler runs deferred so it may build the new view without deadlock. Microsoft's rule: the new webview must be on the same environment and the same profile as the opener, and must not have navigated. Atlas gets the `ICoreWebView2` for the new tab from `WebViewExtWindows::webview()`. |
| HTML5 fullscreen | No | wry has no `ContainsFullScreenElementChanged` handler. Atlas adds it via `webview()` and resizes or hides its chrome (`add_ContainsFullScreenElementChanged` exists on `ICoreWebView2`). |
| `TrySuspend` / `Resume` | No | Atlas casts `webview()` to `ICoreWebView2_3`. Microsoft: `IsVisible` must be false first (so call `WebView::set_visible(false)`), else it fails with `ERROR_INVALID_STATE`. The view resumes automatically when visible. Navigate and some other calls resume it. |
| `MemoryUsageTargetLevel` | Yes | `WebView::set_memory_usage_level(MemoryUsageLevel::Low / Normal)` casts to `ICoreWebView2_19`. Microsoft: pick either suspend/resume or the memory level, not both; `TrySuspend` sets it to low and `Resume` sets it to normal; scripts keep running at low. |
| `ProcessFailed` | No | Atlas adds `add_ProcessFailed` through `webview()`. For a failed browser process an `ICoreWebView2Environment5::BrowserProcessExited` event also fires. |
| Crash-report upload | No | `IsCustomCrashReportingEnabled` (`ICoreWebView2EnvironmentOptions3`, WebView2 1.0.1518.46 or later): setting it to TRUE means "Windows won't send crash data to Microsoft endpoint"; the default FALSE "will respect OS consent". wry builds its options internally and never sets it, so Atlas builds its own environment with `webview2-com`'s `CoreWebView2EnvironmentOptions` and passes it via `with_environment`. |
| Permissions | Yes | `with_permission_handler(...)` maps `PermissionRequested` to a `PermissionKind` enum covering microphone, camera, geolocation, notifications, clipboard read, local fonts, sensors, MIDI, automatic downloads, file system, autoplay and window management. Anything else maps to `Other`. |
| Downloads | Yes | Download start and completed handlers (`DownloadStarting` is wired). |

### 5.1 Does wry inject script into pages? (high)

- Not by itself. `AddScriptToExecuteOnDocumentCreated` is called only for scripts the app passes with `with_initialization_script`, and for one IPC line, `window.ipc = { postMessage }`, which is added only when `with_ipc_handler` is set (`webview2/mod.rs`, lines 508 and 949). A page webview built with neither gets no script.
- `inject_initialization_scripts.rs` is described in its header as "used by the Android backend", not Windows.
- Custom protocols add a `WebResourceRequested` filter only when a protocol is registered (line 1028).
- **Defaults to override.** With no `with_additional_browser_args`, wry passes `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection` (line 301). That switches off Microsoft Defender SmartScreen for every tab. A browser should set its own arguments. The environment also sets the UI language from the user's locale.
- wry hides the status bar (`SetIsStatusBarEnabled(false)`) and creates the container HWND with `WS_CHILD | WS_CLIPCHILDREN`.

## 6. What the earlier notes missed

No no-go. The items below change the work, not the choice.

- **`PopUp` is desktop top-most and unowned** (3.1). Left as is, the palette would float over other apps when Atlas loses focus. This is the largest correction to 06's popup plan.
- **`Blurred` is the old Acrylic accent, not `DWMSBT_TRANSIENTWINDOW`** (3.2). The spike should compare both on the popup.
- **The kit's GPUI base is moving.** `gpui-kit` 0.7.1's `gpui-webview` forces the `gpui-fast` backend, and `gpui-pre` is "not supported" there. Atlas should choose one base (`gpui-pre` snapshot or `gpui-fast`) in the first build issue and pin it with `=`.
- **Exact pins are mandatory** (1.2). A caret requirement already broke the kit once.
- **wry's default browser arguments switch off SmartScreen** (5.1).
- **The popup-as-tab path needs the same profile and environment** (section 5).
- **Visual hosting is not ready to adopt.** wry PR #1762 (DirectComposition hosting for WebView2) is still open (updated 2026-07-22), the Zed overlay PR it depends on is closed unmerged, and Longbridge's Windows version is incomplete. Note 06 already says to adopt it only if popups fail; add that the fallback itself is a research-grade project, so a failed popup spike is a real risk to the schedule.
- **Accessibility of the page itself** comes from WebView2, but GPUI's AccessKit is untested on a real screen reader (1.4).

## Recommendation

1. Build the spike on `gpui-kit` 0.7.1's pinned `gpui-pre` 0.3.8 (or a git dependency on one Zed commit), with upstream wry 0.57 and `webview2-com` 0.39. Write Atlas's own thin tab-view glue instead of depending on `gpui-webview`, which wraps a fork and gives nothing on Windows overlays.
2. Make the popup spike answer, in order: (a) does `WindowKind::PopUp` plus `GWLP_HWNDPARENT` give a popup that stays above the page and not above other apps; (b) does `DwmSetWindowAttribute(DWMSBT_TRANSIENTWINDOW)` blur the page; (c) does Esc return focus through `MoveFocus`; (d) does the hover-peek popup with `focus: false` avoid stealing focus from the page.
3. Add `AcceleratorKeyPressed`, `ProcessFailed` and `ContainsFullScreenElementChanged` handlers, and the crash-report setting, to the tab layer from the start, through `webview2-com`.

## Open questions

- Does a GPUI popup with a transparent swap chain show `DWMSBT_TRANSIENTWINDOW` Acrylic without `DwmExtendFrameIntoClientArea`?
- Does changing a GPUI popup's owner after creation upset GPUI's message handling?
- Does GPUI's AccessKit adapter work with NVDA on a Windows 11 build 26300 machine?
- Which base should Atlas pin: `gpui-pre` or `gpui-fast`?

## Sources

- crates.io API: [gpui](https://crates.io/crates/gpui), [gpui-pre](https://crates.io/crates/gpui-pre), [gpui-pre-platform](https://crates.io/crates/gpui-pre-platform), [gpui-fast](https://crates.io/crates/gpui-fast), [gpui-kit](https://crates.io/crates/gpui-kit), [gpui-component](https://crates.io/crates/gpui-component), [gpui-wry](https://crates.io/crates/gpui-wry), [lb-wry](https://crates.io/crates/lb-wry), [wry](https://crates.io/crates/wry), [webview2-com](https://crates.io/crates/webview2-com)
- Longbridge: [gpui-kit repository](https://github.com/longbridge/gpui-kit) (`Cargo.toml`, `README.md`, `crates/webview/README.md`, `crates/webview/src/lib.rs`), [gpui-fast](https://github.com/longbridge/gpui-fast), [PR #2626](https://github.com/longbridge/gpui-kit/pull/2626), [issue #1787](https://github.com/longbridge/gpui-kit/issues/1787)
- Zed: [gpui platform.rs](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/platform.rs), [window.rs](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/window.rs), [gpui_windows window.rs](https://github.com/zed-industries/zed/blob/main/crates/gpui_windows/src/window.rs), [gpui_windows events.rs](https://github.com/zed-industries/zed/blob/main/crates/gpui_windows/src/events.rs), [AccessKit PR #56065](https://github.com/zed-industries/zed/pull/56065), [PR #61945](https://github.com/zed-industries/zed/pull/61945), [PR #54433](https://github.com/zed-industries/zed/pull/54433), [Zed for Windows](https://zed.dev/blog/zed-for-windows-is-here)
- wry 0.57.0: [webview2/mod.rs](https://github.com/tauri-apps/wry/blob/wry-v0.57.0/src/webview2/mod.rs), [lib.rs](https://github.com/tauri-apps/wry/blob/wry-v0.57.0/src/lib.rs), [web_context.rs](https://github.com/tauri-apps/wry/blob/wry-v0.57.0/src/web_context.rs), [issue #451](https://github.com/tauri-apps/wry/issues/451), [PR #1762](https://github.com/tauri-apps/wry/pull/1762)
- Microsoft Learn (WebView2 Win32 reference): [ICoreWebView2Controller](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller), [ICoreWebView2](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2), [ICoreWebView2NewWindowRequestedEventArgs](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2newwindowrequestedeventargs), [ICoreWebView2_3](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_3), [ICoreWebView2_19](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_19), [ICoreWebView2EnvironmentOptions3](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2environmentoptions3), [DWM_SYSTEMBACKDROP_TYPE](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwm_systembackdrop_type)
