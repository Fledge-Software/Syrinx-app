# Linux native acceptance checks

These checks make Linux page-host and palette behavior reviewable alongside
Windows. They use authored loopback documents, fresh profiles and the existing
GTK thread; no page bridge, injected script, permission grant or engine
sandbox exception is added. Issue #221 owns B4 and its remaining validation.

PR #226 was squash-merged into `main` on 2026-11-04 and `linux-port` was
deleted. Merge `5df4978` preserves current `main` plus B4's changes, adapting
only the popup-owner function to the landed X11 connection/error API. The
B4 PR will target `main`.

## Product defect found by the history fixture

The coordinator ran the first draft outside the sandbox: `page_close` and
`cookie_persistence` passed, while `navigation_history_spike` refused an
available three-row jump. The result is recorded in
[comment 6585434681](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6585434681).
This is a Linux product defect, rather than permission to relax the fixture.

`browser_gtk/page.rs::LoadWatch::started` used WebKit's `is_user_gesture()`
for every new document. Syrinx's own `Page::navigate` calls `load_uri`, which
is not a web-page gesture. The shared `NavigationList` from #126/#212/#213
therefore received page-script moves and hid each preceding row. The shared
model is unchanged from main and correctly applies its documented rule.

`browser_gtk/history_intent.rs` now remembers the address Syrinx requested
and consumes that intent when the matching navigation starts. A later script
move does not inherit it. Reload and Back/Forward withdraw any unconsumed
request. A cancelled older load must not clear a newer host request that has
not started yet. A server redirect preserves the navigation already in
flight. This classification is for history; it does not clear D35 taint or
change page-text capture, which remains off on Linux.

The first display-free regression feeds five host-requested, gesture-free
loads through the real shared list. It failed with zero Back rows before the
intent correction and now checks four literal addresses plus a three-row
jump. That corrected one adapter defect, but it did not establish a green
native history run.

On 2026-11-03 the coordinator's `b26b8c4` run passed page-close and cookie
checks, then timed out on history in three runs. Its old error did not name
the step, so it cannot establish which address was lost. The fixture now
names every wait and includes expected/current URLs, Back/Forward rows and
the last 64 History, URL, loading and completion events at a deadline.
Unexpected external URLs and all titles are excluded from that diagnostic.
No row-order assertion or wait condition was relaxed.

A second display-free regression demonstrates another adapter defect:
`address_changed` discarded URI changes while `is_loading` was true, even
after commit, and `Finished` never delivered them. The fixture's authored
HTML calls replaceState/pushState during parsing, exactly where this loss
path applies. `history_delivery.rs` defers such changes and reports the
latest URI once after a successful finish; a new start/commit or failure
clears the pending delivery. The shared list remains unchanged. The
regression failed with the pre-replacement address and now verifies the
replacement address without an extra Back row. Native validation must still
confirm whether this accounts for the coordinator's timeout; the next logs
will identify any remaining failing step.

WebKit's [load-changed contract](https://webkitgtk.org/reference/webkit2gtk/stable/signal.WebView.load-changed.html)
provides a committed URI, which can subsequently change within that document.
This fix leaves provisional URI handling in `page_text.rs` unchanged.

## Row ordering and the next diagnostic checkpoint

The coordinator's `7f0899b` run completed the initial loads, both jumps and
replaceState: the URI-delivery timeout is fixed. It then failed the Back-row
assertion after replaceState. The previous generic error omitted actual and
expected rows, so it cannot identify whether an entry was hidden, renamed
or duplicated. No additional adapter change is justified by that log alone.

Every explicit history assertion now prints its step, actual and expected
values: address waits, jump acceptance, row comparisons and reload completion.
A failed row/jump assertion includes the same current address, both row lists
and bounded event trace as a timeout. Navigation commands are recorded beside
History/URL/loading/completion callbacks. Only synthetic loopback addresses
and about:blank can be printed; titles and unexpected external URLs remain
excluded. Comparison uses the original values, so redaction cannot turn two
different addresses into a passing assertion.

The expected sequence remains unchanged. After loading 40–44, Back three
lands on 41 with Forward rows 42, 43, 44. Forward two lands on 43. Navigating
to 45 discards Forward's 44, then replaceState renames only the current entry
to 45r. **45r is current; Back remains 43, 42, 41, 40.** After one Back, 45r
becomes the Forward row. A display-free replay through the real shared
NavigationList verifies that complete sequence and its literal expected rows.

The [HTML replaceState contract](https://html.spec.whatwg.org/multipage/nav-history-apis.html#dom-history-replacestate-dev)
replaces the current entry rather than adding one. The inspected
[WebKitGTK 2.54.1 API source](https://github.com/WebKit/WebKit/blob/webkitgtk-2.54.1/Source/WebKit/UIProcess/API/glib/WebKitBackForwardList.cpp)
defines current as offset 0, previous as -1 and next as +1. The adapter uses
that signed `nth_item` lookup. The fixture compares the shared list's
nearest-first rows; it never consumes WebKit's bulk GList getters. Those
getters should not be assumed to have the same order in both directions:
the [underlying arrays](https://github.com/WebKit/WebKit/blob/webkitgtk-2.54.1/Source/WebKit/UIProcess/WebBackForwardList.cpp)
are converted with g_list_prepend, making the returned Back bulk list
nearest-first and the Forward bulk list furthest-first. Reversing the
fixture's expectations to match a bulk getter it does not use was rejected.

The coordinator's `b5af570` run supplied those values in
[comment 6640541799](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6640541799).
The literal rows after replaceState and Reload pass, and pushState reaches
its expected address. No row-order correction was needed. The next failure
is the host's fragment navigation, described below; no row assertion or wait
predicate was relaxed.

## Same-document fragment delivery

The `b5af570` trace reaches `/p/pushed`, then records the host request for
`/p/pushed#fragment`, Loading true, the fragment URL, and Loading false.
There is no History event for the fragment, so the shared list stays on
`/p/pushed`. `address_changed` deferred that URI while WebKit was loading,
but only `load-changed::Finished` could drain the deferred report.

In [WebKitGTK 2.54.1's PageLoadState](https://github.com/WebKit/WebKit/blob/webkitgtk-2.54.1/Source/WebKit/UIProcess/PageLoadState.cpp),
a pending API request makes `isLoading` true even when the document is
already finished. [WebPageProxy's same-document path](https://github.com/WebKit/WebKit/blob/webkitgtk-2.54.1/Source/WebKit/UIProcess/WebPageProxy.cpp#L9198)
updates the URL and clears that request. The GTK
[NavigationClient](https://github.com/WebKit/WebKit/blob/webkitgtk-2.54.1/Source/WebKit/UIProcess/API/glib/WebKitNavigationClient.cpp)
emits document load signals from its provisional/commit/finish callbacks,
not from that same-document path. The native trace confirms why waiting
only for a document finish loses this fragment.

Windows already handles this separately: [WebView2's navigation contract](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/navigation-events)
describes same-document fragments without NavigationStarting. In
`crates/syrinx-platform/src/browser/history.rs`, SourceChanged records
`IsNewDocument=false` and HistoryChanged publishes the report. The shared
`browser/history_state.rs` classifies it as SamePage; the shared list
replaces its current address without adding a document row. Both shared
files and the Windows adapter remain unchanged.

`browser_gtk/history_delivery.rs` now distinguishes an active document load
from a same-document API request. Loading false drains a deferred URI once
only when no document load is active. A real document still waits for its
own Finished success/failure, even if Loading false arrives first. Reporting
every loading URI immediately was rejected because a requested or failed
document must not become a committed same-page report.

The GTK loading handler now feeds that decision into the existing history
reporter and retires only the matching settled request's intent. The same
address-matching function is used at navigation start, and a regression
ensures an older fragment cannot withdraw a newer host request. A display-free
replay failed before the correction with current `/p/pushed` instead of the
fragment address. It now verifies the fragment address, unchanged Back rows
and empty Forward rows. Other regressions cover duplicate notifications,
URI notifications on either side of Loading false, and successful/failed
document completion. The native fixture also asserts literal Back/Forward
rows after the fragment. The coordinator must rerun it and the full gate;
these regressions do not establish a native pass.

## An uncommitted request can duplicate a later Forward row

The coordinator's `e7ce7a1` run in
[comment 6641629095](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6641629095)
failed before reaching the fragment: after Back three, Forward contained
42, 44, 44 instead of 42, 43, 44. The trace already shows the corruption
before the jump: the request for 44 produces SamePage 44 while 43 is current,
then the real NewDocument 44 appends another entry. The first report renames
43 rather than adding 44, so the subsequent Forward rows expose the duplicate.

These rows come from the shared `NavigationList`, not a WebKit bulk list.
`Page::go_to_offset` uses only one signed `nth_item` lookup to choose the
jump destination: negative for Back, positive for Forward. The API forwards
that index directly to its relative-index lookup; its bulk Forward-list
ordering is not involved. Neither that lookup nor the shared model changes.

WebKit's [PageLoadState::activeURL](https://github.com/WebKit/WebKit/blob/webkitgtk-2.54.1/Source/WebKit/UIProcess/PageLoadState.cpp#L190)
returns a pending API request's URI before that document starts or commits.
If 43 is still finishing when `load_uri` exposes 44, the old delivery path
defers that URI and then publishes it as a same-page change at 43's finish.
A display-free replay through the real delivery helper and shared list
reproduced the exact 42, 44, 44 failure.

Both normal navigation and restored tabs now record the request before
calling `load_uri`. An older document's finish cannot consume that request's
URI, regardless of the older load's success. A real start/commit boundary
discards the pending URI; a same-document request still settles at Loading
false. Regressions check the literal Forward rows, Back rows before and
after the jumps, the two-row Forward landing, a URI notification before
document start, and fragment delivery after an older finish. The Windows
adapter, shared history code, native fixture assertions and timeouts stay
unchanged. The coordinator must rerun native checks on the new commit.

## What each native check proves

| Check | Linux implementation and boundary |
| --- | --- |
| `page_close` | Twelve page-requested closes leave native children alive until the host releases them, with exact child destruction, survivor preservation, stale-command rejection, unsafe creation refusal and shutdown cleanup. |
| `cookie_persistence` | An unused host stops promptly; a synthetic persistent cookie survives orderly reopen and the bounded shutdown path. The current draft does not claim Windows' post-Stopped engine-termination scenario. |
| `navigation_history_spike` | Loads, three-row Back and two-row Forward jumps, literal row order, replaceState, reload, pushState, fragments and server/script redirects. WebKit signal reports replace the raw WebView2 COM event spike. |
| `palette-acceptance` | The existing focus, popup, search and timing assertions run against X11 probes. Budgets are unchanged. Linux dispatches committed characters and keys into the GPUI popup's own window; Windows retains native Win32 messages. This does not claim physical fcitx/XIM coverage; the landing smoke test separately covers it. |

X11 has no per-window Unicode message corresponding to Win32 `WM_CHAR`.
Changing the owner's keyboard map or sending Unicode as global key events
was rejected. The Linux harness uses GPUI's existing input dispatcher, outside
the BrowserView borrow, while focus, transient parent, stacking, visibility,
refresh rate and window destruction are read from the actual X server.
The transient-parent property is now set on the owned popup rather than
being fabricated by a probe.

Windows-only boundaries remain explicit: COM event identities and HRESULTs
are WebView2 API details; Win32 end-session messages do not exist on Linux;
WebKitGTK does not offer Windows' hidden-page suspension API or download
pause/resume. Linux uses bounded host shutdown and download Cancel. The
existing Linux unit tests cover denied delegation, compound media refusal,
modal-loop deferral and download teardown. This table describes actual
coverage; it is not a claim of every ancillary Windows suite's native parity.

## Coordinator validation

The sandbox's MIT-SHM attach failure is environmental. It did not occur in
the coordinator's outside run, which retained the real runtime directory,
display, session bus and fcitx with scratch data/cache/config. Do not disable
MIT-SHM, replace the display backend or weaken the engine sandbox. The agent
runs non-display tests, formatting and Clippy; twelve native unit tests,
the examples, palette acceptance and the complete gate run outside it.

The Linux gate now invokes all three examples and palette acceptance after
the existing smoke run. A standalone fixture result is not a complete gate.
A B4 PR waits for the coordinator's green native logs, real-window inspection,
then fresh independent review. Windows CI must pass for the shared changes.

From this worktree, the coordinator runs:

```sh
cd /home/imthe/Work/Syrinx-app/target/linux-worktrees/checks
bash scripts/linux/check-native.sh
```

The script preserves the real `XDG_RUNTIME_DIR`, `DISPLAY`, session bus,
input method and scale. It creates only scratch data/cache/config and uses
this worktree's own target directory. Its exact Cargo steps are:

```sh
cargo run -p syrinx-platform --example page_close --profile local --offline --locked
cargo run -p syrinx-platform --example cookie_persistence --profile local --offline --locked
cargo run -p syrinx-platform --example navigation_history_spike --profile local --offline --locked
cargo xtask palette-acceptance
cargo xtask check
```

Leave each visible acceptance window in front without typing or clicking
into it. Return the script's commit identifier, logs and the generated
`target/p1-33/*-summary.md`/JSON reports, including the failing step if any.
Inspect palette placement and focus return in the real window; capture only
the synthetic Syrinx window if screenshots are supplied. The full gate
repeats the examples and palette run deliberately, proving that they are
part of the gate rather than only separate manual commands.
