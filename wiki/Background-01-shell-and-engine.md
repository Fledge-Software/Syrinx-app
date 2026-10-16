> **Research note dated 2026-10-09.** Read for reasoning, not current fact; later decisions in [`docs/decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/decisions.md) and [`docs/architecture-decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/architecture-decisions.md) win.
>
> This note assumes the Tauri 2 shell and Svelte chrome of the original spec. D3 and D4 replaced both: Syrinx drives WebView2 through wry directly and draws its interface with GPUI. Notes 07 and 08 correct parts of it. Notes 07 and 08 correct parts of the earlier notes, and note 03 assumes Tauri. The product was called Atlas until 2026-10-12; read Atlas here as Syrinx.
>
> Moved from `docs/research/01-shell-and-engine.md` ([original at af3de53](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/01-shell-and-engine.md)) on 2026-10-16.

# Shell and engine: can Atlas run on Tauri 2 child webviews?

**Date:** 2026-10-09. All sources were read that day. Each claim names the source's own version or date, and carries a confidence level: high, medium or low.

**Terms.** A **child webview** is a web view placed at a set position inside an Atlas window, one per tab, created with Tauri's `Window::add_child`. The **chrome webview** is the full-window Svelte UI: the sidebar, the home page and the palette. **`unstable`** is a Cargo feature that unlocks APIs Tauri may change without a major version. **Z-order** is which surface is drawn on top. A **freeze frame** is a screenshot of a page shown in its place while the real web view is hidden. **wry** is Tauri's web view layer, and **webview2-com**, **objc2-web-kit** and **webkit2gtk** are the Rust bindings to the raw engines.

## Questions

1. What is the status of Tauri 2 multi-webview on Windows (WebView2), macOS (WKWebView) and Linux (WebKitGTK)? What is the latest release, and is multi-webview still behind `unstable`? Which open issues matter to a browser?
2. How can Atlas draw its own UI above a page that is a separate native web view, both the palette with a dimmed backdrop and the sidebar that peeks in from the left edge? Which options work on each OS?
3. Do transparent windows and `windowEffects` (Mica, Acrylic, vibrancy, Liquid Glass) work with several child webviews?
4. Which browser plumbing does each engine expose, and does Tauri or wry surface it, or does Atlas need platform code?
5. What are the fallbacks if multi-webview is not viable (CEF, Servo, others), and what would make Atlas switch?
6. What should the Phase 0 spike build and measure, so that it ends with a per-OS verdict?
7. Added by the owner: since performance and efficiency decide, what are the performance numbers for each option?

## Short answer

- **Multi-webview is still unstable, and nobody is assigned to it.** The newest stable release is tauri 2.12.2 (2026-10-09), and `Window::add_child` still needs `unstable` there and in 3.0.0-alpha.4. No stabilisation has been announced, and a maintainer wrote in September 2025 that the team has "no capacity to look into multiwebview issues". (high)
- **Windows and macOS are workable, but Atlas will own platform code.** Another open-source Tauri app, codeg, shipped browser tabs as child webviews on both in September 2026. Atlas will need its own code for keyboard shortcuts, z-order, screenshots, find in page, favicons and, on Windows, crash recovery. (medium)
- **Linux is not viable on stock Tauri.** Tauri packs every child webview into a vertical GTK box and ignores its bounds, so tabs cannot be laid out; the fix is a set of open, unreviewed pull requests. (high)
- **HTML in the chrome can never paint over a page.** Atlas has to cover the page with another native surface: raise a web view above it, or show a freeze frame and hide the page. Freeze frames have a shipping precedent and need no web view transparency, which is fragile on Windows: WebView2 runtime 154 was reported on 2026-10-08 to paint transparent web views opaque. (medium)
- **Window effects compose with child webviews, but cost power on macOS.** On macOS the effect view sits beneath every web view, so Haze and Glass can work, but one report measured a transparent window at about 8 times the GPU power of an opaque one on a static page. On Windows, Mica and Acrylic depend on the same fragile transparency. Linux has no effects. (medium)
- **Plumbing is about half done in Tauri.** Tauri covers navigation, titles, downloads (without progress), popups (only as new windows), permissions (2.12.0), devtools, user agent, cookies and clearing. Missing are find in page, favicons, back and forward (they arrive in 3.0 alpha), context menus, fullscreen video, crash recovery outside macOS, and screenshots. All of them exist in the engines. (high)
- **The only realistic fallback is Tauri 3's CEF runtime, and it is not ready.** It is alpha and runs unsandboxed on Windows. It still uses native child windows, so it does not fix overlays. It adds hundreds of MB, and Chromium ships every two weeks. Servo cannot yet browse the general web. (high)
- **No primary source gives memory per web view for any engine,** so the spike has to measure it. WebView2 tabs share one browser process, WebKit gives each web view its own process up to a limit, and Tauri injects its IPC scripts into every web view, web pages included. (high)
- **Recommendation:** run Phase 0 on Tauri 2.12 with `unstable`, Windows first. Use freeze frames for the palette and a small opaque overlay web view for the sidebar peek. Put all raw-engine calls behind one platform module, and defer Linux. Switch to CEF only if the Windows spike fails on layout, shortcuts or crash recovery and no platform fix exists; overlays alone are not a reason, because CEF has the same limit.

## Findings

### 1. Multi-webview status and the issues that matter

**Releases and maintenance**
- **Versions:** tauri 2.12.0 was published 2026-09-26 and 2.12.2 on 2026-10-09; the 3.0 alpha line runs from alpha.0 (2026-09-13) to alpha.4 (2026-10-01) ([crates.io](https://crates.io/api/v1/crates/tauri)). wry 0.57.0 is from 2026-09-08 ([crates.io](https://crates.io/api/v1/crates/wry/versions)). (high)
- **Still gated:** `add_child` is "Available on desktop and crate feature `unstable` only" in both [2.12.2](https://docs.rs/tauri/2.12.2/tauri/window/struct.Window.html) and [3.0.0-alpha.4](https://docs.rs/tauri/3.0.0-alpha.4/tauri/window/struct.Window.html). In 3.0 the flag moves to the runtime crate ([alpha.0 notes](https://github.com/tauri-apps/tauri/releases/tag/tauri-v3.0.0-alpha.0)). (high)
- **Pinning:** 2.12.0 locked the unstable crates to minor versions, so patch releases should not break them but minor ones may ([changelog](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-runtime-wry/CHANGELOG.md), #15887). (high)
- **Maintenance:** the "no capacity" statement is in [wry#1177](https://github.com/tauri-apps/wry/issues/1177) (2025-09-23). [23 issues are open](https://github.com/tauri-apps/tauri/issues?q=is%3Aissue%20state%3Aopen%20label%3A%22scope%3A%20unstable%20flag%22) under "scope: unstable flag", most without a maintainer reply. (high)

**How each OS builds a child webview** (from source on the `dev` branches, which match 2.12.2 and 0.57.0)
- **Windows:** each web view sits in its own `WRY_WEBVIEW` child HWND (`WS_CHILD | WS_CLIPCHILDREN`) placed at `HWND_TOP` when created. Later `set_bounds` calls keep the order (`SWP_NOZORDER`) ([wry webview2](https://github.com/tauri-apps/wry/blob/dev/src/webview2/mod.rs)). (high)
- **macOS:** the WKWebView is added as a subview of the window's content view, so the newest view is on top ([wry#1763](https://github.com/tauri-apps/wry/issues/1763), wry source). (medium)
- **Linux:** the child goes into the window's vertical `GtkBox` ([tauri-runtime-wry](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-runtime-wry/src/lib.rs)). But wry's `set_bounds` only acts when the parent is a `GtkFixed` ([wry webkitgtk](https://github.com/tauri-apps/wry/blob/dev/src/webkitgtk/mod.rs)), so GTK divides the window among the children. This is [tauri#10420](https://github.com/tauri-apps/tauri/issues/10420), open since July 2024; the fixes are draft [tauri#15463](https://github.com/tauri-apps/tauri/pull/15463) and [wry#1745](https://github.com/tauri-apps/wry/pull/1745), both unreviewed. (high)

**Issues that matter to a browser** (states as of 2026-10-09)

| Area | What happens | OS | State |
|---|---|---|---|
| Focus | Content lost keyboard focus after Alt+Tab ([#13466](https://github.com/tauri-apps/tauri/issues/13466), [#15624](https://github.com/tauri-apps/tauri/issues/15624)); fixed in 2.12.0 by #15625 and #15950 | Windows | Fixed (high) |
| Shortcuts | Menu accelerators don't fire while WebView2 has focus ([wry#451](https://github.com/tauri-apps/wry/issues/451), priority high, "upstream") | Windows | Open since 2021 (high) |
| Shortcuts | Cmd shortcuts fail in multi-webview ([wry#1177](https://github.com/tauri-apps/wry/issues/1177)); Cmd+A/C/V broken under `unstable` ([#9257](https://github.com/tauri-apps/tauri/issues/9257)) | macOS | Open; not retested on 2.12 (medium) |
| Text input | Double characters ([#8705](https://github.com/tauri-apps/tauri/issues/8705)); arrow keys type junk ([#10194](https://github.com/tauri-apps/tauri/issues/10194)) | macOS | Open, 2024 reports (medium) |
| Layout | Bounds ignored ([#10420](https://github.com/tauri-apps/tauri/issues/10420)); wrong bounds on Wayland with scaling ([#15656](https://github.com/tauri-apps/tauri/issues/15656)) | Linux | Open (high) |
| Resizing | Fixed position and `auto_resize` can't combine ([#9611](https://github.com/tauri-apps/tauri/issues/9611)); a custom-title-bar window won't resize ([#12207](https://github.com/tauri-apps/tauri/issues/12207)); `set_bounds` is parent-relative, undocumented ([wry#1875](https://github.com/tauri-apps/wry/issues/1875), 2026-10-04) | Windows | Open (high) |
| Z-order | No API to order web views ([#11124](https://github.com/tauri-apps/tauri/issues/11124)); a child always covers the main web view ([#15682](https://github.com/tauri-apps/tauri/issues/15682)) | All | Open (high) |
| DPI | `HDC` leak in DPI lookups fixed in 2.11.4 (#15614). No open Windows or macOS DPI bug for child webviews found | Windows | Spike must test mixed-DPI monitors (medium) |
| Crashes | SIGSEGV after destroying a second web view window; workaround: hide, wait about 200 ms, destroy ([wry#1730](https://github.com/tauri-apps/wry/issues/1730)) | macOS 26.3 | Open (high) |
| Crashes | Re-entered parent subclass crashed | Windows | Fixed in wry 0.56.1, #1795 (high) |
| Leaks | WebKitGTK IPC handler keeps each closed web view alive; the renderer went blank near 400 closes ([wry#1871](https://github.com/tauri-apps/wry/issues/1871), 2026-10-01) | Linux | Open (high) |
| Input | Cursor flickers in a child layered over the host web view ([wry#1763](https://github.com/tauri-apps/wry/issues/1763)) | macOS | Open (high) |
| Chrome | Child web view not clipped to the window's corner radius ([#14165](https://github.com/tauri-apps/tauri/issues/14165)); traffic-light position ignored ([#14072](https://github.com/tauri-apps/tauri/issues/14072)) | macOS | Open (high) |

**What other projects did**
- **codeg** ships browser tabs as wry child webviews on macOS and Windows. It uses "occlusion leases" and freeze frames ([codeg PR #723](https://github.com/xintaofei/codeg/pull/723), merged 2026-09-19). Its Linux path is a separate top-level window that, by its own account, was never executed. (high)
- **Readest** moved its Linux builds to Tauri's CEF runtime ([PR #6218](https://github.com/readest/readest/pull/6218), 2026-09-16). (high)
- **athas** went the other way: it dropped CEF for the system web views, citing 187–455 MB Linux packages and a git-pinned Tauri ([PR #767](https://github.com/athasdev/athas/pull/767), 2026-10-04). (high)

### 2. Drawing Atlas UI above a page

**Why CSS cannot do it.** A child webview is its own OS surface: an HWND, an NSView or a GtkWidget. A commenter on [#15682](https://github.com/tauri-apps/tauri/issues/15682) (not a maintainer) called this expected native behaviour, and Microsoft's WebView2 team said in 2020 that windowed WebView2 is always on top ([WebView2Feedback#708](https://github.com/MicrosoftEdge/WebView2Feedback/issues/708)). CSS `backdrop-filter` cannot see another surface either ([#15512](https://github.com/tauri-apps/tauri/issues/15512), maintainer, June 2026). (high)

**Option verdicts**

| Option | Windows | macOS | Linux |
|---|---|---|---|
| **a. Raise the transparent chrome webview** above the page and draw the dim in CSS | **Unproven.** Background alpha may only be 0 or 255 ([MS Learn](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller2)). Runtime 154 paints transparent views opaque ([#5752](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5752), open). Transparent areas still take every click ([#1594](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1594)) | **Works with caveats.** Views are layer-backed and overlap ([#9798](https://github.com/tauri-apps/tauri/issues/9798), [wry#1763](https://github.com/tauri-apps/wry/issues/1763)); cursor flicker | **No** on stock Tauri (bounds). X11 child views don't support transparency ([wry#1139](https://github.com/tauri-apps/wry/issues/1139)) |
| **b. A small opaque overlay webview** sized to the sidebar, raised over the page | **Works.** One `SetWindowPos(HWND_TOP)` on its HWND (wry 0.56 `hwnd()`); no transparency needed | **Works.** `addSubview:positioned:relativeTo:` on its WKWebView | **No** on stock Tauri |
| **c. Freeze frame:** capture, show the image in the chrome, hide the page | **Works.** `CapturePreview` is asynchronous and fails before the first `ContentLoading` ([MS Learn](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2)) | **Works.** `takeSnapshot`; codeg reports JPEG capture in "a few milliseconds" ([PR #723](https://github.com/xintaofei/codeg/pull/723)) | **In principle.** `webkit_web_view_get_snapshot` is asynchronous ([docs](https://webkitgtk.org/reference/webkit2gtk/stable/method.WebView.get_snapshot.html)); white snapshots seen on Wayland ([bug 167760](https://bugs.webkit.org/show_bug.cgi?id=167760), 2017) |
| **d. WebView2 visual hosting** (`CompositionController`) | **Heavy.** The host must forward all mouse, touch and pen input and handle scaling ([MS Learn](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/windowed-vs-visual-hosting), updated 2026-05-27). wry never uses it, so this means replacing wry's Windows backend | n/a | n/a |
| **e. A separate transparent top-level window** owned by the main one | **Caveats.** Focus moves to the overlay. Mica turns solid when a window deactivates ([MS Learn](https://learn.microsoft.com/en-us/windows/apps/design/style/mica)) | **Caveats.** The main window loses key status | **No** on Wayland: windows can't be positioned ([tao docs](https://docs.rs/tao/latest/tao/window/struct.Window.html)) |
| **f. Shrink the page** | Works, but the design says the peek must not resize the page | same | same, but bounds are broken |

**Notes on these options**
- Neither Tauri nor wry exposes screenshots ([wry#1358](https://github.com/tauri-apps/wry/issues/1358) is open) or a raise call, so options a, b and c all need platform code through `Webview::with_webview`. A tab created after the overlay lands above it, so Atlas must raise the overlay again after every new tab. (high)
- Hiding a WebView2 throttles it and purges some caches, so a frozen page is cheap while hidden; its video keeps playing audio unseen. (medium)
- **Hover detection:** the chrome cannot see the pointer while it is over a page. Leave a few-pixel strip of chrome at the left edge, or poll `cursor_position()`. (medium)
- **How Electron differs:** `addChildView` on an existing child moves it to the top ([Electron docs](https://www.electronjs.org/docs/latest/api/view)). This works because Chromium composites its own views, which Tauri cannot copy. (high)

### 3. Window effects with several child webviews

**How Tauri applies them.** Tauri passes the window handle to `window-vibrancy`: Mica, Acrylic or Blur on Windows. On macOS the `NSVisualEffectView` or `NSGlassEffectView` goes in with `NSWindowOrderingMode::Below` and no relative view, so it sits beneath every existing subview; Liquid Glass is tried first and falls back to vibrancy on macOS 15 and older ([tauri vibrancy source](https://github.com/tauri-apps/tauri/tree/dev/crates/tauri/src/vibrancy), [window-vibrancy source](https://github.com/tauri-apps/window-vibrancy/tree/dev/src/macos)). (high) So on macOS the web views stay above the effect, which shows through wherever the chrome webview is transparent, and opaque page web views cover it, as the design wants. (medium, inferred from the source; the spike confirms it)

**macOS**
- **Private API gone:** transparency stopped needing a private API in tauri-runtime-wry 2.12.1 (#16166) and wry 0.56.0 (#1780) ([changelog](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-runtime-wry/CHANGELOG.md)). (high)
- **Transparency keeps the GPU busy on a static page** ([#15471](https://github.com/tauri-apps/tauri/issues/15471), M5 Pro, macOS 26.6, Tauri 2.11.2, labelled "upstream"):

  | | GPU power | GPU active residency |
  |---|---|---|
  | `transparent: true` | about 620 mW | 36% |
  | `transparent: false` | about 75 mW | 10% |

  (high for the measurement; medium that it still holds after the 2.12.1 change)

- **Rounded page card:** the WKWebView's layer must be clipped by platform code ([#14165](https://github.com/tauri-apps/tauri/issues/14165)). (high)

**Windows**
- Acrylic has "bad performance when resizing/dragging" on Windows 10 1903+ and Windows 11 build 22000 ([Effect docs](https://docs.rs/tauri/2.12.2/tauri/window/enum.Effect.html)). (high)
- A chrome webview over Mica needs alpha-0 transparency, so it is exposed to [#5752](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5752), and a parent with `WS_CLIPCHILDREN` turned a transparent child black ([#12450](https://github.com/tauri-apps/tauri/issues/12450)). (high) No source shows Mica and several child WebView2s failing together, so this is a spike item. (medium)

**Linux and CEF.** Linux has no effects, because the compositor decides ([window-vibrancy](https://github.com/tauri-apps/window-vibrancy)). The CEF runtime shows a black screen with transparency ([#15718](https://github.com/tauri-apps/tauri/issues/15718), open). (high)

### 4. Browser plumbing

Through `Webview::with_webview`, Tauri hands over the raw engine object on the main thread: `controller()` and `environment()` on Windows, a WKWebView pointer on macOS and a `webkit2gtk::WebView` on Linux. The docs say to pin Tauri's minor version when using it ([Webview docs](https://docs.rs/tauri/2.12.2/tauri/webview/struct.Webview.html)). (high) The table checks tauri 2.12.2 and wry 0.57.0 against [Microsoft's WebView2 reference](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2), Apple's WebKit docs and the [WebKitGTK reference](https://webkitgtk.org/reference/webkit2gtk/stable/). Confidence is high unless marked.

| Feature | What Tauri 2.12 / wry give | Platform code Atlas needs |
|---|---|---|
| Downloads | `on_download`: `Requested` (block or change the path) and `Finished` (on macOS the path is empty). No progress, cancel or pause | WebView2 `DownloadStarting`; `WKDownload`; `WebKitDownload` |
| New windows and popups | `on_new_window` can return only a whole window. wry's handler can return a web view (0.53+) | Use wry's handler or the engine call (`put_NewWindow`, `createWebViewWith:`, the `create` signal) to open a popup as a tab and keep `window.opener` |
| Permissions | `on_permission_request` (2.12.0). wry can't do geolocation or notifications on macOS, and clipboard read only on Windows. **With no handler, wry's macOS delegate grants camera and microphone to every site** ([source](https://github.com/tauri-apps/wry/blob/dev/src/wkwebview/class/wry_web_view_ui_delegate.rs)) | Always install the handler. Persist decisions with WebView2 `SetPermissionState` |
| Devtools | `open_devtools` and related calls; release builds need the `devtools` feature. Windows can't close devtools. The macOS path is a private API | none |
| Find in page | Nothing in Tauri or wry | WebView2 `ICoreWebView2_28::Find` (bound in webview2-com-sys 0.39.1); WKWebView `find(_:configuration:)`; `WebKitFindController` |
| Printing | wry `print()` runs `window.print()` on Windows, a print operation on macOS and a dialog on Linux; no print-to-PDF | `ShowPrintUI` and `PrintToPdf`; `printOperation`; `WebKitPrintOperation` |
| Fullscreen video | wry enables it on macOS 12.3+ only | WebView2 `ContainsFullScreenElementChanged`; GTK `enter-fullscreen`. Resize the tab to fill the window |
| Context menus | Not in Tauri; wry has Windows-only `with_default_context_menus` | WebView2 `ContextMenuRequested`; GTK `context-menu`; macOS has no public delegate, so use a JS `contextmenu` handler plus a native menu (medium) |
| User agent | `user_agent()`, at creation only | Settings or properties per engine to change it later |
| Storage isolation and clearing | `data_directory`, `data_store_identifier` (macOS 14+), `incognito`, all-or-nothing `clear_all_browsing_data`, cookie get, set and delete | Clearing by kind or time: `ClearBrowsingData`, `removeData`, website data manager |
| PDF | Built into WebView2 and WKWebView; WebKitGTK uses PDF.js since 2.38 (medium) | none |
| Navigation, titles, favicons | `on_navigation` (cancelable), `on_page_load`, `on_document_title_changed`. No favicons, pushState URL changes or progress | WebView2 `FaviconChanged`; key-value observation on `URL` and `estimatedProgress`; macOS has **no favicon API**, so read `<link rel=icon>` with a script; GTK `favicon` needs its database |
| Back and forward | In wry 0.56 (`go_back`, `can_go_back`), but not exposed in Tauri 2.12; they arrive in 3.0.0-alpha.4. No change event | `eval("history.back()")` or the raw object |
| Crashed renderer | macOS only: `on_web_content_process_terminate`, which by default reloads (2.11.0) | WebView2 `ProcessFailed` (renderer exit: `Reload`; browser exit: recreate every view, per [MS Learn](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-related-events)); GTK `web-process-terminated` |

**Keyboard shortcuts**
- **Windows:** use WebView2's `AcceleratorKeyPressed`, which fires for Ctrl or Alt combinations and Escape while a page has focus ([MS Learn](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller)). wry does not register it. (high) The newer `AllowHostInputProcessing` option is set on the controller options, which wry builds itself, and a July 2025 comment on [wry#451](https://github.com/tauri-apps/wry/issues/451) says it breaks focus return. (medium)
- **macOS:** menu key equivalents. **Linux:** GTK accelerators. Both need confirming under multi-webview.
- **Extensions:** WebView2 can load unpacked Chrome extensions (wry `with_browser_extensions_enabled`, runtime 120+), but their toolbar UI has nowhere to appear. This is noted only because the spec says the OS web views can't run extensions. (medium)

### 5. Fallbacks

**CEF through Tauri 3 (`tauri-runtime-cef`)**
- **Status:** official but alpha, from alpha.0 (2026-09-13) to alpha.5 (2026-10-01). The docs say its APIs "may still change" ([v3 CEF docs](https://v3.tauri.app/develop/cef/)). (high)
- **Same model as wry:** multi-webview goes through `WebviewBuilderCefExt`, still behind `unstable`. Web views are still native child windows. Off-screen rendering is only an open request with a draft PR ([#15868](https://github.com/tauri-apps/tauri/issues/15868), [#16223](https://github.com/tauri-apps/tauri/issues/16223), 2026-10-07). So CEF has the same overlay limits as WebView2. (high)
- **Sandbox:** on Windows the Chromium sandbox does not run under any policy, which matters a great deal for a browser. (high)
- **Size:** bundles are "hundreds of megabytes larger", and the first build downloads about 1 GB ([v3 CEF docs](https://v3.tauri.app/develop/cef/)). (high)
- **Open bugs:** Linux idles at 100% of a core ([#16189](https://github.com/tauri-apps/tauri/issues/16189)), and transparency gives a black screen ([#15718](https://github.com/tauri-apps/tauri/issues/15718)). (high)
- **Licence:** CEF is BSD and the bindings are Apache-2.0/MIT. The `cef` crate is at 154.5.0, released every few days ([crates.io](https://crates.io/api/v1/crates/cef)), while the runtime still pins 152. (high)
- **Update burden:** Chrome moved to a two-week stable cycle with Chrome 153 on 2026-09-08 ([Chrome blog](https://developer.chrome.com/blog/chrome-two-week-start)). CEF builds every other milestone (about 4 weeks), with extended builds every 4th (about 8 weeks) ([CEF branches](https://chromiumembedded.github.io/cef/branches_and_building)). Atlas would have to ship a new Chromium at least every 4–8 weeks. (high)

**Servo: not viable for a daily browser in 2026.** It has released monthly since `servo` 0.1.0 (2026-04-13), and the newest is 0.7.0 (2026-10-05), MPL-2.0, with downloads of about 50–110 MB per platform ([releases](https://github.com/servo/servo/releases)). It is "not a 1.0" and breaks its API monthly ([Servo blog](https://servo.org/blog/2026/04/13/servo-0.1.0-release/)), and its web compatibility is still being fixed site by site ([July 2026 report](https://servo.org/blog/2026/08/31/july-in-servo/)). Verso, the Tauri bridge, was archived on 2025-10-08 ([repo](https://github.com/versotile-org/verso)). (high)

**Others.** Electron (v44.7.0, 2026-10-07) is mature and solves overlays with Chromium's own views, but is not Rust ([releases](https://github.com/electron/electron/releases)). (high) Ultralight is proprietary and an HTML UI renderer rather than a browser ([pricing](https://ultralig.ht/pricing/)). (high) Ladybird is pre-alpha with no embedding API, and Gecko has no desktop embedding API. (medium)

**What would make Atlas switch to CEF:** child webviews that can't be laid out, focused or kept alive reliably on Windows or macOS even with platform code; shortcuts that can't be caught on Windows; or WebKit compatibility on macOS that proves unacceptable for the owner's sites. Overlays are not a reason, since CEF has the same limit. Even then, wait for the runtime to leave alpha and for the Windows sandbox to work.

### 6. Phase 0 spike checklist

**Build** (Windows first, then macOS; Linux only as a timeboxed patch trial)

1. **Window:** one Tauri 2.12.2 window with `unstable`, a full-window chrome webview (a sidebar stub plus an address field), and a page area with rounded corners.
2. **Tabs:** create, switch and close child webviews from the sidebar. Build one variant through `add_child`, and one created with wry directly so that no Tauri scripts are injected. Switch tabs by hiding and showing.
3. **Address bar:** navigate, reload, back and forward through `with_webview`. Show the title from `on_document_title_changed` and the favicon from platform code.
4. **Overlays:** the palette as a freeze frame (capture, show `<img>` with a dim, hide the page); the sidebar peek as a pre-created opaque overlay webview raised over the page; the transparent raise (option a) as a comparison; and a left-edge hover strip.
5. **Shortcuts:** Ctrl K, T, W, L and Tab while a page has focus. Use `AcceleratorKeyPressed` on Windows and menu key equivalents on macOS.
6. **Effects:** Graphite, Haze and Glass with three tabs open, while dragging and resizing the window.
7. **Crashes:** kill a renderer (by PID or Edge's Browser Task Manager on Windows, the WebContent PID on macOS, `WebKitWebProcess` on Linux), then detect it and reload. On macOS, close tabs by hide-then-destroy.
8. **Focus and DPI:** Alt+Tab, clicking between chrome and page, Tab into a page, typing in form fields (#8705), and dragging the window between 100% and 150% monitors.
9. **Linux:** try PR #15463 with wry #1745, or Atlas's own `GtkFixed` container. Log whether bounds, raising and snapshots work on X11 and on Wayland.

**Measure on each OS**, against Edge on Windows and Safari on macOS loading the same pages on the same machine.

| Measure | How | Proposed pass line (owner may change) |
|---|---|---|
| Cold start to chrome painted, and to the first page loaded | Timestamps from `main` to the first IPC ping and to `on_page_load` Finished | Chrome under 500 ms |
| Memory with 1, 10 and 30 tabs, same site and distinct sites | Sum of private working set across all engine processes | Within 15% of Edge or Safari per tab |
| Memory of a hidden tab after `MemoryUsageTargetLevel` Low or `TrySuspend` (Windows) | Same | Report only |
| Idle CPU and GPU with a static page | Task Manager or WPR on Windows; `powermetrics` on macOS | Under 1% CPU. On macOS, report GPU mW for each palette |
| Tab switch latency | Click to visible | Under 50 ms |
| Palette open (freeze frame) and peek open (raise) | Ctrl K to palette visible, with the capture time on its own; edge hover to sidebar visible | Palette under 100 ms, peek under 16 ms after the animation starts |
| Resize and drag smoothness for each effect | Visible gaps or tearing between surfaces; frame drops | No gaps |
| Shortcut delivery | 20 presses of each shortcut with a page focused | 20 out of 20 |
| Crash recovery | Kill to reloaded | Detected every time |

**Verdict per OS.** *Viable* if layout, shortcuts and crash recovery pass and one overlay approach passes. *Viable with platform code* if it passes only through listed `with_webview` code. *Not viable* if layout, shortcuts or crash recovery fails with no platform fix.

### 7. Performance numbers

**Process model**
- **WebView2:** one browser process, plus GPU and helpers, per user data folder. All tabs in one environment share it. A renderer "can serve frames in multiple" web views, depending on site isolation ([process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model), updated 2026-06-12). Microsoft advises one shared environment for tabbed apps ([performance guide](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance), updated 2026-09-02). (high)
- **WKWebView:** WebKit gives "each web view its own process space until it reaches an implementation-defined process limit". Since macOS 12 the app cannot change this ([WKProcessPool](https://developer.apple.com/tutorials/data/documentation/webkit/wkprocesspool.json)). (high)
- **WebKitGTK:** multiple web processes is the only model since 2.26 ([docs](https://webkitgtk.org/reference/webkit2gtk/stable/method.WebContext.set_process_model.html)). Related views share one process. (high)
- **No published memory per web view:** none of Microsoft, Apple or WebKitGTK gives one, so the spike measures it. (high)

**Tauri's CI benchmarks:** a hello-world app on GitHub Actions runners, from [benchmark_results](https://github.com/tauri-apps/benchmark_results) (2026-10-08 and 09). The project warns that results from different OSes are not comparable. (medium)

| | Windows | macOS | Linux |
|---|---|---|---|
| Startup to `DOMContentLoaded`, Tauri | 0.44 s | 1.67 s | 0.745 s |
| Startup to `DOMContentLoaded`, wry alone | 0.58 s | not recorded | 0.740 s |
| Peak memory, Tauri | not recorded | not recorded | 416 MB |
| Peak memory, wry alone | not recorded | not recorded | 265 MB |
| Binary | 2.8 MB | 2.9 MB | 3.0 MB |

The memory method (`time -v`) and why Tauri reads 150 MB higher are not explained (low). Electron was last measured in 2023, so it is not comparable.

**Overlay costs**
- **Raising a surface:** one `SetWindowPos` or NSView call. No measurement was found. (medium)
- **An overlay webview:** on Windows it adds at most one renderer and shares the browser and GPU processes (inferred from the process model); on macOS it adds one WebContent process. (medium)
- **A freeze frame:** capture, encode, IPC and decode. codeg reports "a few milliseconds" on macOS; no primary number exists for `CapturePreview` or `get_snapshot`. Hiding the page then saves its rendering work. (medium)

**Idle.** On macOS a transparent window draws about 8 times the GPU power (section 3). (high) CEF on Linux idles at about 105% of a core, falling to about 3% with a proposed patch ([#16189](https://github.com/tauri-apps/tauri/issues/16189)). (high)

**Where Tauri adds overhead compared with the bare engine**
- **Injected scripts:** Tauri prepends its IPC, event and plugin scripts to every web view it builds, with no check on the URL, so every web page loads them ([manager/webview.rs](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri/src/manager/webview.rs)). On Windows wry also adds them to subframes. (high)
- **Event-loop hop:** a web view call made off the main thread is posted through the event loop, and getters such as `bounds()` block ([tauri-runtime-wry](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-runtime-wry/src/lib.rs)). (high)
- **Custom protocol on Windows:** served through `WebResourceRequested` with copies into a memory stream (wry source). (high)
- **IPC:** JSON through serde. A 10 MB binary payload took about 5 ms on macOS and 200 ms on Windows, but that is an informal test relayed by a maintainer ([discussion](https://github.com/orgs/tauri-apps/discussions/11915)). (low)
- **Memory APIs not passed through:** WebView2's `MemoryUsageTargetLevel` is in wry but not Tauri, and `TrySuspend` is in neither ([wry source](https://github.com/tauri-apps/wry/blob/dev/src/webview2/mod.rs)). (high)

## What this means for Atlas v1

**Build**
- **Tauri:** pin 2.12 (`=2.12`) with `unstable`. Don't adopt the 3.0 alpha in Phase 0, but recheck it at Phase 1 for back and forward and the runtime split.
- **Page web views as Atlas-owned objects:** build tab web views without Tauri's IPC scripts (try wry directly in the spike). That saves a script load per page and keeps Fledge's IPC away from arbitrary sites. The chrome webview stays a normal Tauri web view.
- **One `shell::platform` module per OS, behind a Rust trait.** It holds all `with_webview` code: raise, capture, find, favicon, back and forward, shortcuts, crash recovery, fullscreen and context menu. The core model never sees an engine type.
- **Overlays:** the palette is a freeze frame, which is robust on every OS, needs no transparency and allows real blur behind the palette. The peek is a pre-created opaque sidebar webview raised over the page.
- **Permissions:** install `on_permission_request` from day one, because of the macOS default grant.
- **Memory on Windows:** for hidden tabs, call `MemoryUsageTargetLevel` Low through platform code.

**Defer:** Linux, until #15463 lands or Atlas decides to own a GTK container; WebView2 visual hosting; CEF; extensions; and tab discarding beyond hide plus the memory level.

**Risks**
- **Maintenance:** no maintainer capacity for multi-webview, and minor-version churn in `unstable`.
- **Evergreen WebView2 regressions:** #5752 shows the runtime can break transparency without any Atlas change.
- **macOS:** the crash on destroy (#1730), input bugs (#8705, #9257) and the GPU cost of see-through palettes.

## Open questions for the owner

1. **Linux:** Is Linux needed in v1, or can it wait for Tauri's fix (#15463)?
2. **Frozen page under the palette:** Is it acceptable that the page behind the palette freezes, so a video's picture stops while its sound continues?
3. **macOS see-through palettes:** On macOS, should Haze and Glass ship enabled if the spike confirms the GPU cost, or should Graphite be the only default there?
4. **Patched forks:** Is a small patched fork of wry or tao acceptable, for example for the Linux `GtkFixed` container or Windows input options?
5. **Fallback if Windows fails:** Would you accept CEF's size, the work of a Chromium update every 4–8 weeks and an unsandboxed Windows build? Or would you rather reopen the Tauri decision?

## Sources

All were read on 2026-10-09. Issue states are as of that day.

- **Releases and docs:** [tauri versions](https://crates.io/api/v1/crates/tauri), [wry versions](https://crates.io/api/v1/crates/wry/versions), [tauri releases](https://github.com/tauri-apps/tauri/releases), [tauri 3.0.0-alpha.0 notes](https://github.com/tauri-apps/tauri/releases/tag/tauri-v3.0.0-alpha.0) (2026-09-13), [Tauri 2.12 blog](https://v2.tauri.app/blog/tauri-2.12/) (2026-09-26), `Window` docs for [2.12.2](https://docs.rs/tauri/2.12.2/tauri/window/struct.Window.html) and [3.0.0-alpha.4](https://docs.rs/tauri/3.0.0-alpha.4/tauri/window/struct.Window.html), [`Webview` docs](https://docs.rs/tauri/2.12.2/tauri/webview/struct.Webview.html), [`Effect`](https://docs.rs/tauri/2.12.2/tauri/window/enum.Effect.html), [wry 0.57.0 docs](https://docs.rs/wry/latest/wry/)
- **Source code** on the `dev` branches:
  - Tauri: [tauri-runtime-wry lib.rs](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-runtime-wry/src/lib.rs) and its [CHANGELOG](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-runtime-wry/CHANGELOG.md), [manager/webview.rs](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri/src/manager/webview.rs), [tauri vibrancy](https://github.com/tauri-apps/tauri/tree/dev/crates/tauri/src/vibrancy)
  - wry: [webview2](https://github.com/tauri-apps/wry/blob/dev/src/webview2/mod.rs), [webkitgtk](https://github.com/tauri-apps/wry/blob/dev/src/webkitgtk/mod.rs), [WKWebView UI delegate](https://github.com/tauri-apps/wry/blob/dev/src/wkwebview/class/wry_web_view_ui_delegate.rs), [CHANGELOG](https://github.com/tauri-apps/wry/blob/dev/CHANGELOG.md); [window-vibrancy](https://github.com/tauri-apps/window-vibrancy) ([macOS source](https://github.com/tauri-apps/window-vibrancy/tree/dev/src/macos))
- **Tauri issues and pull requests:** [#8705](https://github.com/tauri-apps/tauri/issues/8705), [#9257](https://github.com/tauri-apps/tauri/issues/9257), [#9611](https://github.com/tauri-apps/tauri/issues/9611), [#9798](https://github.com/tauri-apps/tauri/issues/9798), [#10194](https://github.com/tauri-apps/tauri/issues/10194), [#10420](https://github.com/tauri-apps/tauri/issues/10420), [#11124](https://github.com/tauri-apps/tauri/issues/11124), [#11376](https://github.com/tauri-apps/tauri/issues/11376), [#12207](https://github.com/tauri-apps/tauri/issues/12207), [#12450](https://github.com/tauri-apps/tauri/issues/12450), [#13466](https://github.com/tauri-apps/tauri/issues/13466), [#14072](https://github.com/tauri-apps/tauri/issues/14072), [#14165](https://github.com/tauri-apps/tauri/issues/14165), [#15471](https://github.com/tauri-apps/tauri/issues/15471), [#15512](https://github.com/tauri-apps/tauri/issues/15512), [#15624](https://github.com/tauri-apps/tauri/issues/15624), [#15656](https://github.com/tauri-apps/tauri/issues/15656), [#15682](https://github.com/tauri-apps/tauri/issues/15682), [#15718](https://github.com/tauri-apps/tauri/issues/15718), [#15868](https://github.com/tauri-apps/tauri/issues/15868), [#16189](https://github.com/tauri-apps/tauri/issues/16189), [#16223](https://github.com/tauri-apps/tauri/issues/16223), [PR #15463](https://github.com/tauri-apps/tauri/pull/15463), [discussion #11915](https://github.com/orgs/tauri-apps/discussions/11915)
- **wry issues and pull requests:** [#451](https://github.com/tauri-apps/wry/issues/451), [#1139](https://github.com/tauri-apps/wry/issues/1139), [#1177](https://github.com/tauri-apps/wry/issues/1177), [#1358](https://github.com/tauri-apps/wry/issues/1358), [#1730](https://github.com/tauri-apps/wry/issues/1730), [#1763](https://github.com/tauri-apps/wry/issues/1763), [#1871](https://github.com/tauri-apps/wry/issues/1871), [#1875](https://github.com/tauri-apps/wry/issues/1875), [PR #1745](https://github.com/tauri-apps/wry/pull/1745)
- **Microsoft WebView2:**
  - Concepts: [process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model) (updated 2026-06-12), [performance](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance) (updated 2026-09-02), [process events](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-related-events), [windowed vs visual hosting](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/windowed-vs-visual-hosting) (updated 2026-05-27)
  - Reference: [ICoreWebView2](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2), [ICoreWebView2Controller](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller) and [Controller2](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller2) (2026-08-24), [Mica guidance](https://learn.microsoft.com/en-us/windows/apps/design/style/mica)
  - Feedback issues [#708](https://github.com/MicrosoftEdge/WebView2Feedback/issues/708), [#1594](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1594), [#5752](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5752) (2026-10-08)
- **Apple and WebKitGTK:** [WKProcessPool](https://developer.apple.com/tutorials/data/documentation/webkit/wkprocesspool.json), [WebKitGTK reference 2.42](https://webkitgtk.org/reference/webkit2gtk/stable/), [set_process_model](https://webkitgtk.org/reference/webkit2gtk/stable/method.WebContext.set_process_model.html), [get_snapshot](https://webkitgtk.org/reference/webkit2gtk/stable/method.WebView.get_snapshot.html), [WebKit bug 167760](https://bugs.webkit.org/show_bug.cgi?id=167760)
- **Other projects:** [codeg PR #723](https://github.com/xintaofei/codeg/pull/723) (2026-09-19), [Readest PR #6218](https://github.com/readest/readest/pull/6218) (2026-09-16), [athas PR #767](https://github.com/athasdev/athas/pull/767) (2026-10-04), [Tauri benchmarks](https://github.com/tauri-apps/benchmark_results) ([gh-pages data](https://github.com/tauri-apps/benchmark_results/tree/gh-pages), 2026-10-08 and 09)
- **Fallbacks:**
  - CEF: [Tauri v3 CEF docs](https://v3.tauri.app/develop/cef/), [cef crate](https://crates.io/api/v1/crates/cef), [tauri-runtime-cef](https://crates.io/api/v1/crates/tauri-runtime-cef), [CEF](https://github.com/chromiumembedded/cef), [CEF branches](https://chromiumembedded.github.io/cef/branches_and_building), [Chrome two-week cycle](https://developer.chrome.com/blog/chrome-two-week-start) (2026-09-08)
  - Servo: [releases](https://github.com/servo/servo/releases), [Servo 0.1.0 post](https://servo.org/blog/2026/04/13/servo-0.1.0-release/), [July 2026 report](https://servo.org/blog/2026/08/31/july-in-servo/), [Verso (archived)](https://github.com/versotile-org/verso)
  - Others: [Electron releases](https://github.com/electron/electron/releases), [Electron View](https://www.electronjs.org/docs/latest/api/view), [Ultralight pricing](https://ultralig.ht/pricing/), [tao Window](https://docs.rs/tao/latest/tao/window/struct.Window.html)
