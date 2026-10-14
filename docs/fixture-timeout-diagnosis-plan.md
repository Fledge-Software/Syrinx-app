# Diagnosis plan: permission and certificate fixture timeouts

**Status, 2026-10-13: diagnosed.** The recorded timeouts were, with high
confidence, the old hidden-page suspension timer freezing the two pages these
suites never show (candidate 1). A deliberate reproduction failed both suites
3 of 3 times on the old timer (`bbe59d3`) and passed 3 of 3 on current `main`
(`6a23836`), and 20 unmodified runs on `main` all passed (Step 0). The five
recorded failures themselves left no trace to compare, so the link to them
rests on matching symptoms; see "Outcome" at the end. The plan below is kept
as written, with each step's result added.

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
  commit `bbe59d3` (17:27, 2026-10-12), when `TrySuspend` fired five seconds
  after a hidden page's last navigation *began*. Commit `82c8111` (17:44) moved
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
did `82c8111` already remove the cause? Note that the six traced passes in the
P1-11 record are weak evidence that `--trace` hides the failure: at a one-in-
three failure rate, six passes in a row happen about 9% of the time by chance.

Then keep each failed run's synthetic directories (`%TEMP%\syrinx-page-close-
harness-*`). The certificate suite's directory holds the Python server's
`server.log`, which records a line for each connection that ended in an
exception (`certificate_server.py:83-86`) and, if the server crashed, its
traceback; it has never been inspected for a failed run.

**Result, 2026-10-13.** Step 0 ran on `main` at `6a23836` on the second
development machine (EZ-LAPTOP-01: Rust 1.97.1, Python 3.13.15 installed per
user), with the machine otherwise quiet and nothing else building. The
fixture was built once and the same executable run 20 times, unchanged and
untraced, with `TEMP` and `TMP` set to `target/session-temp`. All 20 runs
exited 0 and printed all 11 pass lines; wall times ran from 31.6 to 35.8 s,
with a median of about 32.2 s. At the recorded failure rate of 5 in 15, twenty
passes in a row would happen by chance about 0.03% of the time
((10/15)^20), so `82c8111` very probably removed whatever caused the earlier
failures. One caveat weakens that figure: the 15 recorded runs were made on
the original development machine on 2026-10-12, before EZ-LAPTOP-01 had a
toolchain, so the two series differ in machine as well as in commit, and a
faster or quieter machine could pass more often for that reason alone. The
reproduction under candidate 1 is therefore the stronger evidence. There were
no failed runs, so there was no `server.log` from a
failure to read. The per-run logs and a summary (`summary.txt`,
`run-NN.log`) are in the main checkout's `target/fixture-step0/`; they are
build output and are not committed.

Every run, passing or not, left seven `syrinx-page-close-harness-*`
directories behind, about 61 MB in all. That is by design rather than a leak:
`reserve_profile` (`page_close.rs:169-181`) creates one fresh directory for
each of the seven profiles the run uses (five browser suites, the permission
store and the certificate suite) and never deletes them, so a failed run's
evidence, including `server.log`, survives. The cost is that repeated runs
fill `target/session-temp`; whoever runs a series should clear it between
series. Deleting them only after a fully passing run would keep the evidence
and save the space; that is a small owner question (answered: DW-65), not a defect.

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

**As built, 2026-10-13.** The instrumentation lived only in two detached
checkouts under the diagnosis worktree's `target/diag/` (one at `6a23836`,
one at `bbe59d3`, with the same patch adjusted for the old crate names) and
was never committed. The harness's existing `trace` function now always
appends a millisecond-stamped line to a 64-line ring behind a mutex, and
still prints to stderr only under `--trace`; every command sent, every event
received (marked `event` or `stale`) and every loopback request goes in.
`run_browser` clears the ring when a suite starts and prints it only if that
suite fails. The permission waits carry the tab id, the `requested` flag and
the last title; tab 2003's two waits are named; the certificate waits name
the baseline, each attempt's rejection or recovery, and the last
`Connection` state with the flags. Each suite also prints one
`DIAG suite <name> [<seconds> s] ok|FAILED` line, and pass lines carry the
suite's time. In a plain run the suites took 3.9 s (lifecycle), 0.6 s
(popups), 2.3 s (permissions), 0.8 s (essentials), 23.4 s (suspension) and
2.0 s (certificates), so neither of the two suspect suites is anywhere near
its 45 s budget; that weakens candidate 5. One limit: the product's `Debug`
for permission, title and connection events prints `BrowserEvent { content:
redacted }`, so the ring shows that such an event arrived but not its
contents; the labelled waits supply what matters.

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

**Evidence.** The failures predate `82c8111`; only these two suites still had
hidden pages; the failure shape (bare deadline, missing pass line) matches the
lifecycle failure; and the dropped-request path is explicit in source.

**Confirm or reject.** Add a disposable `--stall-hidden-pages` option, like
`--stall-first-close`: hold the fourth `/permissions` response (tab 2003's)
for 6 s with `send_stalled` (`page_close.rs:615-628`), and in a disposable copy
of `certificate_server.py` sleep 6 s after `accept` before `wrap_socket`, once.
Run it 3 times on `bbe59d3` and 3 times on current `main`. Expected if
confirmed: the old timer fails both suites at the labelled waits with a
`Suspended { suspended: true }` in the ring; the new timer passes. If Step 0
then shows no timeouts in 20 runs, record this as the diagnosis.

**Result, 2026-10-13: confirmed.** The option was built as planned: the
loopback server counts `GET /permissions ` requests and holds the fourth
(tab 2003's) for 6 s with `send_stalled`, and the disposable Python server,
when the harness passes it an environment flag, sleeps 6 s after its first
`accept`, before `wrap_socket`. The six runs were serialized, each with its
own `TEMP`, and no `cargo` or `rustc` process was running at any start.

| Build | Runs | Exit | Permission suite | Certificate suite |
| --- | --- | --- | --- | --- |
| `bbe59d3` (old timer) | 3 | 1, 1, 1 | failed at "tab 2003 permission request wait" after 45.1 s | failed at "attempt 0 rejection" after 46.4–46.6 s, last state `Https/Loading`, `failed=true`, `rejected=false` |
| `6a23836` (current `main`) | 3 | 0, 0, 0 | passed in 8.0–8.2 s | passed in 7.7–8.0 s |

On the old timer the ring shows the same sequence every time. For tab 2003,
`Loading 2003 true` arrives, the server starts its hold, and
`Suspended 2003 true` follows 4.9–5.0 s later; the server's hold ends at
6 s (the ring has no line for the delivery itself), yet no permission request
ever reaches the harness, stale or current, and it waits out the deadline.
For tab 4000, `Suspended 4000 true` arrives 5.0 s after the HTTPS
`Navigate`, and the failed `NavigationCompleted 4000 false` 1.0 s later, when
the held handshake is abandoned; the certificate event never comes. The last
connection state stays `Https/Loading` even though the completion failed,
which fits the plan's note that a suspended page's events are dropped. The
server's log agrees: the held first connection and the three parallel ones
WebView2 had queued all ended with `ConnectionAbortedError` or `SSLError`
about 6.4 s after the server started. On `main` the same holds
are harmless, because a loading hidden page is not suspended until its load
finishes (`browser/hidden_timer.rs`); its server log shows the held
connection abandoned as before, then three more connections ending in
`SSLError`, which is the client refusing the untrusted certificate, and the
harness sees both rejections. Every other suite
passed in all six runs. The logs are under the diagnosis worktree's
`target/diag/runs/c1-old-N/` and `c1-main-N/` (`stdout.log`, `stderr.log`
with the ring, `server.log`, `summary.txt`).

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

**Result, 2026-10-13: the mechanism exists; it did not cause the recorded
failures.** There was no failed run's `server.log` to read, because Step 0
had no failures. In the mechanism experiment the disposable server closed its
listener as soon as its first connection ended. Two runs on `main` both
exited 1 with every other suite passing. The certificate suite timed out
after 46.6–46.7 s with last state `Https/Failed`, `failed=true` and
`rejected=false`: once at "attempt 1 rejection", as predicted, and once at
"attempt 0 rejection". In both runs the new timer then suspended tab 4000
five seconds after its failed completion, as designed (33354 → 38368 ms and
32634 → 37643 ms in the two rings), so with the listener closed no
certificate event could follow. In both runs the server accepted exactly one
connection, which ended in `SSLError` (the client refusing the certificate,
as in a passing run), before the listener closed. Why one run still got a
certificate event from that single connection and the other did not is not
shown by the logs. A likely reading is that WebView2 opens several sockets
for one HTTPS navigation (the stall runs above logged four to seven, and a
plain run six) and raises the certificate event only once it has finished
with them, so a refused follow-up socket can end attempt 0 without one; that
is a hypothesis, not an observation. Either way the failed completion arrived
88–94 ms after the `Navigate`, and the harness then sat for 45 s on a failure
it had already seen. Logs: `target/diag/runs/c2-main-1/` and `c2-main-2/`.

So the harness does turn a fast, plain HTTPS failure into a bare deadline,
which is a real weakness: if the Python server ever dies, the report says
only that a deadline expired. It is not the explanation for the five recorded
failures, though. Candidate 1 reproduces them on the old timer and not on
the new one, and a refusal needs the server to have crashed (nothing in
Step 0 or the stall runs suggests it does) or to have passed its 75 s
lifetime, which a suite with a 45 s deadline that starts after the server
cannot reach. The obvious fix, failing as soon as the state becomes `Failed`,
would be wrong: as the trap "Certificate failure is an expected navigation
failure" records, WebView2 can report the failed completion before the
certificate event, and the product's state machine
(`browser/security.rs:75-89`, `:181`) moves through `Failed` to
`CertificateRejected` in that order. A fix therefore needs a short, bounded
wait after `Failed`, which changes a wait in the harness; that is an owner
question (see "Outcome").

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

## Outcome (2026-10-13)

**Diagnosis.** The intermittent permission and certificate timeouts recorded
in `docs/p1-11-popups.md` (5 of 15 untraced runs at `bbe59d3`) were caused by
the hidden-page suspension timer as it was before `82c8111`, which issued
`TrySuspend` five seconds after a hidden page's navigation *began*. Tab 2003
in the permission suite and tab 4000 in the certificate suite are the only
never-shown fixture pages whose suites must wait on their events. Other pages
are never shown either: tab 1000, the popup opener (`popups.rs:63-97`), sits
in a suite that finishes in under a second, and the suspension suite hides
its slow page and tabs 1501–1507 on purpose (`suspension.rs:54`, `:98-126`).
Whenever the load of tab 2003 or 4000, or the HTTPS
handshake queued behind the serial Python server, took longer than five
seconds, the page was frozen mid-load: the permission request was never
raised, or the HTTPS attempt failed without its certificate event, and the
harness waited out its 45 s deadline with no message saying which wait
expired. This is the lifecycle suite's cause again, on the two pages its fix
did not touch. Both halves of the plan's test of "done" are met: the
reproduction fails 3 of 3 on the old timer and passes 3 of 3 on the new one,
and 20 unmodified runs on current `main` all passed. A trap records it in
`docs/trapstoknow.md`.

**What this does not prove.** The five recorded failures were untraced and
kept no ring, so nobody can show that each of them took this path. The link
is the match: the same two suites, the same bare message, a mechanism that
fails exactly those suites and no others, and a timer change that removes it.
What made those loads or handshakes take more than five seconds on the
original machine is also unknown; the reproduction forces it with a 6 s hold.
Step 0's twenty passes were on a different machine, so they confirm that
`main` is healthy here rather than measuring the old failure rate away.

**Nothing in the product or the committed fixture changed.** The diagnosis
needed no code change, because the owner's 2026-10-12 decision to start the
countdown at navigation completion is what removed the cause. The
instrumentation, the `--stall-hidden-pages` and `--close-tls-after-first`
options and the server copy were disposable and are not committed; the
patches remain as build output in the diagnosis worktree's `target/diag/`
(`diag-instrumentation.patch` and `diag-instrumentation-old.patch`) and can
be rebuilt from the descriptions above if that folder is cleared. Candidates 3 to 6 were not
tested, since there is nothing left to explain; they stay here for if a
timeout ever returns, and the labelled waits described above are the first
thing to rebuild in that case.

**Owner questions.**

1. *Should the certificate suite fail fast when an HTTPS attempt fails
   without a certificate event?* Candidate 2 showed that a refused or reset
   connection becomes a bare 45 s deadline. Recommendation: yes. After the
   connection state becomes `Failed`, wait at most 5 s more for
   `CertificateRejected` (WebView2 can report the failure first), then fail
   with a message that names the attempt, the state and the server log.
   This makes the harness stricter and faster, not looser, but it changes a
   wait, so it needs the owner's yes.
   **Owner's answer (2026-10-13/14, in chat): yes.** Wait at most 5 s after
   `Failed` for the certificate event, then fail with a message naming the
   attempt, the state and the server log; the 45 s suite deadline is
   unchanged. Recorded as DW-64 in the plan's "Deferred work" register and
   not built on this docs-only branch.
2. *Should a fully passing fixture run delete its synthetic directories?*
   Each run keeps about 61 MB in seven directories so a failure's evidence
   survives. Recommendation: delete them only after every suite passes,
   and keep them on any failure.
   **Owner's answer (2026-10-13/14, in chat): yes.** Delete the seven
   synthetic profile directories only after every suite passes, and keep
   everything on any failure. Recorded as DW-65 and not built here.
