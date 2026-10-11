# Browser usability source review — 2026-10-10

The next small D49 implementation should make literal address submission
reachable and predictable in the existing palette. Today, the arrival of a
local search result can change Enter from navigating the current tab to
opening or focusing a different destination. Complete the existing navigation
team's work and hand this correction to that team; do not start a competing
edit of its files. End-to-end acceptance and the unresolved WebView2 failure
remain higher-priority release blockers than adding more browser features.

This report is a source review, not execution or visual acceptance. Two
independent subagents audited user journeys and acceptance/dependency evidence;
the primary reviewer checked the central findings against source. This session
created only this report. It ran no Cargo, build, test, benchmark, browser or
native fixture, used no integrations or Computer Use, and read no personal
profile. Line references describe the working tree inspected for this review;
function names remain the anchors if concurrent authorized work moves them.

## Evidence and scope

The current checkpoint is the opening section of
[`handoff-integrated-preview-2026-10-10.md`](handoff-integrated-preview-2026-10-10.md),
lines 11–52. Its reported combined binary SHA-256 is
`3ca0bf045fc0826e7300a647307845733fb5249b085f52feeadbebf0abd4c276`.
This review did not independently hash or execute it.

- The latest gate passed dependency checks, formatting, Clippy and 301 tests,
  then Windows error 4551 prevented the public `frizbee_padding` executable
  from running. Later stages were unreached: the full gate failed.
- Separate rebuilt application smoke and patched native lifecycle runs passed.
  They do not establish interactive usability or explain the original
  `0x80000003` WebView2 failure. Construction fault-injection validation was
  still being prepared at the checkpoint.
- All 1,040 benchmark results retained their exact semantics, but only 10/26
  D48 cases passed; maximum p95 was 13.9768 ms and the worst sample 17.7388 ms.
- The owner stopped Computer Use. The existing synthetic window remains
  untouched, and popup typing/navigation and current visual acceptance remain
  unverified. Earlier permission and passing-gate prose lower in the handoff,
  the implementation ledger, or Plan 1 does not supersede this checkpoint.

[`decisions.md`](decisions.md), lines 10–18, and Plan 1's D49 decision bring
existing tabs, saved-page storage, backups and search into the browser window,
navigation and Ctrl K ahead of normal slice order. They preserve all other
requirements. The normal next unfinished slice is P1-05, followed by P1-06's
idle work and P1-07's design tokens; Plan 1's issue map begins these at lines
929–931. D49 permits corrections to the current integration, not a general
exception for P1-11 onward or Plans 2–4. The live design canvas was not opened;
this review makes no claim of design conformity.

## Journey audit

| Journey | Source-backed state | Remaining work and approved owner |
| --- | --- | --- |
| Open a tab or address | `session.rs::apply_model` resolves queued new-tab/navigation commands on the worker; `Worker::handle` saves before publishing. `ui.rs::sync_pages` creates only the desired active native page and waits for creation acknowledgement. | These paths exist. Native typing, first navigation, rapid queued actions and no duplicate views remain unverified together. P1-08–P1-10 under D49. |
| Save, unload, open/focus and unpin | `session.rs::apply_model` routes the shared core operations; `ui.rs::sidebar` presents saved and ordinary rows. `session/worker_tests.rs:176,704,946` cover fixed saved destinations, closure/restart and removed rows staying removed. | Core/worker behavior is implemented, rather than a missing bookmark feature. Actual saved-row clicks, persistence, native unloading and replacement-page ownership need integrated acceptance. P1-09; folders/import remain P1-21/P1-22. |
| Close and reopen | `ui/page_lifecycle.rs::observed_close` queues closure; `removed` releases native ownership only after the committed model loses the tab. `ui.rs:451–456` closes removed instances; late creation callbacks cannot claim a replacement. | Verify active-right-then-left selection, last-page Home, ordinary and saved reopen, repeated/stale native closes and failed persistence without native teardown. Existing tests at `worker_tests.rs:704,794,946` support parts of this, not the full UI journey. P1-09/D42/D43. |
| Back, forward and reload | `browser/page.rs:123–153` calls native Reload/GoBack/GoForward and checks availability. Source/title callbacks feed the session (`ui.rs:314,362`; `page.rs::register_source`). Toolbar and shortcuts are wired (`ui.rs:519–545,829–855`). | Missing implementation must not be inferred merely from absent visual evidence. Verify two synthetic destinations, title/address changes, saved-target independence and shortcuts with page focus. P1-10. |
| Palette, address and focus | Popup ownership, deferred input focus, reactivation and page focus return exist (`ui.rs:549–622,681–729`; `sync_pages:474–480`). Query identities reject stale replies (`palette.rs:95–119`; `ui.rs:264–276`). | Literal submission has the concrete gap below. Actual typing, Enter/Esc, reopen, Alt Tab, native focus return and stacking remain unverified. P1-10 and D49; full registry/scopes/previews remain P1-30–P1-33. |
| Normal startup and interrupted exit | `session.rs:204–247` opens the guarded store and restores its snapshot. `NavigationIntents::desired_page` and `sync_pages` load the active destination lazily. `worker_tests.rs:176,323` cover restart and queued shutdown. | App restart with several live tabs plus unloaded saved rows needs execution evidence. The unclean-session notice (`ui.rs:235–239`) describes reading committed state, not restoring a damaged database. P1-19 composition brought forward by D49. |
| Damaged database recovery | `session.rs:209–214` directly opens the store and propagates failure. `ui.rs:278–286` reports storage failure. No application recovery offer/confirmation flow exists. | This is missing integration, not merely an unverified UI. Existing `offer_recovery`, `RecoveryOffer::metadata` and `confirm` are available in `atlas-platform/src/storage/recovery.rs:93,147–153`. P1-04/D46, within D49's named backup scope. |
| Shutdown and engine failure | `ui.rs:746–804` waits for browser and session shutdown; browser Stopped triggers store shutdown (`ui.rs:383–397`). `session.rs:214–222` observes store close errors. `browser.rs::run` keeps the apartment alive around its browser state. | Separate smoke/lifecycle evidence supports limited shutdown behavior. Repeat navigation/palette/creation shutdown stress and durable restart through the integration owner. `ui.rs:354–360` reports process failures but does not implement renderer/shared-browser recovery; that feature remains P1-16. Do not equate shutdown ordering with a diagnosed crash. |

Paths in this table are under `crates/atlas-app/src/` except `browser.rs`
and `browser/page.rs`, which are under `crates/atlas-platform/src/`.

## Smallest proposed implementation: stable literal navigation

The source confirms a timing-dependent action. In
[`palette.rs`](../crates/atlas-app/src/palette.rs), `query` clears results and
selects index zero (lines 80–86), `receive` installs results and selects zero
(105–117), and `submit(false)` activates any selected hit (128–138).
[`ui.rs`](../crates/atlas-app/src/ui.rs), lines 700–709, sends a submission to
current-tab navigation but turns activation into FocusTab or OpenSaved. Ctrl L
only supplies initial text; its address intent does not reach `PaletteView`
(571–616).

For a typed `https://example.test/` with a matching saved/live destination,
Enter before the query reply submits that address; Enter afterward activates
the first local result. The displayed fallback is “Search the web,” which
cannot request literal navigation. The exact default ranking of a future
address row is not a settled decision in the documents. The recommendation
here is a concrete implementation proposal for review, not a new decision.

Add a visible “Go to …” action for input recognized by the existing address
parser. Keep it selected across asynchronous result arrival until the user
deliberately selects a local result. Mouse/arrow selection must still activate
that result, and Alt Enter must still search the web. The selected accent must
describe what Enter does, as required by `design/README.md`; silently overriding
Enter while highlighting a different result is rejected. Reuse
`navigation.rs::destination` and its existing validation rather than building
a second URL policy. This needs no full command registry, new dependency,
native page-content access or public crate API change.

**Proposed future file ownership:** integration should assign the navigation
team exclusive ownership for this correction of `crates/atlas-app/src/palette.rs`,
`crates/atlas-app/src/navigation.rs`, and only the necessary palette
construction/event wiring in `crates/atlas-app/src/ui.rs`. Put the pure action
selection tests beside that policy; no new shared source file is required.
The integration session owns acceptance records and serialized execution.
The recorded navigation allocation already includes `navigation.rs`,
`ui/navigation_intents.rs` and `ui/page_lifecycle.rs`; the proposed palette/UI
allocation is additional and requires integration coordination. This report's
author claims no edit ownership over those files. If the navigation team is
finished, integration must explicitly transfer ownership before assigning
implementation elsewhere.

Acceptance checks for that change are:

1. The same explicit synthetic URL chooses the same visible navigation action
   before and after matching results arrive, including empty/failed/stale
   replies and reopening a popup whose local request IDs restart.
2. Deliberately selecting a saved or live hit still follows its shared
   open/focus action; a saved row never gains a second instance. Alt Enter
   retains web-search behavior. The accent and Enter action agree.
3. Mixed-case schemes, refused schemes, embedded credentials, ordinary words
   and bare domains keep the existing parser's behavior. Pure tests cover
   action selection rather than duplicating validation code.
4. Integration runs the required gate and relevant regressions in its one
   execution lane. Error 4551 or an unreached stage stays a failure. Once UI
   testing is authorized again, use a fresh synthetic window to verify Ctrl L/K
   typing, result selection, submission, Esc, focus return and repeated opening.
   No Computer Use retry is authorized by this report.

## Next gaps after that correction

The navigation team should also review normal navigation failure feedback.
`ui.rs:338–352` consumes `NavigationCompleted.success` only in smoke mode,
and lines 363–366 set “Loading…” only on a loading-start event. The application
therefore lacks a corresponding normal completion/failure transition in these
handlers. A native error page may still communicate the failure; that behavior
was not inspected here. This is a D49/P1-10 feedback gap, not proof that a
failed page is blank or silently succeeds. A synthetic successful/failed
navigation sequence should verify truthful status and preserve serious errors.

Recovery is the next concrete missing D49 integration identified by both
reviewers. A future recovery owner can use the unchanged platform API and own
`crates/atlas-app/src/session.rs`, a new `session/recovery.rs`,
`session/worker_tests.rs`, a new `ui/recovery.rs`, and limited `ui.rs` wiring.
These are proposed paths, not files created by this review. Integration must
coordinate exclusive session/UI ownership and transfer any overlapping
palette/UI allocation before recovery implementation begins.
Show the verified backup date and require explicit confirmation before
`RecoveryOffer::confirm`; cancellation must leave the original data alone.
Synthetic checks must distinguish ordinary I/O refusal from proven corruption,
retain exclusive ownership while offering, preserve displaced files, reject
changed identities, handle no valid backup and unsupported deletion state,
and prove restored state can restart. Do not infer corruption from any generic
store-open failure or automatically replace a database. UI rendering and
confirmation remain necessary acceptance steps.

The remaining browser features retain their normal plan dependencies:

| Confirmed gap or incomplete delivery | Existing slice and evidence |
| --- | --- |
| Site-created windows are denied | P1-11; `browser/page.rs:80–83` explicitly returns `NewWindowResponse::Deny`. A successful simple page does not prove popup OAuth works. |
| Permission/notification workflow, reachable browser essentials and security state | P1-12–P1-14. The current browser command/event surface in `browser.rs:57–118` does not supply complete Atlas prompt/find/download/print/zoom/security flows; native defaults do not establish acceptance. |
| Hidden-tab suspension, fullscreen and process recovery | P1-15/P1-16; `Page::show` only changes visibility, and `ui.rs::browser_event` only reports process failure. |
| Draggable wide/compact/hidden sidebar, row reordering and hover peek | P1-07–P1-10/P1-17; fixed `SIDEBAR_WIDTH` and `sidebar` rendering in `ui.rs`, plus the absence of a reorder session command, leave these undelivered. Do not invent D44 thresholds without the planned review. |
| History, folders/import, full text/retention/clearing, complete Home/Favorites/settings and palette | P1-20–P1-33; the implementation ledger lists these open exits. Flat saved rows and typed local catalogue entries do not implement these journeys. |
| Daily-driver delivery and later plugins | P1-18/P1-34–P1-40 and then Plans 2–4. Product site tests, accessibility, default-browser use and performance evidence are still required; placeholder plugin rows are not implementations. |

P1-05 performance work remains the earliest normal implementation lane and
must preserve D48's complete-query semantics and three passing fresh-process
runs. P1-06's idle condition remains open. Neither the proposed palette fix
nor a recovery prompt closes either requirement. Owner-only sign-in, DRM and
daily-use checks remain with the owner.

## Independent review result

The journey reviewer and evidence/dependency reviewer independently confirmed
the missing recovery flow and the difference between source support and native
acceptance. The second reviewer separately checked the palette timing finding
and required a visible action matching Enter, rather than an invisible priority
override. Both agreed that D49 permits the narrow integration correction but
does not advance later features or waive predecessor exits. The final journey
review passed. The final evidence review requested one ownership correction,
now applied: existing navigation allocation is distinguished from proposed
session/palette/UI allocation, which integration must coordinate.

No product, vendor, plan, handoff or other report was edited. No commit, merge,
push, dependency, security setting or public API was changed. The integration
session retains all execution and final acceptance ownership.
