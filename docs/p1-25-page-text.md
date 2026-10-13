# P1-25: reading page text, synthetic feasibility and recommendation

D18 lets the owner find a visited page by what it said. D35 limits that: no
text from Google's sites, banks and payment sites, `Cache-Control: no-store`
pages or sites the owner excludes, and the reading method must inject nothing.
On 2026-10-12 the owner approved
`docs/proposals/p1-25-page-text-extraction.md` (the "proposal" below): one
DevTools-protocol call, `DOMSnapshot.captureSnapshot`, on pages judged
eligible beforehand, with response headers read only through
`WebResourceResponseReceived`. This slice builds that path and proves it on
synthetic pages. Storing the text, the capture setting and expiry are P1-26.

The short answer: the approved method works and no detection signal was
found (F15 identical), so the proposal's fallback is not triggered. One case
still fails, by the owner's decision: WebView2 hides the second line of a
repeated `Cache-Control` header, so a page that sends `no-store` only on a
later line is read (F2). On 2026-10-12 the owner accepted that as a known
limit, approved the `visibility` computed style, and added a rule that
every exit from an excluded site that Atlas did not start is skipped
(F21–F27, "skip all exits", which replaced an earlier user-initiated test
the same day); all are recorded in `docs/decisions.md`. The independent review's fixes are listed under
"Review changes".

## What was built

- **Eligibility** (`crates/atlas-core/src/page_text.rs`, with
  `page_text/sites.rs` and `page_text/cache_control.rs`). A pure function,
  `judge`, runs the proposal's steps 1 to 8 in order from facts the platform
  observed and returns either an `Ineligible` reason or an
  `EligibleDocument`. That token has private fields, so only `judge` can make
  one, and the platform's capture call accepts nothing else. Google is matched
  by registrable domain: a fixed list (YouTube, Gmail, `googleusercontent.com`
  and others) plus `google.<tld>` and `google.co|com.<cc>`, after the `url`
  crate's lower-casing and IDNA mapping, with the trailing dot removed. The
  banking and payment list is a **starter list for the owner to review**
  (proposal question 5). `Cache-Control` is parsed strictly (RFC 9111 list
  syntax, quoted strings); an unreadable header counts as `no-store`.
  Responses from frames are told apart by `Sec-Fetch-Dest`; a missing
  response fails closed. **Exits** (owner's decision, 2026-10-12, "skip all
  exits"): a document reached by a navigation Atlas did not start that left
  an excluded or tainted document is ineligible (`LeftExcluded`), and so is
  any document whose navigation sent a request with a method other than
  `GET`, on any site, including a `POST` answered with a redirect
  (`FormPost`). `PageTextPolicy::taints` applies the same rule, plus excluded
  redirect hops, to decide which committed documents carry the taint on.
- **Observation** (`crates/atlas-platform/src/browser/page_text.rs`). Only
  when capture is switched on as a page is created, the page subscribes to
  `NavigationStarting` (each redirect hop, with the web view's `Source()` read
  as the document a new navigation leaves),
  `WebResourceResponseReceived` (status, `Content-Type`, `Cache-Control`
  and the request's method and `Sec-Fetch-Dest` for the navigation's own
  requests; no filter, no body, no other header), `SourceChanged`,
  `NavigationCompleted` and `ProcessFailed`. Each tab keeps a generation that
  rises on every navigation start, address change and process failure. A
  navigation is Atlas's own only when it starts for the address Atlas just
  asked `Page::navigate` to load (creating a tab, the address bar and every
  other Atlas entry point, and crash recovery, which reloads the address
  Atlas last asked for). Each tab carries a taint flag: set when a document
  commits after leaving an excluded or tainted document without Atlas, or
  through an excluded redirect hop, and cleared only by an Atlas navigation.
  It is tracked with the last exclusion lists even after capture is
  switched off, for pages that were observed. An adopted popup starts with
  its opener's taint, and with taint when its opener was not observed. The
  rules live in `page_text/state.rs`, apart from WebView2, so they have unit
  tests. A response is matched to any hop of the current navigation, so a
  redirect's response that arrives after the next hop starts still counts.
- **Capture.** Two seconds after a successful load (step 8's settle delay)
  the page thread judges the document and, if eligible, makes the one call
  with `{"computedStyles":["visibility"],"includePaintOrder":false,
  "includeDOMRects":false,"includeBlendedBackgroundColors":false,
  "includeTextColorOpacities":false}`. A single wrapper announces every call
  with its address (`PageTextOutcome::Requested`), which is how the harness
  counts calls. Just before the call, the generation is checked again and
  the web view's `Source()` must equal the judged address; otherwise the
  call is not made (`Stale` or `NotCommitted`). When the response arrives, a
  changed generation, or capture switched off, drops it unread
  (`Superseded`); otherwise one bounded worker parses it
  (`page_text/reader.rs` and `page_text/snapshot.rs`): responses over 8 MiB
  are dropped unparsed, one snapshot parses while at most one waits and any
  further one is skipped (`Busy`), `inputValue`
  and `textValue` are deleted before anything else is read, the root
  document's address must match the judged one (step 9), a password or
  payment-card field discards the document (step 10), and only laid-out text
  nodes of the root document outside `input`, `textarea`, `select`,
  editable regions, `script`, `style`, `noscript`, `template` and `title` are
  kept, capped at 256 KiB (step 11). History API address changes are rechecked
  and captured at most once per 30 seconds per tab. While a call is in
  flight the tab schedules no wake-up, and a capture that fell due meanwhile
  runs when the call returns. A capture that falls due while the tab is
  suspended, or while a `TrySuspend` is requested but not finished, waits
  until the tab is shown.
- **Commands and events.** `BrowserCommand::SetPageTextPolicy` switches
  capture on or off; `BrowserEvent::PageText` reports each outcome. The app
  never sends the command, so the running product makes no call and
  subscribes to nothing (F19); `atlas-app` only ignores the new event.
- **Dependency.** `atlas-platform` now declares the already audited
  `serde_json =1.0.151` (`std` only); it was already compiled into the app
  through GPUI. See `docs/dependency-audit.md`, "P1-25".

Nothing in this slice runs a script in a page, adds an IPC handler,
initialization script or request filter, changes the user agent, opens a
debugging port or calls any DevTools method other than
`DOMSnapshot.captureSnapshot` (no `Runtime.enable`, no `*.enable`, no event
subscription). The page web view keeps `with_devtools(false)`.

## The native harness

`crates/atlas-platform/examples/page_text_feasibility.rs` (with
`page_text_feasibility/{cases,pages,server}.rs`) starts a synthetic HTTP
server and a fresh WebView2 profile under the temporary folder for each of
four browser runs. It never visits a real site and signs in to nothing.

**Deviation from the proposal: host names.** The proposal's first choice was
`--host-resolver-rules` mapping `plain.test`, `bank.test` and so on to
loopback. That needs a harness-only browser-argument path in the production
environment code, so the harness uses the proposal's documented alternative:
each site listens on its own loopback address (`plain` 127.0.0.1, `bank`
127.0.0.2, `pay` 127.0.0.3, `owner` 127.0.0.4, `probe` 127.0.0.5, `plain2`
127.0.0.6), and the policy lists `127.0.0.2`/`127.0.0.3` as banking and
`127.0.0.4`/`127.0.0.5` as owner exclusions through
`PageTextPolicy::with_banking` and the owner list, so the same code path runs.
Google's real domains are tested only in the `atlas-core` unit tests.

Run it, with no build of the same executable in progress:

```powershell
cargo build -p atlas-platform --example page_text_feasibility --profile local --offline --locked
.\target\local\examples\page_text_feasibility.exe            # all cases, 500 race runs
.\target\local\examples\page_text_feasibility.exe --only F1,F2 --race-runs 20
```

The recorded run is `target/p125-final-3.log` (2026-10-12, after the
re-review changes and "skip all exits", the WebView2 Evergreen runtime on
this machine; `target/` is not committed). It exits 1 because F2 fails, as
the owner accepted. Earlier runs are kept as `target/p125-final.log` and
`target/p125-final-2.log`.

## Results

Unit tests U1–U7 are in `crates/atlas-core/tests/page_text.rs` (15 tests,
including the exit, taint and `POST` rules) and the module tests (7). The
platform's observation state has 17 tests in `page_text/state.rs`, including
regressions for the in-flight wake-up, step 8's scheduled generation, a
cancelled navigation, script and link exits from a bank, taint carried over
several hops and cleared only by Atlas, Atlas navigation recognised only for
its own address, taint from an excluded redirect hop, taint tracked while
capture is off, popup seeding, a posted document and a `POST` redirect whose
response arrives after the next hop; snapshot reading has 9 tests on
synthetic DevTools responses. `cargo xtask check` passed on 2026-10-12 with
all 967 workspace tests and the application smoke run
(`target/p125-gate-3.log`).

| # | Result | What happened |
|---|---|---|
| F1 | **Pass** | Stored text equals the expected visible text exactly: heading, inline bold run joined correctly, shadow-DOM text, French, Hebrew (RTL) and Japanese; `display:none`, `visibility:hidden`, `<script>`, `<noscript>` and `<style>` absent. The first run with `computedStyles: []`, as the proposal wrote, **kept the `visibility:hidden` text**; requesting only the `visibility` style fixed it (see "Deviations"). |
| F2 | **Fail (accepted limit)** | Six of seven spellings pass: `no-store`, `No-Store`, `private, no-store`, `max-age=0 ,  no-store  ` → ineligible, no call; `no-store="` and `max-age=0 private` → malformed, no call. **Two separate header lines, `max-age=0` then `no-store`, were read.** WebView2 reports only the first line: both `GetHeaders("Cache-Control")` and the full `GetIterator` returned `max-age=0` alone (diagnostic run, 2026-10-12). |
| F3 | Pass | `bank`, `pay`: ineligible (banking); `owner`: ineligible (owner); zero calls. |
| F4 | Pass | `plain` → `bank` by 301, 302, 307 and 308: ineligible, zero calls. |
| F5 | Pass | `bank` → `plain` by 302 and 307: ineligible (redirect through an excluded site), zero calls. |
| F6 | Pass | `no-store` on the redirect hop only: captured. On the final page only: ineligible, no call. |
| F7 | Pass | Timer, `location.assign` and meta refresh to `bank` 500 ms after load: the scheduled capture was cancelled, the bank page judged ineligible, zero calls. |
| F8 | Pass | 500 runs with a 100 ms settle and a navigation to `bank` 50–249 ms after load: 322 race pages captured, 112 cancelled before dispatch, 66 dropped unread because the tab changed while the call was in flight, **0 discarded at step 9**, 0 calls to `bank`, 0 bank text stored. |
| F9 | Pass | Only "Frame host page" stored. The response held 2 documents: the root and the same-origin frame; the cross-origin `plain2` and `bank` frames did not appear (site isolation). |
| F10 | Pass | Password page discarded whole. On the fields page, only "Comment page" stored: no typed input, textarea, editable-region, `value` attribute or option text. |
| F11 | Pass | `autocomplete=cc-number` page discarded. |
| F12 | Pass | PDF, PNG and `text/plain`: ineligible (content type). 404 and 500: WebView2 reports the navigation unsuccessful, so no capture is scheduled. Zero calls for all five. |
| F13 | Pass | First capture at load; five `pushState` calls three seconds later were each rechecked (four cancelled by the next); exactly one more capture, of `/history/5`, 30.02 s after the first. |
| F14 | Recorded | A **service worker's synthetic response raises `WebResourceResponseReceived`** with the worker's headers (0 network hits), so it was judged on those and captured. Back navigation, a second visit to a `max-age=600` page and a reload each raised a matching response and were captured; reload needed a fix, below. Server hits (landing 1, cacheable 2) were **identical with and without the subscription**, so subscribing did not change caching in this test. No case reached the fail-closed branch, so whether a true back/forward-cache restore raises the event is not established. |
| F15 | **Pass** | Probe results in a tab that had captured `plain` and in a fresh tab were identical: `navigator.webdriver` false, console getter not read, `debugger` not paused, 238 enumerable globals with the same name hash, no automation globals. The fallback is not triggered. |
| F16 | Pass | 1,000 nodes: 70.6 KB response, 12 ms call, 4 ms parse. 10,000 nodes: 750 KB, 112 ms, 24 ms. 100,000 nodes: 8.1 MB, 944 ms, 199 ms, text capped at 256 KiB. The page's own long-task observer recorded no new task over 50 ms during any capture. Atlas interface frames were not measured (no Atlas window in the harness). |
| F17 | Pass | A hidden tab suspended at five seconds; its capture fell due at seven and did not run; showing the tab resumed it and the capture ran. |
| F18 | Pass (partial) | The host call works with `with_devtools(false)` (every capture above). No TCP listening socket belonged to this run's WebView2 processes (6 then 7 processes, found by profile path) before or after captures. Whether F12 opens DevTools was not exercised. |
| F19 | Pass | A run with capture off produced zero page-text events, including zero calls. |
| F20 | Pass | From a shown bank page, a 204 and a 205 navigation committed nothing: the bank page stayed shown, no capture was scheduled for it and no call was made. *(Added in review.)* |
| F21 | Pass | The bank page's script set `location.href` to a plain page 300 ms after load. The plain page loaded and was judged `LeftExcluded`; zero calls. |
| F22 | Pass | The bank page auto-submitted a `POST` form to a plain page. The server received the `POST`; the page was judged `LeftExcluded` (the `FormPost` rule would also apply); zero calls. |
| F23 | Pass | A bank page focused its own link; the harness posted an Enter key to the focused page window. The plain page opened and was judged `LeftExcluded`, zero calls: a link out of an excluded site is not an Atlas navigation. (Under the replaced user-initiated test this page was captured, which is what prompted "skip all exits".) |
| F24 | Pass | A plain page auto-submitted a `POST` form to itself: judged `FormPost`, zero calls. |
| F25 | Pass | A plain page auto-submitted a `POST` answered by 302, 303 and 307, to the same site and to `plain2`. All six final pages (`GET` after 302 and 303) were reached and judged `FormPost`, zero calls. This shows WebView2 raises `WebResourceResponseReceived` for the redirect response on this runtime; the harness cannot tell whether it arrived before or after the next hop started, so the late-arrival case is covered by the state test `a_post_redirect_response_arriving_after_the_next_hop_still_counts`. |
| F26 | Pass | The bank's script went to a plain relay page, whose own script moved on 300 ms later (before its settle delay, so the relay itself was never judged). The next plain page was judged `LeftExcluded`, which only the carried taint can cause because the relay is not excluded. An Atlas navigation in the same tab afterwards was captured. |
| F27 | Pass | A focused button on a bank page opened a popup with `window.open` (synthetic Enter); the harness adopted it as Atlas does. The popup's page was judged `LeftExcluded`. The same popup from a plain page was not (`NoResponse`). **Finding:** an adopted popup's first document raised no matching response event on this runtime, so popups are never read today (they fail closed at step 7). |

Earlier runs also found a product defect, fixed before the recorded run: the
runtime sends -1 for an empty attribute value (`contenteditable=""`), which
the first parser rejected as malformed (F10). Two harness defects were fixed
too, without loosening any check: the probe page's initial title began with the awaited
prefix, and the port query lost its quotes on the PowerShell command line and
found no processes (the case now fails if it finds none).

## Review changes (2026-10-12)

The independent review's findings, each fixed in a new commit:

1. **No busy loop while a call is in flight.** `TextState::due` reports no
   wake-up while a call is outstanding; a capture that fell due meanwhile is
   re-armed when the call returns (state test
   `no_wake_up_is_reported_while_a_call_is_in_flight`).
2. **Switching capture off mid-call** drops the response unread
   (`Superseded`). `SetPageTextPolicy(None)` now documents that a snapshot
   already handed to the parser may still report.
3. **Never the previous document.** The call re-checks the generation and
   requires `Source()` to equal the judged address (F20, and the
   `NotCommitted` outcome). Step 8 now compares the generation the capture
   was scheduled for with the one at judging time; the earlier code compared
   a value with itself.
4. **Memory.** The parse limit is 8 MiB (the 100,000-node page was 8.1 MB);
   one worker thread parses and at most one more snapshot waits. **Deviation:**
   the size is checked after webview2-com has converted the UTF-16 response
   to a Rust string, not before. Measuring first means implementing the COM
   completion handler directly, which needs a direct `windows-core`
   dependency that is not approved. Worst case, one oversized response is
   converted and then dropped unparsed. Owner question 4.
5. **`TextState` unit tests,** listed above.
6. **F2 trap reworded** as the owner's accepted limit.
7. **No call while suspending.** Capture is deferred when `TrySuspend` has
   been requested but has not finished, not only when `IsSuspended` reads
   true (F17 passes). A suspension requested while a call is already in
   flight is not exercised by the harness and is unverified.
8. **Hardening:** every trailing dot is stripped from a host; a page-text
   registration failure is reported as a browser error and leaves that tab
   uncaptured instead of failing the page; the header helpers share one
   function; the Google list gained `.goog`, `android.com`, `chrome.com`,
   `gvt1.com`, `gvt2.com` and `googleblog.com` (owner question 2).
9. **Known limit, editors:** text inside script-driven code editors that do
   not use `contenteditable` (Monaco and Ace style, which keep a hidden
   `textarea` and draw the text in ordinary elements) and pages put into
   `designMode` are not recognised as editable, so their visible text can be
   read. Typed text in their hidden `textarea` is still removed.

## Re-review changes (2026-10-12)

1. **"Skip all exits"** replaced the `IsUserInitiated` test (owner's
   decision); F23 now expects the linked page to be skipped.
2. **`POST` answered with a redirect:** a response is matched to any hop of
   the navigation, not only the address requested last (F25 and a state
   test).
3. **Taint across hops** (F26) and **popups seeded from their opener**
   (F27).
4. **`Source()` is read at each new navigation** as the document being left,
   and taint is tracked with the last exclusion lists while capture is off,
   for pages that were observed. Pages created with capture off still
   register nothing, so switching capture on later starts clean only for new
   tabs.
5. **`SetPageTextPolicy`** now documents that a snapshot already queued can
   still report `Captured`; P1-26 must re-check the setting on receipt.

## Deviations from the proposal

1. **One computed style.** The proposal said "computed styles switched off".
   With none, `visibility:hidden` text is laid out and returned (F1 failed).
   Atlas now requests only `visibility` and drops `hidden`/`collapse`. It is
   the same method and still requests no rectangles, paint order or colours.
   **The owner approved this on 2026-10-12.**
2. **Loopback addresses instead of `--host-resolver-rules`,** as above.
3. **Reloads.** WebView2 raises no `SourceChanged` for a reload or a visit to
   the same address, so the document address is then taken from the
   navigation's last requested address on successful completion, still
   judged against its own response.
4. **Report name.** This report is `docs/p1-25-page-text.md`, not the
   proposal's `p1-25-page-text-feasibility.md`.
5. **Exits,** added to step 6 by the owner's decisions ("skip all exits"
   and the `POST` rule); the proposal now records them.
6. **Size check after conversion,** review item 4 above.

## Production recommendation for P1-26

Adopt this path: `atlas_core::judge` before access, the observation and
capture in `atlas-platform/src/browser/page_text.rs`, and the parameters
above. Specifically:

- Keep capture off by default and subscribe only for pages created while it
  is on, as now. P1-26 owns the setting, the cache, expiry and the
  restart/restore rules (D40).
- Keep the two-second settle delay and the 30-second same-document interval.
  F8 shows the in-flight generation check catches every race seen; step 9
  never had to.
- Skip or defer capture above a measured size. A 100,000-node page cost about
  about one second in the renderer and 200 ms of parsing; P1-26 should cap by
  response size (the 8 MiB bound is a safety limit, not a budget) and measure
  Atlas interface frames in the running app, which this harness could not.
- Keep "skip all exits" and the carried taint. Its limits: a tab that was
  created while capture was off is never observed, so switching capture on
  later starts clean only for new tabs, and a popup from such a tab starts
  tainted. Taint is decided with the lists in force when the page commits.
- **P1-26 must re-check the capture setting when it receives `Captured`:**
  a snapshot already queued or parsing when capture is switched off can
  still report its text (`SetPageTextPolicy` documents this).
- Popups are not read at all today (F27's finding). If P1-26 wants popup
  text, it needs a way to observe a popup's first response; until then they
  fail closed.
- F2 is an accepted limit; the settings text in P1-26 should not promise
  that every `no-store` page is skipped.
- Treat a service worker's response as the page's response (F14). That
  follows the proposal's rule, but it means a site's worker, not its server,
  decides `no-store` for pages it serves.

## Owner checks and questions

1. **F2** is decided: accepted as a known limit. Should a report be sent to
   Microsoft? Nothing has been sent; it goes only with your approval.
2. Review the built-in banking and payment list in
   `crates/atlas-core/src/page_text/sites.rs` (proposal question 5) and the
   Google list beside it (question 4), including the six domains added in
   review: `.goog`, `android.com`, `chrome.com`, `gvt1.com`, `gvt2.com` and
   `googleblog.com`.
3. The `visibility` computed style is approved (deviation 1).
4. **Size check after conversion** (review item 4): accept it, or approve a
   direct `windows-core` dependency (already compiled into the app through
   webview2-com) so the size is checked before conversion? Recommended:
   accept for now; the parser still never sees an oversized response.
5. In the running product after P1-26: check that F12 opens no DevTools and
   that capturing a large page causes no visible pause.
