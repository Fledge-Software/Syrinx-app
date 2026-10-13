# Diagnosis plan: permission and certificate fixture timeouts

This plan, dated 2026-10-13, is for a later Windows session. It ranks, from
source alone, the likely causes of the intermittent timeouts in the native
permission and certificate suites, and gives for each the smallest disposable
experiment that would confirm or reject it. **Nothing was run to write it.**
It was written in a Linux container where the native fixture cannot run, so
every claim below comes from reading code and the existing records. No product
or fixture code was changed.

## Why this matters

The native fixture (`crates/syrinx-platform/examples/page_close.rs` and its
suites in `examples/page_close/`) is the only automated check that real
WebView2 pages behave as Syrinx expects. A suite that fails one run in three
for no known reason teaches everyone to rerun until green, and then a real
regression hides among the flakes. `docs/p1-11-popups.md` ("Diagnosis of the
lifecycle timeout", Results) records 15 untraced runs: 10 passed and 5 ended
with bare deadline errors in the permission or certificate suites. "Bare"
means the shared message "Synthetic native event deadline expired; close
delivery is not proven." (`page_close.rs:446-448`), which does not say which
wait expired. Those two suites print no context of their own.

Done means one of two things: a named cause, confirmed by a reproduction that
fails on demand and passes once the cause is removed (as `--stall-first-close`
did for the lifecycle suite); or a recorded series of runs on current `main`
showing the timeouts no longer occur, with the experiments below kept for if
they return.

## Facts from the source that shape every candidate

- **Each suite has 45 seconds** from the start of `run_browser`, including
  WebView2 start-up (`page_close.rs:63`, `:205`). The outer watchdog is 300 s
  (`:77`). No deadline below should change.
- **Two pages in these suites are never shown.** In the permission suite,
  tabs 2000-2002 are shown as soon as `Created` arrives
  (`permissions.rs:72-74`), but tab 2003 in `cancel_pending` is created and
  never shown (`permissions.rs:145-151`). In the certificate suite, tab 4000
  is never shown and its parent window is never made visible
  (`security.rs:116-158`, parent never shown at `:116-133`; compare the
  permission suite's `ShowWindow` at `permissions.rs:33-38`). Hidden pages are what the product suspends.
- **A suspended page loses events.** `EventSink::prepare` (`browser.rs:533`)
  gives most events an epoch ticket; a suspension bumps the epoch, so an event
  emitted before it and read after it is dropped as stale by `into_current`
  (`browser.rs:516`). `PermissionRequested` and `Connection` are such events;
  only the list at `browser.rs:535-546` survives. Worse, a permission request
  that arrives while the controller is suspended is denied and never reported
  (`browser/permissions.rs:107-109`).
- **The timer changed after the failing runs.** The 15 runs were recorded in
  commit `92006d8` (17:27, 2026-10-12), when `TrySuspend` fired five seconds
  after a hidden page's last navigation *began*. Commit `4858d25` (17:44) moved
  the countdown to navigation *completion*, with a 60 s cap on a load that
  never finishes (`browser/hidden_timer.rs:6-8`, `rearm` at `:125-136`). No
  multi-run series of the full fixture has been recorded since.
- **Both fixture servers handle one connection at a time.** The Rust loopback
  server waits up to 1 s on an idle socket (`page_close.rs:536-556`, `:630-641`);
  the Python TLS server waits up to 3 s per socket (`certificate_server.py:72-74`)
  and exits 75 s after binding (`:66`).

## Step 0: re-baseline before testing anything

Run the unmodified fixture 20 times, untraced, on current `main`, with
`TEMP`/`TMP` set as in `docs/p1-14-security-display.md`, nothing else building,
and each run's output kept under `target/`. Record exit code, wall time and
which pass lines printed. This costs nothing and answers the first question:
did `4858d25` already remove the cause? Note that the six traced passes in the
P1-11 record are weak evidence that `--trace` hides the failure: at a one-in-
three failure rate, six passes in a row happen about 9% of the time by chance.

Then keep each failed run's synthetic directories (`%TEMP%\syrinx-page-close-
harness-*`). The certificate suite's directory holds the Python server's
`server.log`, which records a line for each connection that ended in an
exception (`certificate_server.py:83-86`) and, if the server crashed, its
traceback; it has never been inspected for a failed run.

## Shared instrumentation: say which wait expired

Before any reproduction, make one disposable copy of the example (never
committed) that records the last 64 events with a millisecond timestamp in an
in-memory ring, and prints the ring only when a suite fails. Unlike `--trace`,
it writes nothing to stderr during the run, so it barely changes timing. Also
wrap each wait in the two suites with a label, as `create_close_fixture`
already does (`page_close.rs:281-286`): for permissions, the tab id, the
`requested` flag and the last title seen; for certificates, the attempt
number and the last `Connection` state. Add each suite's elapsed time to its
pass line. No assertion or deadline changes. Every candidate below is read
off this output.

## Ranked candidates

### 1. Old-timer suspension of the two never-shown pages (high)

**Mechanism.** This is the lifecycle cause again, on the two pages the
lifecycle fix did not touch. Under the old timer, tab 2003 had five seconds
from navigation start to load, run its `load` handler and have WebView2 raise
`PermissionRequested`. If the machine was slow (the earlier lifecycle failures
overlapped an optimized build), `TrySuspend` won: the request was then denied
silently (`browser/permissions.rs:107-109`) or its event dropped as stale,
and the harness waited at `permissions.rs:152-159` until the deadline. Tab 4000 is exposed the
same way whenever one navigation (for example the HTTPS attempt queued behind
the Python server's 3 s socket wait) took over five seconds.

**Evidence.** The failures predate `4858d25`; only these two suites still had
hidden pages; the failure shape (bare deadline, missing pass line) matches the
lifecycle failure; and the dropped-request path is explicit in source.

**Confirm or reject.** Add a disposable `--stall-hidden-pages` option, like
`--stall-first-close`: hold the fourth `/permissions` response (tab 2003's)
for 6 s with `send_stalled` (`page_close.rs:615-628`), and in a disposable copy
of `certificate_server.py` sleep 6 s after `accept` before `wrap_socket`, once.
Run it 3 times on `92006d8` and 3 times on current `main`. Expected if
confirmed: the old timer fails both suites at the labelled waits with a
`Suspended { suspended: true }` in the ring; the new timer passes. If Step 0
then shows no timeouts in 20 runs, record this as the diagnosis.

### 2. An HTTPS attempt fails without a certificate event (medium-high)

**Mechanism.** `wait_rejection` (`security.rs:190-229`) needs both a failed
completion *and* a `CertificateRejected` connection state. If an attempt fails
for another reason, such as a refused or reset connection, the state becomes
`Failed`, no certificate event comes, and the loop ignores
`Failed` and waits the full 45 s. The harness turns a fast, clear failure into
a bare timeout. A server busy on an earlier socket only delays a new
connection, which waits in the listen backlog; a refusal or reset needs the
server to have crashed or passed its 75 s lifetime.

**Evidence.** The serial server with a 3 s per-socket wait
(`certificate_server.py:67-86`), its `except` list that lets any other
exception kill the server (`:83`), and the lifetime that starts before WebView2
start-up (`:66`; the server starts before `run_browser` at
`security.rs:20-27`). A slow WebView2 start under load eats into the 75 s.

**Confirm or reject.** First read the retained `server.log` of each failed
run: a traceback, or the server's process gone before the second attempt,
confirms it. Then, in a disposable copy of the Python server, close the
listener after the first connection. The expected result is a timeout at
"attempt 1" with last state `Https/Failed`. That shows the mechanism exists;
the labelled ring from real failures shows whether it happened.

### 3. A late certificate event poisons the recovery navigation (medium)

**Mechanism.** The certificate handler sets `CertificateRejected` without
checking which navigation it belongs to (`browser/security.rs:181`), and the
event also covers subresources and extra sockets. If a second event for the
failed attempt arrives after the harness has started the HTTP recovery
navigation, the recovery is marked rejected; `completed` then refuses to
record `Loaded` (`browser/security.rs:75-89`), and `wait_recovery`
(`security.rs:160-188`) waits for an `Http/Loaded` state that never comes.

**Evidence.** Source only; the sticky rule exists on purpose (trap
"Certificate failure is an expected navigation failure"). Microsoft documents
that the event can follow `NavigationCompleted`, so a late second event is
plausible, but unobserved.

**Confirm or reject.** The ring will show `Connection { Http,
CertificateRejected }` at the recovery wait. A disposable unit test feeding
`starting(2, "http://…")`, then the certificate handler's state change, then
`completed(2, true)` confirms the consequence in the state machine; whether
WebView2 ever sends the late event is answered only by the ring. If confirmed,
the product question (should a rejection be bound to its navigation id?) goes
to the owner, since it touches P1-14's truthful labels.

### 4. The notification never reports `onshow` (medium, permissions only)

**Mechanism.** For the allowed tabs 2001 and 2002, the harness waits for the
page title `notification-shown` (`permissions.rs:65-69`, `:113-127`), which
depends on Windows displaying a toast. If Do Not Disturb, an automatic Focus
rule or toast throttling (two toasts seconds apart) suppresses it without an
error, the title stops at `permission-granted` and the harness waits out the
deadline.

**Evidence.** The wait depends on Windows, outside Syrinx; no step checks the
intermediate `permission-granted` title, so the failure would be silent.

**Confirm or reject.** The labelled wait shows the last title. If it is
`permission-granted`, check Windows notification history for the run. A
deliberate reproduction (turning on Do Not Disturb for one run) changes a
Windows setting, so only the owner does it, with an explicit yes.

### 5. The harness thread stops pumping its window during store work (low-medium)

**Mechanism.** The parent window belongs to the harness thread
(`page_close.rs:184-200`); WebView2 runs on the `syrinx-pages` thread
(`browser.rs:431-433`). The harness pumps messages only inside `next_event`
(`page_close.rs:450-462`), not while it opens or shuts down the SQLite store
four times (`permissions.rs:48-54`, `:132-135`). Cross-thread window calls
from the page thread can block until it pumps, and opening the store
revalidates the storage lease, which the trap "A storage lease's full
revalidation is not a cheap row check" says is slow. Delay, not deadlock, but
it spends the shared 45 s.

**Confirm or reject.** The per-step times in the ring show it directly: if
passing runs already spend most of 45 s, this is a budget problem. A
disposable 5 s sleep before each `open_store` call should then turn some
passes into timeouts.

### 6. A navigation racing a pending `TrySuspend` (low, current timer)

**Mechanism.** `Page::load` resumes before navigating (`browser/page.rs:218-
229`); `Lifecycle::resume` invalidates a pending suspension and calls `Resume`
(`browser/lifecycle.rs:231-255`). What WebView2 does when `Resume` arrives
while `TrySuspend` (`:312`) is still in flight is unverified; if the
suspension completes afterwards, the page is frozen while Syrinx believes it
is awake. The harness hits this only if it idles about 5 s after a completion,
so it needs a slow harness to matter.

**Confirm or reject.** In a disposable copy, sleep 5.0 s (then 4.9 and 5.1 s)
before the first certificate `Navigate` at `security.rs:137-143`. A timeout with
`Suspended` events around the navigation confirms the race.

## Candidates the source rules out

- **Certificate event before or after `NavigationCompleted`.** The harness
  waits for both flags in either order, and the product keeps a rejection
  sticky through completion; the order alone cannot cause a timeout.
- **The shared Rust server dying.** Any non-idle error ends `serve`
  (`page_close.rs:547`), but a closed listener refuses connections, which
  `checked_event` reports at once as "navigation failed", not a bare deadline;
  the trailing `server.stop()` error would also print.
- **Blocking mode of accepted sockets.** Fixed and tested
  (`page_close.rs:630-641`, `:662-695`).
- **Reused permission request ids.** Ids are global across controllers
  (`browser/permissions.rs:191-200`), and a repeated request fails the suite
  with its own message (`permissions.rs:81-87`), not a timeout.
- **Foreground denial.** Neither suite moves focus or needs the foreground.

## Order of work and rules

Run Step 0, add the shared instrumentation, then test candidates 1 and 2,
which explain most of the evidence, before the rest. Serialize native runs
with the gate (trap "Do not build an example while its executable is
running"). Keep reproductions in disposable copies or behind a new
diagnostic flag, and never change an assertion or deadline to get a pass.
Use only synthetic pages and fresh profiles. Record results here and in the
P1-11 row of `docs/implementation-status.md`, and add a trap to
`docs/trapstoknow.md` once a cause is confirmed.
