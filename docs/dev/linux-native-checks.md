# Linux native acceptance checks

These checks make Linux page-host and palette behavior reviewable alongside
Windows. They use authored loopback documents, fresh profiles and the existing
GTK thread; no page bridge, injected script, permission grant or engine
sandbox exception is added. Issue #221 owns B4 and its remaining validation.

PR #226 was squash-merged into `main` on 2026-11-04 and `linux-port` was
deleted. Merge `5df4978` preserves current `main` plus B4's changes, adapting
only the popup-owner function to the landed X11 connection/error API. The
B4 PR will target `main`.

At `732aafd`, all three native examples pass, including the complete history
sequence. The coordinator's full gate test portion passed **1,785 tests,
zero failed or ignored**. The gate remains red because palette acceptance
fails page focus and performance checks; the evidence and next focus fix
are recorded below. This is not a full-gate pass.

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
rows after the fragment. At `732aafd` the coordinator confirmed the full
history fixture passes, including this fragment and both row assertions.

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
unchanged. At `732aafd` the coordinator confirmed the entire native history
sequence passes; palette acceptance is the remaining gate failure.

## GTK focus for the adopted foreign page window

The foreground native run at `732aafd`, recorded in
[#221, comment 6672563128](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6672563128),
passed the native fixtures and 1,785 tests, but no Esc dismissal returned
document focus in 20 samples. Baseline focus, rapid dismissal, both Enter
selections and the associated stale-refocus checks also failed. Popup
keyboard ownership, stacking and placement passed.

The native probe's “inside main window” label includes both the page's X11
container and WebKit descendants; it does not identify the WebKit widget.
That broad diagnostic is not the acceptance verdict. The page check reads
the authored loopback document's `document.hasFocus()` result in its title,
through the same path on both platforms. “Page focus none” in the summary
means neither focused nor legitimately deferred, rather than proof that
the title or focus script was missing. These checks remain unchanged.

`Embed::gtk_window` adopts a `GDK_WINDOW_FOREIGN` wrapper for its container.
In GTK 3.24.52, [the X11 focus handler](https://github.com/GNOME/gtk/blob/3.24.52/gdk/x11/gdkdevicemanager-core-x11.c#L815)
returns without delivering focus when
[`_gdk_x11_window_get_toplevel`](https://github.com/GNOME/gtk/blob/3.24.52/gdk/x11/gdkwindow-x11.c#L166)
sees a foreign window. The XI2 handler delegates to that same function.
X11 can therefore give the container the keys while GTK's toplevel focus
and WebKit's document focus remain false. A GTK widget's `grab_focus`
alone does not repair the missing toplevel event.

`page_keys.rs` now uses its existing real X11 ownership query to reconcile
GTK focus/blur for that foreign wrapper. It delivers the missing GTK window
notification only when native and toolkit focus disagree; ordinary GDK
windows keep their own native delivery. The same routing runs after a
programmatic request or click and on core/XI2 focus changes. The existing
foreground guard and key-routing rules remain in force. The GTK host is
held weakly so its signal callbacks cannot create an ownership cycle.

The display-free regression first failed when native focus entered the page
but toolkit focus stayed false. It now verifies focus/blur convergence and
no repeated notification, alongside the ordinary-window guard and existing
XI2/key-mask tests. This tests the delivery decision, not a live GTK/WebKit
focus handoff. The coordinator must rerun the unchanged native acceptance
check to validate that handoff.

## Follow-up failures at `885701d`

The coordinator's next run is recorded in
[#221, comment 6672868517](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6672868517).
The foreign GTK focus bridge restored 24 palette checks, including baseline
document focus, all 20 Escape returns, rapid dismissal and stale-refocus
checks. Three failures still required corrections:

- The find-field check sampled native focus in one GPUI update, yielded,
  then injected its page-focus event in another. The report explicitly
  says focus had left chrome at the first sample but was back on chrome
  when the event was delivered. At that point preserving the field is the
  intended product rule. The harness now waits for both native child focus
  and the authored document's focus report, then plants the field's GPUI
  ring and delivers the event in the same update. It still requires the
  field to lose its ring; it does not retry a failed delivery. A display-free
  guard regression rejects chrome, popup, unrelated and absent focus, and
  a missing or blurred document. The GTK bridge and product focus rule are
  unchanged. The native rerun must prove the actual handoff.
- The frame-ancestry unit asked Hyprland to focus a newly mapped test
  window. Focus prevention made that request fail intermittently. The test
  now builds an unmapped frame with a Syrinx-owned child, a page container
  and an unrelated sibling. It queries the production ancestry function
  directly and checks both owned and unowned branches, without changing
  native focus. Actual foreground ownership remains covered by acceptance
  and the native probes.
- History failed once in three runs after replaceState, but the corrupting
  event preceded it: the two-row Forward request emitted SamePage 43 before
  its BackOrForward 43 commit. The first report completed the pending jump;
  the second had no remaining target and reset the shared list. The next
  document therefore inherited only row 43. Back/Forward and Reload now
  share the same request-before-native-call guard as `load_uri`. The
  display-free replay reproduced exactly `[/43]` before the fix and checks
  all four Back rows afterward, with the older finish both before and
  during the traversal request. Windows and shared history logic are
  unchanged; delaying the fixture or relaxing its rows would conceal the
  defect.

The coordinator's gate stopped at the ancestry test after 1,052 passed;
that run is not a complete gate pass. The corrected native checks and full
gate still require a coordinator run outside the sandbox.

The follow-up patch passes formatting, workspace Clippy with warnings
denied and 1,777 display-free workspace tests, with zero failed or ignored.
Twelve native tests remain coordinator validation. The temporary local
filter does not alter the gate or any committed test configuration.

## Hardware findings from the foreground run

These are the coordinator's Intel HD 4600 results under Hyprland/XWayland,
not measurements from the agent sandbox. Hyprland's activation policy kept
a new Syrinx window behind other applications; the coordinator maintained
foreground only when no Syrinx-class window was active. It did not keep
refocusing the main window while Syrinx's popup was active. That preparation
is part of this run's evidence and must be recorded for the next run too.

The raw report is `target/p1-33/palette-acceptance-1793922176.json`, generated
2026-11-05 at 23:46:27 UTC. Its SHA-256 is
`361fc091c66101c1ca4663e5976971454b5dfb52a3aa8cc072f991bfae97af52`.

Across 20 openings, first-render p95 was **13.890 ms**. Presented p50 was
15.503 ms, p95 **22.991 ms** and maximum 23.685 ms. The measured refresh
interval and unchanged opening budget were **16.677 ms**, so opening
**failed**, missing the budget by 6.314 ms at p95.

The report also contains ranking misses, which must not disappear from the
owner's findings when the focus defect is fixed:

| Synthetic rows | Worker p95, ms (budget 5 ms) | Key-to-presented-rows p95, ms (budget 33.354 ms) |
| --- | --- | --- |
| 1,000 | 1.177, pass | 34.926, fail |
| 10,000 | 10.910, fail | 37.976, fail |
| 100,000 | 20.871, fail | 44.199, fail |

Each size recorded 156 keystrokes and zero timeouts. The separate burst
samples had no missing final rows. Hidden-popup idle CPU passed at 0.456%
of one core against 0.5%; that measurement covers Syrinx's own process,
not the WebKit process tree.

The budgets and failing verdicts stay intact. These observations do not
prove that hardware alone caused the misses. The coordinator must remeasure
with the focus correction; persistent misses remain hardware-run findings
for the owner, without an automatic waiver or a green gate claim.

The later `885701d` report is
`target/p1-33/palette-acceptance-1793923372.json`, SHA-256
`c62a6aa8f5b26ed61174470d7de82eed584560f0b0b6823be54e08e505f56737`.
Opening first-render p95 was **16.697 ms**; presented p50 was 19.873 ms,
p95 **34.557 ms** and maximum 51.630 ms. Presented p95 missed the unchanged
**16.677 ms** budget by 17.880 ms.

| Synthetic rows | Worker p95, ms (budget 5 ms) | Key-to-presented-rows p95, ms (budget 33.354 ms) |
| --- | --- | --- |
| 1,000 | 1.133, pass | 37.826, fail |
| 10,000 | 10.907, fail | 59.102, fail |
| 100,000 | 23.536, fail | 46.240, fail |

Each size again recorded 156 keystrokes, zero timeouts and no missing final
rows in the separate burst samples. Hidden-popup own-process idle CPU was
**0.459%** against 0.5%, a pass. These Intel HD 4600 / i5-4460 findings remain
failures where they miss their budgets; a focus correction grants no
performance waiver.

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
