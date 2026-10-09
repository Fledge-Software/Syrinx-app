# Browser v1 scope: what Atlas needs to be a daily browser

**Date:** 2026-10-09

**Question:** What does a browser need at v1 for its owner to use it as their daily browser, and what can wait? Atlas is a Tauri 2 app with a Rust core and a Svelte 5 chrome that renders pages in the operating system's web view: WebView2 on Windows, WKWebView on macOS, WebKitGTK on Linux. Profiles, Chrome extensions, a built-in password manager and ad blocking are out of scope by the owner's decision; they appear below only where their absence causes a real problem.

Terms used below. The **chrome** is the browser's own interface (sidebar, palette, title bar), as opposed to the page. A **web view** is the embedded engine that draws a page; each Atlas tab is its own child web view. A web view's **default UI** is the dialog or bar the engine shows by itself when the host app does nothing. An **accelerator** is a keyboard shortcut.

## Short answer

- Much of a browser's plumbing already lives inside WebView2: the find bar, printing, the download flyout, permission prompts that remember the answer, certificate-error pages, SmartScreen phishing protection, the right-click menu and web notifications all work with no code. Atlas mostly builds chrome around them. On macOS far more has to be built (find, downloads, certificate warnings and mute have no built-in UI).
- What Atlas must build itself is the browser's memory: the tab model, session restore, reopening closed tabs, history, bookmarks and import. That is Rust and SQLite work and matches the spec's Phase 1.
- Keyboard shortcuts are the hidden v1 cost. WebView2 never handles Ctrl T, Ctrl L, Ctrl Tab, Ctrl 1 to 9, Ctrl D, Ctrl H or Ctrl Shift T, and when a page has focus its key presses do not reach the Svelte chrome. Atlas has to catch every browser shortcut at the native layer in every page web view and route it to Rust.
- **Google sign-in is the biggest daily-use risk.** Microsoft's WebView2 documentation states that Google has disabled Google authentication in embedded web views, WebView2 included. If the owner uses Gmail, YouTube or Google Workspace, Phase 0 has to test signing in before anything else is built on top.
- Session restore on Windows can bring back each tab's address but not its back/forward list: WebView2 has no API for it, and Microsoft has called the request niche. WKWebView and WebKitGTK can save and restore the list.
- HTML5 fullscreen video, popup windows for OAuth sign-in, and DRM-protected media all need deliberate work or will fail: fullscreen fills only the tab's web view, unhandled popups escape into windows Atlas cannot control, and WebView2 has no Widevine.
- Becoming the default browser on Windows 11 is registration plus a link into Windows Settings; no app may make itself the default, and the user confirms it.
- Ship through the Tauri updater, whose signatures cannot be turned off. For Windows code signing, Microsoft's Artifact Signing (from $9.99 a month, individuals in the US or Canada) is the cheapest route, and EV certificates no longer skip SmartScreen warnings. Signing can wait until somebody other than the owner installs Atlas.
- Send nothing home: no telemetry, local crash logs only, and switch off WebView2's default upload of crash dumps to Microsoft. Keep each web view's phishing protection on.
- Picture-in-picture, split view, multiple windows, tab sleeping and a site-settings page can wait until after v1.

## Feature table

| Feature | Priority | Provided by | Notes |
|---|---|---|---|
| Tabs: open, close, switch, reorder, pin | Must | Atlas builds it | The core tab model over Tauri child web views. |
| Reopen closed tab (Ctrl Shift T) | Must | Atlas builds it | A stack of closed tabs in Rust; WebView2 never handles this key. |
| Session restore after quit | Must | Atlas builds it | Windows: address, title, pin and order only. macOS and Linux can also restore back/forward. |
| Crash recovery of the app | Must | Atlas builds it | Write the session to SQLite on every change, not only on exit. |
| Page crash recovery ("sad tab") | Must | Web view event, Atlas UI | `ProcessFailed` (WebView2), `webViewWebContentProcessDidTerminate` (WKWebView). |
| History | Must | Atlas builds it | From navigation and title events into the shared search index. |
| Bookmarks | Must | Atlas builds it | Spec Phase 1; the palette searches them. |
| Import bookmarks (Chrome, Edge, Firefox, HTML file) | Must | Atlas builds it | Without it the owner starts from nothing. |
| Import history | Should | Atlas builds it | Makes palette search useful on day one. |
| Import logins and cookies | Later | Not possible from Chrome | Chrome encrypts cookies for Chrome alone; sign in again. |
| Google account sign-in | Must (test first) | Blocked by Google in embedded web views | The top Phase 0 risk; see Findings 1. |
| Downloads | Must | WebView2 default flyout (Ctrl J); Tauri download hook | macOS has no download UI: Atlas builds a list there. |
| Find in page (Ctrl F) | Must | WebView2 built-in bar; API on macOS | An Atlas-styled bar is Should on Windows, Must on macOS. |
| Zoom (Ctrl plus, minus, 0) | Must | WebView2 per web view; `pageZoom` on macOS | Per-site memory across restarts is Should. |
| Printing (Ctrl P) | Must | WebView2 built-in; `printOperation` on macOS | macOS has no `window.print()` hook. |
| Windows keyboard shortcuts | Must | Atlas builds it | Route keys from every page web view to Rust. |
| Audio indicator and mute | Should | WebView2 API; WebKitGTK API | WebView2's own Ctrl M does nothing; WKWebView has no mute at all. |
| Permission prompts (camera, mic, location) | Must | WebView2 default prompt, saved per site | macOS needs Info.plist usage strings and entitlements. |
| Site settings page | Later | Atlas UI on WebView2 profile APIs | `GetNonDefaultPermissionSettings` lists what was granted. |
| HTTPS indicator and certificate errors | Must | WebView2 warning page; Atlas padlock | No API reports the security state of a page that loaded fine. |
| Phishing and malware protection | Must | WebView2 SmartScreen; WKWebView warning | On by default; none on Linux. |
| Tracking prevention | Should | WebView2 profile setting | Balanced by default when enabled; expose Strict. |
| Clear browsing data | Must | WebView2 by kind and time range; WKWebsiteDataStore | Atlas also clears its own history store. |
| HTML5 fullscreen video | Must | Atlas builds it | Resize the tab's web view to the window; WKWebView needs it switched on. |
| Popups and new windows (OAuth) | Must | WebView2 `NewWindowRequested` | Supply Atlas's own web view so `window.opener` survives. |
| Right-click menu | Must | WebView2 default menu | Customise later ("Open link in new tab"). |
| Link preview in a status bar | Should | WebView2 default status bar | Check it draws inside the page card. |
| DRM video and music | Should | PlayReady on WebView2, FairPlay on macOS | No Widevine in WebView2; Widevine-only sites fail. |
| Passkeys and Windows Hello | Should | WebView2 (undocumented but working) | macOS needs Apple's gated browser entitlement. |
| Default browser registration | Must | Installer registry entries, Atlas | User confirms in Windows Settings. |
| Opening links from other apps | Must | Tauri single-instance, Atlas | Forward the URL to the running instance as a new tab. |
| Auto-update | Must | Tauri updater plugin | Update signatures are mandatory. |
| Windows code signing | Should (Must before others install) | Artifact Signing or a certificate | Unsigned builds show "Windows protected your PC". |
| macOS signing and notarisation | Later (Must for a Mac release) | Apple Developer Program, $99 a year | Hardened Runtime and `notarytool`. |
| Crash reporting | Should | Atlas builds it | Local log and dump folder; nothing uploaded. |
| Telemetry | None | n/a | Deliberately none, as Orion and Zen do. |
| Keyboard-only chrome, F6 between panes | Must | Atlas builds it | Plus moving focus into and out of the page web view. |
| Screen-reader support for the chrome | Should | Web view exposes the Svelte DOM | Correct roles, names and focus order in the components. |
| Screen-reader support for pages | Free on Windows | WebView2 | Unverified on macOS; test VoiceOver. |
| Web notifications (mail, calendars) | Should | WebView2 default UI | Missing in WKWebView. |
| Page dark-mode preference | Should | WebView2 `PreferredColorScheme` | Follow Atlas's palette or the OS. |
| Developer tools (F12) | Should | WebView2, Tauri `devtools` | The owner is a developer. |
| Search engine choice and suggestions | Should | Atlas builds it | The palette is the address bar. |
| Save page as (Ctrl S) | Later | WebView2 `ShowSaveAsUI` | WebView2 never handles Ctrl S itself. |
| Tab sleeping for memory | Later | WebView2 `TrySuspend` | Edge sleeps tabs after two hours. |
| Picture-in-picture | Later | Undocumented on both | Test before promising it. |
| Split view | Later | Atlas builds it | Two child web views in one card; Zen allows four. |
| Multiple windows, tear-off tabs | Later | Atlas builds it | One window is enough for v1. |
| Passwords | Out of scope | n/a | Drop the Passwords button, as the design note suggests. |

## Findings

Each claim carries its source and a confidence (high, medium or low). Short keys: **OV** is Microsoft's [WebView2 features and APIs overview](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/overview-features-apis); **BF** is its [Edge-versus-WebView2 browser features table](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/browser-features); **W/** is the [WebView2 Win32 reference](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/); **A/** is Apple's documentation at `https://developer.apple.com/documentation/`.

### 1. Table-stakes features and what each web view gives

**Google sign-in.** BF states that "Google has disabled Google Authentication in embedded webviews, which includes WebView2", with discussion in [WebView2Feedback #1647](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1647). Google's own announcement blocked all embedded web views from its OAuth endpoint from 30 September 2021 with a `disallowed_useragent` error. ([Google Developers Blog, 2021](https://developers.googleblog.com/2021/06/upcoming-security-changes-to-googles-oauth-2.0-authorization-endpoint.html); high that the policy exists, medium for how it treats a general-purpose browser, which needs a Phase 0 test)

**Shortcut conventions to copy.** Chrome's Windows shortcuts are what a daily user's hands expect: Ctrl T, Ctrl W or Ctrl F4, Ctrl Shift T, Ctrl Tab and Ctrl Shift Tab, Ctrl 1 to 8 and Ctrl 9, Ctrl L, Alt D or F6, Ctrl K or Ctrl E to search, Ctrl F or F3, F5 or Ctrl R, Alt Left and Alt Right, Ctrl plus, minus and 0, Ctrl P, Ctrl H, Ctrl J, Ctrl D, Ctrl Shift Delete, F11, F12, and Ctrl S to save the page. ([Chrome shortcuts](https://support.google.com/chrome/answer/157179); high) Edge documents the same, including F6 "Move focus to next pane". ([Edge shortcuts](https://support.microsoft.com/en-us/microsoft-edge/keyboard-shortcuts-in-microsoft-edge-50d3edab-30d9-c7e4-21ce-37fe2713cfad); high)

**Which shortcuts WebView2 handles.** BF lists Ctrl T, Ctrl N, Ctrl Tab, Ctrl 1 to 9, Ctrl L and Alt D, Ctrl D, Ctrl H, Ctrl Shift T, Ctrl S and F10 as always off in WebView2; Ctrl W and F11 work only in popups WebView2 opened itself. Ctrl F, Ctrl P, F5, zoom, Alt Left and Ctrl J work by default and are switched off together by `AreBrowserAcceleratorKeysEnabled`. The host receives keys while a page has focus through the controller's `AcceleratorKeyPressed` event, and moves keyboard focus with `MoveFocus` and `MoveFocusRequested`. (BF, OV; high)

**The Ctrl S clash.** Atlas's design pins the sidebar with Ctrl S, as Arc does on Windows, but Chrome, Edge and Zen use Ctrl S to save a page, and Zen puts its floating sidebar on Ctrl Alt S. ([Arc: hide the sidebar](https://resources.arc.net/hc/en-us/articles/25619487530519-How-Do-You-Hide-the-Sidebar), medium, read via search results because the page refused direct fetches; [Zen shortcuts](https://docs.zen-browser.app/user-manual/shortcuts), high) Inside Forge, Ctrl S has to save the file.

**Session restore and closed tabs are expected.** Firefox restores the last session after a crash, offers a Restore Session page and lists Recently Closed Tabs and Windows. ([Firefox: restore previous session](https://support.mozilla.org/en-US/kb/restore-previous-session); high) Vivaldi keeps a Closed Tabs menu for the latest session and points users to it when a crash loses the session. ([Vivaldi: opening and closing tabs](https://help.vivaldi.com/article/opening-and-closing-tabs); high)

**Back/forward lists.** WebView2 has no API to read or rebuild a tab's history list: [WebView2Feedback #1093](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1093) has been open since 2021, and in October 2024 a maintainer called it niche and not a higher priority. (high) WKWebView's `interactionState` (macOS 12) saves the list "to restore that state later to another web view" (A/webkit/wkwebview/interactionstate; high), and WebKitGTK has `get_session_state` and `restore_session_state` ([WebKitGTK WebView](https://webkitgtk.org/reference/webkit2gtk/stable/class.WebView.html); high).

**What WebView2 gives for free.** From OV, BF and W/ (high unless marked):

- Find: the Ctrl F and F3 find bar is on by default; the Find API (`ICoreWebView2_28`, release SDK 1.0.3405.78) can drive it or hide it. In PDFs only the first match and the count are reported.
- Printing: Ctrl P by default; `ShowPrintUI` opens print preview or the system dialog; `PrintToPdf` writes a PDF.
- Downloads: a default dialog (Ctrl J); `DownloadStarting` can hide it, cancel a download or set its path.
- Permissions: `PermissionRequested`; unhandled, the default prompt shows and the decision is saved in the profile. `SetPermissionState` and `GetNonDefaultPermissionSettings` (`ICoreWebView2Profile4`) back a site-settings page.
- Zoom: Ctrl plus, minus, 0 and Ctrl wheel work in each web view; `ZoomFactor` sets it. Whether per-site zoom survives a restart is undocumented. (medium)
- Certificate errors: unhandled `ServerCertificateErrorDetected` shows the built-in warning page. There is no API for the certificate or security state of a page that loaded fine ([WebView2Feedback #2642](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2642)). (medium-high)
- Popups: if `NewWindowRequested` is not handled, the runtime opens its own popup the app cannot control. Handing it a fresh Atlas web view keeps `window.opener`, which OAuth popups need. (medium-high)
- Fullscreen: `ContainsFullScreenElementChanged` fires, but the element only fills its web view, so the host resizes it and hides the chrome.
- Audio: `IsMuted` and `IsDocumentPlayingAudio`; the built-in Ctrl M does nothing.
- Also: the context menu, status bar, `NotificationReceived` with a default notification UI, `PreferredColorScheme`, `PreferredTrackingPreventionLevel` (`ICoreWebView2Profile3`), and `TrySuspend` for sleeping tabs.

**DRM and passkeys.** WebView2 supports PlayReady; Widevine is an open request with no commitment ([WebView2Feedback #4828](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4828); medium), so Widevine-only services will likely not play. Passkeys have no WebView2 documentation, but runtime 153's release notes fix passkey prompts opening in the wrong window, which implies they work ([WebView2 runtime release notes](https://learn.microsoft.com/en-us/microsoft-edge/webview2/release-notes/runtime/); medium-low). On macOS a browser needs the gated `com.apple.developer.web-browser.public-key-credential` entitlement (macOS 13.3), granted by Apple through a request form, to use passkeys on arbitrary sites (A/bundleresources/entitlements/com.apple.developer.web-browser.public-key-credential; high).

**macOS: WKWebView needs more host UI.** From Apple's reference (high unless marked):

- `find(_:configuration:)` (macOS 11) is API only; the built-in find panel is iOS-only.
- `WKDownload` (macOS 11.3) makes the host pick the file; there is no download UI.
- `printOperation(with:)` (macOS 11); no public hook for `window.print()`. (medium)
- Media capture prompts by default (macOS 12). The app must declare `NSCameraUsageDescription` and `NSMicrophoneUsageDescription` and the hardened-runtime camera and audio-input entitlements. No web notifications API.
- Element fullscreen (`isElementFullscreenEnabled`, macOS 12.3) is off by default.
- No mute and no "is playing audio" property; `setAllMediaPlaybackSuspended` exists.
- Picture-in-picture is documented for iOS only (`allowsPictureInPictureMediaPlayback`). (medium-low)
- No built-in certificate warning; the host answers server-trust challenges and must draw the warning.

**Precedent for later items.** Edge sleeps background tabs after two hours by default ([Edge performance features](https://support.microsoft.com/edge/learn-about-performance-features-in-microsoft-edge); medium). Zen's split view shows up to four tabs side by side ([Zen split view](https://docs.zen-browser.app/user-manual/split-view); high).

### 2. Importing from Chrome, Edge and Firefox

- Chrome imports history, bookmarks, search engines and passwords from other browsers, and bookmarks from an exported HTML file. That HTML ("Netscape bookmark") format is the fallback every browser can export. ([Chrome: import bookmarks and settings](https://support.google.com/chrome/answer/96816); high)
- Chrome and Edge keep bookmarks in a JSON file named `Bookmarks` and history in an SQLite file named `History`, in `%LOCALAPPDATA%\Google\Chrome\User Data\Default` and `%LOCALAPPDATA%\Microsoft\Edge\User Data\Default` on Windows, and `~/Library/Application Support/Google/Chrome/Default` on macOS. The JSON roots are `bookmark_bar`, `other` and `synced`, and times count microseconds from 1601. ([Chromium bookmark_codec.cc](https://source.chromium.org/chromium/chromium/src/+/main:components/bookmarks/browser/bookmark_codec.cc); medium, not re-read for this note)
- The running browser holds `History` open, so the safe approach is to copy the file and read the copy, or ask the owner to close Chrome first. ([Chromium history_database.cc](https://source.chromium.org/chromium/chromium/src/+/main:components/history/core/browser/history_database.cc); low, verify in Phase 1)
- Since July 2024 Chrome on Windows protects cookies with app-bound encryption, so another app cannot decrypt them and signed-in sessions cannot be carried over. ([Google Security Blog, 2024-07-30](https://security.googleblog.com/2024/07/improving-security-of-chrome-cookies-on.html); medium, the page body did not render)
- Firefox keeps bookmarks and history together in `places.sqlite` in the profile folder (`%APPDATA%\Mozilla\Firefox\Profiles\<profile>` on Windows, `~/Library/Application Support/Firefox/Profiles` on macOS), with backups in `bookmarkbackups`, and locks the profile while running. ([Mozilla: profiles](https://support.mozilla.org/en-US/kb/profiles-where-firefox-stores-user-data); medium, the page did not render)
- Safari's data sits behind macOS Full Disk Access, which covers "data from other apps (for example, Mail, Messages, Safari, and Home)"; Safari's File > Export Browsing Data writes an unencrypted zip of bookmarks and history instead. ([Mac Help: privacy settings](https://support.apple.com/guide/mac-help/change-privacy-security-settings-on-mac-mchl211c911f/mac); [Safari: export browsing data](https://support.apple.com/guide/safari/export-browsing-data-ibrwebf10132/mac); high)

### 3. Becoming the default browser

- **Windows 11.** A browser registers a `Capabilities` key with `ApplicationDescription`, `FileAssociations` (.htm, .html and so on) and `UrlAssociations` (http, https) pointing to its own ProgIDs (program identifiers in the registry), plus a value under `HKLM\SOFTWARE\RegisteredApplications`. The installer then calls `SHChangeNotify(SHCNE_ASSOCCHANGED, ...)`, after which Windows may show a notification offering the new browser as default. ([Microsoft Learn: Default Programs](https://learn.microsoft.com/en-us/windows/win32/shell/default-programs); high)
- The same page marks the programmatic default-setting APIs (`SetAppAsDefaultAll`, `QueryAppIsDefault`) as deprecated or unsupported since Windows 8, says Windows 10 changed how defaults work, and says "the choice of default should be user driven". Atlas registers itself and then opens Windows Settings at its own entry for the owner to confirm. (high for the deprecation; medium for the Settings link, which is common practice rather than something this page states)
- **macOS.** A browser declares `http` and `https` in `CFBundleURLTypes`. `NSWorkspace.setDefaultApplication(at:toOpenURLsWithScheme:)` (macOS 12) changes the default and "if a change requires user consent, the system asks"; `LSSetDefaultHandlerForURLScheme` is deprecated since macOS 12. (A/appkit/nsworkspace; high)

### 4. Shipping and updating

- **Updater.** Tauri's updater "needs a signature to verify that the update is from a trusted source. This cannot be disabled." It reads a static JSON file or a dynamic server, updates the NSIS `.exe` or MSI on Windows, a `.app.tar.gz` on macOS and the AppImage on Linux, and defaults to a `passive` install with a small progress window. ([Tauri updater](https://v2.tauri.app/plugin/updater/); high)
- A lesson from Zen: before 1.19.9b its updater shipped with signature checks removed (CVE-2026-41431). Keep Tauri's check and keep the private key out of the repository. ([OSV record](https://osv.dev/vulnerability/CVE-2026-41431); high)
- **SmartScreen.** Unsigned and self-signed files show "Windows protected your PC". Signed files, OV or EV, are flagged as unrecognised until reputation builds over weeks of clean installs; "EV certificates no longer bypass SmartScreen". Store-installed apps never see the warning, and on Windows 11 Smart App Control can block unsigned files outright. ([Microsoft Learn: SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation); high) Tauri's guide agrees and adds that OV is cheaper and available to individuals. ([Tauri: Windows signing](https://v2.tauri.app/distribute/sign/windows/); high)
- **Artifact Signing** (formerly Trusted Signing) starts at $9.99 a month, needs no hardware token but a paid Azure subscription, is open to individual developers in the United States or Canada only, and takes 1 to 20 business days to validate identity. Tauri calls it through `signCommand`. ([quickstart](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart); [FAQ](https://learn.microsoft.com/en-us/azure/artifact-signing/faq); high)
- **WebView2 runtime.** The Evergreen runtime is part of Windows 11, updates itself, and now ships every two weeks with Edge; a Fixed Version runtime is over 250 MB and never updates itself. ([WebView2 distribution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution); high) Use Evergreen.
- **macOS.** Notarisation needs a Developer ID Application certificate (only the Account Holder can create one), the $99-a-year Apple Developer Program, the Hardened Runtime, and uploads through `notarytool`, with the ticket attached by `stapler`. (A/security/notarizing-macos-software-before-distribution; [Apple Developer Program](https://developer.apple.com/programs/); high)

### 5. Reliability and privacy

- **No telemetry is a normal stance.** Orion calls itself a "zero-telemetry" WebKit browser. ([Orion](https://orionbrowser.com/); high, as Kagi's own claim) Zen says "We do not collect any telemetry data or crash reports", while still connecting at startup to check for updates. ([Zen privacy policy](https://zen-browser.app/privacy-policy/); high)
- **WebView2 uploads crash dumps by default.** Minidumps go to Microsoft subject to Windows diagnostic-data consent, unless the host sets `IsCustomCrashReportingEnabled` to true; dumps land in `FailureReportFolderPath`. `ProcessFailed` distinguishes a dead browser process (recreate every web view) from a dead renderer (reload the tab). ([data privacy](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/data-privacy); [process events](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-related-events); high, though OV contradicts the reference on the property's name and value)
- **Crash dumps can hold private data.** Sentry warns minidumps "might contain sensitive information", such as environment variables and paths, and does not scrub stored dumps. ([Sentry: minidumps](https://docs.sentry.io/platforms/native/guides/minidumps/); high) For one owner, a local dump folder and a Rust panic hook writing a log are enough.
- **Phishing protection.** WebView2's SmartScreen is on by default (`IsReputationCheckingRequired`); if any web view on the same data folder leaves it on, it stays on for all, and the Windows "SmartScreen for Microsoft Edge" switch overrides it. Leaving it on obliges the app to tell users it "collects and sends the user's information to Microsoft". (OV, [data privacy](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/data-privacy); high) WKWebView's `isFraudulentWebsiteWarningEnabled` defaults to true (macOS 10.15); Safari uses Google Safe Browsing, or Tencent in mainland China, and sends values derived from the URL rather than the URL. ([Safari privacy](https://www.apple.com/legal/privacy/data/en/safari/); high for the API, medium that WKWebView uses the same providers) WebKitGTK has no safe-browsing setting. ([WebKitGTK Settings](https://webkitgtk.org/reference/webkit2gtk/stable/class.Settings.html); high)

### 6. Accessibility

- Microsoft treats the keyboard as a primary input: every control reachable by Tab, and F6 moving between panes, with Edge's address bar, tab strip and content as the example. ([keyboard accessibility](https://learn.microsoft.com/en-us/windows/apps/design/accessibility/keyboard-accessibility); high)
- By default WebView2 joins the accessibility tree as a child of its parent window, so screen readers reach page content with no work. (OV; high) An older bug where NVDA and JAWS could not read WebView2 in WinUI was fixed in 2024. ([WebView2Feedback #2330](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2330); medium) Apple documents nothing on VoiceOver and WKWebView, so test it. (low)
- Atlas's chrome is itself HTML in a web view, so it is accessible exactly as far as its Svelte markup has the right roles, names and focus order. Windows contrast themes reach web content through the `forced-colors` media query. ([MDN: forced-colors](https://developer.mozilla.org/en-US/docs/Web/CSS/@media/forced-colors); high)

## What this means for Atlas v1

1. **Add four tests to Phase 0**, because each is a Must that depends on the multi-webview design: Google and Microsoft account sign-in, including an OAuth popup; browser shortcuts while a page has focus; HTML5 fullscreen video in a child web view; and Tab and F6 moving focus between the chrome and a page.
2. **Build a shortcut router early.** Register `AcceleratorKeyPressed` on every page web view (and the macOS menu equivalents), send each browser shortcut to one Rust command table, and let the palette and sidebar read from it.
3. **Phase 1 holds the browser-memory Musts:** tab model, closed-tab stack, a session written on every change, history, bookmarks, and import from Chrome, Edge and Firefox files plus HTML. Restore address-only on Windows; use `interactionState` on macOS later.
4. **Lean on WebView2's default UI first** for find, downloads, printing, permissions, notifications and the context menu, and replace each only when it looks wrong in the page card. macOS will need Atlas versions of most of them; that is a reason to make Windows the v1 daily target.
5. **Handle `NewWindowRequested` and fullscreen from day one.** Unhandled popups escape into uncontrolled windows, and fullscreen video otherwise fills only the card.
6. **Privacy defaults:** no telemetry; `IsCustomCrashReportingEnabled` on so dumps stay local; SmartScreen kept on with a one-line notice in Settings, since turning it off leaves no phishing protection.
7. **Ship the updater from the first daily-use build**, signed with a Tauri key kept outside the repository. Add Windows code signing when anyone else installs Atlas.
8. **Accessibility is discipline in the Svelte chrome:** semantic elements, visible focus, F6 pane cycling, and a pass with Narrator or NVDA before v1.
9. **Defer** picture-in-picture, split view, multiple windows, tab sleeping, a site-settings page, save-page-as and a custom downloads panel.

## Open questions for the owner

1. **Google accounts:** do you sign into Google (Gmail, YouTube, Workspace) in your daily browser? If Phase 0 confirms Google blocks WebView2, the options are to open Google sign-in in another browser, present a different user-agent string (which works against Google's stated policy), or accept the gap. This may decide whether OS web views are viable at all, alongside the spec's CEF fallback.
2. **DRM:** do you use Spotify's web player or other Widevine-only services in the browser? WebView2 cannot play them.
3. **Ctrl S:** keep it for pinning the sidebar (as Arc does), or move pinning to Ctrl Alt S (as Zen does) so Ctrl S can save a file in Forge? The recommendation is Ctrl Alt S.
4. **Logins without a password manager:** WebView2 can save passwords in its own profile (`IsPasswordAutosaveEnabled`, off by default). Leave it off, consistent with the scope decision, or turn it on as a stopgap?
5. **Code signing:** are you eligible for Artifact Signing as an individual in the US or Canada, or is a business identity or the Microsoft Store the route? Not needed until someone else installs Atlas.
6. **Platforms:** is v1 a Windows-only daily browser, with macOS following? That decides whether notarisation and the Apple Developer Program are v1 costs.
7. **Search:** which engine should "Search the web" use, and should the palette fetch suggestions as you type (which sends keystrokes to that engine)?

## Sources

Microsoft: [WebView2 overview](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/overview-features-apis) · [WebView2 browser features](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/browser-features) · [WebView2 Win32 reference](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/) · [WebView2 data privacy](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/data-privacy) · [WebView2 process events](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-related-events) · [WebView2 distribution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution) · [WebView2 runtime release notes](https://learn.microsoft.com/en-us/microsoft-edge/webview2/release-notes/runtime/) · WebView2Feedback issues [#1093](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1093), [#1647](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1647), [#2330](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2330), [#2642](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2642), [#4828](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4828) · [Default Programs](https://learn.microsoft.com/en-us/windows/win32/shell/default-programs) · [SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation) · [Artifact Signing quickstart](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart) · [Artifact Signing FAQ](https://learn.microsoft.com/en-us/azure/artifact-signing/faq) · [Keyboard accessibility](https://learn.microsoft.com/en-us/windows/apps/design/accessibility/keyboard-accessibility) · [Edge shortcuts](https://support.microsoft.com/en-us/microsoft-edge/keyboard-shortcuts-in-microsoft-edge-50d3edab-30d9-c7e4-21ce-37fe2713cfad) · [Edge performance features](https://support.microsoft.com/edge/learn-about-performance-features-in-microsoft-edge)

Apple and WebKit (Apple pages read through the JSON form of `developer.apple.com/documentation/`): WKWebView `interactionState`, `find(_:configuration:)`, `printOperation(with:)`, `pageZoom`; `WKDownload`; `WKUIDelegate`; `WKPreferences.isElementFullscreenEnabled` and `isFraudulentWebsiteWarningEnabled`; `NSWorkspace.setDefaultApplication`; the `com.apple.developer.web-browser.public-key-credential` entitlement; [notarizing macOS software](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution) · [Apple Developer Program](https://developer.apple.com/programs/) · [Safari privacy](https://www.apple.com/legal/privacy/data/en/safari/) · [Mac Help: privacy settings](https://support.apple.com/guide/mac-help/change-privacy-security-settings-on-mac-mchl211c911f/mac) · [Safari: export browsing data](https://support.apple.com/guide/safari/export-browsing-data-ibrwebf10132/mac) · [WebKitGTK WebView](https://webkitgtk.org/reference/webkit2gtk/stable/class.WebView.html) · [WebKitGTK Settings](https://webkitgtk.org/reference/webkit2gtk/stable/class.Settings.html)

Google and Chromium: [Chrome shortcuts](https://support.google.com/chrome/answer/157179) · [Chrome import](https://support.google.com/chrome/answer/96816) · [Google OAuth in embedded webviews](https://developers.googleblog.com/2021/06/upcoming-security-changes-to-googles-oauth-2.0-authorization-endpoint.html) · [Chrome cookie app-bound encryption](https://security.googleblog.com/2024/07/improving-security-of-chrome-cookies-on.html) · [bookmark_codec.cc](https://source.chromium.org/chromium/chromium/src/+/main:components/bookmarks/browser/bookmark_codec.cc) · [history_database.cc](https://source.chromium.org/chromium/chromium/src/+/main:components/history/core/browser/history_database.cc)

Other browsers: [Arc: hide the sidebar](https://resources.arc.net/hc/en-us/articles/25619487530519-How-Do-You-Hide-the-Sidebar) · [Zen shortcuts](https://docs.zen-browser.app/user-manual/shortcuts) · [Zen split view](https://docs.zen-browser.app/user-manual/split-view) · [Zen privacy policy](https://zen-browser.app/privacy-policy/) · [Zen updater CVE](https://osv.dev/vulnerability/CVE-2026-41431) · [Firefox session restore](https://support.mozilla.org/en-US/kb/restore-previous-session) · [Mozilla profiles](https://support.mozilla.org/en-US/kb/profiles-where-firefox-stores-user-data) · [Vivaldi closed tabs](https://help.vivaldi.com/article/opening-and-closing-tabs) · [Orion](https://orionbrowser.com/)

Tauri and others: [Tauri updater](https://v2.tauri.app/plugin/updater/) · [Tauri Windows signing](https://v2.tauri.app/distribute/sign/windows/) · [Sentry minidumps](https://docs.sentry.io/platforms/native/guides/minidumps/) · [MDN forced-colors](https://developer.mozilla.org/en-US/docs/Web/CSS/@media/forced-colors)
