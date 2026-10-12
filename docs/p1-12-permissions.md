# P1-12: keep site consent explicit and durable

Sites need camera, microphone and other permissions without acquiring them merely
by asking. This slice on `codex/p1-12-permissions` adds an Atlas permission bar
above the native page, with Allow, Deny and an initially unchecked Remember for
this site control. The requesting origin and permission are displayed. A request
from a background tab waits until that tab is shown. Unknown permission kinds
and unsupported origins are denied.

The native `browser/permissions.rs` keeps WebView2 event arguments and deferrals
on the browser thread. Every request begins denied; navigation, page closure and
shutdown release pending requests. A late reply cannot revive an obsolete
request. A maximum of 16 pending requests per page bounds retained native state.
Buttons retain the displayed request identity and have request-specific element
IDs, so a stale click cannot authorize the next prompt. Clipboard auto-grant is
explicitly disabled. No page identity, scripts, IPC or request filters changed.

Atlas sets `SavesInProfile` false and stores remembered choices in `atlas-store`.
The main alternative, allowing WebView2 to keep a second permission store, would
make one-shot decisions unexpectedly persistent. The session worker queries and
writes SQLite away from the UI thread; a remembered grant is applied only after
the write succeeds. Read/write failures deny the request and show a structural
error. Keys contain the canonical requesting HTTP(S) origin and a stable Atlas
permission identifier; paths, query strings and page contents are not stored.
Scheme, port and subdomain remain distinct. Invalid stored decision values never
become grants.

This base had schema 1. Migration **2** adds `site_permissions`, preserves the
session and accepts older backups for read-only validation and subsequent
migration. **Renumbering is expected at integration**: the integration branch
already uses schema 2 for folders, while parallel history/home work uses 3 and
may use 4. No lockfile, dependency pin or vendor source changed.

Notifications use the same Atlas consent flow and retain WebView2's default
notification UI. Native delivery was proved by the synthetic document receiving
its `show` event, after which it closes the notification. This is not a visual
review of the Windows notification or GPUI permission bar.

## Evidence and limitations

- `target/p1-12-native-3.log` passes the synthetic permission, notification,
  original lifecycle and popup suites. Consent is re-requested in new views of
  the same profile, a saved grant survives SQLite reopening, and navigation
  cancels a pending request before a delayed allow. No real account or resource
  such as the camera, microphone, clipboard or physical location was accessed.
- `target/p1-12-native-1.log` and `target/p1-12-native-2.log` preserve an incorrect
  new fixture expectation: PermissionRequested.Uri is the requesting origin,
  not its document path. The corrected assertion checks that exact origin.
  No original lifecycle assertion or per-suite deadline was changed. The outer
  watchdog now budgets 180 seconds for three independent 45-second suites and
  cleanup, instead of 120 seconds for two.
- `target/p1-12-gate-1.log` passed the full gate before the last review fixes.
  `target/p1-12-gate-final.log` passes dependency checks, formatting, workspace
  Clippy, all 445 tests and the application smoke run. No error 4551 occurred.
- Store regressions cover schema-1 migration and session preservation, independent
  origin/kind choices, restarting, changing a remembered choice, invalid-value
  refusal, byte-preserving old-backup validation and permission snapshot recovery.
  App tests cover origin canonicalization and stale consent actions.
- Independent source review found the stale-click problem, which was corrected
  and re-reviewed. It found no further confirmed blocker. Permission requests
  also preserve unrelated pending Home-entry intent.
- The original lifecycle navigation timeout remains a known pre-existing
  intermittent failure, documented in `docs/fixture-transport-timing-review.md`
  and the P1-11 report. Passing this run does not diagnose or fix it. The owner
  explicitly authorized proceeding despite that original intermittent failure.

## Owner visual checks

Native desktop inspection is unavailable in this session. The gate runs the
application with a disposable synthetic profile, but visual acceptance is not
claimed. To inspect the actual bar without personal data:

1. Serve the HTML constant in
   `crates/atlas-platform/examples/page_close/permissions.rs` from a local HTTP
   server, using a fixed loopback port. Its script is ordinary fixture content,
   not an injected script.
2. Run `cargo atlas -- --test-data-dir <new-absolute-directory-under-target>`.
   The directory must not already exist. Open the loopback fixture with Ctrl L.
3. Verify the exact loopback origin and ?show notifications? text are visible
   above the page. Deny without Remember; open the fixture in a new tab and
   verify that the prompt returns.
4. Allow with Remember. Verify a synthetic Windows notification appears. Quit
   and relaunch using that same test directory and loopback port; the saved
   consent should be used. Reuse only the synthetic directory created for this
   check; never substitute the daily profile.
5. With a prompt pending, navigate away, switch tabs and close the requesting
   tab. Verify no obsolete prompt or delayed click grants a different request.
   Check narrow-window layout and that the page resumes focus after clicking it.

## Integration handoff

Update the P1-12 ledger row only. P1-11's historical partial result remains
accurate; this newer successful run is additional evidence, not a rewrite of it.
The shared `crates/atlas-app/src/ui.rs` changes wire permission events, state,
bar placement and native bounds, and allow request-specific button identities.
The other shared files named in the task (`Cargo.toml`, `Cargo.lock`, platform
`lib.rs`) are unchanged. The 2026-10-12 review adds DW-7 to the plan and its
owner-requested marker at the decision store: clearing a remembered choice
belongs to P1-29 settings or a P1-12 follow-up. Permission results now use early
returns, with regressions for absent choices, allow/deny, storage failures and
stale replies. The schema number stays unchanged on this branch; integration
will renumber permissions to version 5 after folders, settings and history.

The post-restart review gate passed in `target/restart-p1-12-gate.log`, including
the new result-handling regressions and the application smoke run.

## API references

Microsoft documents [request origins and deferrals](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2permissionrequestedeventargs?view=webview2-1.0.4191.47),
[permission persistence control](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2permissionrequestedeventargs3?view=webview2-1.0.2277.86)
and [default notification UI](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2notificationreceivedeventargs?view=webview2-1.0.4191.47).
