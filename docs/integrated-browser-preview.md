# Make the existing product work usable in a browser window

## Current combined preview checkpoint

The owner-approved stable visible “Go to …” row is implemented and independently
reviewed, preserving address selection across asynchronous local results. The
current `target/local/atlas-app.exe` has SHA-256
`0fe3b8daad72ddf3383f025756a5174b39f098d2fa7355d81bfd45eaec89c858`.
The full `target/continuation-socket-close-commit-gate.log` exited zero, with all 376 tests
passing (including 54 app, 164 search unit tests and five public matcher tests),
none ignored or filtered, plus dependency checks, formatting, Clippy,
documentation tests and smoke. Its SHA-256 is
`ca28dc0a417a3e00929ece8c652c21dc8b7b2e58da6acd820fec72af0a03ca62`.
On October 11 the owner reported all three manual checks passed on this
synthetic build: `about:blank` text appeared, “Go to about:blank” stayed selected
after two seconds, and Enter closed the palette. This narrow owner-reported
pass does not establish other destinations or resolve the original breakpoint.
On the same live `0fe3b8da…` build, a second owner-operated check opened the
local fixture, typed `before` in its Typing area, used page-focused Ctrl K,
typed `about:blank` in the palette, pressed Escape and typed `after` without
another page click. The owner confirmed “Yes, beforeafter appeared”. Page
typing, page-focused Ctrl K, palette typing and Escape focus return therefore
passed that single cycle. This does not establish general reliability or
Enter focus return in other destinations. The source ordering concern was not
reproduced; no focus fix or security change was made.
Earlier normal-start attempts produced black captures and two activation failures;
a direct launch of the same binary in another fresh synthetic profile then
showed the Windows lock screen. The integration agent stopped Computer Use
and asked the owner to unlock. This does not establish a Syrinx black-window
failure. On October 11 the owner explicitly said “GO AHEAD”, confirmed Windows
unlocked and Syrinx visible/responsive. The integration agent then observed Home
and the palette rendered normally in synthetic `atlas-app-focus-20261010`
process 46216, window 6097856. Clicking edit element 3 showed a caret/popup
focus, but value entry failed with “element 3 is not available in cached app
state for atlas-app-focus-20261010.exe”; a refreshed list exposed only the main
window. The manual `about:blank`, two-second wait and Enter check was initially
pending, then the owner confirmed “All three worked”. Unlock is no longer the
blocker; the tool-targeting failure did not establish a Syrinx typing failure.
No new focus-ordering fix is applied, and broader focus acceptance remains open.

The disposable pending-creation shutdown test passes its `controller` and
`completed` cases; its original `environment` case exits one before successful
native initiation or queued shutdown is evidenced. That failure is not proof
of a shutdown defect. The diagnostic build subsequently passed; its one
environment run exited one after successful callback/object creation and
successful native return, with `pending_rejected=completion_already_entered`.
Initialization succeeded, but the early callback failed the pending-state
coverage precondition. Pending environment shutdown remains untested. Preserve
`target/continuation-pending-shutdown-environment-diagnostic.*` alongside the
original failure and controller/completed passes.
The separate native-status v1 run fails cycle 4, tab 103, at fixture
navigation with `success=false` and UNKNOWN status 0. Reviewed v2 adds native
HTTP status and shared callback-entry ordering in its disposable copy, with
the fixture and assertions unchanged. The v2 build completed successfully in
`target/continuation-native-ordering-build.log`; its single native run exited
one, preserved under `target/continuation-native-ordering.*`. The only native
callback line records sequence 1, tab 1, navigation 2, `success=false`,
`web_error=0`, `http_status=0`, followed by fatal fixture-navigation failure.
No close-request callback was recorded. This does not establish a transport
cause or close-timer race. These diagnostic jobs are terminal, and the earlier
failures remain recorded.

The reviewed v3 disposable native overlay then passed four fresh-process runs
(the first plus three bounded repeats), each completing 12 lifecycle cycles
and multi-page shutdown. All records under
`target/continuation-native-correlation.*` and
`target/continuation-native-correlation-repeat-{1,2,3}.*` report exit zero and
the same diagnostic binary SHA-256
`942fc597df9cbef496a570d09658448aad1e5b8c62fc6aae92ee1921bb932fea`.
Each log has 31 successful start/completion pairs, 13 fixture HTTP 200 responses,
13 close callbacks and no server error. The fixture and strict checks remain
unchanged, but instrumentation can affect timing. These successful hidden
synthetic runs do not explain the preserved failures, resolve the original
WebView2 breakpoint or establish visible app usability. The handoff records
the full evidence; all four processes are terminal.

The subsequent fixed v2/v4 comparison retains all three pairs under
`target/continuation-native-server-pair{1,2,3}-{v2,v4}.*`. V2 fails all three
runs, at initial tab 1, cycle 5/tab 104 and cycle 8/tab 107; v4 passes all three
full 12-cycle and multi-page-shutdown runs. V4 keeps the v2 page module and
adds only server counters/logging, preserving the fixture and all assertions.
The third v2 failure reports native `CONNECTION_ABORTED` (code 9) and HTTP 0;
the earlier two report UNKNOWN 0 and HTTP 0. All six binary hashes/results
were independently checked and are recorded in the handoff. The difference
supports a server-work/timing hypothesis in this small fixed sample, without
proving connection-close causality, resolving the original crash or establishing
visible app acceptance. No product fix or assertion relaxation is made.
All six processes are terminal.

The owner-authorized A/B/B/A socket-lifetime comparison (v4/v5/v5/v4) then
completed all four runs with exit zero. Each passes all 12 lifecycle cycles
and multi-page shutdown, with 31 successful native completions (13 HTTP 200,
18 HTTP 0), 13 expected close callbacks and contiguous shared sequences 1–44.
Each server reports 13 headers/13 writes, no empty/partial EOF and no error;
accepted connections/empty timeouts are 39/26, 35/22, 38/25 and 35/22.
B's only source difference releases the stream before the existing final
response log. Retaining it through that log was not necessary in these runs.
This does not identify a TCP mechanism, explain the earlier failures, establish
reliability or resolve the original breakpoint. Both variants remain disposable.
Preserve `target/continuation-socket-close-{01-A,02-B,03-B,04-A}.*` and the
series JSON (SHA-256
`b40081339240a797f4c12720b2bc4460ec487bd270011da0f771cec4008a7c21`).
`docs/fixture-transport-timing-review.md` records verified source, binary and log
identities; no production transport change or intermediary promotion is made.

The reviewed disposable browser-thread page-host prototype subsequently built
successfully (`target/continuation-page-host-build.log`), but its one native
run exited one at cycle 2/tab 101's fixture-creation/navigation wait. The
278-file `prototype_v2` manifest is
`53def35c680db3a5b44595fc6c56cf41a71f840ad5175fbe139cb4eb0f58d736`;
`target/continuation-page-host-native.*` preserves empty stdout and the sole
navigation-failure stderr. No breakpoint or host-cleanup diagnostic appeared,
but the complete host lifecycle test did not pass. This remains an isolated,
unpromoted prototype; the live `0fe3b8da…` app and 376-test gate are unchanged.
The owner-reported `about:blank` text/selection/Enter-close pass remains limited
to those observations; the additional local-fixture Ctrl K/Escape focus-return
pass covers one cycle, without a broader keyboard or crash-fix claim.
On October 11 the owner authorized committing all current work after completion
and the full gate. The latest gate above passed; this records authorization,
not a completed commit. Merges, remotes, pushes, publication and security changes
remain unauthorized. Earlier evidence follows.

## Previous combined preview checkpoint

The latest built binary is `target/local/atlas-app.exe`, SHA-256
`3d8a38e8b542609961a1949cdb9d1f0b21c8ec054cceb257194aa38cc56f15aa`.
It includes reviewed navigation-status feedback, scheme handling, popup
reactivation and the approved wry construction-ownership patch. The latest
full gate passed 369 tests and app smoke, including the previously blocked
public matcher executable. No security setting changed. The separately
preserved `atlas-app-parallel-preview.exe` is the earlier `3ca0bf0…` binary.

All 30 disposable construction-fault cases passed, but the expanded repeated
native lifecycle harness failed local-fixture navigation twice. A disposable
diagnostic copy and a separately requested pending-creation shutdown test are
being validated in the single integration execution lane. The original
breakpoint remains unresolved; neither the full gate nor these narrower
checks establish its cause.

The owner reapproved Computer Use. A fresh visible synthetic window rendered
Home and Ctrl K, but tool targeting could not enter text into the owned popup.
That process closed cleanly; the earlier owner-stopped window was untouched.
Manual typing and page-focused keyboard acceptance remain unverified. The
owner-approved stable address row is being implemented after this passing
checkpoint. The handoff records hashes, failures and source ownership.

## Preceding diagnostics checkpoint

The latest verified binary at that checkpoint was `target/local/atlas-app.exe`, SHA-256
`6edfe4113ccc7b0d5995faccaf7cb23e66541af147712443eb1b17bd9adddf1c`.
It includes the approved frizbee correction. Dependency checks, formatting,
workspace Clippy and all 347 workspace tests pass, including 153 search tests
and five public matcher regressions. None are ignored or filtered, and the
documentation-test stages pass. The original NUL regressions now succeed.

The restricted-context gate failed: its rebuilt
app's smoke run exceeds the 30-second watchdog, followed by a
`Chrome_WidgetWin_0` unregister error 1411. This failure remains recorded in
`target/continuation-frizbee-final-gate.log`; passing tests do not make that
gate successful. The unchanged complete gate subsequently passed on the normal
desktop in `target/continuation-frizbee-desktop-gate.log`, including all 341
tests, documentation-test stages and application smoke. No continuation job
remains active.

All eight available scalar/SIMD backend variants pass the independent padding
harness. The unchanged benchmark preserves all 1,040 exact results, but only
9/26 cases meet D48 (worst sample 16.6731 ms). The range index, including
retained-metadata bounds and guarded mutations, remains test-only. Production
loading and full latency acceptance remain open. `docs/p1-05-search.md` and
`docs/p1-05-frizbee-maintenance-review.md` record hashes and detailed evidence.

The owner approved the exact frizbee patch and narrow upstream comment/lint
exception, fresh synthetic computer-use testing, and Plans 2–4's 12/16/21
slice maps. The patch is applied and independently reviewed. Later-plan
predecessor exits still apply; commits, merges, publication and unrelated
patches remain unauthorized.

Earlier in this session, the boost-helper binary rendered Home and Ctrl K in
a fresh synthetic window (process 41380; TEMP profile
`AtlasApprovedWindow-d8f8830e3b6e4779806d246aa0b6da9c`). The initial tool approval
timeout recovered, but typing raised the parent instead of reaching the popup.
Navigation and remaining interactive acceptance are unverified. The synthetic
window later closed with Alt F4; its process is absent and stderr is empty.
No older or potentially owner-used process was terminated. This visual evidence
predates the patched binary and does not resolve the original WebView2 crash.
All uncommitted work and other existing windows are preserved.

The independently reviewed stage, phase-timing and restore diagnostics are now
implemented and validated. The latest normal-desktop full gate passes in
`target/continuation-range-diagnostics-gate.log`, SHA-256
`e83f60da582496bb825066c58674ad870b2c9453dde22a2dc543ed762acf91a8`.
All 347 tests pass with none ignored or filtered, including the corrected
overlong-query fixture. Documentation tests and application smoke pass; the
production binary is unchanged. No continuation process remains active.

Candidate collection dominates mixed accented-query timings. The restore
diagnostic builds once after loading and preserves complete results and
capacity recovery. These test-only measurements do not establish D48 or total
memory usage. See `docs/p1-05-search.md` for logs, hashes and limitations.
The next proposal filters ASCII fields using the ASCII portion of a Unicode
query after accounting for forced deletions. It is not implemented: subagents
hit an account usage limit before work began. Resume with focused parity tests,
paired measurements and independent review. Production range-index loading
still needs a concrete reviewed API proposal and its required authorization.

## Historical checkpoints

Latest binary: `target/local/atlas-app-digit-counts.exe`, SHA-256
`3ea8af80db81795667d32c7732bb30c2f09e6db1d9da26c360f4d221e91668da`.
Original digit counts reject impossible fields while preserving all 1,040
benchmark results. Rare-identity p95 falls to 7.3758 ms; only 9/26 cases meet
D48. The awaited fresh synthetic desktop smoke exits 0 with empty stderr.
The full gate passes dependency checks, formatting and Clippy, then records
277 passes and the same two NUL failures (141/143 search tests).
The test-only range index now has measured fallback-locality improvements,
verified query routes and a guarded in-place update that reduces eligible
large-bucket timestamp costs; production is unchanged.
Mixed timings and loading/update work keep it test-only. The original
WebView2 crash, interactive acceptance and pending approvals remain open.
The following checkpoints are historical.

The latest continuation changes test-only experiments, preserving the production
binary below. Its final gate passes dependency checks, formatting and Clippy,
then stops at the same two NUL failures after 250 passes (114/116 search tests).
Packed numeric matching remains test-only after mixed timings. The new
occupancy evidence supports a bounded partial range-index experiment, while
unique-field performance remains open. See `docs/p1-05-search.md` for results.

Latest binary: `target/local/atlas-app-highlight-traces.exe`, SHA-256
`a84d7dfc9fc834332401e31505d3eda26cad0173c86e8036b35c7d5ff074f9bf`.
It adds reviewed query-local original-highlight trace reuse, retaining range
validation on each original field. The awaited fresh synthetic desktop smoke
run exits 0 with empty stderr (`target/continuation-highlight-traces-app-smoke.*`).
That production checkpoint's full gate passes dependency checks, formatting and Clippy, then stops at
the same two NUL failures after 246 passing tests (110/112 search tests).
All 1,040 benchmark results are unchanged; 9/26 timing cases meet D48.
Numeric-span and hybrid-ranking experiments remain test-only after mixed
timings. The WebView2 crash, interactive acceptance and approvals remain open.
The following checkpoints are historical.

Latest binary: `target/local/atlas-app-group-scores.exe`, SHA-256
`03e314bd3a689d30f28a49e8df6f70b530960e4937c391aea83e7dbc636dafa7`.
It adds reviewed live-group score reuse and exact narrower recency arithmetic.
The awaited fresh synthetic desktop smoke run exits 0 with empty stderr
(`target/continuation-group-score-app-smoke.*`). The final gate passes
dependency checks, formatting and Clippy, then stops at the same two NUL
failures after 235 passing tests (99/101 search tests). All 1,040 benchmark
results are unchanged; long near-match p95 falls to 14.81 ms, but only 7/26
cases pass D48. Numeric-span matching remains test-only after mixed timings.
The original crash, interactive acceptance and pending approvals remain open.
The following binary checkpoints are historical.

Latest binary: `target/local/atlas-app-grouped-search.exe`, SHA-256
`2b0c955a07d50b291510aa295dc9c9f25e287a50482ad98182f9b9c695c6f98e`.
It adds independently reviewed bounded membership groups and query-local equivalent-score reuse to the
earlier page-close and Home fixes. Its awaited normal-desktop synthetic smoke
check exits 0 with empty stderr (`target/continuation-grouped-membership-app-smoke.*`;
build log `target/continuation-grouped-membership-app-build.log`). The complete gate
passes dependency checks, formatting and Clippy, then stops after 221 passing
tests and the same two NUL failures. Search latency improves materially for
long near matches and shared fields, but only 6/26 cases meet D48. The original crash and interactive
acceptance remain open. This checkpoint supersedes the earlier binaries below.

Current continuation binary: `target/local/atlas-app-continuation.exe`,
SHA-256 `8ba6694077c977af211b157a2d1ed347580b078379c2092aa6fc74b3aff89201`.
It includes the approved page-close patch and correlated Home-palette fix.
All 36 app tests pass; the final full gate passes dependency checks,
formatting and Clippy, then stops at the same two search failures after 189
passing tests (`target/continuation-final-gate-2.log`). A separate fresh
normal-desktop smoke run, explicitly awaited with `Start-Process -Wait`,
exits 0 with empty stderr (`target/continuation-app-smoke-waited.*`).
Build evidence is `target/continuation-app-build.log`. The first direct
PowerShell GUI invocation supplied no exit code and is not exit evidence.
Window access remains pending; the original breakpoint remains unresolved.
The older evidence below is retained and does not supersede this checkpoint.

Latest verification: `target/local/atlas-app-search.exe` includes the queued
navigation and recovery-notice fixes, the Unicode-query/ASCII-field and short
ASCII bitset search improvements, and a smoke-only independent 30-second deadline. Smoke startup
exclusively reserves a synthetic directory before that deadline is armed.
All 23 app tests, focused app/search Clippy and independent source reviews
passed. The rebuilt unsigned binary's normal-desktop `--smoke` run exited zero
with no stderr (`target/integrated-preview-search-smoke-1.*`). This is startup
and orderly-shutdown evidence, not visual acceptance, a proven fix for the
owner's breakpoint, or full browser completion. Older open windows predate it.
The corresponding logs are `integrated-preview-bitset-app-tests-1.log`,
`integrated-preview-bitset-clippy-1.log` and
`integrated-preview-build-search-1.log` under `target/`.

The owner requested the full planned browser for daily use before Monday,
2026-10-12. On 2026-10-10 they explicitly approved building this integrated
preview next (D49), replacing strict slice order for that work. The Phase 0
prototype does not contain the product libraries and is not the requested
deliverable. Product code is written to the repository rules, not copied
from that throwaway spike.

Work is on `codex/integrated-browser-preview`, based on `33dade0`, preserving
the uncommitted P1-05 search work. No commit, merge, remote or push is approved.

| Integration | Existing foundation | Reviewable behavior |
|---|---|---|
| Session worker | `syrinx-core`, `syrinx-store`, `syrinx-platform` directory guard | Tab and saved-page mutations persist before the UI treats them as saved; reopening restores the logical session; blocking storage stays off the window thread. |
| Page host | Approved wry 0.57 / WebView2 stack | A shared Evergreen environment displays real pages with navigation and page-focused shortcuts, local crash reporting and SmartScreen enabled. Page views get no custom identity, IPC or injected scripts. |
| Browser window | Exact GPUI snapshot and checked-in design direction | Vertical rows read the same model as the palette; saved destinations survive unloading; native page bounds follow resizing. |
| Ctrl K | Existing `syrinx-search` catalogue | Local results focus or open shared destinations; explicit navigation and Google web search work without live suggestions. |
| Backups and shutdown | Existing P1-04 store lifecycle | Idle daily backups use the validated seven-copy mechanism; closing drains saved mutations and shuts the store down cleanly. |

Unsigned local execution remains required (D38 clarification). The preview
uses a separate local application-data directory; tests use fresh synthetic
directories. Do not open or migrate an existing daily database while testing.
Never record browsing addresses, page titles or profile contents in logs.

This preview is not full v1. The plugin runtime, Fledge demo, Notes, Forge,
folders/import, Favorites, complete sidebar interactions and other remaining
plan requirements still need implementation and verification. D48 search
latency, D37 idle CPU, independent review and a passing complete quality gate
remain outstanding; D49 changes sequencing, not their acceptance rules.

## Current validation, 2026-10-10

The product executable compiled unsigned under `profile.local`; the first
successful build is recorded in ignored `target/integrated-preview-build-3.log`.
This is compilation evidence, not a passed integration gate. A restricted
executor smoke run returned zero but coincided with an owner-visible
`msedgewebview2.exe` breakpoint dialog (`0x80000003`); treat that run as failed.
Do not infer a signing requirement or change Windows security settings.
The same executable launched in the normal desktop context with a fresh
synthetic profile, and the GPUI home window and Ctrl K popup rendered.
The cause of the earlier runtime failure remains unproved. The computer-use
tool then failed to activate the owned popup; typing and real navigation
were not yet verified. No owner accounts or existing profiles were used.

An independent source review identified shutdown ordering, missing worker
termination handling, optimistic page-creation state and inadequate smoke
completion checks. The implementation now drains browser metadata before
closing storage, handles ended worker channels, acknowledges page creation,
reads native navigation success, and forwards structural process-failure
codes. The revised smoke check retains the page for a short health interval;
it still needs execution and does not claim every runtime child has exited.

For a manual test that must survive a restart, pass
`--test-data-dir <fresh-local-directory>`. This is an explicit test-only
override; never point it at the owner's daily database or old spike profile.
Normal launches retain the separate `%LOCALAPPDATA%\SyrinxProductPreview`
profile. Nothing is installed or registered with Windows.

The dependency-boundary check, complete workspace Clippy with warnings
denied, formatting, and all 178 unit/integration tests plus documentation
test stages passed after the integration fixes. Logs remain ignored under
`target/integrated-preview-*`. A final focused Clippy pass also covers the
review correction that queues a second navigation behind pending page
creation. These executable test passes supersede the earlier blocked test
attempts for these rebuilt files, without explaining Windows' earlier
classification. The updated application smoke run and interactive browser
verification remain required before claiming the preview ready.

The next rebuild (`target/integrated-preview-build-4.log`) reached linking
but could not replace `target/local/atlas-app.exe`: Windows returned access
denied while the earlier preview was still running. The owner has been asked
to close that window. Do not terminate a possibly used window or present the
old executable as containing the latest fixes. No commit or merge is approved.

Subsequent validation no longer needs to replace that running executable.
`cargo rustc -p syrinx-app --bin syrinx-app --profile local --offline --locked -- --emit=link=target/local/atlas-app-validation.exe`
built the same current application source to a separate ignored executable.
It remains unsigned. Running that executable with `--smoke` in the normal
desktop context returned success in approximately five seconds, including
the revised native-navigation and health-dwell checks. This is one successful
startup/shutdown check, not a diagnosis of the earlier breakpoint or proof
of interactive browsing.

The additional session-worker and native-host regressions passed: the focused
app/platform run now contains 80 passing tests (10 app, 48 platform unit,
4 backup integration and 18 storage integration), with none ignored or
filtered. They cover real guarded-store restart, saved versus live metadata,
shutdown queue draining, idle backup publication, native command order,
creation acknowledgments, bounds, URL validation and shortcut ownership.
Focused Clippy with warnings denied, formatting and diff checks also passed.
See `target/integrated-preview-regressions.log` and
`target/integrated-preview-regression-clippy.log`. The earlier 178-test
workspace run preceded these 11 added regressions; no complete 189-test
workspace gate is claimed yet.

A fresh synthetic-profile window from the validation executable is open for
interactive checks. The computer-use tool timed out waiting for app-access
approval before inspecting this window. The owner has been asked to approve
that access; do not infer success for typing, navigation or popup focus.

Further independent review found stale palette replies crossing popup
lifetimes, relative shortcuts using an outdated UI snapshot, and serious
failures being overwritten by ordinary status notices. The current source
invalidates pending replies on popup boundaries, resolves repeated close/cycle
actions on the authoritative session worker, retains serious errors and
reports failed startup as an unsuccessful exit. Arrow selection stays within
actual result rows. A queued worker regression covers consecutive cycle/close
commands, wraparound and closing the last page to Home.

All 11 application tests and focused Clippy passed after those changes
(`target/integrated-preview-shortcut-tests.log` and
`target/integrated-preview-shortcut-clippy.log`). The same separate-output
build technique produced `target/local/atlas-app-smoke.exe`; its revised
startup/shutdown check passed in the normal desktop context. These newer
changes are not in the already-open validation window. Interactive testing
and the complete gate are still outstanding.

Search failures now carry their query identity through the app-private session
event, just like successful results. The palette can replace its waiting text
with a structural error, and both global and local identity checks reject stale
successes and failures. The worker regression queues a failing query followed
by a valid one, then verifies unchanged durable state and a clean restart.
All 12 app tests passed after independent review, recorded in
`target/integrated-preview-query-errors-final-tests.log`. This corrects error
routing; it does not correct the pending frizbee NUL defect or pass the search
performance gate. The running preview binaries predate this change.

An additional fix retains the latest explicitly submitted destination while an
unloaded tab waits for native page creation. Earlier storage snapshots can no
longer replace that pending address. Intents survive delayed readiness or a
failed enqueue, remain per tab, and disappear when the logical tab closes.
Five sequence tests were added in `ui/navigation_intents.rs`, and independent
source review passed. The resulting 17-test executable was blocked before
execution by Application Control, error 4551: the original attempt, unchanged
retry and normal-desktop attempt are preserved in
`target/integrated-preview-navigation-intents-*.log`. No 17-test pass is claimed.

Focused Clippy passed in
`target/integrated-preview-navigation-intents-clippy.log`. The same current
product source built unsigned as `target/local/atlas-app-preview.exe` (build
log `target/integrated-preview-build-navigation-intents.log`), and its fresh
`--smoke` run exited successfully in the normal desktop context. The source
and smoke evidence do not replace the blocked unit-test run, interactive
approval or the full gate. No Windows security settings were changed.

After integration of the bounded search experiment, the rebuilt application
test executable ran successfully: all 17 tests passed, none ignored or
filtered (`target/integrated-preview-bounded-search-tests.log`). This verifies
the five pending-navigation regressions as well as the session tests; it
supersedes the execution block for this new executable without explaining
the earlier Application Control refusal. The separate search suite still
has two failing NUL-highlight tests, and interactive acceptance remains open.

The latest unsigned `atlas-app-preview.exe` includes that bounded-search
source and passed another fresh normal-desktop `--smoke` run (exit zero).
Its build is recorded in `target/integrated-preview-build-bounded-search.log`.
A normal interactive launch with a separate fresh synthetic profile produced
process 36164, which reports responding. No visual inspection has occurred;
neither this observation nor the smoke pass proves the earlier crash fixed.

## Queued navigation correction under verification

An address submitted immediately after a queued selection could previously
navigate the tab from the UI's older saved snapshot. `NavigateActive` now
resolves the target on the session worker after earlier selection, new-tab
and close commands. Its snapshot acknowledges the destination only after
storage succeeds; the UI then sends native navigation or retains the
destination until page creation. Repeated submissions of the same address
still trigger navigation, and restart never replays the acknowledgment.

Two real-worker regressions cover those queue sequences, saved-destination
preservation and restart. Independent source review and all 19 application
tests passed. In the same run, search passed 47 of 49 tests, failing only
the two existing NUL-highlight regressions. Focused Clippy and formatting
also passed. Logs are
`target/integrated-preview-field-bounds-navigation-tests.log` and
`target/integrated-preview-field-bounds-navigation-clippy.log`. The already
launched preview predates this correction.

The corrected source built unsigned to `target/local/atlas-app-navigation.exe`
and passed a fresh normal-desktop `--smoke` run with exit zero. Its build log
is `target/integrated-preview-build-queued-navigation.log`. The existing owner
window was left intact; this newer binary has not been visually inspected.

The separate page-requested-close mismatch is corrected through the approved
patch in `docs/page-close-event-review.md`. A close request now waits for
the session's durable acknowledgement before native teardown. All 99 app and
platform tests and an actual loopback native close fixture passed.
The rebuilt standard `target/local/atlas-app.exe` passed a fresh desktop
smoke run. This supersedes older binaries for this fix; it is not presented
as the breakpoint's proven cause or interactive acceptance. The report
records the exact binary hash, evidence and remaining gate failures.

A delayed new-Home snapshot could reopen the palette after an earlier
submission. The continuation corrects this with app-private request IDs:
New Tab, saved-page open and reopen carry their own Home-entry acknowledgement
through the session worker. The UI opens only for the current request's
committed new Home. Submission/dismissal and superseding navigation, focus or
close actions cancel intent. Matching no-op/non-Home acknowledgements consume
it; failed enqueue preserves previously accepted intent. Startup/restart never
replays it. A simple pending boolean was rejected in independent review because
a later real-page open could rearm it for an older Home snapshot.

The new `ui/home_palette.rs` has six ordering regressions, and the real worker
test covers acknowledgement identity after new/reopen/saved actions and restart.
Independent review passes, and all 36 app tests pass in
`target/continuation-final-gate-2.log`, with no ignored or filtered tests.
The change remains unverified in an interactive window; window access is pending.

The separate recovery-notice bug is corrected in source: the first received
session state now identifies startup regardless of its persisted revision.
Empty and clean initial states still consume that first-state marker, so a
later mutation cannot masquerade as recovery. Independent review, focused
Clippy and all 19 application tests passed. Logs use
`target/integrated-preview-recovery-notice-tests.log` and
`target/integrated-preview-recovery-notice-clippy.log`. The latest executable
and its smoke result predate this small UI-source correction; visual
verification remains pending.
