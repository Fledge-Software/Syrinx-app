# Engine and UI performance: what Atlas should be built on

**Date:** 2026-10-09

**Status:** Research note 06. It began as "what could an all-Rust Atlas be built on". The same day the owner corrected the brief: Atlas does not have to be all Rust; TypeScript and Svelte are fine. The deciding criteria are performance, efficiency and web compatibility, not language purity. Two decisions still stand. The engine is embedded, not written, and Rust owns state and logic. Related notes: `02-browser-v1-scope.md` (what a daily browser needs) and `02a-tauri-wry-facts.md` (Tauri multiwebview detail).

**Terms used below.**
- A **web view** is an embedded browser engine drawing one page.
- The **chrome** is Atlas's own interface: sidebar, palette and home page.
- A **child window** is an operating-system window placed inside Atlas's window: an HWND on Windows, an NSView on macOS. The system draws it as its own layer.
- A **texture** is an image in GPU memory. **Zero-copy** means one GPU hands it to another without a trip through the CPU.
- **Off-screen rendering (OSR)** means the engine paints into a texture instead of a window, and the app draws that texture itself.
- A **renderer process** runs one or more sites' pages. **Site isolation** gives each site its own renderer.
- **WPT** (Web Platform Tests) is the shared conformance suite all engines run.
- **MSE** (Media Source Extensions) is the API that streaming video players use.
- **EME** is the API for encrypted media, and **Widevine** is Google's DRM behind it.
- **AccessKit** is the Rust bridge to screen readers. **IME** is the input method used for typing Chinese, Japanese and similar languages.
- **Mica** and **Acrylic** are the Windows 11 window materials. **Vibrancy** is the macOS equivalent.
- **DirectComposition (DComp)** is the Windows compositor API that stacks GPU layers.

## Questions

1. Is Servo usable as an embeddable engine in late 2026? Cover how it is released and consumed, its embedding API, platform support, multiple web views, how it renders (WebRender, surfman, ANGLE, rendering to a texture) and how servoshell builds its chrome.
2. Which of the owner's daily sites work in Servo? How does it score on WPT against Chromium, Gecko and WebKit, and what platform features are missing?
3. What other Rust engines and renderers exist (Blitz, Verso and others), and what is each good for in Atlas?
4. Which UI toolkit should draw the chrome and plugin UIs: a Rust toolkit (GPUI, iced, Slint, egui, Dioxus, Makepad, Xilem, Floem) or Svelte in a web view? Where does the web-view option cost measurable performance?
5. How do the architectures compare on web compatibility, effort, size, memory per tab and at idle, process model, cold start, input-to-paint latency, idle CPU, GPU use, overlays, translucency, accessibility, security updates and licence?
6. Which is the most efficient stack that still loads the owner's daily sites, and what Phase 0 spike decides it?

## Short answer

- **Servo cannot be the daily engine yet.** Google sign-in fails. YouTube won't play because MSE is missing. Google Drive crashes and GitHub's Issues tab is dead. There are no downloads, PDF viewer, printing or passkeys. By default it runs as a single process with no sandbox. It is improving fast: its own WPT score rose from 48.2% to 61.6% in 2025 and is 67.8% today. Since April 2026 it has had a real embedding crate. Revisit it as an optional per-tab engine, not for v1.
- **The owner's sites need a Chromium-class engine (or WebKit on macOS).** On Windows the most efficient one is the system's WebView2. It ships with Windows 11, adds about 3 MB to a Tauri app, shares binaries with Edge and updates itself. CEF is the same Chromium but adds hundreds of megabytes and a private copy in memory. It also leaves the developer a security-update duty every 4 to 8 weeks.
- **The engine, not the wrapper, sets the cost of a page.** In one measurement a page in Tauri/WebView2 used 399 MB against 318 MB in Electron. The levers that matter are one shared WebView2 environment and suspending background tabs. Edge's equivalent, sleeping tabs, measured 16–32% less memory and 26–37% less CPU.
- **A Svelte chrome costs measurable performance in three places.**
  - Startup: the chrome cannot paint until WebView2 has started. Microsoft advises against WebView2 for initial UI, and a Tauri hello world takes 0.44 s to its first page event on Windows CI.
  - One more renderer process and JavaScript heap. No published figure exists, so the spike measures it.
  - Forge's editor. Zed claims under 10 ms typing latency against VS Code's 50 ms or more, but that is the vendor's claim.

  It makes no difference to page rendering, scrolling inside a page, or overlays.
- **The page is always a separate child window**, whether the chrome is Rust or Svelte, with an OS web view or CEF in windowed mode. So nothing in Atlas's window can draw over it.
  - The fix is to put the palette and sidebar peek in borderless popup windows above the page. On Windows 11 the system can give those windows real Acrylic blur.
  - A later fix is WebView2 "visual hosting".
  - Only texture engines (Servo, CEF off-screen) make overlays free, and they cost input, IME and accessibility work.
- **Recommendation:** a native Rust chrome on GPUI (Zed's toolkit, used through Longbridge's gpui-kit) plus the system web view through wry.
  - **Fallback:** the same engine with a Tauri 2 + Svelte chrome, which is the spec's plan.
  - **Contingency engine:** CEF through cef-rs, if WebView2 fails a must-have site or Linux becomes a priority.
- **GPUI is the riskiest piece.** Its crates.io release is a year old, so apps use snapshot crates. It has had AccessKit only since May 2026. Zed's editor is GPL, so Forge's editor must come from gpui-kit or be written. Slint is the alternative only if a Servo texture path matters, and its royalty-free licence forbids exposing its API to plugins.
- **Phase 0 runs on Windows.** It builds a GPUI window, WebView2 tabs and popup overlays, then loads the 15 daily sites, Google sign-in first. It measures against Edge and against a Tauri/Svelte control build.

## Findings

### 1. Servo as an embeddable engine

**Releases**
- **Consumed from crates.io.** The `servo` crate is at 0.7.0 (2026-10-05) under MPL-2.0. The first release was 0.1.0 (2026-04-13), and releases have come monthly since ([crates.io](https://crates.io/crates/servo); high). Monthly releases "contain breaking changes". 1.0 is undefined ([Servo blog, 2026-04-13](https://servo.org/blog/2026/04/13/servo-0.1.0-release/); high).
- **LTS line.** A long-term-support (LTS) branch arrives every 6 months and is supported for 9 months, with security fixes only. It is "AS IS" and best-effort, servoshell is out of scope, and git dependencies may be needed ([Servo book: LTS](https://book.servo.org/embedding/lts-release.html); high). 0.6.0 (2026-09-29) is the next LTS base, moving SpiderMonkey to ESR 153 ([v0.6.0 release](https://github.com/servo/servo/releases/tag/v0.6.0); high).
- **Docs.** docs.rs failed to build 0.5 to 0.7. Current API docs live at [doc.servo.org](https://doc.servo.org/servo/index.html) (high).

**Embedding API**
- **Setup.** You implement `EventLoopWaker`, build `Servo` with `ServoBuilder`, supply a `RenderingContext`, and create each web view with `WebViewBuilder::new(&servo, rendering_context)` plus a `WebViewDelegate` ([doc.servo.org](https://doc.servo.org/servo/struct.WebViewBuilder.html); high). The `winit_minimal` example is about 200 lines.
- **Delegate.** It covers URL, title, favicon, load-status and history changes. It also handles `request_navigation`, `request_create_new` (for `window.open`), `request_permission`, `request_authentication` and `show_embedder_control` (selects, dialogs and file pickers). The rest are fullscreen, cursor, notifications, `notify_crashed` and accessibility-tree updates ([WebViewDelegate](https://doc.servo.org/servo/trait.WebViewDelegate.html); high).
- **No downloads.** "Servo currently doesn't support file downloads" ([#43936](https://github.com/servo/servo/issues/43936), 2026-04; high).
- **DevTools.** These speak the Firefox DevTools protocol ([December 2025 post](https://servo.org/blog/2026/01/23/december-in-servo/); high).
- **Upcoming.** A C API for a prebuilt shared library is being designed ([June post](https://servo.org/blog/2026/07/31/june-in-servo/); high).

**Platforms and rendering**
- **Builds.** Every release has Windows x64, macOS, Linux, Android and OpenHarmony builds. The Windows zip is about 100 MB ([v0.7.0 assets](https://github.com/servo/servo/releases/tag/v0.7.0); high). Desktop media needs GStreamer; on Windows that is 1.22.8 from Servo's build deps ([Servo book: embedding](https://book.servo.org/embedding/overview.html); high).
- **Rendering.**
  - Servo renders with WebRender over OpenGL through surfman, and on Windows through ANGLE.
  - `WindowRenderingContext` paints into a window. Its `OffscreenRenderingContext` paints into a framebuffer and exposes a GL blit callback.
  - `read_to_image` copies a frame to the CPU ([RenderingContext](https://doc.servo.org/servo/trait.RenderingContext.html); high).
- **Several web views per window.** Each `WebView` takes its own rendering context, and multiple windows arrived in December 2025 (high).
- **Into another toolkit.** Slint renders Servo into its wgpu scene with zero copy.
  - On macOS it uses IOSurface, and on Linux Vulkan external memory.
  - On Windows it creates a D3D11 shared NT-handle texture from ANGLE, opens it on D3D12 and blits each frame on the GPU.
  - Not done yet: JavaScript-to-Rust messaging and multiple web views ([Slint, 2026-01-19](https://slint.dev/blog/using-servo-with-slint), [2026-05-07](https://slint.dev/blog/servo-with-slint-update); high).
- **How servoshell builds its chrome.** It uses egui, egui_glow and egui-winit. Servo paints off-screen into a GL paint callback on egui's background layer, so egui draws on top of the page. Only the active tab is painted. Each web view's AccessKit tree is grafted in, and the Stop button is unimplemented ([gui.rs](https://github.com/servo/servo/blob/main/ports/servoshell/desktop/gui.rs); high).

**Process model, team and readiness**
- **Process model.** By default all script threads run in the embedder's process ([architecture](https://book.servo.org/design-documentation/architecture.html); high). Multiprocess mode is "not enabled by default" and fully supported only on Linux and macOS ([NLnet project](https://nlnet.nl/project/Servo-Multiprocess/); high). The sandbox is "very likely to be broken everywhere" ([#12868](https://github.com/servo/servo/issues/12868); medium).
- **Team.** Servo is a Linux Foundation Europe project with Igalia as steward. 146 people contributed in 2025 ([Igalia, 2026-01-14](https://blogs.igalia.com/mrego/servo-2025-stats/); high). Donations fund one part-time maintainer ([2026-09-15](https://servo.org/blog/2026/09/15/one-year-of-sponsorship/); high).
- **Readiness.** The download page says "Please don't log into your bank with Servo just yet!" ([servo.org/download](https://servo.org/download/); high).

### 2. Web compatibility for daily use

| Site | Servo 0.7.0 today | Evidence (confidence) |
|---|---|---|
| Google sign-in | Fails: "Cannot log in". reCAPTCHA fails on cross-origin frames, and text in the login field can't be selected | [#38063](https://github.com/servo/servo/issues/38063) (updated 2026-09-10), [#27820](https://github.com/servo/servo/issues/27820) (high) |
| Gmail | Message pane stuck on "Loading..." unless experimental features are on; a scroll panic | [#40711](https://github.com/servo/servo/issues/40711), [#42593](https://github.com/servo/servo/issues/42593) (medium) |
| Google Calendar | One user reports it works in experimental mode | [#40711](https://github.com/servo/servo/issues/40711) comment, 2026-09-09 (low) |
| Google Drive and Docs | Crash; the Docs tracker is open | [#13942](https://github.com/servo/servo/issues/13942) (medium) |
| YouTube | "Your browser can't play this video": no MSE, which is assigned but not started. Search results don't render on Windows | [#41859](https://github.com/servo/servo/issues/41859) → [#22416](https://github.com/servo/servo/issues/22416), [#47963](https://github.com/servo/servo/issues/47963) (high) |
| GitHub | Very slow. Issues tab dead by default (`adoptedStyleSheets` and `IntersectionObserver` missing); links dead in experimental mode | [#9914](https://github.com/servo/servo/issues/9914) (high) |
| Claude.ai, Perplexity, Stripe, Canva, Miro, Notion, Make.com, Neon | No reports either way. Expect failures: these apps rely on features Servo has off by default (IndexedDB, IntersectionObserver, adoptedStyleSheets, service workers, WebGL2) or lacks (WebAuthn) | Inference (low) |
| Mercury | As above, and passkeys are not implemented | [#37148](https://github.com/servo/servo/issues/37148), [download page warning](https://servo.org/download/) (high) |

What works, per Servo's own posts: Google Photos, lichess, Zulip, Speedtest, DuckDuckGo and Gumroad. Google Maps renders but has interaction problems ([June](https://servo.org/blog/2026/07/31/june-in-servo/), [July](https://servo.org/blog/2026/08/31/july-in-servo/) posts; high).

**WPT**
- **Servo's score.** On 2026-10-09, Servo's dashboard scored 0.7.0 at 67.8% of 58,099 tests (93.4% of subtests). Areas: CSS 68.9%, Grid 43.9%, IndexedDB 57.4%, editing 49.6% ([servo.org/wpt](https://servo.org/wpt/), data at [wpt.servo.org/scores.json](https://wpt.servo.org/scores.json); high). The dashboard's history is recomputed against today's test set, so use Igalia's figures for the trend: 48.2% on 2025-01-01 and 61.6% on 2025-12-31 ([Igalia](https://blogs.igalia.com/mrego/servo-2025-stats/); high).
- **No fair cross-engine number.** Servo does not run every WPT test, so its numbers are not comparable with wpt.fyi's. The same-day runs of Chrome 157, Firefox 160 and Safari TP 254 on [wpt.fyi](https://wpt.fyi/results/?product=servo&product=chrome&product=firefox&product=safari) render their totals only in the browser, so this note has no like-for-like percentage. Servo is not part of Interop 2026 (medium).

**Missing features** (defaults from `components/config/prefs.rs` and the [experimental-features page](https://book.servo.org/design-documentation/experimental-features.html); high unless noted)
- **Off unless experimental features are on:** IndexedDB, IntersectionObserver, WebGL2, WebGPU and the async clipboard.
- **Off even then:** service workers (partial, [#40781](https://github.com/servo/servo/issues/40781)), WebRTC (the stack is still being chosen, [#41396](https://github.com/servo/servo/issues/41396); medium), adoptedStyleSheets, Web Animations and container queries.
- **Absent:**
  - MSE and EME. The only EME issue, [#27623](https://github.com/servo/servo/issues/27623), is a 2020 placeholder.
  - Printing and a PDF viewer ([#40892](https://github.com/servo/servo/issues/40892); medium).
  - Passkeys and extensions.
- **Accessibility.** "Very basic" and behind a pref, under a Sovereign Tech Fund commission ([Igalia, 2025-10-09](https://www.igalia.com/2025/10/09/Igalia,-Servo,-and-the-Sovereign-Tech-Fund.html); high).

**Chromium-class engines**
- **WebView2** "uses the same core browser engine as Microsoft Edge" ([performance doc](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance), updated 2026-09-02; high). So the owner's sites should behave as in Edge, with two exceptions:
  - **Google sign-in.** Google blocks sign-in "from all embedded frameworks", "CEF-based apps" included, and accepts only full browsers that identify themselves in the User-Agent string and offer no automation ([Google Developers Blog, 2020-08-28](https://developers.googleblog.com/2020/08/guidance-for-our-effort-to-block-less-secure-browser-and-apps.html); high that the policy exists). How it treats WebView2 inside a full browser is untested; see note 02 (medium).
  - **Widevine.** It is an open, uncommitted request for WebView2 ([WebView2Feedback #4828](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4828); medium). Ordinary YouTube playback needs MSE, not DRM; paid films do need DRM (low, not checked against a YouTube document).
- **CEF** is the same Chromium, and Widevine is possible only with a Google licence and VMP signing ([Widevine overview](https://developers.google.com/widevine/drm/overview); low to medium).
- **macOS and Linux.** macOS's WKWebView is Safari's engine. Linux's WebKitGTK is WebKit too, but wry's child web views there are X11-only ([wry docs](https://docs.rs/wry/latest/wry/struct.WebViewBuilder.html); high).

### 3. Other Rust engines and renderers

- **Blitz** 0.3.0-beta.2 (2026-08-24, MIT or Apache) renders HTML and CSS with Stylo, Taffy, Parley and Vello. It has no JavaScript and is "in a beta state". It is good for rendering Atlas's own HTML natively (a reader view, notes' HTML export, a home page), never for the open web ([DioxusLabs/blitz](https://github.com/DioxusLabs/blitz); high).
- **Verso** (Servo-based) was archived on 2025-10-08: "Verso is currently no longer maintained". Its `tauri-runtime-verso` is effectively dead ([versotile-org/verso](https://github.com/versotile-org/verso); high). Servo's book still points embedders at it (stale).
- **Ultralight** is proprietary. Its free tier has no JIT, a 60 fps cap and no media. It suits embedded UI, not browsing ([pricing](https://ultralig.ht/pricing/); high). Ladybird is C++ and was not checked.
- **Stylo, Taffy and Vello** are the reusable pieces. They are useful inside a native UI, but none is a browser engine (high).

### 4. UI toolkits, and the web-view alternative

| Toolkit | Version (date) | Code and markdown editing | AccessKit | Windows rendering | Translucency | Hosts a foreign surface | Licence |
|---|---|---|---|---|---|---|---|
| GPUI (Zed) | crates.io 0.2.2 (2025-10-22); used via snapshot crates | Best available: gpui-kit claims a 200K-line editor with Tree-sitter and LSP, plus Markdown; Zed's own editor is GPL-3 | Since 2026-05-27, fragile | DirectX 11 and DirectWrite; Zed for Windows shipped 2025-10-15 | Built in: Transparent, Blurred, Mica, Mica Alt | Child HWND via gpui-wry (the page covers GPUI); texture import is macOS-only | Apache-2.0 |
| Slint | 1.18.1 (2026-09-21) | Plain TextEdit, markdown display only | Yes | wgpu or Skia | Swapchain alpha; no Mica API found (low) | wgpu texture import; verified zero-copy Servo | GPLv3, royalty-free (attribution; no exposing its API) or commercial |
| egui | 0.36.2 (2026-09-08) | TextEdit is not a code editor | Always on since 0.34 | wgpu or glow | Via winit and window-vibrancy (unverified) | Paint callbacks and native textures; servoshell proves it with Servo | MIT/Apache |
| iced | 0.14.0 (2025-12-07) | `text_editor` with highlighter; IME added | None ([#552](https://github.com/iced-rs/iced/issues/552)) | wgpu | Not verified | Shader widget gets the wgpu device; iced_wry 0.1 (third-party) | MIT |
| Dioxus | 0.7.10 (2026-07-30) | Desktop *is* a web view (wry); Native (Blitz) is beta | Goal only (native) | WebView2 | As web view | — | MIT/Apache |
| Makepad | crates 1.0.0 (2025-05); repo active | Repo has a code editor | Not documented | DirectX 11 | Not documented | Not documented | MIT |
| Xilem / Masonry | 0.4.0 (2025-10-29) | Experimental | Yes | Vello on wgpu | Not documented | Not documented | Apache-2.0 |
| Floem (Lapce) | crates 0.2.0 (2024-11); repo active | Lapce is a full editor on it | Not documented | wgpu | Not documented | Not documented | MIT |
| Svelte in WebView2 | — | Best ecosystem (CodeMirror, ProseMirror) | Chromium's, plus NVDA issue in frameless Tauri windows ([#12901](https://github.com/tauri-apps/tauri/issues/12901)) | Edge's GPU process | Tauri window effects | Cannot overlay a child web view ([#15682](https://github.com/tauri-apps/tauri/issues/15682)) | — |

Sources for the table:
- GPUI: [crates.io gpui](https://crates.io/crates/gpui), [gpui-component / gpui-kit](https://github.com/longbridge/gpui-component), [Zed for Windows](https://zed.dev/blog/zed-for-windows-is-here), [AccessKit PR #56065](https://github.com/zed-industries/zed/pull/56065), [platform.rs](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/platform.rs) for `MicaBackdrop`, and [gpui-wry docs](https://docs.rs/crate/gpui-wry/latest) ("any GPUI elements behind the WebView bounds will be covered").
- Slint: [licence](https://github.com/slint-ui/slint/blob/master/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md) and [Image::try_from(wgpu::Texture)](https://docs.slint.dev/latest/docs/rust/slint/struct.Image).
- egui: [README](https://github.com/emilk/egui) and [egui-wgpu](https://docs.rs/egui-wgpu/latest/egui_wgpu/struct.Renderer.html).
- iced: [shader Primitive](https://docs.rs/iced/latest/iced/widget/shader/trait.Primitive.html).
- Mostly high; editor-quality claims are the projects' own (medium).

### 5. Performance by architecture

**Process model**
- **Chromium (Edge, WebView2, CEF).** One browser process, a GPU process and helpers, plus renderers assigned by site isolation. Site isolation has been on by default on desktop since Chrome 67, at about 10–13% more memory with many tabs ([Chromium](https://www.chromium.org/Home/chromium-security/site-isolation/); high).
- **WebView2 sharing.** Each user data folder gets one process group. Controllers created from one environment share the browser and GPU processes, and a renderer can serve frames in several WebView2 instances ([process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model), updated 2026-06-12; high). Microsoft's performance page says instead that "each WebView2 control creates its own set of processes" ([performance](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance); the pages conflict, so the spike measures it).
- **WebKit.** WKWebView runs a process per web view up to an implementation-defined limit ([Apple](https://developer.apple.com/documentation/webkit/wkprocesspool); medium).
- **Servo.** A single process by default (see 1).

**Memory**
- **Per tab.** No vendor publishes per-tab figures for WebView2 or CEF. A 2022 Windows 10 measurement found postman.com at 399 MB in Tauri/WebView2 against 318 MB in Electron, and vscode.dev at 370 against 312 MB ([tauri #5889](https://github.com/tauri-apps/tauri/issues/5889); high as a measurement, old). Wrapping Chromium differently does not make pages cheaper.
- **Hello world.** On Linux (WebKitGTK) a Tauri hello world peaks at 288–416 MB, against Electron's 476 MB (2023). Windows memory isn't measured ([tauri-apps/benchmark_results](https://github.com/tauri-apps/benchmark_results), 2026-10-09; medium, CI runners).
- **Background tabs.** Suspending them is the big lever. Edge's sleeping tabs measured 32% less memory and 37% less CPU across about 40,000 devices ([Edge blog, 2020-12-09](https://blogs.windows.com/msedgedev/2020/12/09/sleeping-tabs-beta-performance/)), and later 16% and 26% ([Edge 89, 2021-03-04](https://blogs.windows.com/msedgedev/2021/03/04/edge-89-performance/); high as Microsoft's figures).
  - WebView2's `TrySuspend` is "similar to putting a tab to sleep" and needs the view hidden.
  - `MemoryUsageTargetLevel = Low` trims inactive views ([ICoreWebView2_3](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_3), [performance](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance); high).
- **Servo.** A Blink large-table test used about 4,066 MB in Servo against 730 MB in Firefox, with the leak in the box tree ([#42537](https://github.com/servo/servo/issues/42537), open; high). One Wikipedia page took over 1 GB ([#47520](https://github.com/servo/servo/issues/47520); medium).
- **Native editors.** Zed claims "~600MB RAM" against VS Code's "3-4GB+" ([zed.dev/compare/vscode](https://zed.dev/compare/vscode), 2026-05-01, no method; low as data).

**Cold start**
- **WebView2.** On a cold launch WebView2 "must spin up its processes and disk caches". Microsoft says "Don't use WebView2 for initial UI". A running Edge of the same version starts it faster because "the required WebView2 binaries are already in memory" ([performance](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance); high).
- **Measured.** Time to DOMContentLoaded for a hello world, averaged over warm runs on GitHub runners (2026-10-09; medium):

  | App | Windows | Linux | macOS |
  |---|---|---|---|
  | Tauri | 0.441 s | 0.745 s | 1.67 s |
  | wry alone | 0.576 s | — | — |
  | Electron (2023) | — | 0.475 s | — |

  Source: [benchmark_results](https://github.com/tauri-apps/benchmark_results).
- **Native.** A native chrome paints without waiting for WebView2. No primary first-frame figure exists for GPUI; Zed claims under 1 s against VS Code's 3–5 s or more (vendor; low). Servo publishes none.

**Input-to-paint latency**
- **Inside a page.** No primary measurement exists for WebView2, CEF or Servo. Typing and scrolling inside a page are the engine's job, so a native chrome does not change them.
- **Chrome and Forge.** Zed claims under 10 ms typing latency against VS Code's "can exceed 50ms under load" ([compare page](https://zed.dev/compare/vscode); low as data) and targets 120 fps, an 8.33 ms frame ([Zed blog, 2023](https://zed.dev/blog/videogame); medium). egui takes "1-2 ms per frame" ([README](https://github.com/emilk/egui); high as a claim).

**Idle CPU and battery**
- **Toolkits.** egui repaints only on input or animation. iced 0.14 renders reactively ([#2662](https://github.com/iced-rs/iced/pull/2662); high). A Svelte chrome is also idle when nothing changes, but its web view is always visible, so it can never be suspended.
- **Tabs.** Background-tab suspension is where the battery is won (figures above).

**GPU**
- **Chromium engines.** WebView2 and CEF composite in Chromium's shared GPU process.
- **GPUI.** Adds its own D3D11 device and swap chain on Windows.
- **Servo.** Renders through ANGLE inside Atlas's own process. Slint's bridge does one GPU blit per frame.
- **CEF off-screen.**
  - The off-screen path hands over a D3D11 shared handle, and the frame "must be copied" in the callback.
  - `windowless_frame_rate` defaults to 30 fps.
  - The header still says shared textures are Windows-only ([cef_render_handler.h](https://github.com/chromiumembedded/cef/blob/master/include/cef_render_handler.h); medium).
- No source gives GPU-memory figures.

**Cost of drawing UI above a page**
- **Windowed web views and CEF.** The web view is a child HWND that "always occludes everything under it" ([wry PR #1762](https://github.com/tauri-apps/wry/pull/1762); high). The same holds for GPUI with gpui-wry and for Tauri ([#15682](https://github.com/tauri-apps/tauri/issues/15682), [#11124](https://github.com/tauri-apps/tauri/issues/11124)). Rust and Svelte chromes are equal here.
- **Popup windows.** An owned, borderless popup window above the page costs one more swap chain, composed by the Desktop Window Manager (DWM). On Windows 11 build 22621 and later, `DWMSBT_TRANSIENTWINDOW` gives a window Desktop Acrylic ([Microsoft](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwm_systembackdrop_type); high). That should blur the page underneath, which a CSS blur cannot (medium; the spike confirms it).
- **A transparent second web view on top.** WebView2's background alpha can only be 0 or 255, and "transparent" shows "hosting app content". Whether a sibling web view shows through is undocumented ([DefaultBackgroundColor](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2controller.defaultbackgroundcolor); high and low). It also costs a renderer process.
- **Visual hosting.** WebView2 draws into a DComp visual that Atlas stacks with its own visuals, but Atlas must forward mouse, touch and pen input ([hosting modes](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/windowed-vs-visual-hosting); high). wry lacks it; PR #1762 is open and unreviewed (high).
- **Texture engines** (Servo, CEF off-screen). Overlays are free, but Atlas owns input, IME, drag and drop, and accessibility.

## Comparison of architectures

| Architecture | Daily sites | Effort, one developer | Added download | Memory and processes | Startup | Overlays above a page | Translucency | Accessibility | Security updates | Licence |
|---|---|---|---|---|---|---|---|---|---|---|
| **A.** Rust UI + Servo as a texture (Slint or egui) | Fails Google sign-in, YouTube, Drive, GitHub | Very high: downloads, dialogs, input and accessibility are all Atlas's | About 100 MB, plus GStreamer for media | Single process by default; known bloat bugs | No data | Free: the page is a texture | Full; can blur page pixels | Servo's is basic, behind a pref | Atlas rebuilds monthly or follows LTS (9 months, best effort) | MPL-2.0, plus the toolkit's |
| **B.** Rust UI (GPUI) + system web views via wry | Edge-class on Windows, Safari-class on macOS; Google sign-in to test | High for the chrome and editor; the engine is free | About 3 MB; WebView2 ships with Windows 11 | Chromium model; one shared environment; suspend background tabs | Chrome paints before WebView2 is up | Popup windows (Acrylic), later visual hosting | Mica built into GPUI | The page's is native per child window; GPUI's AccessKit is new | Microsoft updates the Evergreen runtime | Apache-2.0 (GPUI), MIT/Apache (wry) |
| **C.** Rust UI + Chromium via CEF (cef-rs 154) | Chromium everywhere, Linux included; Google's policy names CEF | High; off-screen mode adds input, IME and accessibility forwarding | Large: Linux package 116 MB download, 364 MB installed ([Arch](https://archlinux.org/packages/extra/x86_64/cef/)) | Chromium model; a private copy, not shared with Edge | Its own Chromium cold start | Windowed: as B. Off-screen: free, 30 fps default | Off-screen: full. Windowed: as B | Windowed: Chromium's. Off-screen: Atlas bridges it | Atlas ships each CEF update (about every 4 weeks; extended branches about 8) ([CEF branches](https://chromiumembedded.github.io/cef/branches_and_building); medium) | BSD (CEF), MIT/Apache (cef-rs) |
| **D.** Hybrid: Rust UI, each tab on Servo or the system web view | As B for system-web-view tabs | Highest: two engines | B plus about 100 MB | Both models | As B | Mixed | Mixed | Mixed | Both | Both |
| **E.** Svelte UI in Tauri 2 + system web views (the spec's plan) | As B | Lowest; CodeMirror and ProseMirror exist | About 3 MB | B plus one renderer and a JS heap for the chrome | Chrome waits for WebView2 (0.44 s hello world, Windows CI) | Separate web view or popup windows; no z-order API | Tauri window effects | Chromium's; NVDA bug in frameless windows | Evergreen | MIT/Apache |
| **F.** Svelte UI + CEF (Tauri v3 `tauri-runtime-cef`, alpha.5, 2026-10-01) | As C | Low to medium, but alpha | As C | As C, plus the chrome's renderer | As C | As C | As C | As C | As C | As C |

## What this means for Atlas v1

**Recommendation: architecture B.** It is the most efficient stack that still loads the owner's sites.
- **Engine.** Atlas uses the system web view through wry: WebView2 on Windows, WKWebView on macOS.
  - All tabs come from one `CoreWebView2Environment`.
  - Hidden tabs get `TrySuspend` after a delay, and inactive ones `MemoryUsageTargetLevel = Low`.
- **Chrome.** The sidebar, home page and Forge are native GPUI, pinned to gpui-kit's snapshot crate. GPUI gives the window Mica.
- **Overlays.** The palette and the sidebar peek are GPUI popup windows above the page, with Desktop Acrylic on Windows 11.
- **Why this beats E.** The chrome is on screen before WebView2 starts, a renderer and a JS heap are saved, and Forge gets a native editor. The engine and its update burden are the same.
- **Why it beats A, C and D.** A and D fail the owner's sites today. C costs hundreds of megabytes and a monthly security-release duty for no compatibility gain on Windows or macOS.

**Fallback: architecture E**, Tauri 2 with Svelte, on the same engine. Atlas moves to it if GPUI's churn, its accessibility, or a native editor proves too costly for one developer. Its costs are the measurable but modest ones above, plus the multiwebview gaps listed in note 02a. **Contingency engine: CEF via cef-rs** applies only if WebView2 fails a must-have site or Linux becomes a v1 platform.

**Where "all Rust" would hurt daily browsing.** A Rust engine today means:
- no Google sign-in, YouTube playback or Drive, and a broken GitHub;
- no downloads, PDF viewer, printing or passkeys;
- a single unsandboxed process holding the owner's bank session.

Rust everywhere *except* the engine costs nothing in compatibility, because the engine is the same as in E.

**Defer**
- **Servo as an optional per-tab engine (D).** Re-check at each Servo LTS, roughly every six months, against the site table above.
- **CEF and Linux.** On Linux, wry's child web views are X11-only.
- **WebView2 visual hosting.** Adopt it only if popup overlays fail, or once wry merges PR #1762.
- **Blitz.** For a no-JS reader view.
- **A native WYSIWYG Notes editor.** Notes' rich editor can ship as a local web-view tab first. It costs one renderer only while open.

**Risks**
- **GPUI.** Breaking changes and snapshot crates, AccessKit only months old, and Zed's editor under GPL, which Atlas must not copy.
- **Google sign-in** in embedded web views, which is the same risk for CEF.
- **App shortcuts.** They don't reach Atlas while a page has focus (wry #451, note 02a).
- **Focus hand-off** between a popup overlay and the page.
- **Effort.** One developer building a native code editor.

### Phase 0 spike checklist (architecture B, Windows 11 first)

1. **Window.** A GPUI window (gpui-kit pinned) with a Mica background, a 264px native sidebar and a page-card region. Record cold-start time to the first GPUI frame.
2. **Tabs.**
   - Add WebView2 tabs through wry (`build_as_child`, or gpui-wry) from one shared environment.
   - Create, switch and close 10 tabs. Navigate, go back and forward, and keep the address in sync through wry's handlers.
   - Hide inactive tabs and `TrySuspend` them after 30 s.
3. **Overlays.**
   - The palette (Ctrl K) and the sidebar peek (hover the left edge), each as a borderless GPUI popup window above the page with `DWMSBT_TRANSIENTWINDOW`.
   - Check that the Acrylic blurs the page, that the overlay takes keyboard focus, and that Esc returns focus to the page.
   - Check that Ctrl K, T, W and L work while a page has focus.
4. **Sites, in order, signed in as the owner.** Record pass, partial or fail with a one-line reason:
   1. Google sign-in
   2. Gmail
   3. Google Calendar
   4. Google Drive (open a Doc)
   5. YouTube (play at 1080p, go fullscreen)
   6. GitHub (2FA or passkey, issues, a PR diff)
   7. Claude.ai
   8. Perplexity
   9. Notion
   10. Stripe dashboard
   11. Canva (the editor)
   12. Miro (a board)
   13. Make.com (the scenario editor)
   14. Neon console (OAuth sign-in)
   15. Mercury (last; read-only, passkey or 2FA)
5. **Numbers, against Edge running the same 10 tabs.** Use Task Manager, WebView2's `OpenTaskManagerWindow`, and a WPR trace with Microsoft's `WebView2.wprp` profile. Measure:
   - process count and total private memory at 0, 1 and 10 tabs, and the delta per tab;
   - idle CPU over 5 minutes with 8 tabs suspended;
   - cold start to the first chrome paint and to the first page paint;
   - the palette's open time and typing frame times.
6. **Control.** Build the same sidebar and palette in Tauri 2 + Svelte and take the same numbers, so the Svelte cost is measured, not assumed.
7. **Servo (one hour).** Load the 15 sites in servoshell 0.7.0 with experimental features on, and record the results for the per-tab question.

**Go** if every one of these holds:
- Google sign-in works.
- At least 13 of the 15 sites pass.
- Both overlays draw above a page with correct focus.
- The shortcuts work.
- Memory with 10 tabs is within 10% of Edge's, and idle CPU is near zero.
- The GPUI chrome paints before WebView2 finishes starting.

**No-go, and what follows:**
- Google sign-in blocked: the owner decides (see the questions below). CEF is blocked by the same policy.
- Overlays unworkable: try WebView2 visual hosting through webview2-com, or switch to E.
- GPUI unusable: switch to E.

These thresholds are this note's proposal and the owner may change them.

## Questions to put to the owner

1. Is Linux a v1 platform? If yes, the CEF contingency becomes the plan there, because wry's Linux tabs are X11-only.
2. Do you accept GPUI's churn and young accessibility in exchange for a native, faster chrome and editor? Or would you rather take the Tauri + Svelte fallback and its measured costs?
3. If Google blocks sign-in in an embedded web view, which do you prefer: sign into Google in another browser, accept the gap, or present a different user agent (against Google's stated policy)?
4. Do you use any Widevine-only services, such as Spotify's web player or Netflix? WebView2 has no Widevine.
5. Should Notes' rich editor be native GPUI (more work) or a local web-view tab (sooner, one renderer while open)?
6. Are the go/no-go thresholds right: memory within 10% of Edge, 13 of 15 sites passing?

## Sources

Read 2026-10-09 unless dated.

**Servo**
- Releases and docs: [crates.io](https://crates.io/crates/servo) · [releases](https://github.com/servo/servo/releases) · [0.1.0 post](https://servo.org/blog/2026/04/13/servo-0.1.0-release/)
- Book: [LTS](https://book.servo.org/embedding/lts-release.html) · [embedding](https://book.servo.org/embedding/overview.html) · [architecture](https://book.servo.org/design-documentation/architecture.html) · [experimental features](https://book.servo.org/design-documentation/experimental-features.html)
- API: [API docs](https://doc.servo.org/servo/index.html) · [servoshell gui.rs](https://github.com/servo/servo/blob/main/ports/servoshell/desktop/gui.rs)
- Monthly posts: [December](https://servo.org/blog/2026/01/23/december-in-servo/) · [June](https://servo.org/blog/2026/07/31/june-in-servo/) · [July](https://servo.org/blog/2026/08/31/july-in-servo/) · [sponsorship](https://servo.org/blog/2026/09/15/one-year-of-sponsorship/)
- Other: [download](https://servo.org/download/) · [WPT dashboard](https://servo.org/wpt/) · [Igalia 2025 stats](https://blogs.igalia.com/mrego/servo-2025-stats/) · [Igalia/STF](https://www.igalia.com/2025/10/09/Igalia,-Servo,-and-the-Sovereign-Tech-Fund.html) · [NLnet multiprocess](https://nlnet.nl/project/Servo-Multiprocess/) · [Slint + Servo](https://slint.dev/blog/using-servo-with-slint) · [Slint Windows update](https://slint.dev/blog/servo-with-slint-update)

**Servo issues**
- [#38063](https://github.com/servo/servo/issues/38063) · [#27820](https://github.com/servo/servo/issues/27820) · [#40711](https://github.com/servo/servo/issues/40711) · [#42593](https://github.com/servo/servo/issues/42593) · [#13942](https://github.com/servo/servo/issues/13942) · [#41859](https://github.com/servo/servo/issues/41859) · [#22416](https://github.com/servo/servo/issues/22416) · [#47963](https://github.com/servo/servo/issues/47963) · [#9914](https://github.com/servo/servo/issues/9914) · [#37148](https://github.com/servo/servo/issues/37148)
- [#27623](https://github.com/servo/servo/issues/27623) · [#40781](https://github.com/servo/servo/issues/40781) · [#41396](https://github.com/servo/servo/issues/41396) · [#40892](https://github.com/servo/servo/issues/40892) · [#43936](https://github.com/servo/servo/issues/43936) · [#12868](https://github.com/servo/servo/issues/12868) · [#42537](https://github.com/servo/servo/issues/42537) · [#47520](https://github.com/servo/servo/issues/47520) · [#46070](https://github.com/servo/servo/issues/46070)

**Microsoft and Chromium**
- WebView2: [process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model) · [performance](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/performance) · [hosting modes](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/windowed-vs-visual-hosting) · [distribution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution) · [DefaultBackgroundColor](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2controller.defaultbackgroundcolor) · [TrySuspend](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_3) · [Widevine request #4828](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4828)
- Windows, Edge and Chromium: [DWM backdrop types](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwm_systembackdrop_type) · [sleeping tabs 2020](https://blogs.windows.com/msedgedev/2020/12/09/sleeping-tabs-beta-performance/) · [Edge 89](https://blogs.windows.com/msedgedev/2021/03/04/edge-89-performance/) · [Chromium site isolation](https://www.chromium.org/Home/chromium-security/site-isolation/)

**Google, Apple and CEF**
- [Google embedded-framework sign-in block](https://developers.googleblog.com/2020/08/guidance-for-our-effort-to-block-less-secure-browser-and-apps.html) · [Widevine overview](https://developers.google.com/widevine/drm/overview) · [WKProcessPool](https://developer.apple.com/documentation/webkit/wkprocesspool)
- CEF: [cef-rs](https://github.com/tauri-apps/cef-rs) · [CEF branches](https://chromiumembedded.github.io/cef/branches_and_building) · [cef_render_handler.h](https://github.com/chromiumembedded/cef/blob/master/include/cef_render_handler.h) · [Arch cef package](https://archlinux.org/packages/extra/x86_64/cef/)

**wry and Tauri**
- [wry](https://github.com/tauri-apps/wry) · [WebViewBuilder](https://docs.rs/wry/latest/wry/struct.WebViewBuilder.html) · [wry PR #1762](https://github.com/tauri-apps/wry/pull/1762) · [benchmark_results](https://github.com/tauri-apps/benchmark_results) · [tauri #5889](https://github.com/tauri-apps/tauri/issues/5889) · [#15682](https://github.com/tauri-apps/tauri/issues/15682) · [#11124](https://github.com/tauri-apps/tauri/issues/11124) · [#12901](https://github.com/tauri-apps/tauri/issues/12901)

**Toolkits**
- GPUI: [gpui](https://crates.io/crates/gpui) · [gpui-kit](https://github.com/longbridge/gpui-component) · [gpui-wry](https://docs.rs/crate/gpui-wry/latest) · [Zed for Windows](https://zed.dev/blog/zed-for-windows-is-here) · [Zed AccessKit PR](https://github.com/zed-industries/zed/pull/56065) · [Zed vs VS Code](https://zed.dev/compare/vscode) · [Zed 120 fps](https://zed.dev/blog/videogame)
- Slint: [licence](https://github.com/slint-ui/slint/blob/master/LICENSES/LicenseRef-Slint-Royalty-free-2.0.md) · [Image](https://docs.slint.dev/latest/docs/rust/slint/struct.Image)
- egui: [egui](https://github.com/emilk/egui) · [egui-wgpu](https://docs.rs/egui-wgpu/latest/egui_wgpu/struct.Renderer.html)
- iced: [iced #552](https://github.com/iced-rs/iced/issues/552) · [iced #2662](https://github.com/iced-rs/iced/pull/2662) · [iced shader](https://docs.rs/iced/latest/iced/widget/shader/trait.Primitive.html)

**Other engines**
- [Blitz](https://github.com/DioxusLabs/blitz) · [Verso](https://github.com/versotile-org/verso) · [Ultralight pricing](https://ultralig.ht/pricing/)
