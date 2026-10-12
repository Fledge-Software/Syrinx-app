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
fails and needs the owner's attention: WebView2 hides the second line of a
repeated `Cache-Control` header, so a page that sends `no-store` only on a
later line is read (F2, below).

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
  response fails closed.
- **Observation** (`crates/atlas-platform/src/browser/page_text.rs`). Only
  when capture is switched on as a page is created, the page subscribes to
  `NavigationStarting` (each redirect hop), `WebResourceResponseReceived`
  (status, `Content-Type`, `Cache-Control` and the request's
  `Sec-Fetch-Dest` of the navigation's own requests; no filter, no body, no
  other header), `SourceChanged`, `NavigationCompleted` and `ProcessFailed`.
  Each tab keeps a generation that rises on every navigation start, address
  change and process failure.
- **Capture.** Two seconds after a successful load (step 8's settle delay)
  the page thread judges the document and, if eligible, makes the one call
  with `{"computedStyles":["visibility"],"includePaintOrder":false,
  "includeDOMRects":false,"includeBlendedBackgroundColors":false,
  "includeTextColorOpacities":false}`. A single wrapper announces every call
  with its address (`PageTextOutcome::Requested`), which is how the harness
  counts calls. When the response arrives, a changed generation drops it
  unread (`Superseded`); otherwise a short-lived thread parses it
  (`page_text/snapshot.rs`): responses over 32 MiB are refused, `inputValue`
  and `textValue` are deleted before anything else is read, the root
  document's address must match the judged one (step 9), a password or
  payment-card field discards the document (step 10), and only laid-out text
  nodes of the root document outside `input`, `textarea`, `select`,
  editable regions, `script`, `style`, `noscript`, `template` and `title` are
  kept, capped at 256 KiB (step 11). History API address changes are rechecked
  and captured at most once per 30 seconds per tab. A suspended tab's capture
  waits until the tab is shown.
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

The recorded run is `target/p125-final.log` (2026-10-12, the WebView2
Evergreen runtime on this machine; `target/` is not committed). It exits 1
because F2 fails.

## Results

Unit tests U1–U7 are in `crates/atlas-core/tests/page_text.rs` (12 tests) and
the module tests (7); snapshot reading has 9 tests on synthetic DevTools
responses. `cargo xtask check` passed on 2026-10-12 with all 930 workspace
tests and the application smoke run (`target/p125-gate.log`).

| # | Result | What happened |
|---|---|---|
| F1 | **Pass** | Stored text equals the expected visible text exactly: heading, inline bold run joined correctly, shadow-DOM text, French, Hebrew (RTL) and Japanese; `display:none`, `visibility:hidden`, `<script>`, `<noscript>` and `<style>` absent. The first run with `computedStyles: []`, as the proposal wrote, **kept the `visibility:hidden` text**; requesting only the `visibility` style fixed it (see "Deviations"). |
| F2 | **Fail** | Six of seven spellings pass: `no-store`, `No-Store`, `private, no-store`, `max-age=0 ,  no-store  ` → ineligible, no call; `no-store="` and `max-age=0 private` → malformed, no call. **Two separate header lines, `max-age=0` then `no-store`, were read.** WebView2 reports only the first line: both `GetHeaders("Cache-Control")` and the full `GetIterator` returned `max-age=0` alone (diagnostic run, 2026-10-12). |
| F3 | Pass | `bank`, `pay`: ineligible (banking); `owner`: ineligible (owner); zero calls. |
| F4 | Pass | `plain` → `bank` by 301, 302, 307 and 308: ineligible, zero calls. |
| F5 | Pass | `bank` → `plain` by 302 and 307: ineligible (redirect through an excluded site), zero calls. |
| F6 | Pass | `no-store` on the redirect hop only: captured. On the final page only: ineligible, no call. |
| F7 | Pass | Timer, `location.assign` and meta refresh to `bank` 500 ms after load: the scheduled capture was cancelled, the bank page judged ineligible, zero calls. |
| F8 | Pass | 500 runs with a 100 ms settle and a navigation to `bank` 50–249 ms after load: 313 race pages captured, 117 cancelled before dispatch, 70 dropped unread because the tab changed while the call was in flight, **0 discarded at step 9**, 0 calls to `bank`, 0 bank text stored. |
| F9 | Pass | Only "Frame host page" stored. The response held 2 documents: the root and the same-origin frame; the cross-origin `plain2` and `bank` frames did not appear (site isolation). |
| F10 | Pass | Password page discarded whole. On the fields page, only "Comment page" stored: no typed input, textarea, editable-region, `value` attribute or option text. |
| F11 | Pass | `autocomplete=cc-number` page discarded. |
| F12 | Pass | PDF, PNG and `text/plain`: ineligible (content type). 404 and 500: WebView2 reports the navigation unsuccessful, so no capture is scheduled. Zero calls for all five. |
| F13 | Pass | First capture at load; five `pushState` calls three seconds later were each rechecked (four cancelled by the next); exactly one more capture, of `/history/5`, 30.02 s after the first. |
| F14 | Recorded | A **service worker's synthetic response raises `WebResourceResponseReceived`** with the worker's headers (0 network hits), so it was judged on those and captured. Back navigation, a second visit to a `max-age=600` page and a reload each raised a matching response and were captured; reload needed a fix, below. Server hits (landing 1, cacheable 2) were **identical with and without the subscription**, so subscribing did not change caching in this test. No case reached the fail-closed branch, so whether a true back/forward-cache restore raises the event is not established. |
| F15 | **Pass** | Probe results in a tab that had captured `plain` and in a fresh tab were identical: `navigator.webdriver` false, console getter not read, `debugger` not paused, 238 enumerable globals with the same name hash, no automation globals. The fallback is not triggered. |
| F16 | Pass | 1,000 nodes: 70.6 KB response, 12 ms call, 2 ms parse. 10,000 nodes: 750 KB, 94 ms, 17 ms. 100,000 nodes: 8.1 MB, 979 ms, 170 ms, text capped at 256 KiB. The page's own long-task observer recorded no new task over 50 ms during any capture. Atlas interface frames were not measured (no Atlas window in the harness). |
| F17 | Pass | A hidden tab suspended at five seconds; its capture fell due at seven and did not run; showing the tab resumed it and the capture ran. |
| F18 | Pass (partial) | The host call works with `with_devtools(false)` (every capture above). No TCP listening socket belonged to this run's WebView2 processes (6 then 7 processes, found by profile path) before or after captures. Whether F12 opens DevTools was not exercised. |
| F19 | Pass | A run with capture off produced zero page-text events, including zero calls. |

Earlier runs also found a product defect, fixed before the recorded run: the
runtime sends -1 for an empty attribute value (`contenteditable=""`), which
the first parser rejected as malformed (F10). Two harness defects were fixed
too, without loosening any check: the probe page's initial title began with the awaited
prefix, and the port query lost its quotes on the PowerShell command line and
found no processes (the case now fails if it finds none).

## Deviations from the proposal

1. **One computed style.** The proposal said "computed styles switched off".
   With none, `visibility:hidden` text is laid out and returned (F1 failed).
   Atlas now requests only `visibility` and drops `hidden`/`collapse`. It is
   the same method and still requests no rectangles, paint order or colours.
2. **Loopback addresses instead of `--host-resolver-rules`,** as above.
3. **Reloads.** WebView2 raises no `SourceChanged` for a reload or a visit to
   the same address, so the document address is then taken from the
   navigation's last requested address on successful completion, still
   judged against its own response.
4. **Report name.** This report is `docs/p1-25-page-text.md`, not the
   proposal's `p1-25-page-text-feasibility.md`.

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
  one second in the renderer and 170 ms of parsing; P1-26 should cap by
  response size (the 32 MiB bound is a safety limit, not a budget) and measure
  Atlas interface frames in the running app, which this harness could not.
- **Decide the repeated-header gap (F2)** before switching capture on for
  daily use; see the owner questions.
- Treat a service worker's response as the page's response (F14). That
  follows the proposal's rule, but it means a site's worker, not its server,
  decides `no-store` for pages it serves.

## Owner checks and questions

1. **F2, repeated `Cache-Control` lines.** WebView2 shows only the first line
   of a repeated header, and no API Atlas is allowed to use exposes the rest.
   A site that sends `no-store` on a second `Cache-Control` line will have its
   text stored. Options: (a) accept this as a documented residual of D35,
   relying on the site lists and the password/card guard for sensitive pages
   (recommended, because servers normally send one line and no allowed method
   can see the second); (b) report it to Microsoft and keep capture off until
   it is fixed; (c) reconsider the method, which the proposal's other
   candidates do not solve either. This is your decision.
2. Review the built-in banking and payment list in
   `crates/atlas-core/src/page_text/sites.rs` (proposal question 5) and the
   Google list beside it (question 4).
3. Confirm the `visibility` computed style (deviation 1).
4. In the running product after P1-26: check that F12 opens no DevTools and
   that capturing a large page causes no visible pause.
