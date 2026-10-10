# Google sign-in and protected video in WebView2

**Date:** 2026-10-09

**Status:** Research note 07. The owner said they stay signed into Google every day (Gmail, Calendar, Drive, YouTube) and watch YouTube, Disney+ and Hulu in the browser. Notes 02 and 06 flagged Google sign-in and DRM as risks but could not size them. This note replaces those guesses with sources and a local test on the owner's machine. It also **corrects notes 02 and 06 on Widevine**: the WebView2 runtime on this machine ships Widevine and plays Widevine-protected video (see 2.1).

**Terms used below.**
- An **embedded web view** is Google's phrase for a browser engine hosted inside another app. WebView2 inside Atlas is one.
- The **User-Agent (UA)** is the string a browser sends to say what it is. **Client hints** (UA-CH) are the newer, structured version: the `Sec-CH-UA` request header and the script value `navigator.userAgentData.brands`. A **brand** is one name and version in that list, such as "Chromium".
- **Signing in** here means a person using accounts.google.com in a browser. **OAuth** is the different flow where an app asks Google for access to a user's data. Google writes separate rules for the two.
- **EME** is the browser API for encrypted media. A **CDM** is the decryption module behind it, and a **key system** is its name (`com.widevine.alpha`, `com.microsoft.playready`). **Widevine** is Google's DRM and **PlayReady** is Microsoft's. Widevine **L3** is the software-only level; L1 needs hardware.
- **MSE** is the API ordinary streaming players use. It needs no DRM. **HDCP** is the copy protection on display links; some services cap quality when they cannot verify it.

## Questions

1. Google sign-in inside WebView2 as of today: what Google's policy says and whether it names WebView2 or CEF; whether signing into accounts.google.com works in practice with default settings; how Google tells an embedded web view from Edge, and what WebView2 sends in the User-Agent and client hints; whether an app can change them; whether Google offers any route for a browser vendor; and what other apps do.
2. Protected video (DRM) in WebView2: whether it supports PlayReady and Widevine and what has to be enabled; what Disney+ and Hulu require; and which YouTube content needs DRM.

## Short answer

**Google sign-in**
- Google's written rules block sign-in from "embedded frameworks" and name CEF and Embedded Internet Explorer. They never name WebView2. Microsoft's own WebView2 page does say Google has disabled authentication in embedded web views, "which includes WebView2", and that page was updated on 2026-10-02.
- In practice the picture is better than that page suggests, but it is not proven. On the owner's machine (WebView2 runtime 154.0.4258.62, default settings) the Google sign-in page loads normally. What makes that work is a surprise: toward Google hosts the runtime presents itself as **Google Chrome** (no `Edg/` token, brands "Chromium" and "Google Chrome"), while toward every other site it presents as Edge with a "Microsoft Edge WebView2" brand. Microsoft's runtime does this; Atlas does not, and nothing documents it. I could not finish a sign-in (no credentials were entered), so the step where the 2021 block appeared, submitting the email address, is untested.
- That override disappears if Atlas sets a custom UA through the WebView2 settings API: Google then sees "Microsoft Edge" and "Microsoft Edge WebView2" brands. Setting the UA with the `--user-agent` browser argument leaves the override in place. A DevTools-protocol override lets Atlas choose both the UA and the brands.
- Google documents no registration or allow-list for browser vendors. The only written bar is the browser-eligibility list in a 2020 Google post. If default sign-in fails, there is no sanctioned fix inside Atlas.

**Protected video**
- The WebView2 runtime ships Google's Widevine CDM, byte-identical to Edge's, and a Widevine test stream played in it. This contradicts the "no Widevine" statements in notes 02 and 06. It is software-only (L3). No flag or setting was needed.
- WebView2 advertises PlayReady too, and Microsoft's issue tracker has reports of it working on the Evergreen runtime but not the Fixed Version runtime. On this machine the PlayReady CDM failed to start in Edge and WebView2 alike, so I could not confirm it here.
- Neither Disney+ nor Hulu names its DRM in its help pages. Google's Widevine page lists both as Widevine partners, and both support Firefox, so Widevine is enough. Their own pages cap browsers at 720p (Hulu only with verified HDCP, else 480p).
- Ordinary YouTube playback is MSE only: a test video played in WebView2 with zero EME calls. Google's YouTube help names no DRM for rentals, purchases, YouTube TV or Premium downloads.

## How this was checked

- **Documents and trackers.** Google's help and developer pages, Microsoft Learn, the WebView2Feedback and Tauri/wry issue trackers, and the services' own help pages. Each claim below carries its source and a confidence.
- **A local test on the owner's machine.** Windows 11 build 26300; WebView2 Evergreen runtime 154.0.4258.62 and Edge 154.0.4258.62. The host was a WPF app using SDK 1.0.2903.40 (the copy shipped with Visual Studio Build Tools), run from 32-bit Windows PowerShell because that was the loader on hand. Each run used a fresh, empty profile folder. A tiny Node server on 127.0.0.1 echoed the request headers and received the results of a probe page. The only outside pages loaded were the sign-in page, the home pages of a few sites, one ordinary YouTube video and public DRM test streams. No credential was entered and no form was submitted.
- **What "observed" means.** These are facts about this machine on 2026-10-09. They are strong evidence of what the runtime does but not a guarantee for other machines or other runtime versions. The scripts live in the agent's scratch area, not in the repository. Phase 0 should rebuild the useful parts as a proper test.

## Findings

### 1. Google sign-in

#### 1.1 What Google's policy says

- **The account help page lists "embedded in a different application" as a reason sign-in may be stopped.** The page, "Sign in with a supported browser", says Google may stop sign-ins from browsers that lack JavaScript, have unsecure extensions, are controlled by software automation, or are embedded in another application. It lists Chrome, Safari, Firefox, Opera and Edge as supported. A developer note says anyone using "Sign in with Google" through the Chromium Embedded Framework must move to browser-based OAuth or a Progressive Web App. ([Google Account Help](https://support.google.com/accounts/answer/7675428), page footer 2026; high)
- **The reason is man-in-the-middle phishing.** Google's security blog (2019-04-18) says an embedded framework such as CEF lets a middleman sit between the user and Google and collect credentials, and that Google "can't differentiate" that from a legitimate sign-in on these platforms. It announced blocking sign-ins from embedded browser frameworks from June 2019, "similar to" the webview restriction announced in April 2016. ([Google Security Blog](https://security.googleblog.com/2019/04/better-protection-against-man-in-middle.html); high)
- **The 2020 developer post is the only written bar for a browser.** It says sign-ins "from all embedded frameworks" are blocked from 2021-01-04, affecting "CEF-based apps and other non-supported browsers", and that it does not allow sign-in from frameworks like CEF or Embedded Internet Explorer. Modern browsers with security updates stay supported if they:
  - run JavaScript and have a reasonably complete implementation of web standards;
  - do not proxy or alter network communication (no server-side rendering, HTTPS proxy, replaying requests or rewriting HTTP headers);
  - **identify themselves clearly in the User-Agent** when connecting to accounts.google.com, and do not use another browser's UA string such as Chrome's or Firefox's on that host;
  - provide no automation features, such as scripts that click or type.

  The post names CEF and Embedded Internet Explorer and does not mention WebView or WebView2. It also gives a test header, `Google-Accounts-Check-OAuth-Login:true`, for developers who want to see whether their authorization flow would be affected. ([Google Developers Blog, 2020-08-28](https://developers.googleblog.com/2020/08/guidance-for-our-effort-to-block-less-secure-browser-and-apps.html); high)
- **The 2021 post is about OAuth, not ordinary sign-in.** It blocked OAuth authorization requests from "embedded webviews" from 2021-09-30, answering with a `disallowed_useragent` error, and names Android WebView, WKWebView and UIWebView. It mentions neither WebView2 nor CEF. ([Google Developers Blog, 2021-06-29](https://developers.googleblog.com/2021/06/upcoming-security-changes-to-googles-oauth-2.0-authorization-endpoint.html); high)
- **The current OAuth policy defines "embedded" by control.** An app must not send an OAuth request to an embedded user-agent under the developer's control, and that includes libraries that let a developer insert scripts, change request routing or read session cookies. The desktop-app guide (last updated 2026-09-14) says installed apps must open the system browser and use a local redirect (the loopback address). ([OAuth policies](https://developers.google.com/identity/protocols/oauth2/policies), changelog to 2026-08-05; [OAuth for desktop apps](https://developers.google.com/identity/protocols/oauth2/native-app); high)
- **What follows for Atlas.** Google's written rules never mention WebView2, but "embedded in a different application" describes it, and Atlas can insert scripts and read cookies in a way the OAuth policy treats as embedded. That reading is mine, not Google's. (medium) Any Atlas feature that wants Google API data (a Fledge-style integration, say) must use the system browser and a loopback redirect, not a tab.

#### 1.2 What Microsoft says

- **WebView2's documentation says the opposite of "it works".** The Edge-versus-WebView2 page has a "Google Authentication" section: Google has disabled Google authentication in embedded webviews, "which includes WebView2", because of a security policy, and it links Google's 2021 OAuth post and the tracking issue. The page's metadata shows it was updated on 2026-10-02. ([WebView2 browser features](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/browser-features); high)
- **The tracking issue is closed.** WebView2Feedback #1647 was opened on 2021-08-13. Its first post recommends launching the system browser for the flow, with a Web Authentication Broker as the long-term answer (UWP-only at the time). The conversation was locked and moved to Discussion #3828 on 2023-09-28; the discussion has 128 comments, the last on 2024-09-23. ([#1647](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1647), [Discussion #3828](https://github.com/MicrosoftEdge/WebView2Feedback/discussions/3828); high)
- **Microsoft's stance in 2021 was "Google decided".** A collaborator said the block "is indeed caused by google changes" ([#1578](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1578), 2021-08-03). When users found that a fake UA string worked, a Microsoft maintainer replied on 2021-10-27 that Google's stance is to block WebView2, and that Google would "almost certainly" block the workaround too ([#1647](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1647); high).

#### 1.3 What happened in practice, with dates

| Date | Source | What it shows | Confidence |
|---|---|---|---|
| 2021-07-28 | [#1578](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1578), [#1584](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1584) | A WPF app with default settings gets "This browser or app may not be secure" after typing the email. Edge and Chrome work. | high |
| 2021-10-27 to 2022-08 | [Discussion #3828](https://github.com/MicrosoftEdge/WebView2Feedback/discussions/3828) | Setting a bogus UA string made sign-in work for some; others reported it stopped; one reported months of daily use with a signed-in Gmail tab. | medium (user reports) |
| 2022-06-27 | [#2552](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2552) | Same error in Outlook's built-in browser. Closed as stale in 2024-01 as "likely not relevant to newer runtimes". | medium |
| 2023-05-24 | [Discussion #3828](https://github.com/MicrosoftEdge/WebView2Feedback/discussions/3828) | A user reports Google login works with no workaround on two computers, and Gmail stays signed in. | medium-low (one user) |
| 2024-10-22 | [#4873](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4873) | A Google Workspace organisation using Context-Aware Access (which needs an "Endpoint Verification" extension) cannot sign in from Microsoft's apps. Microsoft says that app uses the Windows account broker and an older web view, not WebView2. | medium |
| 2025-07-23 | [#5319](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5319) | Google's page loads in apps on runtime 138 but a script error stops the email step. A contributor with a Microsoft handle says adding a Google account in Outlook worked for them. The reporter said it resolved itself within days. Not a policy block. | medium |
| 2026-05-13 | [Tauri #11612](https://github.com/tauri-apps/tauri/issues/11612) | A Tauri 2 app on Windows loads Google Sheets in per-tab child web views. The page broke on a script clash with `window.ipc`, not on sign-in (see 1.8). | medium |
| 2026-10-02 | [T3 Code #14815](https://github.com/pingdotgg/t3code/issues/14815) | An Electron app on macOS signs in and passes two-step verification, then lands on Google's "browser is not supported" page. Observed once, not reproduced; the triage reply is machine-written. It shows Google still turns embedded browsers away. | medium-low |
| 2026-10-09 | This note's test | The sign-in page loads in default WebView2 (see 1.4). Submitting the email was not tested. | high for what loaded |

**Reading.** No source states that Google lifted its block for WebView2, and none states that it is enforced against default WebView2 today. The 2021 failures were with the then-default UA and client hints; since 2023 users report success; the runtime now presents as Chrome to Google (1.4). Whether the owner's account passes the email step and the later risk checks is unknown until someone tries it. (medium that sign-in works end to end today; this is the top Phase 0 test)

#### 1.4 What WebView2 sends: observed on runtime 154.0.4258.62

All rows are default WebView2 unless the first column says otherwise. "Localhost" means my loopback server, which stands in for any ordinary site.

| Configuration | To localhost (UA tail, brands) | To accounts.google.com |
|---|---|---|
| Default | UA ends `Chrome/154.0.0.0 Safari/537.36 Edg/154.0.0.0`; brands Microsoft Edge, Not A(Brand, **Microsoft Edge WebView2**, Chromium | UA ends `Safari/537.36` with no `Edg/`; brands Chromium, **Google Chrome**, Not A(Brand. Seen in script and in the HTTP request headers, including `Sec-CH-UA-Full-Version-List`. |
| `Settings.UserAgent` set to a string | The string; **brands unchanged** (Edge and WebView2 still sent) | The app's string; brands **Microsoft Edge, Microsoft Edge WebView2, Chromium**. The Google-specific behaviour is gone. |
| `--user-agent=` browser argument | The string; brands unchanged | **Unchanged from default**: Chrome UA, "Google Chrome" brand. The argument does not reach Google hosts. |
| `Emulation.setUserAgentOverride` through `CallDevToolsProtocolMethodAsync`, with `userAgentMetadata` | The app's UA; brands exactly as the app lists them | The app's UA and brands (a list of "Not.A/Brand", "Chromium", "Atlas" was sent). |

- **Other hosts.** The same Chrome presentation appeared on mail.google.com, calendar.google.com, drive.google.com, docs.google.com, www.google.com and www.youtube.com. Disney+, Hulu and GitHub got the Edge-and-WebView2 presentation. (observed; high)
- **The sign-in page loads in every configuration.** In all four, the title was "Sign in - Google Accounts" and the email field and Next button showed. None of them proves the email step passes. (observed; high)
- **Other facts the page can see.** `window.chrome.webview` is an object on Google's page; `navigator.webdriver` is false. Wry leaves the UA alone unless the app sets one (I did not check Tauri's builder), and wry's default browser arguments (`--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`) did not change any of the above. ([wry `webview2/mod.rs`](https://github.com/tauri-apps/wry/blob/dev/src/webview2/mod.rs), commit 9a5f3aad of 2026-09-29; observed; high)
- **Microsoft's documentation is out of date on one point.** The `UserAgent` property page says setting it "may clear" the client-hint headers and script values and that behaviour is subject to change. On 154 it does not clear them. ([CoreWebView2Settings.UserAgent](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2settings.useragent), updated 2026-09-28; observed; high) Earlier runtimes did clear them: a 2022 report shows empty brands after setting a UA ([#2576](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2576); high). That may be why the bogus-UA workaround worked in 2021 and why it is no longer a reliable test.
- **The default brand list is not in Microsoft's documentation.** Besides this test, a 2024 header log from a WebView2-based Flutter app shows the same four brands ([flutter_inappwebview #2093](https://github.com/pichillilorenzo/flutter_inappwebview/issues/2093); medium).

#### 1.5 How Google could tell an embedded web view from Edge

- **Google does not document its detection.** Nothing above says which signals it uses. What is knowable is what it can see. (high that it is undocumented)
- **Signals visible in the default non-Google presentation.** A UA with `Edg/`, and a client-hint brand "Microsoft Edge WebView2" that Edge itself does not send (the Edge control run listed only Chromium, Microsoft Edge and the filler brand). Also `window.chrome.webview`, which only a host provides. (observed; high for the facts, no evidence Google uses them)
- **Toward Google the runtime hides the first two.** I could not find the cause. A plausible one is a Microsoft-maintained list of sites that get a Chrome UA, inherited from Edge. Press coverage from 2019 (not read directly, so only a lead) says Edge downloads such a list. No Microsoft page documents one, and I do not know when this one began. (low)
- **A remote debugging port is a known tripwire.** JxBrowser's troubleshooting guide says that when the Chromium remote debugging port is configured, Chromium blocks sign-in to its services, with Gmail and YouTube as examples ([JxBrowser docs](https://teamdev.com/jxbrowser/docs/guides/troubleshooting/issues.html); medium, vendor claim, WebView2 not mentioned). A WebView2 comment from 2022 says the same for the debugging switch ([Discussion #3828](https://github.com/MicrosoftEdge/WebView2Feedback/discussions/3828); low). It fits Google's "no automation" rule.

#### 1.6 What an app can change

- **`CoreWebView2Settings.UserAgent`** replaces the UA. It applies from the next top-level navigation, so the first request can still carry the default; its effect on workers is described in the page above. In my test it also dropped the Google-specific presentation. ([docs](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2settings.useragent); [#2862](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2862), [#4993](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4993); observed; high)
- **`--user-agent` through `AdditionalBrowserArguments`** (or the `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` variable) set the UA for ordinary sites but not for Google hosts, and left the brands alone. (observed; high)
- **Client-hint brands** can be set only through the DevTools protocol: `Emulation.setUserAgentOverride` with `userAgentMetadata`. A Microsoft maintainer suggested exactly this in 2021 ([#1562](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1562)) and users confirmed it works ([#2576](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2576)); my test confirms it for the main page. One user reports it does not reach cross-origin sub-frames ([#2576](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2576), 2024-05; low, not tested).
- **There is no brand setting in the WebView2 API.** I found none in the settings reference or the issue tracker. (medium)

#### 1.7 Sanctioned routes for a browser vendor

- **None is documented.** Google's account help, the 2020 post and the OAuth pages describe no registration, allow-list, certification or contact for browser makers, and a web search found none. (medium; absence of evidence)
- **The written bar is the 2020 list.** Atlas can meet most of it: it runs JavaScript, is current, and can avoid rewriting headers or proxying. It can also avoid automation features. The line "identify itself clearly in the User-Agent" is the awkward one: default WebView2 hides its identity from Google (1.4), so the Chrome presentation is arguably Microsoft's departure from Google's rule, and Atlas's own UA token only reaches Google if Atlas takes over the UA through the settings API or DevTools, which then exposes the WebView2 brands.
- **For API access, the route is the system browser and loopback** ([OAuth for desktop apps](https://developers.google.com/identity/protocols/oauth2/native-app); high).
- **So the only route for normal sign-in is to be accepted as a supported browser.** Nothing says how Google decides. (medium)

#### 1.8 What other apps do

- **Microsoft's own advice (2021):** launch the system browser for Google flows ([#1647](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1647)). Its Microsoft 365 apps use the Windows account broker, not WebView2, for Google accounts ([#4873](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4873); medium).
- **Pake** (a Tauri-based tool that wraps websites as desktop apps; WebView2 on Windows) documented on 2026-03-14 that Google and some other providers may block sign-in in embedded web views, called it provider policy, and recommended a normal browser ([Pake #1148](https://github.com/tw93/Pake/issues/1148); high that this is its stance).
- **T3 Code** (Electron) keeps its native UA and offers "Open in system browser" ([#14815](https://github.com/pingdotgg/t3code/issues/14815); medium-low).
- **A small WebView2 app, grok-app,** moved Google sign-in into a separate top-level window sharing the app's cookie jar after hard freezes in a child web view, in 2026-09 ([#1154](https://github.com/RongleCat/grok-app/issues/1154); low: one project, maintainer comments are machine-written).
- **Standalone browsers are what Google lists as supported** (Chrome, Safari, Firefox, Opera, Edge). No source describes how Google treats a browser such as Atlas that is built on an embedded engine.

#### 1.9 Related traps found on the way

- **`window.ipc` breaks Google Docs and Sheets in Tauri apps.** Wry defines a frozen global `window.ipc` in every web view that has an IPC handler. A user who traced the failure says Google's own page script also declares `ipc`, and the editor dies with "Identifier 'ipc' has already been declared". The user fixed it by routing messages through `window.chrome.webview.postMessage`; a Tauri maintainer said changing the name is a breaking change for Tauri 3 and that Google Docs was not worth the risk in 2.x. Wry only injects `window.ipc` when an IPC handler is attached. ([Tauri #11612](https://github.com/tauri-apps/tauri/issues/11612), comments of 2026-05-13 to 2026-05-19; [wry source](https://github.com/tauri-apps/wry/blob/dev/src/webview2/mod.rs); high for the mechanism, medium that it is still unfixed)
- **Extensions.** A Google Workspace organisation that requires an extension for sign-in will not work in a web view with no extension (#4873, above).
- **Injecting scripts or intercepting requests on Google hosts** is what Google says it blocks for. My test used a request filter on accounts.google.com only to read headers; Atlas should not do that in production. (medium)

### 2. Protected video

#### 2.1 Widevine in WebView2

- **Observed: the runtime ships Widevine and it works.** The Evergreen runtime folder contains `WidevineCdm` version 4.10.3050.1, byte-for-byte the same DLL as Edge's (same hash). `requestMediaKeySystemAccess('com.widevine.alpha')` succeeded; it accepted software robustness (`SW_SECURE_CRYPTO`) and refused hardware (`HW_SECURE_ALL`), so this is L3. A multi-DRM test vector from Axinom, played through Shaka Player 4.7.11 with a public Widevine licence server, decoded 241 frames and played 9.9 seconds, with Edge doing the same. No flag, switch or environment option was set. (observed, runtime 154.0.4258.62, 2026-10-09; high that it works here, medium for other machines and versions)
- **This contradicts earlier notes.** Notes 02 and 06 say WebView2 has no Widevine, based on an open feature request ([WebView2Feedback #4828](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4828), opened 2024-09-21, one Microsoft reply, no commitment). Either support arrived after that request or the request was never accurate for Evergreen. I found no release note or Microsoft page that mentions Widevine in WebView2. The Microsoft Learn search for WebView2 and DRM returns no page on it. (medium)
- **Licensing.** Google's Widevine page says a licence agreement is required to use Widevine products, and it lists Edge and Chromium Embedded Framework / Electron as platforms. Here the CDM comes inside Microsoft's runtime, so Atlas does not distribute it. Whether that fully covers Atlas is a legal question I did not research. ([Widevine overview](https://developers.google.com/widevine/drm/overview); high for the page, low for the conclusion)
- **Fixed Version runtime:** I did not test it. Atlas should use Evergreen anyway (note 02), and PlayReady already fails there (2.2).

#### 2.2 PlayReady in WebView2

- **Reports say yes on Evergreen, no on Fixed Version.** The developers of Cider, a media player, reported that PlayReady playback fails with the Fixed Version runtime and works with Evergreen, and a Microsoft collaborator asked the question that confirmed it ([#4632](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4632), 2024-06-15, still open; medium, reporter's claim). A French streaming app (myCANAL) used PlayReady key systems in WebView2 on Windows 11 and Microsoft engineers troubleshot it with Media Foundation traces, which shows PlayReady runs through Media Foundation as it does in Edge ([#4935](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4935), 2024-11-19 to 2025-01-07; medium). Nothing in Microsoft's WebView2 documentation mentions PlayReady. (high that it is undocumented)
- **No setting needed.** None of those reports enables anything: it works because the Evergreen runtime is Edge's engine. (medium)
- **Observed on this machine: advertised, but the CDM would not start.** `requestMediaKeySystemAccess` succeeded for `com.microsoft.playready`, `.recommendation`, `.recommendation.3000` and `.hardware`, but `createMediaKeys` failed with "Failed to initialize CDM" for both `com.microsoft.playready` and `.recommendation`. Edge with a fresh profile failed identically, so this is not a WebView2 gap. The machine is a Windows 11 build 26300 laptop with a hybrid AMD and NVIDIA GPU and a **Parsec virtual display adapter**; virtual displays have no protected output, so that is my first suspect, but I did not isolate the cause. (observed; high that it failed, low on why)
- **HDCP.** For Widevine, the CDM reported HDCP 1.0 usable and 1.4 and 2.2 output-restricted, identically in Edge and WebView2. That matters for Hulu (2.3). (observed; high; may reflect this display setup)

#### 2.3 Disney+ and Hulu

- **Neither help page names a DRM.** Disney+ says it is supported on the latest Chrome, Firefox, Edge and Opera on Windows 10 and newer ([Disney+ help](https://help.disneyplus.com/article/disneyplus-computer-browser-requirements); high). Its streaming-quality page says browsers are limited to 720p HD with no 4K ([Disney+ data usage](https://help.disneyplus.com/article/disneyplus-data-usage); high). Hulu (page dated 2026-09-11) supports the latest Chrome, Firefox, Safari and Edge, streams at most 720p, gives 720p on Chrome and Edge on Windows only when HDCP 1.4 is verified or no external display is connected, and caps Firefox at 480p because HDCP cannot be verified ([Hulu help](https://help.hulu.com/article/hulu-supported-computers); high).
- **Google names both as Widevine users.** Its Widevine overview lists Disney+, Hulu, Netflix and YouTube among its partners. ([Widevine overview](https://developers.google.com/widevine/drm/overview); high)
- **So Widevine is enough.** Both services accept Firefox, and Google's Widevine page lists Firefox as a Widevine platform, so a Widevine-only browser can play them; neither "requires PlayReady". The services' pages do not say which key system Edge uses, so I cannot say whether Edge picks PlayReady for them. (medium; inferred, I did not read Mozilla's page on which CDM Firefox uses)
- **Not tested:** no account was used, so I cannot say Disney+ or Hulu accept WebView2 at playback. Their home pages loaded and, like GitHub, saw the Edge and WebView2 presentation. The 720p caps are service policy and match an L3 CDM, so expect 720p at best (480p on Hulu if HDCP cannot be verified). (observed; medium)

#### 2.4 YouTube

- **Ordinary videos need no DRM.** A normal video (Big Buck Bunny, a Blender Foundation film) played in WebView2: one `MediaSource`, a blob URL, 15.8 seconds of playback, and **zero** `requestMediaKeySystemAccess` calls, with a hook installed before the page's scripts ran. (observed; high)
- **Which YouTube content needs DRM is not documented by YouTube.** YouTube's help says rentals and purchases play "on web browsers that support HTML5 playback", that HD and 4K availability follows studio licensing agreements, that a browser's purchased-movie quality can be lower than on other devices, and that Premium downloads on a computer work in Chrome, Edge, Firefox and Opera. Its help pages never use the words Widevine or DRM. ([YouTube system requirements](https://support.google.com/youtube/answer/78358), [purchased movies](https://support.google.com/youtube/answer/9679194), [HD and 4K](https://support.google.com/youtube/answer/3306741), [Premium offline](https://support.google.com/youtube/answer/11977233); high)
- **What can be said:** Google's Widevine page lists YouTube as a Widevine user. YouTube TV's help page says the browser may ask to verify the device's identity "for enhanced playback of protected media", and that without it quality may drop; it also says external monitors get 480p on Mac and Linux because of copyright protections ([YouTube TV troubleshooting](https://support.google.com/youtubetv/answer/7129766)). Chrome's help says some music and movies are protected by copyright and that Chrome plays protected content by default. So rentals, purchases and YouTube TV very probably use Widevine (or PlayReady in Edge), but no YouTube page says so. Which key system YouTube chooses, and whether Premium downloads need a persistent licence (this CDM's manifest says it does not support persistent licences), I did not test. (low-medium)

## What this means for Atlas v1

1. **Treat Google sign-in as a test, not a risk to design around yet.** The page loads and Google's own text never names WebView2, but only a real sign-in answers it. Make it the first Phase 0 check, with the owner's account, because the owner's daily use depends on it.
2. **Leave the user agent alone on page web views.** Do not call wry's `with_user_agent` or Tauri's `user_agent`, do not pass `--user-agent`, and do not use the DevTools override, unless the test shows a need. Default WebView2 already gets the Chrome presentation toward Google; the settings API removes it and shows Google the WebView2 brand instead. If the default fails and Phase 0 tries a custom identity, the DevTools override with an "Atlas" brand is the version that follows Google's "identify itself" rule, and it must be tested separately.
3. **Keep Atlas's hands off Google pages.** Do not inject scripts, add request filters, enable a remote debugging port or turn on ad-blocking for accounts.google.com. Google's stated reason for blocking is a middleman, and its list forbids rewriting headers.
4. **Do not attach an IPC handler to page web views.** It adds `window.ipc`, which breaks Google Docs and Sheets (by one user's trace), and it is a risk on untrusted pages anyway. Wry only adds it when a handler is set, so wry used directly has full control. The Tauri report shows Tauri's own IPC script reaching a Google page; I did not check whether Tauri can leave it off a remote page. Test a Google Sheets tab in the Phase 0 build.
5. **Do not plan on the system browser for browsing.** It is Google's route for OAuth only; the cookies do not carry over, and Chrome's app-bound encryption stops importing them (note 02). If the owner's sign-in fails, there is no sanctioned fallback inside Atlas; CEF is blocked by name (note 06).
6. **Use the Evergreen runtime only.** Fixed Version fails PlayReady per a vendor report, and Evergreen gives Widevine and Edge-grade updates. This agrees with note 02.
7. **Assume DRM works through Widevine L3 and verify with the owner's accounts.** Set expectations at 720p for Disney+ and Hulu, and 480p on Hulu if HDCP cannot be verified. Do not promise 1080p or 4K.
8. **Add no DRM code to Atlas.** EME and the CDMs are inside the runtime. What Atlas owns is fullscreen handling for video (note 02), not decryption.

## Phase 0 checks to add

Add these to the Google line of note 06's checklist, in order:

1. **Real sign-in, default settings, owner's account.** Open accounts.google.com in an Atlas tab; enter the address, password and two-step verification (including a passkey or security key if used). Record whether any "not secure" or "not supported" page appears. Then open Gmail, Calendar, Drive, Docs and YouTube, wait ten minutes, close and reopen Atlas, and check the session survived.
2. **The same, with a custom UA** through the settings API (an `Atlas/…` token), then through the DevTools override. Record which pass. These show how much depends on Microsoft's override.
3. **Repeat in the final shell** (wry child web views, with Atlas's real browser arguments), not only a WPF test, and with an IPC handler attached and detached on a Google Sheets page.
4. **DRM:** play a Disney+ title and a Hulu title; in DevTools record which key system was used (`navigator.requestMediaKeySystemAccess` log) and the resolution reached. Play one YouTube rental or free-with-ads movie to see whether it uses EME.
5. **Re-run the capability probe** on any machine the owner will use, with and without the Parsec display.

## Questions for the owner

1. **Is the Google account a consumer account or a Google Workspace one?** A Workspace organisation with Context-Aware Access or required Endpoint Verification cannot work in a web view that lacks the extension.
2. **How do you sign in:** password and a phone code, a passkey, or a hardware security key? Each takes a different path through Windows and the web view.
3. **Do you use the Parsec virtual display, or a TV or monitor over HDMI, when watching Disney+ or Hulu?** It changes HDCP and PlayReady behaviour.
4. **If Google blocks the default sign-in, which do you prefer:** keep a regular browser for Google, accept an Atlas that identifies itself as "Atlas" through the DevTools override (and may still be blocked), or stop? There is no fourth option that Google documents.

## Corrections to earlier notes

These are listed for the reader to apply; this note edits no other file.

- **Notes 02 and 06 say WebView2 has no Widevine** (02: short answer, feature table row "DRM video and music", and "DRM and passkeys"; 06: "Widevine" under Chromium-class engines, and question 4 to the owner). The runtime on this machine ships and plays Widevine (L3); PlayReady is also advertised.
- **Note 06 says how Google treats WebView2 inside a full browser is untested.** It is now partly tested: the sign-in page loads, and the runtime presents as Chrome to Google. The email step and later checks are still untested.
- **Note 02's question 1 offers "present a different user-agent string (which works against Google's stated policy)".** In current WebView2, the default already differs from Edge toward Google, and setting a UA through the settings API makes it more recognisable as WebView2, not less.

## Sources

**Google:** [Sign in with a supported browser](https://support.google.com/accounts/answer/7675428) · [Security blog, 2019-04-18](https://security.googleblog.com/2019/04/better-protection-against-man-in-middle.html) · [Developers blog, 2020-08-28](https://developers.googleblog.com/2020/08/guidance-for-our-effort-to-block-less-secure-browser-and-apps.html) · [Developers blog, 2021-06-29](https://developers.googleblog.com/2021/06/upcoming-security-changes-to-googles-oauth-2.0-authorization-endpoint.html) · [OAuth policies](https://developers.google.com/identity/protocols/oauth2/policies) · [OAuth for desktop apps](https://developers.google.com/identity/protocols/oauth2/native-app) · [Widevine overview](https://developers.google.com/widevine/drm/overview) · YouTube Help: [system requirements](https://support.google.com/youtube/answer/78358), [purchased movies](https://support.google.com/youtube/answer/9679194), [HD and 4K](https://support.google.com/youtube/answer/3306741), [Premium offline](https://support.google.com/youtube/answer/11977233), [YouTube TV troubleshooting](https://support.google.com/youtubetv/answer/7129766) · [Chrome: protected content](https://support.google.com/chrome/answer/4410268)

**Microsoft:** [WebView2 browser features](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/browser-features) · [CoreWebView2Settings.UserAgent](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2settings.useragent) · [WebView2 release notes](https://learn.microsoft.com/en-us/microsoft-edge/webview2/release-notes/) · WebView2Feedback issues [#1562](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1562), [#1578](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1578), [#1584](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1584), [#1647](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1647) with [Discussion #3828](https://github.com/MicrosoftEdge/WebView2Feedback/discussions/3828), [#2552](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2552), [#2576](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2576), [#2862](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2862), [#4632](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4632), [#4828](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4828), [#4873](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4873), [#4935](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4935), [#4993](https://github.com/MicrosoftEdge/WebView2Feedback/issues/4993), [#5319](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5319)

**Tauri, wry and other apps:** [Tauri #11612](https://github.com/tauri-apps/tauri/issues/11612) · [wry `src/webview2/mod.rs`](https://github.com/tauri-apps/wry/blob/dev/src/webview2/mod.rs) · [Pake #1148](https://github.com/tw93/Pake/issues/1148) · [T3 Code #14815](https://github.com/pingdotgg/t3code/issues/14815) · [grok-app #1154](https://github.com/RongleCat/grok-app/issues/1154) · [flutter_inappwebview #2093](https://github.com/pichillilorenzo/flutter_inappwebview/issues/2093) · [JxBrowser troubleshooting](https://teamdev.com/jxbrowser/docs/guides/troubleshooting/issues.html)

**Services:** [Disney+ browser requirements](https://help.disneyplus.com/article/disneyplus-computer-browser-requirements) · [Disney+ data usage and quality](https://help.disneyplus.com/article/disneyplus-data-usage) · [Hulu computer and browser requirements](https://help.hulu.com/article/hulu-supported-computers)

**Local test (this machine, 2026-10-09):** WebView2 runtime 154.0.4258.62 and Edge 154.0.4258.62; WebView2 SDK 1.0.2903.40; Shaka Player 4.7.11 with Axinom's public multi-DRM test vector (the asset and licence servers listed in [Shaka's demo asset list](https://github.com/shaka-project/shaka-player/blob/main/demo/common/assets.js)). Scripts are in the agent's scratch area and are not part of the repository.
