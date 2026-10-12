# P1-11: preserve popup openers when opening Atlas tabs

Status: partial checkpoint on `codex/p1-11-popups`. The 2026-10-12 full gate
passed all 439 tests and the application smoke run without error 4551. The final
native popup suite passed adoption, opener access, message reply, document focus,
the focus-command barrier and active-child visibility, even when Windows denied
foreground activation. The combined native fixture still exits unsuccessfully:
its original lifecycle suite fails to finish the first synthetic navigation.
P1-11 is not complete. This is not OAuth or visual acceptance.

A sign-in page can open a child window and wait for a message from that child.
Opening its address as an unrelated tab breaks that exchange. Atlas must adopt
the native new-window request, rather than navigate a second view to its URL.

`crates/atlas-platform/src/browser/popups.rs` retains WebView2's new-window
arguments and deferral on the browser thread. Its callback marks the request
handled, so WebView2 does not open an unmanaged window. Only an owned request
number, opener ID and address cross to the UI and session worker. The session
saves the new tab before acknowledging the request. Atlas then constructs a
page with the same environment and default profile, without calling Navigate,
sets it as the requested new window, and completes the deferral. Native failure
is reported rather than represented as a successfully created page.

The pinned wry handler with no application callback also sets Handled. Atlas
registers its native handler after construction, through wry's exposed WebView2
object. The synthetic opener/message test establishes that both handlers coexist correctly.
No dependency or vendor source is changed. Page identity, scripts, IPC, request
filters, SmartScreen and custom crash reporting keep their existing configuration.

Closing an opener cancels its outstanding requests. Shutdown completes remaining
deferrals before destroying pages and the COM apartment. Requests from an opener
already closed in the durable model do not allocate a new tab. There can be at
most 32 simultaneous pending deferrals; unsupported destinations or an exhausted
request queue are explicitly blocked. These bounds prevent retained native
requests from growing without limit while storage is busy.

Adopted blank pages remain native pages rather than Atlas Home. Their native
request identities are not persisted or restored. Persistence tests cover
correlation, closed openers and restarting with the committed tab metadata.

## Validation

Run the required gate from this worktree and keep its output under `target/`:

```powershell
cargo xtask check *> target/p1-11-gate.log
cargo run --package atlas-platform --example page_close --profile local --offline --locked *> target/p1-11-native.log
```

The existing `page_close` example now also serves two synthetic loopback pages.
The opener calls `window.open`; the child reads a synthetic property through
`window.opener`, posts a same-origin reply, and reports actual document focus
through its title. The harness checks the reply's sender and origin, adopts the
child using the production command path, and focuses it through that same path.
It waits for a FIFO command barrier after the document-focus report, so an
earlier title event cannot conceal a failed `MoveFocus`, and verifies that the
child is visible while the opener is hidden.
Its accepted sockets explicitly enter blocking mode before bounded reads. It
uses an exclusively reserved disposable profile and no real account or OAuth
provider. The focus phase briefly displays a native test window.
The pre-existing close, geometry and repeated-lifecycle checks remain required.
Each suite gets its own fresh profile and unchanged 45-second native deadline;
the outer watchdog allows 120 seconds for both suites and their cleanup.


### Evidence from this worktree

- `target/p1-11-gate-2026-10-12.log`: the full gate passed on 2026-10-12:
  dependency boundaries, formatting, warnings-denied workspace Clippy, all
  439 tests, documentation tests and the synthetic application smoke run.
  No Application Control error 4551 occurred. The first optimized test build
  took 35 minutes 55 seconds, including the existing search test targets.
- `target/p1-11-final-fixture-checks.log`: formatting and platform all-target
  Clippy with warnings denied passed after the final fixture-only edits.
- `target/p1-11-native-2026-10-12-final.log`: the final-source popup suite
  passed, including document focus after foreground activation was denied.
  The original lifecycle suite failed on synthetic tab 1, before its stress
  cycles: creation and source change arrived, but navigation completion and
  page-close delivery did not arrive within the unchanged 45-second deadline.
  The combined fixture exited 1. This run followed the completed gate and
  focused Clippy check; no other build from this worktree was running alongside it.
- `target/p1-11-platform-check.log`: platform library/examples type-check passed.
- `target/p1-11-app-check.log`: application and test type-check passed.
- `target/p1-11-clippy.log`: application/platform all-target Clippy passed with
  warnings denied. The vendored dependency retains its existing dead-code warning.
- `target/p1-11-native.log` and `target/p1-11-native-retry.log`: the combined
  harness exceeded its existing native deadline during lifecycle cycles 12 and
  5, before reaching popups. Both failures are retained.
- `target/p1-11-native-isolated.log`: after separating the suites into fresh
  browser instances, the original lifecycle suite completed; popup window
  activation failed. A visible launch also failed activation, recorded in
  `target/p1-11-native-visible.log` and `target/p1-11-native-visible-errors.log`.
- `target/p1-11-native-opener.log`: the original lifecycle suite completed and
  the popup's adoption, opener access and message reply passed. Foreground
  activation still failed, so the fixture exits unsuccessfully and focus is
  explicitly unverified. No assertion was removed to report success.
- `target/p1-11-gate-first.log`: the gate passed dependency boundaries and
  formatting, then Windows blocked the `proc-macro2` and `serde` build helpers
  with error 4551 during workspace Clippy. Unit tests and application smoke were
  not reached. `target/p1-11-gate-retry.log` records the unchanged retry, which
  again failed because Windows blocked the same `proc-macro2` helper with 4551.

On 2026-10-12 the unchanged executable passed popup focus, but the original
lifecycle suite exceeded its deadline at cycle 3
(`target/p1-11-native-2026-10-12-baseline.log`). The next run
(`target/p1-11-native-2026-10-12-focus.log`) passed popup focus even after
`SetForegroundWindow` returned false; the lifecycle suite and its cleanup still
timed out. The fixture now reports foreground denial and always submits the
production focus command. It still requires the child's `document.hasFocus()`
title report. No page-focus assertion or lifecycle deadline was relaxed.

This separates two different operations: Windows deciding which application is
foreground, and WebView2 moving focus into an Atlas page. Microsoft's
[foreground rules](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow)
permit activation to be denied, whereas
[MoveFocus](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller#movefocus)
is the controller operation used by wry's `focus()`. The observed successful
document-focus report establishes a harness activation limitation, not a popup
focus defect. The tests also assert the adopted tab becomes active in the
durable model. GPUI rendering and owner typing remain separate manual checks.

`target/p1-11-native-2026-10-12-diagnostic.log` is an intermediate build with
the stronger FIFO and visibility assertions; the popup suite passed. The original lifecycle suite
failed at cycle 1 with creation and source change observed, but neither
navigation completion nor close delivery. This identifies the missing events;
it does not establish why navigation stalled. Both suites kept their 45-second
deadlines. These runs overlapped this worktree's first full optimized test build.
A trial early URL validation guard was present in that diagnostic build and
then removed to preserve the existing failed-creation cleanup path; the final
source is covered by the separately recorded final run above.

`target/p1-11-focused-tests-2026-10-12.log` records five passing popup-filtered
tests from the gate's already-built application and platform test executables.
They cover durable allocation, active selection, closed-opener rejection,
restart correlation, blank-popup display, native acknowledgment failure and the
existing palette-popup routing regressions. This focused run is not the full gate.

No signing, executable relocation, policy changes, dependency changes or vendor
edits were used to get around the block. The separate focused build directory
was created before the gate encountered error 4551; its checks are not a green
gate.

To repeat native focus acceptance in the owner's interactive desktop terminal,
run the already-built fixture without moving it:

```powershell
& 'C:\Users\<user>\Documents\GitHub\Atlas-wt\p1-11-popups\target\popup-check\local\examples\page_close.exe'
```

It should briefly show the synthetic window, print that native page focus passed,
and exit zero. A foreground-denied diagnostic may appear; a missing document-focus
report still fails the check. Neither result alone establishes visual acceptance.
Do not run P1-12 until P1-11's gate and native acceptance are green.

## Diagnosis of the lifecycle timeout (2026-10-12)

The intermittent failure, tab 1 created with its source changed but no
navigation completion or close within 45 seconds, was the product's own
hidden-page suspension (P1-15) freezing the fixture's page mid-load. The
diagnosis ran on `claude/diagnose-lifecycle-timeout`, branched from the
integrated preview at `94b3cb1`; its logs are under that worktree's
`target/diag-runs/`.

**How it happens.** The close fixture created its page hidden and never
showed it. `browser/lifecycle.rs` calls `TrySuspend` on a hidden page five
seconds after its last navigation began. The fixture page must load, run a
100 ms timer and call `window.close()` inside that window. Timestamped traces
showed the load itself takes about 0.4 s, but the hidden page's close timer
fired between 0.3 and 4.3 s after `NavigationCompleted`. Anything that slows
the load, such as a concurrent optimized build, pushes it past five seconds;
earlier failing runs above are recorded as overlapping an optimized build. Suspension then succeeds
mid-load and the frozen page sends nothing more, while the harness, which
ignores `Suspended` events, waits out its deadline.

**Evidence.** A diagnostic loopback server that sent the headers and title at
once but held the rest of the close page for 6 s reproduced the recorded
failure exactly in 2 of 2 runs: `TrySuspend` was issued 5.0 s after the
navigation, completed with success, and no completion arrived in the
remaining 40 s even though the body was delivered at 6 s. Holding it for 3 s
instead passed. The earlier hypotheses were checked and excluded: both
fixture servers put accepted sockets in blocking mode; navigation, source
and completion handlers are registered before `Navigate`; and every traced
request was accepted, read and answered within milliseconds.

**The fix.** The fixture now checks that each new close page starts hidden,
then shows it as soon as `Created` arrives, which is what the popup and
permission suites already did. A shown page is never suspended, and a page
already suspended is resumed when shown. With the page shown, the close
arrived about 0.1 s after completion in every traced run. `--stall-first-close`
keeps the same 6 s hold as a regression check: the old fixture failed under
the temporary diagnostic version of that hold, and the fixed one passes. `--trace` prints timestamped commands, events and
loopback requests. No deadline was changed.

**A second, deterministic failure.** At `94b3cb1` every run first stopped at
lifecycle cycle 1 with "Repeated resize did not preserve matching survivor
bounds". P1-08's placement change (`browser/placement.rs`, `7c6f2db`)
deliberately leaves a hidden page's bounds alone until it is shown, and the
fixture still expected the hidden survivor to move. The cycle now asserts the
new contract instead: the hidden survivor keeps its bounds through the
resize and has the card's bounds once it is shown again.

**Results.** Before: the stall reproduction failed 2 of 2, and 5 of 5 plain
runs failed on the resize assertion before the stress cycles. After: with
`--stall-first-close` the whole fixture passed 3 of 3 traced runs, with
completion at 6.4 s and the close 0.1 s later. Six traced plain runs also
passed the whole fixture. Of 15 untraced runs (12 plain, 3 with the stall),
10 passed the whole fixture and 5 exited 1 with bare deadline errors in the
permission or certificate suites, whose pass lines were missing. The
lifecycle suite printed no pass line in those builds, so they do not prove
it passed, but each run's error count and duration fit one or two late
suites, not a lifecycle failure. Those permission and certificate timeouts
were not reproduced under `--trace` and remain undiagnosed. The lifecycle
suite now prints its own pass line.

Whether Atlas should suspend a hidden page while it is still loading is a
product question, now under the v1 plan's "Open threads".

## Owner checks after the gate and native fixture pass

1. Run `cargo atlas` from this worktree. From a page that opens a sign-in popup,
   trigger the popup yourself. A new ordinary Atlas tab should appear and become
   active; there should be no separate unmanaged browser window.
2. Type in the new tab, use Ctrl L and Escape, and confirm typing returns to the
   page. Switch to the opener and back. Close the child; its sidebar row should
   disappear and the opener should remain usable.
3. Perform Google and GitHub sign-in yourself if desired. The child should be
   able to complete its message exchange with the opener. These real-account
   checks belong to the owner and are not covered by synthetic acceptance.
4. Resize Atlas with the popup tab active, then close and reopen Atlas. The tab
   metadata should restore normally; a restarted tab cannot restore a live
   JavaScript opener connection from the previous process.

The automated fixture exercises the native page host, not GPUI rendering. Window
appearance and real sign-in behavior remain manual checks until explicitly observed.

## Handoff boundary

This checkpoint retains the failing native evidence rather than treating a green
workspace gate as native acceptance. The navigation stall was diagnosed later
(see above); no deadline or vendor patch was changed to obtain a pass.

P1-12 through P1-14 have not started because P1-11's combined native fixture is
not green. In addition, the continuation instruction prohibits
creating another branch and permits commits only on the current branch; the
per-slice workflow requires a separate branch for P1-12. Resolve that boundary
before starting the next slice. No merge was made. The only shared
application file changed is `crates/atlas-app/src/ui.rs`, with popup routing,
state and blank-page display wiring. `Cargo.toml`, `Cargo.lock`, platform exports
and vendor patches remain unchanged. Shared documentation changes are the P1-11
row in `docs/implementation-status.md` and the focus-fixture entry in
`docs/trapstoknow.md`. All changes and the checkpoint commit stay in this
worktree's current branch; the integration checkout is untouched.

## Adoption failure review fix, 2026-10-12

A failed adoption cannot recreate the requested address as a normal page: that
would lose its live opener. `ui/popups.rs` now requests a durable close for the
failed popup and retains pending ownership until the committed close arrives.
The UI reports why it is closing the popup. If persistence fails, selection
still cannot queue ordinary page creation. Ordinary page failures remain
retryable. The regression covers pending ownership, committed removal and the
ordinary-page retry path.

After the machine restart, `cargo xtask check` passed in
`target/restart-p1-11-gate.log`. A fresh synthetic-profile window was captured
in `target/restart-p1-11-window.png` and closed through its normal window close
path. This window check does not simulate failed native adoption or real sign-in.
