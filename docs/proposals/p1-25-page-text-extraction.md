---
status: approved by the owner, 2026-10-12
slice: P1-25 (plan 1, Phase 3)
decisions: D18, D35, D40; D39 is not involved
date: 2026-10-12
---

# P1-25: how Atlas reads page text

## Why this proposal exists

D18 lets the owner find a page they visited by what it said, not only by its
title or address. D35 limits that: page text is kept for seven days and is
never taken from Google's sites, banking and payment sites, pages sent with
`Cache-Control: no-store`, or sites the owner excludes, and the method that
reads the text "must inject nothing on excluded sites". D40 adds that the
text lives in an expiring cache outside the daily backups.

Plan 1's P1-25 row says the concrete way Atlas reaches into a page must be
approved by the owner **before** any guarded page API is used, even in a
prototype. This document is that request. It compares five methods, sets
out the order in which Atlas decides whether it may look at a page at all,
recommends one method and writes down the synthetic test P1-25 would run
once approved. It changes no code and no decision.

If this step were skipped, the first implementation would choose a method by
convenience. The convenient one, running a script in the page, is the one
that Google's sign-in rules and this repository's own rules forbid (see
"Candidate 2").

## Terms used here

- **Page web view**: a WebView2 control showing an ordinary website in a
  tab, as opposed to Atlas's own local web views (the future editors).
- **Injection**: anything that places Atlas-authored code or objects inside
  the page's own JavaScript world, where the page's scripts can see it or
  run into it. Examples: `ExecuteScript`, `AddScriptToExecuteOnDocumentCreated`,
  wry's `with_initialization_script`, and the `window.ipc` global that wry
  adds with an IPC handler. A request filter or header rewrite is not
  injection, but `AGENTS.md` forbids it on Google's sites for the same
  reason: it makes Atlas a middleman.
- **DevTools protocol (CDP)**: Chromium's internal inspection protocol. WebView2
  lets the host application call it directly with `CallDevToolsProtocolMethod`
  over an in-process channel. This is **not** a remote debugging port: no TCP
  socket is opened and no `--remote-debugging-port` argument is passed. The
  remote port is the documented sign-in tripwire (note 07, section 1.5); the
  in-process call is a different mechanism.
- **Eligibility**: Atlas's decision, made from facts it already observes
  (address, redirect chain, response headers, settings), that a document may
  be read. Access happens only after a document is eligible.

Confidence labels follow the research notes: *high* means Microsoft's or
Chromium's documentation or source says so; *medium* means read from
documentation but not tried on this machine; *unverified* means the
feasibility test below must establish it before anything is built on it.

## What any method must satisfy

| Requirement | Source | What it means in practice |
|---|---|---|
| Inject nothing on excluded sites | D35 | No Atlas code in the page's JavaScript world on an excluded document, including by a race where the page navigated after Atlas decided. |
| No script injection on any page web view | `CLAUDE.md`, "Hands off Google's pages"; `AGENTS.md` | The standing rule is wider than D35: never inject scripts on a page web view at all. |
| No request filters, header rewrites or remote debugging port on Google | `AGENTS.md`; note 07, 1.9 and "What this means", item 3 | Observing a response header is allowed; installing `AddWebResourceRequestedFilter` is not. |
| Decide before access | Plan 1, Search, tier 2 ⚠️ | Eligibility, including redirects and frame boundaries, is settled before Atlas reads anything. |
| Detect `no-store` | D35 | From the headers of the response that produced the document. |
| No form contents | D42; D35's spirit | Text the owner typed into inputs, text areas or editable regions is never stored. |
| Cheap enough | S6 (performance is the deciding goal), D37's idle bar | No measurable idle cost; capture must not freeze the page or Atlas's interface. |

## The five candidates

### Candidate 1: DevTools protocol through `CallDevToolsProtocolMethod`

**How it works.** Atlas calls `ICoreWebView2::CallDevToolsProtocolMethod`
with one method name and JSON parameters and receives JSON back
asynchronously (high). Two methods are relevant:

- `DOMSnapshot.captureSnapshot` returns the document tree with its layout
  tree in one flattened response. The layout part lists, for every rendered
  text node, the text as laid out, so text hidden with `display: none` or
  never laid out is absent. Parameters let Atlas ask for no computed styles,
  no rectangles and no paint order, which keeps the response smaller (high
  for the protocol's shape; response size on real pages unverified).
- `Accessibility.getFullAXTree` returns the accessibility tree: roles and
  names as a screen reader would hear them (high for the method).

**Does it inject?** Not in the sense that matters here. `DOMSnapshot.captureSnapshot`
is served by the renderer's DevTools agent from C++; it adds no script, no
global and no listener to the page's JavaScript world, and does not need
`Runtime.enable`, the call behind the best-known way pages detect an
inspector (it makes the page's `console` serialise objects, which a page can
notice). It is still *page access*: the renderer pauses to serialise its
document, and a DevTools session becomes attached to that web view. So this
proposal treats a CDP call exactly like injection for eligibility: it is
never made on an excluded document. Whether the attached session survives
later navigations of the same tab, and whether a page can observe it, is
**unverified** and is test F15 below.

`Accessibility.getFullAXTree` is worse on cost: requesting the tree switches
Chromium's accessibility mode on for that renderer, and that mode stays on,
making layout and every DOM change more expensive for the life of the page
(medium, from Chromium's design; not measured here).

**Google risk.** By construction none: the call is never made on Google's
sites, because eligibility excludes them first. The residual risk is the
session that may stay attached after the tab later navigates to Google.
Google's stated triggers are automation and middlemen (note 07); the
documented tripwire is the remote debugging port, which this does not open.
Test F15 compares a tab that has captured a page with a fresh one, using
the detection probes a page could run. If any difference shows, this
candidate falls back to candidate 4's body reading (see "Fallback").

**Frames and redirects.** The call is addressed to the web view's top-level
page target. Same-process child frames are serialised into the same response
as separate documents; Atlas keeps only the root document and discards the
rest before anything leaves `atlas-platform`. Cross-origin frames run in
other processes under site isolation and, as far as the documentation shows,
are not reached by the top-level call at all (medium; test F9). Redirects do
not affect the call itself, because it only runs after the final document
is committed and judged eligible. The response names the root document's
address, so Atlas can check that it is still the document it judged.

**`no-store`.** The snapshot does not carry response headers, so this method
cannot detect `no-store` by itself. It must be paired with candidate 4's
header observation (without its body reading).

**Cost and reliability.** One asynchronous call per captured document. The
renderer is busy while it serialises; on a large page the JSON can reach
megabytes and must be parsed off the interface thread. The text is what the
page actually rendered, after its scripts ran, which matters for the many
sites that build their content in the browser. Shadow DOM text is included
(medium). Two catches: the snapshot also carries input values
(`inputValue`, `textValue`), which Atlas must never read; and the rendered
text of an editable region is the owner's own typing, so editable subtrees
are dropped (test F10).

**Verdict: recommended, paired with header observation from candidate 4.**

### Candidate 2: `ExecuteScript` reading `document.body.innerText`

**How it works.** `ICoreWebView2::ExecuteScript` runs a string of JavaScript
in the current top-level document and returns its result as JSON (high).
`document.body.innerText` gives rendered text with line breaks, which is
excellent text.

**Does it inject?** Yes, unambiguously. The script runs in the page's own
JavaScript world. The page can see it, intercept it (a page may redefine the
`innerText` getter) and break it, and Atlas's result can be poisoned.

**Is it acceptable on non-excluded sites?** D35 alone would allow it there,
since it only forbids injection on excluded sites. It is still rejected, for
three reasons:

1. **It breaks a standing rule.** `CLAUDE.md` and `AGENTS.md` say never to
   inject scripts on a page web view, not only on Google. Changing that is a
   guardrail change the owner would have to make explicitly, and this
   proposal recommends against asking for it.
2. **The race lands the script on the wrong page.** `ExecuteScript` runs on
   whatever document is current when it arrives. If the tab navigates after
   Atlas decides eligibility (a redirect by script, a sign-in button), the
   script runs on the new document, which may be accounts.google.com or a
   bank. Atlas would learn this only afterwards. With candidate 1 the same
   race yields a read-only snapshot that Atlas discards unread; with this
   candidate it yields script execution on an excluded site, a D35 violation
   that cannot be undone.
3. **Forced layout on the page's main thread.** `innerText` forces a style
   and layout pass inside the page's own thread, visible as jank on heavy
   pages (medium).

**Google risk.** Low only if eligibility is perfect and no race occurs;
otherwise it is exactly the "injecting scripts" behaviour Google blocks for.

**Frames, redirects, `no-store`.** Main frame only; nothing for headers.

**Verdict: rejected.**

### Candidate 3: UI Automation from outside the page

**How it works.** Windows UI Automation (UIA) is the accessibility interface
screen readers use. Atlas would walk the WebView2 control's UIA tree from
outside, through COM, and collect text (high for the mechanism).

**Does it inject?** No script. But the first UIA client to ask switches on
Chromium's accessibility mode for the renderer, with the same lasting cost
as `getFullAXTree`, and possibly for every page in the shared browser
process (medium; Chromium enables it when an assistive client is detected).
It would also make Atlas look like a screen reader to every site it reads,
which some sites change their behaviour for.

**Google risk.** Low for injection, but the accessibility switch would
apply to Google tabs sharing the process, which D35's spirit forbids:
Atlas's capture would change how an excluded page runs.

**Frames and redirects.** The UIA tree mixes frames together with no
reliable document address per node, so Atlas cannot prove the text came
from the eligible document. Hidden tabs expose little or nothing.

**`no-store`.** No access to headers.

**Cost and reliability.** Slow: one cross-process COM call per node, for
thousands of nodes. It would also interfere with the owner's own NVDA use
(P1-36).

**Verdict: rejected.**

### Candidate 4: `WebResourceResponseReceived`, headers and bodies

**How it works.** `ICoreWebView2_2::add_WebResourceResponseReceived` reports
every response the web view receives, with the request, the response status
and headers, and a `GetContent` call that returns the body (high). Unlike
`WebResourceRequested`, it needs no `AddWebResourceRequestedFilter`: it
observes, it does not intercept (high). Nothing is changed in flight.

**Does it inject?** No. It never touches the renderer.

**Google risk.** Very low. Atlas would receive events for Google's
responses as for any others; the handler ignores them by address before
reading anything. It is not a request filter and rewrites nothing, so it
stays inside `AGENTS.md`. Note 07's test used a filter to read headers; this
event does not need one. Whether subscribing has a side effect inside the
runtime (for example, enabling network instrumentation that changes caching)
is **unverified** (test F14 compares cache behaviour with and without it).

**Frames and redirects.** Each redirect hop and each frame's document
response arrives as its own event. Atlas has to match the top-level
document's response to the navigation it belongs to, using the request
address and the navigation's final address; there is no navigation
identifier on this event (medium). That matching is the riskiest part, and
it fails closed (see the check order).

**`no-store`.** This is the right source: the `Cache-Control` header of the
response that produced the document. It is the only candidate that sees it.

**Cost and reliability of the body.** `GetContent` returns the HTML as
sent, before scripts run. Static pages index well; sites that build their
content in the browser (most web applications) yield little more than a
shell. Atlas would also need an HTML parser and boilerplate removal; the
workspace already resolves `html5ever` (through a dependency), but using it
in a product crate would need its own audit. Bodies cost memory for every
response unless Atlas only asks for the top-level document's body.

**Verdict: adopt for header observation only** (the `no-store` check and the
redirect chain's statuses). Keep body reading as the **fallback extraction
method** if test F15 shows a CDP session is observable.

### Candidate 5: do not extract, index titles and addresses only

**How it works.** Tier 1 search already indexes titles and addresses from
history (P1-20). Atlas would add nothing.

**Does it inject? Google risk?** No, and none.

**Trade-off.** It gives up D18's recall by content, which D35 recorded the
owner wanting ("Dropping it for v1: the owner wants recall by content").
Choosing it would reverse that decision, so it is not a choice this
proposal can make.

**Verdict: the floor.** It is what Atlas does today, and what it keeps doing
for every ineligible page and whenever capture is off. It becomes the whole
answer only if the owner rejects both candidate 1 and candidate 4's body
reading.

### Summary

| | Injects? | Google risk | Frames | Redirects | `no-store` | Text quality | Cost |
|---|---|---|---|---|---|---|---|
| 1. CDP `DOMSnapshot` | No script; page access with an attached session | None by construction; residual session question (F15) | Root only; children discarded | After commit; address rechecked | No (needs 4) | Rendered, after scripts | One async call; parse off-thread |
| 1b. CDP `getFullAXTree` | No script; turns on accessibility mode | As above, plus lasting mode change | As above | As above | No | Names, not prose | Lasting renderer cost |
| 2. `ExecuteScript` | **Yes** | Race can run script on Google | Main only | Race risk | No | Best | Forced layout |
| 3. UI Automation | No script; global accessibility mode | Mode change reaches excluded pages | Mixed, unprovable | Unprovable | No | Fair | Very slow |
| 4. Response events | No | Very low | Per-response | Every hop seen | **Yes** | Poor on web apps (body) | Low for headers |
| 5. Titles only | No | None | n/a | n/a | n/a | None | None |

## The eligibility check, in order

Eligibility is a pure function in `atlas-core`, so it can be tested without a
window (S3). `atlas-platform` only reports facts to it and receives back an
`EligibleDocument` token. The function that performs the CDP call accepts
only that token, so the type system makes "access without eligibility"
impossible to write.

For every top-level navigation, Atlas evaluates these steps in order and
stops at the first that fails. A failure means the document is ineligible
and nothing is read; only its title and address reach tier 1 as today.

1. **Capture is switched on.** The P1-26 setting. Off means no CDP call is
   ever made in that session.
2. **Scheme is `https` or `http`.** Atlas's own pages, `file:`, `data:`,
   `blob:`, `about:` and error pages are never read.
3. **Not one of Google's sites.** A fixed, built-in list that the owner cannot
   remove entries from: `google.com` and Google's country domains
   (`google.co.uk`, `google.de`, and so on), `youtube.com`, `gmail.com`,
   `googleusercontent.com` and the other Google-owned domains named in the
   list, each with all subdomains. The match uses the registrable domain,
   lower-cased and IDNA-normalised, with the trailing dot removed, so
   `notgoogle.com` passes and `google.com.evil.test` passes, while
   `docs.google.com.` is excluded.
4. **Not a banking or payment site.** A built-in list of payment processors
   and banks (question 4 asks the owner which), plus a second, independent
   guard after capture (step 10), because no list of banks is complete.
5. **Not on the owner's exclusion list** (settings, D35).
6. **Every redirect hop passed steps 2 to 5.** WebView2 raises
   `NavigationStarting` for each redirect with `IsRedirected` set and the same
   navigation identifier (high), so Atlas records the chain as it happens.
   If any hop is excluded, the final document is ineligible, even when the
   final site is ordinary. A sign-in or payment flow that returns to an
   ordinary site often carries account details or tokens in that return
   page. The reverse case (an ordinary site redirecting into an excluded
   one) is ineligible at step 3, 4 or 5 on the final address.

   *Added by the owner's decision of 2026-10-12 (`docs/decisions.md`):*
   sign-in hops are skipped too. A sign-in flow often leaves the excluded
   site by its own script or an auto-submitting form rather than a redirect,
   and WebView2 reports that as a new navigation, not a redirect hop. So a
   document is also ineligible when (a) its navigation was not started by
   the owner (`NavigationStarting`'s `IsUserInitiated` is false) and the
   document committed when it began was on an excluded site, or (b) any
   response of its navigation was requested with a method other than `GET`
   (a form `POST`, read from the request in `WebResourceResponseReceived`),
   on any site. A link the owner follows out of an excluded site is
   user-initiated, so its page stays eligible if it passes every other step.
   Both facts are read from events Atlas already observes; no filter and no
   injection.
7. **The final response was observed and allows storing.** Atlas must have
   matched a `WebResourceResponseReceived` event to this document's final
   address with a 2xx status, a `text/html` or `application/xhtml+xml` type,
   and no `no-store` directive in any `Cache-Control` header (parsed as
   comma-separated directives, case-insensitively, across repeated headers).
   **If no matching response was observed** (a service-worker response, the
   back/forward cache, a missed event), the document is ineligible: Atlas
   fails closed rather than assuming the header was absent. Only the final
   document's headers count; a `no-store` on an intermediate redirect hop
   does not make the final page ineligible, and a final `no-store` page is
   ineligible however it was reached.
8. **Nothing has changed since.** Each tab carries a navigation generation
   that increases on every `NavigationStarting`, `SourceChanged` and process
   failure. A capture is scheduled after `NavigationCompleted` plus a short
   settle delay (proposed two seconds, so the page's scripts can render), and
   is cancelled if the generation changes. Immediately before the call,
   Atlas checks the generation again, on the same interface thread that
   receives navigation events, so no Atlas-side event can slip in between.

Only then is the CDP call made. After it returns:

9. **The snapshot is the judged document.** The root document's address in
   the response must equal the address judged in steps 2 to 7, and the
   generation must be unchanged. Otherwise the response is discarded without
   being parsed further. (A navigation that started in the browser process
   after the check can still race the call; this step is what catches it,
   and test F8 measures how often it happens.)
10. **The document has no sign-in or payment form.** If the snapshot contains
    an `input type=password` or a field marked with a payment `autocomplete`
    value (`cc-number`, `cc-csc`, `cc-exp`), the whole document is discarded.
    This is defence in depth for banks missing from the list.
11. **Only rendered, non-editable text of the root document is kept.** Child
    frame documents, input values, text inside `input`, `textarea`,
    `contenteditable` and their internal shadow trees, and `script`/`style`
    content are dropped. The text is capped (proposed 256 KiB per document)
    and handed to P1-26's expiring cache.

**Same-document navigations** (an application changing its address with the
History API) raise `SourceChanged` without a new document (high). The new
address runs through steps 2 to 5 again; the earlier response's headers
still apply, because no new response produced the document. One extra
capture is allowed per such change, at most once every 30 seconds per tab,
so long-lived web applications are not re-serialised constantly.

**Frames** are never read. An eligible page that embeds a payment widget,
a Google sign-in button or a bank's frame is still read, but only its own
root document's text.

## Recommendation

Approve **candidate 1 with `DOMSnapshot.captureSnapshot` only** for
extraction, paired with **candidate 4's `WebResourceResponseReceived` for
headers only**, under the eleven-step check above. In detail:

- One `DOMSnapshot.captureSnapshot` call per eligible document (plus the
  rate-limited same-document captures), with computed styles, rectangles and
  paint order all switched off.
- No other DevTools method: no `*.enable` call, no `Runtime`, `Debugger`,
  `Network`, `Emulation` or `Accessibility` domain, and no DevTools event
  subscription. No `--remote-debugging-port`, no user-agent change.
- `WebResourceResponseReceived` is used to read status, content type and
  `Cache-Control` of top-level document responses; no request filter, and no
  `GetContent` call in the recommended path.
- The page web view keeps `with_devtools(false)` (which hides the DevTools
  window from users; whether it also blocks host calls is test F18).
- JSON parsing runs on a worker thread. `serde_json` is already in the
  workspace for `xtask`; adding it to `atlas-platform` still needs the
  pre-add audit `AGENTS.md` requires.

**Fallback.** If test F15 shows a page can tell that a tab previously ran a
capture, or F16 shows unacceptable cost, P1-25 stops and reports. The fallback
it would then propose is candidate 4 with `GetContent` on top-level document
responses only, parsed in Rust; it touches no renderer at all but indexes
web applications poorly. The final fallback is candidate 5.

**Why not the alternatives:** candidate 2 breaks the no-injection rule and,
through the navigation race, can execute on an excluded site; candidate 3 is
slow, cannot prove where text came from and changes how every page in the
process runs; candidate 4 alone reads the HTML as sent, missing most
content of modern web applications; candidate 5 reverses D18.

## The feasibility test P1-25 runs after approval

**Where.** A disposable example program,
`crates/atlas-platform/examples/page_text_feasibility.rs`, modelled on the
existing `examples/page_close` harness, with a freshly generated WebView2
user-data folder under `target/` for each run. It never uses the owner's
profile, makes no request outside the machine and signs in to nothing.
A local synthetic HTTP server serves every page. Following the trap in
`docs/trapstoknow.md`, each accepted socket is set to blocking mode before
its bounded I/O.

**Host names.** Pages are served under reserved names (`plain.test`,
`bank.test`, `pay.test`, `owner.test`, `probe.test`) mapped to `127.0.0.1`
for the harness's own environment only, through WebView2's
`--host-resolver-rules` browser argument (unverified that WebView2 accepts it;
if not, distinct loopback addresses `127.0.0.2` and up are used, and the
lists below name those addresses instead). The harness puts `bank.test`
into the banking list and `owner.test` into the owner list through test-only
configuration, so the same code path runs. Google's real domains are tested
only in the pure `atlas-core` unit tests; no automated test navigates to
Google.

**Observation without injection.** The fixture pages are Atlas's own
synthetic content, so their own scripts may report measurements; they do so
by setting `document.title`, which Atlas already receives through its title
event. Atlas adds no IPC handler and no script to the page web view, even in
the harness. Every CDP call goes through one counting wrapper that records
the target address, so a test can assert that **zero** calls were made.

**Pure unit tests (`atlas-core`, run in the normal gate):**

- U1. Google list: `google.com`, `www.google.co.uk`, `mail.google.com`,
  `GOOGLE.COM.`, `youtube.com`, punycode forms are excluded;
  `notgoogle.com`, `google.com.example.test` and `googlex.com` are not.
- U2. Bank, payment and owner lists match by registrable domain and
  subdomain, and only those.
- U3. Scheme filter: only `http` and `https` pass.
- U4. Redirect chains: any excluded hop makes the result ineligible.
- U5. `Cache-Control` parsing: `no-store`, `No-Store`, `private, no-store`,
  repeated headers, `no-store` with whitespace, and malformed values
  (malformed means ineligible); `max-age=0` and `no-cache` alone are
  eligible.
- U6. Missing response, non-2xx status and non-HTML types are ineligible.
- U7. A generation change between scheduling and dispatch cancels.

**Harness cases (run by hand on this machine, results in the report):**

| # | Case | Pass condition |
|---|---|---|
| F1 | Plain page with visible, `display:none`, `visibility:hidden`, `<script>`, `<style>`, shadow DOM and Unicode/RTL text | Stored text equals the expected visible text exactly, after whitespace normalisation |
| F2 | Same page with `Cache-Control: no-store` (and each U5 spelling) | Zero CDP calls; nothing stored |
| F3 | Direct visits to `bank.test`, `pay.test`, `owner.test` | Zero CDP calls |
| F4 | `plain.test` redirects (301, 302, 307, 308) to `bank.test` | Zero CDP calls |
| F5 | `bank.test` redirects to `plain.test` | Zero CDP calls (chain rule) |
| F6 | `no-store` on a redirect hop only, then on the final page only | Eligible, then ineligible |
| F7 | Eligible page that navigates itself to `bank.test` during the settle delay (timer, `location.assign`, meta refresh) | Capture cancelled; zero calls to `bank.test` |
| F8 | 500 runs of a page that navigates to `bank.test` at random moments around the capture | No `bank.test` text stored ever; report how many snapshots were discarded at step 9 |
| F9 | Eligible page with same-origin, cross-origin (`plain2.test`) and `bank.test` iframes | Only the root document's text stored; record whether child documents appeared in the response |
| F10 | Page whose own script fills `input`, `textarea`, `contenteditable` and a `type=password` field | Password page discarded entirely; on the page without one, no typed text stored |
| F11 | Page with a `cc-number` field | Discarded |
| F12 | PDF, image, `text/plain`, 404 and 500 responses | Not captured |
| F13 | Same-document address change by History API, repeated quickly | At most one capture per change, and at most one every 30 s per tab; each new address is rechecked against steps 2 to 5 |
| F14 | Page served by a service worker; back/forward cache restore; reload from HTTP cache, each with and without the response subscription | Fail closed whenever no matching response was seen; report whether the subscription changes cache hits |
| F15 | Detection probes on `probe.test`: `navigator.webdriver`, the `console` getter trick for `Runtime.enable`, `debugger`-statement timing, enumerable globals; in a tab that captured `plain.test` first versus a fresh tab | Identical results; any difference stops P1-25 and triggers the fallback proposal |
| F16 | Pages of 1,000, 10,000 and 100,000 nodes | Report call latency, response bytes, parse time off-thread and the page's own long-task measurement. Proposed budget: on 10,000 nodes no added task over 50 ms in the page and no interface frame over 16 ms in Atlas; above a measured size, skip and report |
| F17 | Suspended (`TrySuspend`) and hidden tabs | No capture while suspended; deferred until visible or resumed |
| F18 | With `with_devtools(false)` | The host call still works; F12 still opens nothing; no TCP listening port appears for the WebView2 processes before or after capture |
| F19 | Capture switched off | Zero CDP calls over the whole run |
| F20 | A 204 and a 205 response requested from an excluded page | No call; the excluded page stays shown and unread *(added in review)* |
| F21 | `bank.test` sends itself to `plain.test` by script (`location.href`) | Ineligible, zero calls *(owner's decision, 2026-10-12)* |
| F22 | `bank.test` auto-submits a form `POST` to `plain.test` | Ineligible, zero calls *(same)* |
| F23 | The owner follows a link out of `bank.test` to an ordinary page | Captured *(same)* |
| F24 | An ordinary page auto-submits a form `POST` to itself | Ineligible, zero calls *(same)* |

**Evidence.** P1-25 delivers `docs/p1-25-page-text-feasibility.md` with each
case's result, the measurements, every failure kept rather than retried away,
and a production recommendation for P1-26. Nothing from real browsing is
captured; asking the owner to switch capture on for daily use waits for
P1-26 and its review.

## Questions for the owner

Each is a yes or no. The recommendation is in brackets.

1. Do you approve the DevTools-protocol call `DOMSnapshot.captureSnapshot`,
   and only that call, as the way Atlas reads page text on eligible pages,
   with `WebResourceResponseReceived` used only to read response headers?
   *(Recommended: yes.)*
2. Do you agree that Atlas should never run a script inside a web page to
   read its text, keeping the rule in `CLAUDE.md` as it is, on every site?
   *(Recommended: yes.)*
3. Should a page reached through a redirect chain that passed through an
   excluded site (for example, returning from a bank's or Google's sign-in to
   an ordinary site) be left unread? *(Recommended: yes.)* *The owner
   answered yes, and on 2026-10-12 extended it to script and form sign-in
   hops and every form `POST` (step 6).*
4. Should "Google's sites" include YouTube and Google's other own domains
   (such as `gmail.com` and `googleusercontent.com`), not only `google.com`
   and its country domains? *(Recommended: yes.)*
5. Should Atlas ship a built-in list of major banks and payment processors,
   plus the password-field and card-field guard, rather than relying on your
   own exclusion list alone? *(Recommended: yes; you would review the list
   before P1-26.)*
6. Should Atlas skip any page it cannot prove was sent without `no-store`
   (for example, one served from a cache or a service worker)?
   *(Recommended: yes; fail closed.)*
7. If test F15 shows a page can tell that a tab previously captured text,
   should P1-25 stop and come back to you with the header-and-body fallback,
   rather than continuing? *(Recommended: yes.)*
