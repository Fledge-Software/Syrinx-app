# Linux native acceptance checks

These checks make Linux page-host and palette behavior reviewable alongside
Windows. They use authored loopback documents, fresh profiles and the existing
GTK thread; no page bridge, injected script, permission grant or engine
sandbox exception is added. Issue #221 owns B4 and its remaining validation.

PR #226 was squash-merged into `main` on 2026-11-04 and `linux-port` was
deleted. Merge `5df4978` preserves current `main` plus B4's changes, adapting
only the popup-owner function to the landed X11 connection/error API. The
B4 PR will target `main`.

The latest native receipt is **`8c70acb`**, recorded in
[#221, comment 6706258774](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6706258774).
`page_close` passed completely: X11 probes, lifecycle, popups, controls,
permissions, fullscreen, muted audio, repeated TLS rejection/recovery with
no application bytes, and all five download cases including exact bytes,
close deferral and Cancel cleanup. `cookie_persistence` also passed,
including fresh-process HTTP cookies. History failed **3/3** at the authored
`/h/fragment#section` report: `SamePage { navigating: true }` instead of
`SamePage { navigating: false }`. Palette and the complete gate did not run.

The earlier **`a01b28d`** receipt in
[comment 6683986953](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6683986953)
passed X11 probes, lifecycle (including hidden-survivor geometry), popups,
controls and permissions. It reached `GStreamer element autoaudiosink not
found` and WebKitWebProcess aborted before the plugins were installed.

The separate Find checks **passed** at `a01b28d`. Swapping only native Next
and Previous in a disposable worktree with its own target failed with
expected occurrence 2, actual occurrence 3; the real branch was unchanged.
The visible Find field showed `1 of 3`, then `2 of 3` after F3, then cleared
its label and highlights. Screenshots remain in `/tmp/sx-findlabel/shots/`.
Runtime package guidance is in [installing.md](../installing.md#linux-2026-10-31);
an actionable in-app prerequisite message is proposed in #246.

The last broad native baseline is the coordinator's **`dec469e`** run in
[#221, comment 6673543416](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6673543416),
not the later review-progress comments. Source commit
`dec469ea203d6883f0f6cb1d8cff140ff597d533` passed `page_close` **3/3 with the
focus helper off**, cookies, history **5/5**, **1,789 workspace tests** and
all **25 palette behaviour checks**. Palette acceptance and the complete
gate exited **1 only for timing misses** on the i5-4460 / Intel HD 4600.
That is functional evidence for that commit, not a green gate.

The subsequent review corrections add Reload completion correlation, Find
numbering/empty-result/native-completion checks, permission cancellation,
and the expanded native suites listed below. **Coverage beyond the explicit
partial receipts above remains native-pending.** Display-free tests establish
parsers and observers, not a WebKit pass. A
[fresh independent whole-branch source review approved `main..812b19f`](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6675422981).
The subsequent geometry correction has independent scoped source approval
and passed native lifecycle validation at `a01b28d`; the B4 PR against `main`
and Windows CI remain pending. No native pass is claimed beyond the named
receipts. A complete native rerun remains necessary after the history correction
below.

## Fragment policy and actual document starts after `8c70acb`

The expectation is unchanged. The [recorded Windows spike](https://github.com/Fledge-Software/Syrinx-app/issues/126#issuecomment-6468977926)
reports a fragment as SourceChanged with IsNewDocument false, then
HistoryChanged; it has no NavigationStarting. The
[WebView2 contract](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/navigation-events)
also describes that distinction. `browser/history.rs` feeds those facts
into shared `history_state.rs::Pending`, which produces
`SamePage { navigating: false }` after the previous navigation completed.
`syrinx-core/src/tabs/navigation_list.rs` reserves true for a load or step
to another document still in flight: a same-page change then cannot be
that jump's landing. Accepting true here would hide a real difference in
jump completion, not merely rename an engine-specific event.

WebKitGTK 2.54.1's
[`FrameLoader::loadURL`](https://github.com/WebKit/WebKit/blob/webkitgtk-2.54.1/Source/WebCore/loader/FrameLoader.cpp#L1737)
checks navigation policy for a fragment and continues through
`loadInSameDocument`, without starting a provisional document load. The
Linux adapter previously allocated a navigation ID and set shared
`Pending::started` directly from that policy callback. An immediate settled
URI report then saw a nonexistent document navigation. Deferred URI flags
were not the root cause; their delivery rules remain unchanged.

`page.rs` now stages the policy classification and promotes it to pending
history, with a new internal navigation ID, only at the actual document
`LoadEvent::Started`. A fragment policy alone leaves settled history clear
and cannot overwrite a real Back already in flight. Real document loads
retain their user/script, Back/Forward and Reload classifications. Windows,
shared rules, all history assertions and timing budgets are unchanged.

Two display-free regressions failed against the old operations: the exact
true/false mismatch, and loss of a real Back landing's kind after a later
policy decision. Both now pass; a third checks policy classification on
actual document starts. They exercise the Rust operations called by native
signals and shared report classification, not a live WebKit event trace.
The full native history sequence and later gate stages remain pending.

## TLS failure URI after `b8db1a5`

The fixture checks `BrowserEvent::Connection`, not a page address string.
`watch_loading` discarded `load-failed-with-tls-errors`'s `failing_uri`
argument and classified `WebView::uri()` instead. WebKitGTK 2.54.1's
[`didFailProvisionalLoadForFrameShared`](https://github.com/WebKit/WebKit/blob/webkitgtk-2.54.1/Source/WebKit/UIProcess/WebPageProxy.cpp#L8448)
commits the failed provisional state before calling the navigation client.
[`PageLoadState`](https://github.com/WebKit/WebKit/blob/webkitgtk-2.54.1/Source/WebKit/UIProcess/PageLoadState.cpp#L300)
then exposes the previous committed URI, while the TLS signal receives the
failing request's URI separately. This is an adapter field-selection bug,
not evidence that WebKit stripped HTTPS from its failure argument. The
receipt identifies neither the offending Connection status nor a raw URI,
so the exact callback values in that run remain unobserved.

`page.rs` now classifies the signal's failing URI. The same helper serves
ordinary active-URI connection reports; rejection still sets the failed
and rejected flags before emitting its event, and the signal remains
handled without any certificate exception. Windows already retains the
navigation target's scheme in `browser/security.rs`; it is unchanged.
Neither the native security assertion nor TLS policy changed.

The existing native failure is the regression's failing evidence. Two
additional display-free tests exercise the actual Rust rejection event and
failure flags after an HTTP connection, and ensure an absent or non-HTTPS
failure URI is never relabelled HTTPS. They cannot reproduce WebKit's
native rollback or verify signal delivery without a display. The complete
security suite still requires two failed completions, two certificate
rejections, safe HTTP recovery, no late success or untrusted content, and
no server-observed application bytes. Fresh independent scoped source
review approved the correction; the coordinator's `8c70acb` receipt passed
the complete security suite without changing its assertions.

## Fixture activation after `cb44257`

`permissions.rs::request_consent` creates and loads its authored document,
calls `Fixture::focus_page`, then delivers Enter. Only that key handler
calls `Notification.requestPermission()`. The failing guard precedes the
key, so these receipts do not show a changed media or capture permission
flow. Previously, `FixtureWindow::show` mapped an unmanaged
`override_redirect` window and called `XSetInputFocus`; `focus_page` waited
only for a command barrier. Neither operation established that Hyprland
had activated the fixture. Production page focus intentionally refuses to
take keys while another application is in front.

Visible fixture parents now use managed X11 toplevels with an input hint.
`acceptance/window_focus.rs` waits for the WM's `_NET_CLIENT_LIST`, sends
the [EWMH `_NET_ACTIVE_WINDOW` request](https://specifications.freedesktop.org/wm/latest-single/),
and requires both the WM's active-window acknowledgement and X11 keyboard
focus inside that toplevel. After the ordinary host Focus command,
`focus_page` waits for actual focus inside its sole page child. Keys still
run the unchanged `require_native_focus` check immediately before delivery.
There is no permission grant, product focus-policy bypass or focus retry
that hides a failed input operation.

Polling runs on the fixture caller, with each native operation dispatched
to `syrinx-gtk`, so GTK can process focus events between observations.
Each polling deadline is 15 seconds and native query failures propagate;
the fixture's outer watchdog still covers a blocked native call.
The visible X11 probes use the same managed setup; their negative case
activates an owned synthetic window without Syrinx's PID marker and checks
that product focus requests leave its keys alone. Hidden unit-test parents
remain unmapped and require no window-manager cooperation. Native fixtures
now require an EWMH window manager, such as this desktop's Hyprland or the
Openbox session in the proposed #222 CI job.

The display-free regression rejects transient native focus without WM
acknowledgement, checks the actual activation message's target and direct
test-action source, and preserves native query errors. It cannot establish
desktop activation: the coordinator must rerun the unchanged native
assertions, with the fixture focus helper off. No Hyprland settings or
timing budgets were changed. A fresh independent scoped source review
approved this correction. The coordinator's `b8db1a5` receipt subsequently
passed the native probes and permission/media focus paths; the full gate
still awaits the security correction and later fixture stages.

## Hidden-survivor geometry after `812b19f`

The failure's expected rectangle `(19, 29, 480, 300)` is the survivor's
previous rectangle. Actual `(7, 11, 320, 240)` is the new card rectangle
requested for the active, closing page. The hidden survivor moved when it
should have stayed still; the failure does not show X11 deferring its resize.

GTK's `Engine::apply(Resize)` visited every page, and `Page::resize` always
called `Embed::place` (`XMoveResizeWindow`) and GTK allocation. It had no
visibility check. Windows deliberately skips those hidden-page changes:
`browser/page.rs` uses `browser/placement.rs::Placement::on_resize`, then
`on_show` catches up to the current card before the page becomes visible.
The Windows native lifecycle fixture explicitly asserts both halves.

Linux now reuses that unchanged shared policy. `page_placement.rs` decides
when to invoke the native bounds and mapping operations; the engine passes
the latest card to `Page::show`. Creation still initializes the X11 and GTK
geometry, hidden resizes preserve it, and activation applies current bounds
before mapping. Repeated unchanged rectangles also skip redundant geometry
calls. Windows code and both native fixture expectations are unchanged.

Two display-free tests at that operation boundary failed against the
extracted eager-resize path, including a bounds call with the actual native
failure's `(7, 11, 320, 240)` tuple. Both pass with the shared policy and check
the bounds-before-map ordering on activation. These tests prove the Rust
decision and call order, not native X11 completion. The coordinator's
`a01b28d` run subsequently passed the unmodified native lifecycle assertion.

The following diagnostic sections retain the historical state at their named
commits (2026-11-02 through 2026-11-05). Their then-pending requests and old
failures do not supersede the receipt above or validate the later changes.

## Historical history diagnostics, 2026-11-02 through 2026-11-05

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

## Historical GTK focus diagnostics at `732aafd`, 2026-11-05

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

## Historical follow-up failures at `885701d`, 2026-11-05

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

## Historical X11 unit-suite audit after `fe5ea09`, 2026-11-05

The coordinator's next run,
[#221, comment 6673181928](https://github.com/Fledge-Software/Syrinx-app/issues/221#issuecomment-6673181928),
passed history **five of five times** and all **25 palette behaviour checks**.
The find ring and native document-focus handoffs now pass. The gate stopped
after 1,054 tests at another managed-window setup:
`focus_chrome_ignores_a_minimised_window_and_reports_a_gone_one` tried to
give a freshly mapped window the keys. These results establish functional
progress, not a complete gate pass.

The audit covered Xlib creation, mapping and focus calls, GTK show/focus
calls and their unit-suite callers across the workspace:

| Unit suite or caller | Deterministic setup and retained coverage |
| --- | --- |
| `browser_gtk/x11.rs`, five native units | Every root child stays unmapped. Child mapping occurs only below an unmapped ancestor, without a window-manager request. Viewability, stale IDs and ancestry use real X11 queries. Private focus operations accept the foreground observation; public wrappers still read it from X11. Hidden/stale targets and both foreground guard branches are tested without moving keyboard focus. |
| The same suite's cleanup and error handling | Remove the global-focus reset and `FocusGuard`. Run the native queries on GTK's thread so intentional stale-window errors cannot overlap GDK's process-wide temporary error traps; assert results on the test thread. The hidden-target unit also covers a mapped child under a hidden ancestor. |
| `linux/acceptance_tests.rs` | Keep real ownership, hidden visibility and destruction probes on unmapped fixture windows. Move global keyboard-focus, visible stacking and monitor-refresh checks to the explicit native fixture. |
| `browser_gtk/permission_tests.rs` | The parent stays unmapped. WebKit children are embedded below that hidden parent; delegation assertions require no keyboard focus or window-manager activation. No change needed. |
| `browser_gtk/downloads_shutdown_tests.rs` | Ephemeral WebKit contexts/views are never placed in a mapped toplevel. Download assertions depend on engine signals, not focus. No change needed. |
| `browser_gtk/page_tests.rs`, modal callback test | Exercises deferred callbacks and host borrows; it opens no real dialog or managed window. No change needed. |
| `active_frame/tests.rs` | Windows-only: Linux selects `linux/active_frame.rs`, which has no window-activation tests. Windows coverage stays unchanged. |

`acceptance::check_native_probes` now runs the desktop checks explicitly at
the start of the existing `page_close` fixture. Its unmanaged
`override_redirect` windows map directly through X11, without asking the
window manager. The fixture retains every former native probe assertion
and additionally checks both public focus paths, hidden and destroyed
targets, and refusal to take keys from an unowned foreground window.
The gate already runs `page_close`; no check is ignored, no gate stage is
removed and no workflow changes. The success marker is `Linux X11 probes:`.
Palette acceptance continues to prove focus inside actual WebKit pages.

These changes require a fresh coordinator native run outside the sandbox,
including the whole unit suite, the enhanced `page_close`, cookies, history,
smoke and palette acceptance. Review and the B4 PR against `main` follow
once only the recorded timing misses remain.

Local verification passes formatting, workspace Clippy with warnings
denied and all 1,777 display-free workspace tests, with zero failed or
ignored. The twelve display-dependent unit tests and the enhanced native
fixture remain coordinator validation; the local filter is not committed
and the gate remains unfiltered.

## Current recorded timing findings at `dec469e`

The receipt's raw report is
`target/p1-33/palette-acceptance-1793925880.json`, SHA-256
`f42d878af09ea181b1cf0a9a01551439cf1a5f95f2fc618557aa10081dc20840`.
It records the coordinator's ordinary desktop run, with the real runtime,
under Hyprland/XWayland on an i5-4460 / Intel HD 4600. Opening presented p95
is **27.925 ms**, failing the unchanged **16.677 ms** budget.

| Synthetic rows | D48 worker p95, ms (5 ms budget) | Key-to-presented-rows p95, ms (33.354 ms budget) |
| --- | --- | --- |
| 1,000 | 1.988, pass | 35.102, fail |
| 10,000 | 10.842, fail | 37.953, fail |
| 100,000 | 22.585, fail | 40.614, fail |

Hidden-popup idle CPU is **0.479% of one core**, within its 0.5% budget.
This measures Syrinx's own process, **not the WebKit process tree** and not
B1's CPU/memory/startup comparison. B1's real coordinator measurements remain
pending in [#218](https://github.com/Fledge-Software/Syrinx-app/issues/218)
after B4; no substitute figures are supplied here. Timing failures remain
failures. The owner decides whether the opening, D48 worker and composed
budgets apply to this hardware; neither the budgets nor their verdicts changed.

## Historical hardware findings, earlier 2026-11-05 commits

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

At `fe5ea09`, all 25 behaviour checks passed. The raw report is
`target/p1-33/palette-acceptance-1793924542.json`, SHA-256
`75a1c65086fc01beecffbdc17495d37b1d3ad5fd46a7209415d6330c69fcede9`.
Opening first-render p95 was **15.458 ms**; presented p50 was 21.549 ms,
p95 **33.534 ms** and maximum 81.304 ms. Presented p95 missed the unchanged
**16.677 ms** budget by 16.857 ms.

| Synthetic rows | Worker p95, ms (D48 budget 5 ms) | Key-to-presented-rows p95, ms (budget 33.354 ms) |
| --- | --- | --- |
| 1,000 | 1.253, pass | 33.015, pass |
| 10,000 | 15.707, fail | 38.349, fail |
| 100,000 | 24.211, fail | 46.359, fail |

Each size recorded 156 keystrokes and zero timeouts. The burst samples had
no missing final rows. Hidden-popup own-process idle CPU passed at
**0.479%** against 0.5%. Palette acceptance therefore **fails only its timing
budgets** in this run on the i5-4460 / Intel HD 4600. These observations do
not establish that hardware alone caused the misses. **The owner decides
whether the opening, D48 worker and composed budgets apply to this class
of machine.** The budgets and failed verdicts remain unchanged; this is a
recorded finding, not a waiver or a green palette-acceptance claim.

## Windows scenario applicability and implemented Linux coverage

The inventory below records intended coverage, not a pass for every case.
Only the explicit scenarios named in the receipts above have current
native evidence; `dec469e` covers the earlier, smaller suites. The inventory
comes from `examples/page_close/{lifecycle,popups,essentials,permissions,
suspension,audio,security,downloads}.rs`, `cookie_persistence/windows.rs`
and `navigation_history_spike/windows.rs` (including its authored scripts).
Paths in this table are relative to `crates/syrinx-platform/examples/linux/`.

| Windows scenario | Linux fixture and actual assertion | Boundary |
| --- | --- | --- |
| Native focus, stacking, foreground and refresh probes | `lifecycle.rs` runs actual X11 probes with managed fixture parents and an owned unmanaged popup. | Focus helper must be off; WM activation and native focus are both required. |
| Hidden/show/no-active lifecycle and resize | Exact mapped state and physical rectangles, invalid bounds preserve prior rectangle. | Real native children; no source-text substitute. |
| Duplicate/rejected/reused tab ID | Duplicate refusal; invalid creation leaks no child; retry and reuse succeed. | Shared protocol errors, not HRESULT identity. |
| Page close deferral and survivor | Twelve close requests leave children alive until host release; closing page remains usable; survivor takes current bounds; multi-child shutdown destroys all children. | Real owned child IDs. |
| Popup adoption and focus | `popups.rs` retains opener marker and reply, checks both child visibilities and the adopted document's authored `hasFocus()` title. | No new navigation replacing the adopted document. |
| Find count, direction, wrap, clearing | `controls.rs` requires native completions, one-based labels, absent/empty `0/-1`, literal independent selected-span sequences, StopFind and growing queries retaining chrome keys. | Sequential operations only; overlapping completion/index correlation remains [#245](https://github.com/Fledge-Software/Syrinx-app/issues/245). |
| Zoom and shown/hidden mute | Native readbacks after increase/decrease/reset and mute/unmute across hide/show. | No UI-only mirror. |
| Print | Actual owned GTK dialog appears, host commands remain usable during its modal loop, Cancel closes it. | No print job is submitted. |
| Permission deny/re-prompt/cancel | `permissions.rs` denies notification requests, re-prompts in a new view, cancels pending requests on navigation/close and refuses stale denied replies. | No automatic Allow. |
| Positive permission Allow, notification delivery, remembered positive consent, physical camera/microphone | Not automated by these fixtures. | Owner-only; these are not claimed to be absent engine APIs. Device/delegation limits remain #234. |
| Video fullscreen | `media/fullscreen.rs` enters an authored synthetic video, checks exact viewport, exits through Escape, F11 shortcut, page script and chrome-focused host command, and restores exact bounds. | Supported despite originating in Windows' suspension suite. |
| Muted audio activity | `media/audio.rs` joins authored start/stop with native AudioChanged, holds hidden playback and rejects unmute. | Audio reporting is supported; native backend must be exercised. |
| Hidden-page suspension and audio grace | No fabricated IsSuspended/resume assertions. | WebKitGTK has no TrySuspend or matching grace API. |
| Repeated invalid TLS and safe recovery | `security.rs` requires both failed completion and CertificateRejected, rejects successful/Loaded/sentinel outcomes through barriers and shutdown, checks exact HTTP recovery and server-observed absence of application bytes. | Fresh untrusted key; no trust-store or validation bypass. |
| Same-tab, adopted-popup and self-closing-popup downloads | `downloads.rs` checks real attribution, totals and saved bytes; held popup close uses the shared DownloadList deferral until completion. | Held reports may precede Created; both reports and acknowledgement are required. |
| Cancel held transfer | Actual incomplete progress becomes Interrupted(Cancelled), with no late completion and no remaining partial/destination artifacts. | Pause/resume and engine-owned download UI have no WebKit counterpart. |
| Immediate close diagnostic and executable protection | Required ordinary download deferral is checked; Windows' diagnostic does not impose a fixed immediate-close outcome. | Linux has no SmartScreen (B3); no executable launch is performed. |
| Idle/orderly/bounded cookie shutdown | `cookies.rs` retains the previous within-process checks and host shutdown bounds. | No Win32 WM_ENDSESSION or engine-process-enumeration claim. |
| Cookie disk persistence across application processes | Parent creates one private synthetic profile; a writer exits successfully before a fresh reader starts with that profile. Actual HTTP Cookie header at the parent server is required after the reader exits successfully. | HttpOnly persistent cookie; a page title or same-process cookie cache cannot pass this case. Owned children have bounded cleanup. |
| History host loads, 3/2 jumps, replaceState, Reload, pushState, fragments and redirects | `history.rs` preserves literal projected rows and redacted traces; Reload requires its own Loading before successful noncancelled completion after a FIFO drain. | Windows COM recorder identities are unavailable; shared semantics remain required. |
| Authored anchor, location.assign/replace, script Back, beforeunload without gesture | `history/scenarios.rs` requires exact addresses/report kinds and projected rows on shown parents. Authored timers receive no gesture; no host injection occurs. | Beforeunload must not trap a gesture-free navigation in a dialog. |
| Cacheable Back three/Forward one | Four `/c/` documents carry explicit cache headers; actual traversal and literal projected rows are checked. | Cache eligibility is tested; neither HTTP nor back-forward cache hits are promised. |
| Traversal after pushState/replaceState and same-document fragment Back/Forward | Follow the renamed projected row, then check replacement traversal or reset when WebKit lands on an untracked original same-document entry; assert empty reset rows and subsequent engine Forward. | The shared model intentionally collapses same-document states; it is not expanded to match WebKit's entries. |
| Palette behavior and timing | Existing acceptance checks run with real X11 focus, visibility, transient parent, stacking and refresh measurements. | All budgets unchanged; the last broad baseline exits 1 for timing. Current-head receipt is incomplete. |
| Physical input, Unicode and IME | Palette drives GPUI's existing input dispatcher; native authored controls use owned simple GDK keys. | No global keyboard map change; these do not prove physical fcitx/XIM/IME delivery, which remains the landing smoke/manual boundary. |

The startup path prepares private data/cache/config and an explicit Downloads
folder before GLib or any threads initialize. Each actual download checks
GLib's destination against that prepared path and fails closed on mismatch.
Native runs retain the real `XDG_RUNTIME_DIR` for desktop/audio sockets;
sandbox display-free Cargo commands use a fresh private runtime instead.
Cookie workers enter the same startup path, then dispatch their explicit
internal mode without recursively running the suite. Their fresh XDG paths
are separate from the one explicitly shared cookie profile.

## Coordinator validation

The sandbox's MIT-SHM attach failure is environmental. It did not occur in
the coordinator's outside run, which retained the real runtime directory,
display, session bus and fcitx with scratch data/cache/config. Do not disable
MIT-SHM, replace the display backend or weaken the engine sandbox. The agent
runs non-display tests, formatting and Clippy; twelve native unit tests,
the examples, palette acceptance and the complete gate run outside it.

The Linux gate now executes the shared example unit harness once with
`cargo test -p syrinx-platform --example page_close --profile local --offline --locked`,
then invokes all three native examples and palette acceptance after the
existing smoke run. All three examples compile the same unit module; Windows
test scope remains unchanged. A standalone fixture result is not a complete gate.
A B4 PR waits for the affected native rerun, real-window inspection and fresh
independent whole-branch review. Timing misses must remain visible, with an
owner decision on applicability. Windows CI must actually pass for the shared
changes; none of those pending steps is implied by display-free verification.

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

The script stops immediately on a native fixture failure. Once those pass,
it collects palette and full-gate results independently, retains both logs
(`palette-acceptance.log` and `gate.log`), and exits nonzero if either fails.
A palette timing failure must not prevent the full gate from being collected.

Keep any coordinator Hyprland focus helper **off during every native fixture
and native probe**, including those inside `cargo xtask check`. Enable it
only while palette acceptance itself runs, standalone or within the full
gate. A helper refocusing the managed main window can invalidate probes of
owned unmanaged windows. Do not run one indiscriminately around this script.
Leave each visible palette acceptance window in front without typing or
clicking into it. Return the script's commit identifier, logs and the generated
`target/p1-33/*-summary.md`/JSON reports, including the failing step if any.
Inspect palette placement and focus return in the real window; capture only
the synthetic Syrinx window if screenshots are supplied. The full gate
repeats the examples and palette run deliberately, proving that they are
part of the gate rather than only separate manual commands.


The corrected Find baseline needs a native positive run and an independent
mutation demonstration: in a disposable source copy with a separate target,
swap only `controller.search_next()` and `controller.search_previous()` in
`Page::step_find`, leaving the Rust index and fixture expectations intact.
The first three-match Next must reject occurrence 3 when it expects 2.
Retain source identity, command and failing output; never apply that mutation
to this branch. The fixture uses authored selected-span evidence rather than
its Rust-reported index as the direction oracle. Also inspect the real Find
bar on a synthetic three-match page: **1 of 3 → Next → 2 of 3 → clear**.
Those native/mutation/visible-label checks are still pending.
