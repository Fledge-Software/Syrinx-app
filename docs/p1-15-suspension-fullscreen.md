# P1-15: hidden pages and fullscreen

Long-lived inactive pages need to stop doing work, and a fullscreen video needs
the whole viewport. Hiding a child window alone proves neither requirement.
This slice uses WebView2's native suspension and fullscreen interfaces, with
no page scripts, user-agent override, inter-process communication handler,
request filters, dependency changes or database migration.

## Behavior and ownership

`crates/atlas-platform/src/browser/lifecycle.rs` waits five seconds after a page
is hidden, calls `TrySuspend`, and requires both successful completion and a
true `IsSuspended` readback. Five seconds is an implementation grace period;
the plan specifies a delay without fixing its length. A refused suspension
retries after another grace period. Activation calls `Resume`, verifies the
native state, then makes the view visible. Atlas does not also set a memory
usage target. The worker's timer disappears when no grace periods remain.

Each controller owns a callback lifetime. Native events carry a ticket through
the worker/UI queues; consumers call `BrowserEvent::into_current`. Suspension
invalidates queued ordinary callbacks, and closing permanently retires the
controller's ticket. A newly created controller has a separate lifetime even
when a test reuses a numeric tab ID. Suspension completions also carry a request
number, so activation followed by hiding cannot accept the previous request.
Process-failure notifications and permission cleanup remain deliverable while
suspended, but not after controller retirement. Native popup and permission
callbacks refuse new pending work on a retired or suspended controller.

`crates/atlas-app/src/ui/fullscreen.rs` observes native
`ContainsFullScreenElementChanged`, remembers the original window fullscreen
state, hides Atlas chrome, and sizes the page to the entire viewport. Page
exit restores the original window state and computes the ordinary page bounds
again. Switching or closing the owning tab also restores chrome. F11 while a
page element is fullscreen forwards Escape to the focused native descendant
of that controller; it never sends input to an unrelated window. Escape and
the page's own exit use the engine's normal fullscreen path. F11 outside page
element fullscreen toggles the window's fullscreen state.

The main alternative was a low-memory target, which keeps scripts running and
does not satisfy D37's suspended-tab requirement. Script-based fullscreen exit
was excluded by the page-view guardrails. Native callbacks follow Microsoft's
[fullscreen contract](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2?view=webview2-1.0.2420.47)
and [suspension contract](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_3?view=webview2-1.0.4191.47).

## Eight-tab measurement command

Run from this worktree in PowerShell, after building the local application:

```powershell
New-Item -ItemType Directory -Force target/session-temp | Out-Null
$env:TEMP = Join-Path (Get-Location) 'target/session-temp'
$env:TMP = $env:TEMP
cargo atlas --test-suspended-tabs 8
```

This mode exclusively reserves a fresh synthetic profile, creates eight
`about:blank` tabs in the real session model, and keeps their native controllers
hidden. It refuses combining this mode with a supplied profile or `--smoke`.
Wait for `Synthetic suspended tabs: 8/8 (native readback).` before beginning
the five-minute D37 measurement. The window stays open until normally closed.
No steady-state polling is added. It does not claim P1-06's idle CPU or startup
criteria passed; Claude's separate measurement still owns that evidence.

## Validation and remaining acceptance

The focused native run is recorded in `target/p1-15-native-4.log`. It confirmed
eight real suspensions, activation/resume, all-controller teardown, recreation,
and a locally generated video entering fullscreen and exiting through Escape,
the F11 exit command and the page's own Fullscreen API call. Earlier runs are
preserved: one timed out during repeated fullscreen gestures, another during
navigation. The fixture now spaces gestures and shows the video page before
waiting for its navigation; these observations do not diagnose the pre-existing
original lifecycle timeout. No old assertion or timeout was weakened.

The first complete gate passed in `target/p1-15-gate-1.log`. The final-source
full gate passed all 456 tests and application smoke in
`target/p1-15-gate-final.log`. All six combined native suites passed in
`target/p1-15-native-final.log`, including the unchanged original lifecycle
suite and the new rectangle and focused-page F11 checks. No error 4551 occurred.
The real application measurement mode confirmed 8/8 native suspensions in
`target/p1-15-eight-tabs.stdout.log`; its own window was captured and inspected
in `target/p1-15-eight-tabs.png`, then closed normally. This establishes the
measurement setup, not GPUI fullscreen visual acceptance or idle CPU results.
Unit regressions cover queued callbacks across suspend/resume/close/recreation
and late suspension completion after activation, re-hiding, duplicate completion
and closure. The native fixture also checks fullscreen viewport dimensions and
exact restoration of the original child-window rectangle.

The owner should still inspect GPUI fullscreen video layout, Escape/F11/page
exit, maximized and windowed restoration, monitor/DPI changes, and a tab switch
or close while fullscreen. Verify ordinary background playback behavior and
resume with synthetic pages first. Real streaming and account tests remain
owner-only. Independent review and the final D37 measurement remain open.

Shared integration files touched are `crates/atlas-app/src/ui.rs`,
`crates/atlas-app/src/main.rs`, `crates/atlas-platform/src/browser.rs`, the native
fixture dispatcher, and the P1-15 row of `docs/implementation-status.md`.
The workspace manifest, lockfile, platform crate root and vendor files are
unchanged. There are no deferred-work markers or schema changes.
