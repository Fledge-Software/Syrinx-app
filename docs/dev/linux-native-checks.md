# Linux native acceptance checks

These checks make Linux page-host and palette behavior reviewable alongside
Windows. They use authored loopback documents, fresh profiles and the existing
GTK thread; no page bridge, injected script, permission grant or engine
sandbox exception is added. Issue #221 owns B4 and its remaining validation.

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

The shared-list replay passes. The native pre-replacement row checks also
passed, confirming the fixture's jump/order convention through that point.
The next native run must supply the missing actual values before choosing
between an adapter event/classification defect and a fixture timing defect.
No row assertion, wait predicate or engine behavior is weakened here.

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
runs non-display tests, formatting and Clippy; five named native unit tests,
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
