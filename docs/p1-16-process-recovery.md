# P1-16: recover the engine without replacing the session

A failed renderer belongs to the affected page; a failed shared browser
process invalidates every controller in its environment. Treating both as a
generic error either leaves dead pages behind or needlessly replaces healthy
ones. This slice gives the two failures separate native paths and retains the
existing tab identities throughout recovery.

## Native ownership and recovery

`crates/atlas-platform/src/browser/recovery.rs` classifies the native
`ProcessFailed` kind. Main-frame renderer exit retires only that controller,
retains its live ID and last valid destination, and emits `PageCrashed`. The UI
shows a page-local card with Reload. Reload recreates only that controller;
unaffected controllers keep their original native objects. If a renderer was
shared, each affected WebView receives its own failure notification.

Browser-process exit retains every hosted live ID and destination, clears
pending native popup deferrals, releases all controllers and the environment,
then creates a new environment with the same dedicated profile and existing
privacy/security options. It rebuilds controllers under the same IDs and
restores which tab is visible. It never asks the session worker to close or
reopen a logical tab. Saved associations, tab order and selection therefore
remain in the existing session model. Unloaded logical rows remain unloaded.

Native failure callbacks only enqueue owned structural events. Reconstruction
runs in the worker message loop, outside WebView2's callback stack. Pending
native failures are drained before each subsequent browser command so resize
or navigation commands need not run against a known-dead generation.
Controller-lifetime tickets from P1-15 reject repeat events from the retired
generation, including events already queued from other tabs. A later failure
of a replacement controller is a new failure, not a duplicate of the old one.

Environment and individual-controller reconstruction can also fail. Their live
records remain in an unavailable-page map, with a visible retry state rather
than a missing tab. Reload retries the environment when needed; explicit
navigation can replace an unavailable page's destination. Closing a failed tab
removes that live record so a later retry cannot resurrect it.

Unresponsive-renderer, frame-only, GPU, utility and other auxiliary kinds do
not trigger shared-environment reconstruction. The active page gets a notice;
for an unresponsive renderer the notice offers waiting or reloading. Native
frame/auxiliary recovery stays with WebView2, following Microsoft's
[process failure definitions](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2processfailedkind).
No frame URLs, process descriptions, page text or account data are read for
diagnostics. No process is terminated to test this implementation.

## UI and delayed replies

`crates/atlas-app/src/ui/process_recovery.rs` keeps transient crash presentation
apart from the durable model. Failures retire the affected find/permission and
connection presentation and exit page fullscreen. Duplicate recovery-start and
completion notices do not reset the current UI generation. A failed tab keeps
its sidebar row and can still be selected, closed or reloaded.

Recreating a controller exposed a permission race: its old per-controller
request counter would start again while a storage-worker reply could still be
queued. `browser/permissions.rs` now allocates unique process-local request IDs
across controller lifetimes. The UI also forgets old prompts. A late reply
therefore cannot grant a new prompt that happens to belong to the same tab.
Popup request IDs already survive clearing their shared pending-request map.

The alternative for a renderer exit was WebView2's native Reload. Recreating
only the affected controller gives this implementation a fresh callback
lifetime and makes delayed failure deduplication explicit. No live form state
or in-memory document state can be preserved after the process that owned it
has failed; tab/session retention does not promise recovery of unsaved input.
The user-data directory is preserved, never deleted or replaced. No migration,
dependency, vendor patch, user-agent override, injected script or IPC handler
is introduced.

## Synthetic evidence

Fault injection exists only under `cfg(test)` in
`crates/atlas-platform/src/browser/recovery_tests.rs` and the page's private
test helper. It creates actual WebView2 controllers in fresh synthetic
profiles, then feeds guarded failure events into the production worker's
recovery path. A separate test supplies an environment-construction error.
An independent test-process deadline bounds native hangs. There is no product
command-line fault switch or process-killing hook.

The focused command, with temporary files confined to this worktree, is:

```powershell
$env:TEMP = Join-Path (Get-Location) 'target/session-temp'
$env:TMP = $env:TEMP
cargo test --package atlas-platform --profile local --offline --locked browser::recovery::tests -- --nocapture
```

`target/p1-16-faults-1.log` records passing focused fault tests. They prove:

- Repeated renderer failure produces one crash transition and keeps the
  sibling's actual COM identity unchanged.
- Three notifications for one shared failure cause one recovery and preserve
  all three live IDs, destinations and the active ID.
- A late failure for a closed/recreated numeric identity cannot recover the
  replacement again; duplicate creation remains rejected.
- Failed environment construction retains every live record. Closing one
  during the failed state prevents resurrection, and Reload recovers the
  remaining record.
- Failed controller reconstruction retains its record and accepts a later
  valid explicit navigation.

App tests cover session identity, order, selection and saved associations
through crash/recovery notices, closure of a failed tab, and duplicate/stale
recovery completion. The permission identity regression covers replacement
controllers' request IDs. These are synthetic recovery-path tests, not evidence
from killing a real browser process or a diagnosis of the original reported
WebView2 crash. The full final-source gate passed all 461 tests and application smoke in
`target/p1-16-gate-2.log`. The earlier gate stopped on Clippy's item-ordering
check; its fix moved the permission allocator before its test module without
changing lint levels. All six combined native suites passed in
`target/p1-16-native-final.log`, including the original lifecycle suite and
P1-15's eight native suspension readbacks and fullscreen rectangle/F11 checks.
No error 4551 occurred. The final app's eight-tab measurement window was
captured and inspected in `target/p1-16-eight-tabs.png`, confirmed 8/8 native
suspensions, and was closed normally. This startup/measurement-window check
does not establish crash-card visual or post-recovery focus acceptance.

## Remaining owner acceptance and integration

Independent review and owner visual acceptance remain. Check the crash card,
Reload, switching between a failed and healthy tab, and closing a failed tab.
Check recovery presentation while a permission/find popup or fullscreen page
was open. Confirm keyboard focus and normal page interaction after Reload.
Do not terminate daily-browser processes or test with real credentials to
perform these checks; use synthetic profiles and the test fixture.

P1-15's eight-tab measurement command remains unchanged in
`docs/p1-15-suspension-fullscreen.md`. P1-06 still owns its final D37 idle/startup
measurements. The known original lifecycle-fixture timeout remains undiagnosed.

Shared integration files touched are `crates/atlas-app/src/ui.rs`,
`crates/atlas-platform/src/browser.rs`, `docs/trapstoknow.md` and the P1-16
ledger row. Supporting edits stay in the page/permission/fullscreen modules;
the workspace manifest, lockfile, platform crate root and vendor files are
unchanged. There are no schema changes or deferred-work markers.
