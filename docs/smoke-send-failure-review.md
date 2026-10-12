# Preserve command-send failures through smoke shutdown

The synthetic smoke check must fail when Atlas observes a worker transport
error, even if both workers subsequently stop cleanly. This is a reporting
correction; it does not establish the cause of the unresolved native crash.

## Source finding

`crates/atlas-app/src/ui.rs` marks the smoke outcome successful after its
runtime health wait, then requests orderly shutdown. Previously, the
`page_command` and `command` methods put actual send errors only in the
transient status message. Neither changed the stored outcome.

`BrowserHost::send` in `crates/atlas-platform/src/browser.rs` enqueues a
command before calling `PostThreadMessageW` to wake the native worker. A
failed wake therefore does not prove that the command was rejected. The
worker can still consume the queued shutdown and report `Stopped`. With
clean joins and no later error event, the earlier smoke success could reach
`main.rs` unchanged and produce exit status zero. This is a source-level
failure path, not a reproduced native failure or a claim about how often
Windows rejects the wake.

## Narrow correction

Both actual send-error branches now call the existing `BrowserView::fail`
method. It stores outcome `2` and retains a failure message that ordinary
loading or save notices cannot conceal. Missing-handle branches are
unchanged. Shutdown still waits for both workers and their joins; it does
not retry a command whose enqueue may already have succeeded.

The existing event path already handles emitted `ProcessFailed`, browser
errors, worker panics and storage cleanup errors during orderly shutdown.
This correction concerns errors returned directly to the sending UI.

## Validation and limits

Independent review should verify that an actual send error changes an
already successful smoke outcome to failure, and that later clean worker
stops cannot restore success. In particular, an injected accepted enqueue
followed by a failed wake must remain a failure through both stop
notifications. Successful sends must retain their existing behavior.

There is no lightweight existing test fixture for constructing this GPUI
view without starting its native workers. No duplicate outcome model or
new native fault hook was added just to mirror these two assignments. The
change reuses the existing failure path; root will run the normal gate and
smoke check. Those checks do not force a failed wake, so they cannot prove
that conditional native path executed. No build or native run was performed
as part of this source edit.

## Keep the watchdog independent of stderr

The hard smoke watchdog had a separate reporting defect: `terminate_smoke`
wrote to stderr before calling `process::exit`. Another thread holding the
stderr lock could therefore prevent the supposed last-resort termination.
The correction in `crates/atlas-app/src/smoke_watchdog.rs` removes terminal-path
output and reports the structural failure through the process status alone:
124 means the hard deadline expired; 125 means the cancellation channel
disconnected. Both are failures. Ordinary error exits remain unchanged.

This keeps the existing 30-second timer, cancellation/join behavior and
smoke-only startup guard. A separate logging thread would add another shutdown
dependency and still could not guarantee a message, so it is not introduced.
The watchdog is never started by a normal application launch.

A new subprocess regression invokes the real termination action for both
failure modes while the child's test thread retains the stderr lock. It uses
the current test executable with an exact test filter and `--nocapture`, so
libtest cannot divert stderr writes away from the contested lock. Only each
child command receives the private mode variable; the parent environment is
not mutated. The parent requires status 124 or 125 within a separate ten-second
bound, and kills/reaps its own child on timeout, surfacing cleanup errors.
Cleanup polls for at most two additional seconds, even when termination
fails, and preserves both termination and reap errors without an unbounded wait.
The existing cancellation, expiration and disconnected-control tests remain.
No GUI, profile or native browser is needed for this regression.

Independent review passed, including the bounded cleanup path. Root's complete
application suite passed all 70 tests, with none ignored or filtered, in
`target/continuation-recovery-watchdog-tests.log`. The full gate subsequently
passes all 400 tests and application smoke in
`target/continuation-recovery-watchdog-full-gate.log`. The frozen process-role
diagnostic intentionally retains
its reviewed older watchdog and still requires its external 45-second launcher
deadline. Neither correction diagnoses the original WebView2 breakpoint.
