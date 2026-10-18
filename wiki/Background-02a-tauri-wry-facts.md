> **Research note dated 2026-10-09.** Read for reasoning, not current fact; later decisions in [`docs/decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/decisions.md) and [`docs/architecture-decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/architecture-decisions.md) win.
>
> An appendix to note 02. Its wry facts still matter; its Tauri facts were overtaken by D3. Note 08 rechecks the wry details Syrinx depends on. Notes 07 and 08 correct parts of the earlier notes, and note 03 assumes Tauri.
>
> Moved from `docs/research/02a-tauri-wry-facts.md` ([original at af3de53](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/02a-tauri-wry-facts.md)) on 2026-10-16.

# Tauri 2 and wry: what a browser tab can and can't do

Checked 2026-10-09 against tauri 2.12.2, wry 0.57.0 and the tauri-apps
plugins. A helper of the browser-scope researcher ([`02-browser-v1-scope.md`](Background-02-browser-v1-scope))
gathered this and reported after that note was written, so it is kept here as
an appendix. Confidence is marked (high, medium, low) as the helper gave it.

## Short answer

Tabs as child webviews work, but:

- on Linux they don't lay out (child bounds are ignored);
- app keyboard shortcuts don't reach Syrinx while a page has focus, on Windows
  and on macOS;
- the Svelte UI cannot draw over a tab, because there is no z-order API and a
  child webview always draws above the main one;
- Tauri's maintainers said in September 2025 they have no capacity for
  multiwebview bugs.

The blockers are listed at the end.

## A. What a browser tab can do

### 1. Webview and WebviewBuilder (high)

`WebviewBuilder` and `Window::add_child` need the `unstable` feature. On
Windows the builder can deadlock if used from synchronous commands, so use async
ones. Since 2.12.0, Tauri pins the unstable crates to minor versions (#15887).

- **All desktop platforms:** `navigate`, `url`, `reload`, `eval`, `set_focus`,
  `set_auto_resize` (child webviews are fixed-size by default), `set_bounds`,
  `show` / `hide`, `reparent`.
- **Back and forward:** Tauri has no native back/forward. `go_back`,
  `go_forward` and `can_go_back` exist on wry's `WebView` but Tauri doesn't
  expose them. Use JS `history.back()` or the raw platform webview.
- **Zoom:** `set_zoom` on all desktops (macOS 11+). `zoom_hotkeys_enabled` is
  native on Windows and a JS polyfill on macOS and Linux.
- **Print:** `print` is "currently only supported on macOS"; `window.print()`
  works everywhere.
- **Navigation and loading:** `on_navigation` can cancel a navigation by
  returning `false`. `on_page_load` gives Started and Finished only, with no
  progress and no load-error event.
- **Downloads:** `on_download` gives `Requested` (the save path can be changed)
  and `Finished`. On macOS the path in `Finished` is always empty.
- **New windows and titles:** `on_new_window` and `on_document_title_changed`
  arrived in 2.8.0 (August 2025). `NewWindowResponse::Create` accepts only a
  `WebviewWindow`, so a popup cannot be handed back as a new tab.
- **Browsing data:** `clear_all_browsing_data` is asynchronous. On Linux it
  clears the whole shared WebKit context; on Windows it errors on an old
  WebView2 runtime.
- **Cookies:** `cookies`, `cookies_for_url`, `set_cookie`, `delete_cookie`. On
  Windows they can deadlock in synchronous handlers.
- **Storage:** `data_directory`. On WKWebView the replacement is
  `data_store_identifier`, which needs macOS 14+.
- **Private browsing:** `incognito` needs WebView2 101+ on Windows and uses a
  non-persistent data store on macOS.
- **Devtools:** release builds need the `devtools` feature. On macOS it is a
  private API (not allowed in App Store builds). `close_devtools` and
  `is_devtools_open` are unsupported on Windows.
- **Other builder options:**
  - `background_throttling` needs macOS 14+.
  - `proxy_url` takes `http://` or `socks5://`; macOS needs 14+ and the
    `macos-proxy` feature.
  - `user_agent` must be set per builder, because the config value isn't
    applied to child webviews (#9492, closed as not planned).
- **Raw platform access:** `with_webview` gives the platform objects:
  - Windows: `ICoreWebView2Controller` and its environment;
  - macOS: the `WKWebView`, its `WKUserContentController` and the `NSWindow`;
  - Linux: the `webkit2gtk::WebView`.
- **No webview focus event:** `WebviewEvent` only has `DragDrop`.

Sources: https://docs.rs/tauri/latest/tauri/webview/struct.Webview.html,
https://docs.rs/tauri/latest/tauri/webview/struct.WebviewBuilder.html,
https://docs.rs/tauri/latest/tauri/webview/enum.NewWindowResponse.html,
https://docs.rs/wry/latest/wry/struct.WebView.html,
https://v2.tauri.app/release/tauri/v2.8.0/

### 2. Window level (high)

- **Fullscreen:** `set_fullscreen` works on all desktops; `set_simple_fullscreen`
  is native on macOS only.
- **Window effects:** `set_effects` needs a transparent window and is
  unsupported on Linux. Mica and Tabbed are Windows 11 only. Acrylic has resize
  performance warnings. macOS has vibrancy materials, plus Liquid Glass on
  macOS 26.
- **Shortcuts on Windows:** menu accelerators do not fire while a webview has
  focus. wry #451 has been open since December 2021, priority high; WebView2
  appears to consume the keys.
- **Shortcuts on macOS:** "global shortcuts not working in multiwebview" (wry
  #1177) is open. On 2025-09-23 a maintainer said the team "currently ha[s] no
  capacity to look into multiwebview issues". Tauri #9257 (the `unstable`
  feature breaks Cmd+A/C/V) is open.
- **global-shortcut plugin:** it registers OS-wide shortcuts, which makes it
  unsuitable for in-app shortcuts like Ctrl T. (medium)

Source: https://docs.rs/tauri/latest/tauri/window/enum.Effect.html,
https://v2.tauri.app/plugin/global-shortcut/

### 3. Open multiwebview issues (23 under "scope: unstable flag")

- **Focus:** #13466 (Alt+Tab back leaves content unfocused on Windows) is open,
  though the 2.12.0 notes (2026-09-26) claim fixes in #15625 and #15950.
- **macOS input:** #8705 (double characters), #10194 (arrow keys print invalid
  characters), #10148 (undo/redo), all open.
- **Z-order:**
  - #11124: no API to order webviews (open);
  - #15682: a child webview always draws over the main one, so the DOM cannot
    overlay it (open);
  - #9798: new children appearing behind on Windows (fixed).
- **Linux:** #10420 (children are packed into a vertical GTK box, so
  `set_bounds` is ignored; fix PR #15463 open) and #15656 (wrong bounds on
  Wayland).
- **Custom title bars:** #12207 (can't resize a custom-titlebar window with
  `unstable` on Windows) and #14072 (macOS traffic-light position broken).
- **Accessibility:** tracking issue #207 has been open since 2019. #12901 (NVDA
  reads nothing in frameless windows on Windows, a regression since 2.3) is
  open.

## B. Plugins and shipping

### 4. Updater, plugin 2.13.2 (high)

- **Signing is mandatory** ("This cannot be disabled"). It uses a minisign key
  pair from `tauri signer generate`, with the public key content in config.
- **Server:** either a static JSON file keyed `OS-ARCH`, or a dynamic server
  answering 204 (no update) or 200 with JSON.
- **Windows install modes:** `passive` (the default), `basicUi` or `quiet`. The
  app exits during the install.
- **Artifacts:**
  - macOS: `.app.tar.gz`;
  - Windows: `-setup.exe` or `.msi`, plus a `.sig` file;
  - Linux: AppImage (deb and rpm per the 2.10.0 changelog).
- **TLS is enforced.**

Source: https://v2.tauri.app/plugin/updater/

### 5. Deep links and single instance (medium)

- **Custom schemes** go in `plugins.deep-link.desktop.schemes`. Registering at
  runtime works on Windows and Linux only.
- **Delivery:** on Windows and Linux a link arrives as a new process's
  arguments. single-instance with its `deep-link` feature forwards them to the
  running instance. On macOS the running app gets `on_open_url`.
- **http and https:** Tauri has no desktop API for becoming the http/https
  handler (discussion #9184). Default-browser registry entries on Windows are
  not covered anywhere, so Syrinx writes them through installer hooks.

Sources: https://v2.tauri.app/plugin/deep-linking/,
https://v2.tauri.app/plugin/single-instance/,
https://github.com/orgs/tauri-apps/discussions/9184

### 6. Bundler (high)

- **Windows:** an NSIS `-setup.exe` or a WiX v3 `.msi`.
- **macOS:** `.app` and `.dmg`.
- **Linux:**
  - AppImage, 70+ MB; build it on the oldest distro with WebKitGTK 4.1;
  - deb and rpm;
  - Flatpak and Snap only as manual guides.
- **Browser registration:** `bundle.fileAssociations` registers file types.
  Extra registry entries, such as a browser's Capabilities key, go through NSIS
  `installerHooks`, WiX fragments, or a custom template.

Source: https://v2.tauri.app/distribute/windows-installer/

### 7. WebView2 install modes (high)

| Mode | Added size | Notes |
|---|---|---|
| `downloadBootstrapper` | 0 MB | The default; needs internet |
| `embedBootstrapper` | ~1.8 MB | |
| `offlineInstaller` | ~127 MB | |
| `fixedVersion` | ~180 MB | You ship WebView2's security updates yourself |
| `skip` | 0 MB | The app won't work without the runtime |

### 8. Windows signing (high)

- **SmartScreen:** "Since 2024, an EV Certificate no longer gives your app an
  immediate reputation with Microsoft SmartScreen."
- **Supported signing:** certificate thumbprints, Azure Key Vault (through
  `relic`), and Azure Artifact Signing (through `artifact-signing-cli` as
  `signCommand`).

Source: https://v2.tauri.app/distribute/sign/windows/

### 9. macOS signing (high)

- **Account:** a paid Apple Developer account ($99 a year).
- **Certificate:** "Developer ID Application" outside the App Store.
- **Notarization** is required.

Source: https://v2.tauri.app/distribute/sign/macos/

### 10. Microsoft Store (high)

- **No MSIX:** Tauri only makes EXE and MSI installers.
- **Store requirements:** the installer must be the offline type, signed,
  install silently, and handle its own updates.

Source: https://v2.tauri.app/distribute/microsoft-store/

### 11. Crash reporting and telemetry (low)

- **No official Tauri guidance** covers crash reporting or states a telemetry
  policy; issue #166 (opt-in telemetry) is open with no decision.
- **sentry-tauri** is a community plugin.
- **Tauri's own data collection:** the helper found no evidence that Tauri
  itself collects data, but did not audit the source.

## Would block a daily-use browser built on Tauri multiwebview

1. **Linux tabs don't lay out.** Child webview bounds are ignored (#10420).
2. **App shortcuts don't reach Syrinx** while a tab has focus: wry #451 on
   Windows; wry #1177 and Tauri #9257 on macOS.
3. **No z-order and no overlay.** The Svelte UI cannot draw over tab content
   (#11124, #15682), so the palette, the sidebar peek and any menus over a page
   need their own webview or another approach.
4. **Missing tab APIs:**
   - no back/forward or `can_go_back` in Tauri;
   - no load progress or load-error event;
   - no webview focus event;
   - `print` works on macOS only;
   - `on_new_window` can only return a window, not a tab.
5. **macOS 14 floor** for separate data stores and for the proxy.
6. **NVDA reads nothing** in frameless windows on Windows (#12901), which
   matters if Syrinx uses a custom title bar.
7. **No maintainer capacity** for the `unstable` multiwebview (stated September
   2025).
