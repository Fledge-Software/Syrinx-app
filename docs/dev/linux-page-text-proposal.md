---
status: proposal; not approved or enabled
scope: Linux page-text method only
decisions: D18, D35, D40; 2026-10-12 and 2026-10-27 taint and deletion rules
date: 2026-11-02
---

# Linux page text: reading the document response

Linux needs an explicit choice because WebKitGTK does not provide the host-side DOM snapshot that the owner approved for Windows in P1-25 (`docs/page-text-indexing.md`). Reading the document response is a different method with weaker visibility and sensitive-form checks. Treating it as an interchangeable implementation would silently change the owner's privacy expectations.

The recommendation is to keep capture off while the owner considers these limits. If the owner wants this fallback, first approve a synthetic feasibility run against the cases below, followed by review of its evidence. Approval of this proposal would not itself enable capture for daily browsing. Linux has no reader today: `READS_PAGE_TEXT` in `crates/syrinx-platform/src/browser_gtk/mod.rs` is `false`, and no implementation or dependency change is included here.

## The reader at ad1cf78

A reader for this method was written during the port and kept switched off behind a `READS_SENT_HTML = false` flag. The review of #226 removed it on 2026-11-03, because the owner has not approved the method and the house rules allow no dead code. It is kept in history at commit `ad1cf78` on `linux-port`, so #219 can restore it rather than rewrite it:

- `crates/syrinx-core/src/page_text/sent_html.rs`: `sent_html_text`, its refusals (`SentHtmlRefusal`), the 8 MiB `SENT_HTML_LIMIT_BYTES` bound and its parser tests, plus `is_sign_in_or_card_field` and `bounded_page_text`, which it shared with the Windows snapshot reader (`crates/syrinx-platform/src/browser/page_text/snapshot.rs`, then edited to call them);
- `crates/syrinx-core/src/bookmark_import/tokenizer.rs`: the quote-aware mode (`Tokenizer::quoted`, `skip_raw_text`) that only this reader used, and the `pub(crate)` visibility it needed there and in `text.rs` and `bookmark_import.rs`;
- `crates/syrinx-platform/src/browser_gtk/page_text.rs`, lines 329–500: the `schedule` → `tick` → `read` → `arrived` → `parse` → `captured` path that fetched the main resource's body with `WebResource::data` and parsed it on a short-lived thread, at most two at once.

The observation and taint tracking that Linux needs today stay in `browser_gtk/page_text.rs`; a capture that falls due is judged with capture off.

## What the method reads

A main resource is the response that supplies a tab's own document, rather than an image, script, or embedded frame. WebKitGTK's [`get_main_resource`](https://webkitgtk.org/reference/webkit2gtk/stable/method.WebView.get_main_resource.html) identifies it, and [`WebResource.get_data`](https://webkitgtk.org/reference/webkit2gtk/stable/method.WebResource.get_data.html) asynchronously returns its resource data. These are host APIs, with no page script, message bridge, inspector server, request filter, or header rewrite. The installed WebKitGTK 2.54.1 interface descriptions were also inspected; the linked API 4.1 reference describes an older library release and is evidence for the API contract, not a test of this runtime.

“As sent” means the document response data that WebKit exposes, before page scripts modify the document object model (DOM). It does not mean a capture of raw compressed HTTP packets. A cached or service-worker response also needs its own eligibility evidence; this proposal does not assume that the bytes came directly from the origin server.

The reader kept at `ad1cf78` (above) retains ordinary response text, decodes the supported HTML entities and collapses whitespace. It omits scripts, styles, the document head, embedded-frame markup, form-field values, editable regions and known hidden subtrees. A password field or a payment-card autocomplete marker present in the response makes it discard the document.

## What it misses, and why that matters

| Page behavior | Result from response HTML | Consequence |
| --- | --- | --- |
| JavaScript builds the article after loading | Only the original shell is available. | Search may miss most of the page. Waiting longer does not fix it. |
| A style sheet hides text or makes an element editable | The reader has no computed styles. | It can retain text the owner never saw. Inline hiding and `hidden` are only partial protections. |
| JavaScript adds a password or card field | That field is absent from the response. | The Windows defense-in-depth check cannot be reproduced. An unlisted sign-in page can appear eligible. |
| JavaScript removes sensitive text that was in the response | The original text remains available to the reader. | It can retain content absent from the displayed page. |
| The owner types into an input or live editor | Those later DOM edits are not response data. | This method does not collect the live buffer, but it also cannot promise that arbitrary ordinary response text contains no personal information. |
| Invalid or unusual HTML changes how the browser parses it | The small shared tokenizer is not WebKit's DOM parser. | Its output can differ from the rendered document; boundary and malformed-input fixtures are required. |
| A response exceeds the parser's limit | The Rust parser rejects more than 8 MiB and caps accepted text at 256 KiB. | The native API has already returned the body before that size check. This is not an 8 MiB allocation bound on the native read. |

These are method limitations, not reasons to weaken D35. In particular, this proposal cannot claim that it preserves P1-25's “only rendered text” or detection of dynamically added sensitive fields. Accepting those differences is a separate, explicit owner choice. No amount of testing makes the missing DOM information available.

## What D35 still requires before any body access

The existing address-based rules remain mandatory. `browser_gtk/page_text.rs` uses the shared `browser/page_text/state.rs` and `syrinx-core` eligibility policy; a new Linux-specific allow list must not replace them.

1. Capture must be on, the scheme must be HTTP or HTTPS, and the final address must pass the built-in Google, bank/payment and owner exclusion lists. Label-boundary matching must still distinguish a domain and its subdomains from lookalikes.
2. The complete redirect chain must be established and every hop must pass those exclusions; unknown or incomplete navigation evidence fails closed. A navigation leaving an excluded or tainted document remains tainted unless Syrinx itself starts the replacement navigation. Links, page scripts, popups, forms, reload, Back and Forward do not clear the mark. A restored, reopened, duplicated or recovered tab starts tainted, under the 2026-10-27 decision.
3. Every relevant request method must be known to be GET. Missing methods, unmatched navigation facts, non-HTML or unsuccessful responses, and missing response evidence must fail closed. A form POST followed by a redirect must remain unread.
4. The matched final document response must permit storage. `Cache-Control: no-store`, including repeated header lines and malformed values under the existing parser, must prevent a body read. Linux's libsoup header joining is a hypothesis to prove with the repeated-header fixture, not evidence that all redirect/cache cases are correct.
5. The committed document, resource identity, policy and navigation generation must still match immediately before requesting data and after the asynchronous result arrives. Superseded data must be discarded. Same-document changes must retain the existing rate limit and be rechecked for exclusions.

An exclusion means no body call, not “read it and throw it away afterward.” The sensitive-field check on an eligible response necessarily follows its read, just as the Windows snapshot's sensitive-field check follows its snapshot. No iframe body may be requested. The fallback must not refetch the page over a new network path.

D40 remains unchanged: eligible text belongs only in the expiring index outside durable-state snapshots, expires after seven days, and expired or explicitly erased text must not return through restart, recovery or replay. The 2026-10-27 decisions still erase stored page text when capture is switched off or a site is excluded. History, titles and addresses already handled by the browser keep their existing rules; this proposal adds no logs or analytics.

## Alternatives considered

The Windows `DOMSnapshot.captureSnapshot` method remains preferred where available because it observes the rendered DOM without injecting script. It is a Chromium host API, not a WebKitGTK facility. Copying its method name into the Linux layer would not supply its behavior.

Running `innerText`, a user script, an isolated-world script, or a web-process extension that reads the DOM is rejected for this proposal. Script execution violates the page-view guardrail; an extension would be another page-access mechanism needing its own investigation and approval. A navigation race must never cause code to run on an excluded page.

Reading the accessibility tree is not an equivalent shortcut. It exposes a different, potentially frame-mixed view, lacks the response eligibility evidence by itself, and would require separate proof of its side effects and document boundaries. Screenshot OCR loses structure and introduces a new toolchain without resolving these issues.

Refetching the address from Rust is rejected. It adds a network path, does not reproduce the authenticated response already shown, and can repeat side effects or read different content. Replacing WebKitGTK or adding a Chromium media engine is outside this proposal and remains in the separate engine decisions.

Keeping Linux capture off is the recommended current behavior. Local tab, title and address search still works. It gives up recall by page content on Linux, which is already disclosed by the Linux note in Settings, while preserving the owner's ability to reject the weaker method.

## Evidence required before any later enabling PR

A later enabling change also needs explicit owner acceptance of the weaker sensitive-form checks and revised Linux Settings wording in `crates/syrinx-app/src/ui/page_text_settings.rs`. Its current shared description promises never to read pages with password or card fields, which this method cannot guarantee for fields added by script. The replacement must explain that limitation plainly and must be inspected in the real window. This proposal changes neither that wording nor the usable switch; the existing Linux note continues to say no text is read.

All fixtures use reserved domains or loopback responses and disposable data folders; no account or owner profile is involved. Fixture scripts are authored parts of the served test documents, never scripts added by Syrinx.

| Check | Required evidence |
| --- | --- |
| Exclusions, redirect chains, chained script/link exits, POST redirects and tainted popups | Instrumented body-call counts remain zero for every excluded or tainted document. Use the P1-25 F2–F8 and F20–F28 scenarios adapted to WebKitGTK. |
| Repeated no-store headers, cache and service-worker responses, reload and Back/Forward | Match each response to its document and fail closed when evidence is missing. Record request counts to detect any refetch. |
| Static versus script-built text, stylesheet hiding and dynamically created sensitive fields | Show exact synthetic expected text and refusals, including the cases this method cannot detect. Do not change an expected refusal into a pass to hide a limit. |
| Parent document with same-origin, cross-origin and excluded frames | Read the main resource only; never request a child resource's body. |
| Navigation, close, process failure, policy/exclusion change during an asynchronous read | Discard stale results and prove nothing enters the index. |
| Non-UTF-8, malformed, very large and slow responses | Report rejection, end-to-end latency and peak process-tree memory; resolve the native API's missing early size bound before treating the parser limit as a resource guarantee. |
| Switch-off, exclusion, seven-day expiry, restart and recovery | Prove deletion and non-restoration through the production cache, not just absence from search results. |
| Runtime observability and performance | Compare otherwise identical synthetic pages with reading off and on; record body bytes, call/parse time, CPU and memory. Any observable page change returns to the owner. |

The report must separate observed passes, failures, known information losses and untested cases. Passing pure parser tests alone cannot approve a new extraction method.

## Owner question

Do you want a synthetic feasibility investigation of this response-HTML fallback, knowing it cannot reproduce rendered-only text or detect password fields added by scripts? The recommendation is to keep capture disabled unless you explicitly accept those method limitations and the later evidence resolves the remaining resource and navigation questions. No approval is inferred from silence, and this PR leaves the flag off.
