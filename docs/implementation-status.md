# Atlas implementation and evidence ledger

This ledger prevents tested foundations from being mistaken for a finished browser. It records implementation evidence against all four approved plans; it changes no requirement, decision or approval. Update it when a slice gains code, verification or an approved merge. The source of authority remains `docs/decisions.md`, the spec, the design and the active plans, in the order defined by `AGENTS.md`.

## Current checkpoint — 2026-10-11

The atomic startup and retained-handle correction now pass the complete
normal-desktop gate: 436 tests, dependency boundaries, formatting, Clippy,
documentation tests and synthetic application smoke. Evidence is
`target/continuation-atomic-startup-desktop-gate.log`, SHA-256
`5654ffa30e3f5af2b8711410ea862db2d5dcb8c42b474450b294048f49ccad79`.
The current `target/local/atlas-app.exe` SHA-256 is
`252044439eb668680360e28009a3245e2992a754cb22d82b2dae8b6b8839982d`.
This supersedes the pending-validation checkpoints below. The restricted-context
gate passed all tests but timed out in smoke with watchdog code 124; its log
remains `target/continuation-atomic-startup-full-gate.log`. The unchanged binary
then passed normal-desktop smoke and the complete normal-desktop gate. This
does not establish the cause or resolution of the original WebView2 crash.
Native typing/focus acceptance remains open. No commit or merge was made.

The approved atomic startup implementation is now frozen after independent
review. Its first platform run exposed two genuine rename-protection failures:
metadata-only handles did not enforce the intended sharing denial. The private
guarded-open correction preserves metadata-only control/sidecar inspection,
keeps both failing assertions and adds three compatibility regressions.
All 60 focused platform storage tests now pass in
`target/continuation-atomic-startup-guarded-platform-tests.log`; the initial
55-pass/two-failure evidence remains in
`target/continuation-atomic-startup-platform-tests.log`. Worker and full-gate
validation are pending. This supersedes the incomplete-source notes below,
but does not replace the last verified application binary yet.

The owner approved the three Windows binding features in
`docs/missing-primary-startup-review.md`; the usability chat has resumed
the missing-primary startup implementation and its remaining regressions.
Its retained recovery and session paths have independent source review, but
the fresh-start branch deliberately fails closed pending atomic creation.
The 13 added regressions have not been executed. This partial source state
is not covered by the 411-test gate below and is not a usable replacement
for the last verified binary. Only the exact three-feature amendment is now
authorized; no new build or native run has been started for this partial work.

The palette now forwards the 11 existing browser shortcuts without ending its
event stream. Independent review caught and corrected a queued Ctrl K/L then
Enter/Escape race: obsolete popup refocus no longer discards the retained
identity needed by the completion event. The corrected full gate passes all
411 tests (78 app tests), dependency checks, formatting, Clippy and synthetic
smoke in `target/continuation-palette-refocus-gate.log`, SHA-256
`10ddb5b7f6b9167614d710f049305859c52499855c4eb993cfe766403e945742`.
Current `target/local/atlas-app.exe` SHA-256 is
`4c437c976290615a00ee06287b5338b94b26267e1546002d8b307674d55cdfde`.
Native caret/focus verification and the original crash remain unresolved.
The preceding 409-test shortcut gate remains evidence of a test-coverage gap,
not acceptance of its known race. No Cargo/native execution remains active.

The latest shutdown-action correction retires the palette and rejects new user
commands once closing begins, while preserving final browser metadata and
ordered worker shutdown. Independent source review passes. The full gate passes
407 tests (75 application tests), dependency checks, formatting, Clippy and
synthetic app smoke in `target/continuation-shutdown-actions-gate.log`, SHA-256
`a9a3838e16c4916f394b7aded39daee7401841ba1c82ce39aefe641813ad9e8b`.
The latest verified `target/local/atlas-app.exe` SHA-256 is
`2e3bc975b0b1ea894c12a4c94f5171401635324712b1e0d378f72cf558b47b9e`.
Visual acceptance remains open. The Unicode-selected search experiment also
passes its two focused tests in `target/continuation-unicode-selected-triplets.log`;
the later unchanged 26-case comparison also preserves all 1,040 exact results.
Independent timing review rejects promotion: the paired diagnostic baseline
passes 8/26 case thresholds and the candidate 5/26, with inconsistent accented
query results and regressions among equivalent ASCII controls. These are
diagnostic results, distinct from the earlier production 10/26 measurement.
Production search is unchanged; see `docs/p1-05-search.md` for full evidence.

The owner requested browser-first work on October 11: make browsing usable and
reliable before returning to plugins, Fledge, Notes or Forge. The remaining
plans stay approved and deferred; their requirements and approval limits have
not changed. The preceding integrated checkpoint is committed locally as
`8239511`, with no merge or push. New investigation and corrections follow it.

The continuation uses three coordinated chats for crash diagnostics,
navigation reliability and browser usability, with independent subagent review.
The integration chat alone runs Cargo, native harnesses and window checks.
The independent slice audit corrects the earlier informal 20%-of-all-v1
estimate: roughly 15–25% of the browser plan has implementation, with only
four of forty slices fully accepted and merged. Partial-credit arithmetic is
not effort weighting or an acceptance result. See the current
`docs/parallel-browser-usability-review-2026-10-10.md` assessment.

The first correction after that checkpoint fixes the synthetic close-page
server's inherited nonblocking socket mode. The explicit example regression
passes one test; the built harness then passes one normal-desktop run with all
12 lifecycle cycles and multi-page shutdown. Evidence is
`target/continuation-blocking-fixture-test.log`,
`target/continuation-blocking-fixture-build.log` and
`target/continuation-blocking-fixture-native.*`; harness SHA-256 is
`3a8fc36f2723bba0d66a05a31bfcb35d14337eee5873b77c501b7a7966c9f651`.
This confirms the socket defect and a corrected fixture run, not the cause of
every older navigation failure or the original breakpoint. The send-error
reporting and open-palette refresh corrections are independently reviewed.
The first full-gate attempt stopped on formatting; that failure remains in
`target/continuation-browser-first-gate.log`. After `cargo fmt --all`, the
repeat full gate passes all 382 tests and application smoke, with no ignored
tests, in `target/continuation-browser-first-formatted-gate.log` (SHA-256
`5cc1618aed8eda7ece671bceeb3d4f465ff4c2996e6b150f139a57d2b0dcae68`).
The rebuilt application SHA-256 is
`0da392f0800e5e3b739bfeabe8122903111d38301e41a6d610168e0b882debd3`.

The integration agent observed another concrete UI defect on the unchanged
synthetic `0fe3b8da…` build: Ctrl T created a second Home tab and opened the
palette; Ctrl W delivered to the main window closed that tab while the open
palette retained two rows and its old count across later snapshots. The
main window was deliberately activated by the input tool, so this is not a
page-focused or palette-focused shortcut test. Maximizing kept the owned
palette visible, and clicking a result dismissed it. This observation directs
the palette refresh correction; it does not establish full resize/focus
acceptance. The rebuilt app launched with a fresh synthetic profile, recorded
in `target/continuation-browser-first-visible.json`, but Computer Use returned
`failed to activate captured window` on both initial activation and the allowed
selection-refresh retry. UI automation stopped. This is no new visual pass
and does not establish an application failure; before/after verification is
still required.

The next independently reviewed increment adds explicit failed-start backup
confirmation and makes smoke watchdog termination independent of stderr.
All 70 application tests pass in
`target/continuation-recovery-watchdog-tests.log`, including cancellation,
stale confirmation, one-time restore and restart, and both watchdog exits
while stderr is locked. Missing-primary preflight and visual recovery
acceptance remain open. The complete gate then passes all 400 tests, dependency
checks, formatting, Clippy and fresh synthetic app smoke in
`target/continuation-recovery-watchdog-full-gate.log` (SHA-256
`afa83cd812bae5dda7be9f1f5f68f18d9003658ed9f0ef72b40da25ead10be2e`).
The latest verified `target/local/atlas-app.exe` SHA-256 is
`9b2fded8f7dc79d436ddbfe15af63133a1be005645c188c98750f17fcfb36119`.
The cached-first experiment passes correctness but remains test-only because
paired measurements show regressions in long near-match and mixed no-match.

The isolated process-role diagnostic has not run: its original build and one
unchanged compiler-trace attempt both exit 101 while loading `gpui_macros`.
Evidence remains in `target/continuation-process-role-build.log` and
`target/continuation-process-role-build-trace.log`. The trace's ordinary-library
metadata rejection precedes a normal procedural-macro fallback; it is not by
itself the cause. No dependency, cache or security setting was changed.

A single explicit-import compiler probe then exposed the hidden loader error:
Windows Application Control blocks that exact diagnostic macro DLL with error
4551. `target/macro-load-probe/result.json` records its unchanged SHA-256 and
exit 1; `stderr.log` preserves the compiler diagnostic. The compiler reports
five internal loader retries, not five agent compiler invocations. No Atlas
application ran. The diagnostic build path stops here without a security,
signing or cache workaround. This explains that build failure, not the original
WebView2 breakpoint.

### Preceding committed checkpoint

The owner-approved stable visible “Go to …” row is implemented and reviewed;
address selection survives asynchronous local results. The full gate
`target/continuation-socket-close-commit-gate.log` exited zero: all 376 tests pass (54 app,
164 search unit tests and five public matcher regressions among them), with
none ignored or filtered. Dependency checks, formatting, Clippy, documentation
tests and app smoke pass. Gate SHA-256:
`ca28dc0a417a3e00929ece8c652c21dc8b7b2e58da6acd820fec72af0a03ca62`.
Current `target/local/atlas-app.exe` SHA-256:
`0fe3b8daad72ddf3383f025756a5174b39f098d2fa7355d81bfd45eaec89c858`.
On October 11 the owner reported all three requested manual checks passed on
this synthetic build: `about:blank` text appeared, its Go to row stayed selected
after two seconds, and Enter closed the palette. On the same live `0fe3b8da…`
build, the owner also opened the local fixture, typed `before` in its Typing
area, used page-focused Ctrl K, typed `about:blank`, pressed Escape, and typed
`after` without another page click. Their “Yes, beforeafter appeared” confirms
page typing, the shortcut, palette typing and Escape focus return for one
cycle. Repeated reliability, Enter focus return in other destinations and the
original breakpoint remain open. No focus fix was applied; the source ordering
concern was not reproduced by this check.
The earlier lock-screen capture and black captures/activation failures remain
historical evidence, not an Atlas black-window diagnosis. On October 11 the
owner explicitly authorized continuation, confirmed Windows unlocked and Atlas
visible/responsive. The integration agent observed Home and the palette in
synthetic `atlas-app-focus-20261010` process 46216, window 6097856. Clicking edit
3 showed a caret/popup focus; setting its value failed because the element was
unavailable in cached app state, and the refreshed list exposed only the main
window. Manual `about:blank` entry, a two-second wait and Enter were requested
to verify typing, stable Go to selection and palette closure. That initially
pending check is now superseded by the owner's “All three worked” confirmation.
Unlock is no longer required. This tool limitation does not prove an Atlas
typing failure. The source focus-ordering concern has no new fix applied.

Disposable pending-creation `controller` and `completed` cases exit zero;
the original `environment` case exits one before a successful native-return
or queued-shutdown marker, so it does not prove a shutdown defect. The diagnostic
build subsequently passed. Its single environment run exited one after a
successful native callback with an object and successful native return:
`pending_rejected=completion_already_entered` identifies an early callback that
missed the intended pending-state coverage. Initialization succeeded; pending
environment shutdown remains untested, with no shutdown defect established.
The logs are `target/continuation-pending-shutdown-diagnostic-build.log` and
`target/continuation-pending-shutdown-environment-diagnostic.*`. Separately,
the first native-status overlay fails cycle 4, tab 103, with unsuccessful
navigation and UNKNOWN native error status 0. Its reviewed v2 adds HTTP status
and close/completion entry ordering in a disposable copy. V2 built successfully
(`target/continuation-native-ordering-build.log`), but its single native run
exited one (`target/continuation-native-ordering.result.json` and matching logs).
Its sole native callback records sequence 1, tab 1, navigation 2, unsuccessful
completion, UNKNOWN error 0 and HTTP status 0; the fixture wait then fails.
No close-request callback was recorded. This does not identify a transport
failure, close-timer race or original breakpoint cause. Diagnostic jobs are
terminal; all earlier failures remain preserved.

The reviewed v3 instrumented native harness subsequently passes its first run
and all three bounded fresh-process repeats, with 12 lifecycle cycles and
multi-page shutdown per run. Each has 31 successful paired navigation starts/
completions, 13 fixture responses, 13 close callbacks and no server error.
The four result records under `target/continuation-native-correlation.*` and
`target/continuation-native-correlation-repeat-{1,2,3}.*` share diagnostic
binary SHA-256 `942fc597df9cbef496a570d09658448aad1e5b8c62fc6aae92ee1921bb932fea`.
All four processes are terminal. These are successful instrumented synthetic
runs, not an explanation of prior failures, original-breakpoint resolution or
visual app acceptance. The handoff records server totals and preserved logs.

The fixed v2/v4 three-pair comparison then records v2 exit one in all three
runs (initial tab 1, cycle 5/tab 104, cycle 8/tab 107), versus v4 exit zero in
all three complete 12-cycle/multi-page-shutdown runs. V4 changes only server
instrumentation relative to frozen v2. The third v2 failure reports native
error 9 (`CONNECTION_ABORTED` in the pinned bindings) and HTTP 0; the first two
report UNKNOWN 0 and HTTP 0. Preserve all six records under
`target/continuation-native-server-pair{1,2,3}-{v2,v4}.*`; the handoff records
verified binary hashes and per-run counts. This small fixed sample supports
investigating a server-work/timing effect, not a particular transport cause,
reliability claim or original-breakpoint fix. No assertions or product behavior
changed, and all six processes are terminal.

The subsequent authorized socket-lifetime comparison completed its fixed
A/B/B/A order (v4/v5/v5/v4), all four exit zero. Every run passes the unchanged
12-cycle/multi-page-shutdown harness, with 31 successful native completions
(13 HTTP 200, 18 HTTP 0), 13 close callbacks and shared sequence numbers 1–44.
Each server reports 13 headers/13 writes, zero empty/partial EOFs and no error;
accepted connections/empty timeouts are 39/26, 35/22, 38/25 and 35/22.
B differs only by dropping the stream before the existing final response log.
Retention through that log was not necessary in these runs, without identifying
a TCP mechanism, explaining prior failures or resolving the original crash.
`docs/fixture-transport-timing-review.md` records verified executable/source
hashes and the complete `target/continuation-socket-close-*` results/logs.
The series JSON SHA-256 is
`b40081339240a797f4c12720b2bc4460ec487bd270011da0f771cec4008a7c21`.
The experiment remains disposable; no product change or prototype promotion follows.

The isolated browser-thread page-host `prototype_v2` then built successfully
in `target/continuation-page-host-build.log`, after reviewed 278-file manifest
verification (SHA-256 `53def35c680db3a5b44595fc6c56cf41a71f840ad5175fbe139cb4eb0f58d736`).
Its single native run exited one at cycle 2/tab 101's fixture-navigation wait,
with empty stdout and only that failure in stderr; preserve
`target/continuation-page-host-native.*`. No original breakpoint or host-cleanup
diagnostic was observed, which does not establish a full lifecycle pass.
The prototype is unpromoted and isolated. The live `0fe3b8da…` binary and
376-test product gate are unchanged. The owner-reported `about:blank` text,
stable selection/Enter-close pass and one local-fixture Ctrl K/Escape focus-return
pass remain separate from broader keyboard acceptance.
On October 11 the owner authorized committing all current work after completion
and the full gate. The gate above passed; no merge, remote, push, publication or
security change is authorized. This records authorization, not a completed commit.
Earlier gate and binary records below are historical.

### Previous crash and usability checkpoint

The latest full gate now PASSES all 369 tests and app smoke, including the
navigation-status correction and test-only bound-reuse equivalence tests.
`target/continuation-crash-usability-gate.log` has SHA-256
`88b153e055d1e64b48feb2b18ff6b34199ebeb650e64cb22600d33e9e288b39d`;
current `target/local/atlas-app.exe` has SHA-256
`3d8a38e8b542609961a1949cdb9d1f0b21c8ec054cceb257194aa38cc56f15aa`.
No security setting changed. Separate expanded native stress runs failed
navigation during repeated local-fixture creation; diagnosis and the original
breakpoint remain open. The owner-approved stable address row is being built
after this checkpoint and is not yet validated. See the current handoff.

The disposable v5 failed-construction harness now passes all 30 synthetic
cases, with matching exit status, PASS markers and expected cleanup diagnostics.
Its per-executable Common Controls v6 manifest corrects the v4 harness loader
failure; it does not change Atlas's release configuration. This is partial
runtime evidence for the approved wry ownership patch, not a diagnosis of the
original crash or a passing full gate. The aggregate is
`target/continuation-wry-fault-v5-results.json`, SHA-256
`384902be7ab55637cc496b05d89028a557547e4bd7269b63af7bd7d2a10c72a0`.
The handoff records source identities and remaining coverage limitations.

### Historical combined browser checkpoint after parallel work

The current binary is `target/local/atlas-app.exe` (also preserved as
`target/local/atlas-app-parallel-preview.exe`), SHA-256
`3ca0bf045fc0826e7300a647307845733fb5249b085f52feeadbebf0abd4c276`.
It includes reviewed popup reactivation, navigation scheme handling, narrowed
ASCII projection and the newly owner-approved exact wry construction patch.
The patch was approved in its investigation chat and is recorded in
`AGENTS.md`; applied bytes and provenance passed independent review.

The latest full gate `target/continuation-wry-construction-gate.log`
(SHA-256 `7cfd8360dbaf2ca898d53bed45afc6c983fb6641c31b56e8d491f5a1801433b9`)
passes dependency checks, formatting, Clippy and 301 tests, including all
41 app and 160 search unit tests. It then fails because Windows blocks
`frizbee_padding-42be448c0cf9ca7a.exe` with error 4551 before execution.
Earlier combined-gate attempts hit the same block. Later gate stages are
unreached; the full gate has NOT passed. No security setting was changed.

Separate rebuilt-app smoke and patched native lifecycle runs both exit zero:
`target/continuation-wry-construction-app-smoke.log` (SHA-256
`0211a1fc9cfb54c33703ec847ab7e29696673007d69d976c6b64f23ee2cd3b05`)
and `target/continuation-wry-construction-native.log` (SHA-256
`e6e205ae6af103bf2d28af87d058256ecca3cbfa4d0b751e5f8c774d5b5c5618`).
They do not diagnose the original WebView2 crash. Disposable construction
fault-injection validation is being prepared by the parallel investigation
chat; hooks must remain outside the approved vendor source.

The narrowed projection diagnostic passes and independent evidence review
confirms all 1,040 production benchmark samples preserve their exact results.
The latest benchmark passes 10/26 D48 cases (maximum p95 13.9768 ms, worst
sample 17.7388 ms); search acceptance remains open. See
`docs/p1-05-search.md` and the narrowed-profile/benchmark logs under `target/`.

A fresh synthetic window was launched from `atlas-app-parallel-preview.exe`,
process 22080, TEMP profile
`AtlasParallelPreview-e88536c4ea344dc7ac2af6e7cbe298f5`.
The owner stopped Computer Use with physical Escape during its first
inspection. No further Computer Use actions were issued in that turn.
The window was left open; do not terminate it or assume interactive acceptance.
The popup reactivation change remains visually unverified. No Cargo job remains
active at this checkpoint; the parallel validation-overlay preparation may
still be active. Preserve uncommitted work, existing windows and all approvals.

### Earlier evidence

The latest verified binary is `target/local/atlas-app.exe`, SHA-256
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

### Earlier in-place mutation checkpoint

The current binary is `target/local/atlas-app-digit-counts.exe`; its awaited
fresh synthetic desktop smoke exits 0 with empty stderr. Original digit-count
rejection preserves all 1,040 benchmark results and reduces rare-identity p95
from 36.2976 to 7.3758 ms. Only 9/26 cases meet D48, so acceptance remains open.
The full gate passes dependency boundaries, formatting and workspace Clippy,
then records 277 passes and the same two NUL failures (141/143 search tests).

The independently reviewed test-only range index now uses cached fallback
positions, improving multiple mixed-data cases with verified route counters.
Guarded in-place timestamp updates now reduce eligible 100,000-row mutation
medians from 1,380.850 to 2.350 microseconds. Other mutation and query costs
remain open. `docs/p1-05-search.md` records all evidence and the next unmeasured
position-free range-bound proposal. Review bulk loading and meet complete-query
acceptance before promotion. The WebView2 crash and all pending
approvals remain unresolved.
Uncommitted work is preserved and no continuation job remains active.

### Earlier experiment checkpoints

The latest test-only continuation adds packed numeric transitions and an index
occupancy diagnostic. The final gate passes dependency checks, formatting and
workspace Clippy, then records 250 passes and the same two NUL failures
(114/116 search tests); later stages remain unreached. Both isolated diagnostics
pass. Packed numeric timings are mixed, so production remains unchanged.
Occupancy supports a bounded partial range-index experiment, not an all-rows
index: entirely unique data groups only 1,024/100,000 rows. The proposed index
still needs memory and mutation measurements; unique-field scanning remains
open. The score-bound precomputation experiment was rejected and removed.
See `docs/p1-05-search.md` and the handoff for evidence and limits. No job remains
active; pending approvals and the WebView2 crash are unchanged.

The current binary is `target/local/atlas-app-highlight-traces.exe`; its awaited
fresh synthetic desktop smoke run exits 0 with empty stderr. Original highlight
traces are reused within eligible queries, with range validation against every
original field. All 1,040 benchmark results are unchanged. Nine of 26 cases meet
D48, so acceptance remains open; long near-match p95 is 14.5201 ms. The full
gate at that preceding production checkpoint passes dependency checks, formatting and workspace Clippy, then records
246 passing tests and the same two NUL failures (110/112 search tests).

The revised numeric-span and bounded direct-ranking experiments remain
test-only after mixed timings. The partial grouped-count/range-ranking design
needs actual index memory and mutation measurements using the recorded
occupancy and provisional limits. See the latest handoff and
`docs/p1-05-search.md` for hashes, evidence and limitations. The WebView2
breakpoint, interactive acceptance and pending approvals remain unresolved.
All uncommitted work is preserved and no continuation job remains active.
Earlier checkpoints follow.

The current binary is `target/local/atlas-app-group-scores.exe`; its awaited
fresh synthetic desktop smoke run exits 0 with empty stderr. Live group IDs
reuse original field scores within each query, and recency arithmetic keeps
the same exact result with a narrower common path. The unchanged benchmark
preserves all 1,040 sample results and improves long near-match p95 from
30.78 to 14.81 ms. D48 still fails: only 7/26 cases pass. The full gate passes
dependency checks, formatting and workspace Clippy, then records 235 passing
tests and the same two NUL failures (99/101 search tests).

The numeric-span prototype passed its row comparisons but remains test-only
after mixed timings and a long-query regression. Bounded direct ranking and
shared highlight tracing are reviewed future experiments. See the latest
handoff and `docs/p1-05-search.md` for hashes, logs and proofs. The WebView2
breakpoint and pending approvals remain unresolved. The uncommitted tree is
preserved, and no continuation job remains active. Earlier checkpoints follow.

The latest continuation preserves the whole uncommitted tree and builds
`target/local/atlas-app-grouped-search.exe`. Its awaited fresh synthetic
normal-desktop smoke check exits 0. All 36 app tests pass. Bounded membership
groups and query-local score reuse reduce unchanged benchmark long near-match p95
from 367.56 to 30.78 ms and preserve all 1,040 sample results, but D48 still
passes only 6/26 cases. A separate positional index remains test-only after
mixed performance and a measured 46.7 MiB memory cost. The complete gate
passes dependency boundaries, formatting and workspace Clippy, then records
221 passing tests and the same two NUL-highlight failures (85/87 search).
See `docs/handoff-integrated-preview-2026-10-10.md` for the binary hash and
logs. Frizbee-patch, window/integration access and later slice-map approvals
remain pending; the original WebView2 breakpoint remains unresolved. No
continuation jobs remain active. The following checkpoints are historical.

The owner-approved page-close patch is now implemented and independently
reviewed. All 99 app/platform tests pass, as do dependency boundaries,
workspace Clippy and scoped formatting. A synthetic loopback page proves
actual native close delivery, deferred destruction and safe host shutdown.
The rebuilt standard `target/local/atlas-app.exe` passed a fresh desktop
smoke check. See `docs/page-close-event-review.md` for its binary hash and
logs. A final workspace formatting check found a concurrent change in the
unrelated search profiler; it was left untouched. Search correctness and
performance, interactive acceptance and the original breakpoint cause remain
open. No commit, merge or upstream publication has been made.

Latest evidence supersedes the historical checkpoints below: the final
watchdog/profile-reservation changes pass all 23 app tests, focused Clippy
and independent review. `target/local/atlas-app-search.exe` passed a fresh
normal-desktop smoke run with exit zero. Interactive acceptance and the
breakpoint diagnosis remain open. Unicode-query matching against cached ASCII
fields and short-ASCII bitsets improve relevant benchmark cases while preserving
all 1,040 results. The full search suite passes 53/55 tests with the same two NUL
failures, and only 1/26 timing cases passes D48 (worst 382.4652 ms). No test,
build, benchmark or smoke job is still running. See the detailed preview and
search reports; no full gate, slice completion, commit or merge is claimed.

- All four plans and Plan 1's revised 40-slice map are approved. The full four-plan goal remains unfinished. D49 explicitly brings the integrated browser preview forward while preserving unfinished features and performance gates.
- Current branch: `codex/integrated-browser-preview`, checkpoint base `33dade0`. P1-01 through P1-04 were independently reviewed and owner-approved for merge; product commits are `ed19d6d`, `892d3bc`, `c4a6631` and `7710c06`. On October 11 the owner authorized committing all current work after completion and the full gate. P1-05 and the preview still lack merge approval. Check Git for the resulting commit state; no remote or push is authorized.
- `target/integrated-preview-tests.log` records **178 passing tests**, zero ignored/filtered cases, and completed documentation-test stages: app 7, core 30, platform 62, search 26, store/engine 39 and xtask 14. Formatting, dependency boundaries and workspace Clippy passed. A subsequent focused Clippy pass covers the pending-page navigation correction. These results do not constitute the final application smoke/interactive gate.
- The unsigned executable rendered Home and Ctrl K in the normal desktop context. Typing, real page navigation, page-focused shortcuts, save/unload/restart and final shutdown are **not yet verified**. An earlier restricted-context run produced a WebView2 breakpoint dialog; its cause remains unproved. Do not count its zero process exit as success.
- At this check, old preview process **37612** is still running. `target/integrated-preview-build-4.log` shows Windows refusing to replace its open executable. The owner has been asked to close it. Recheck the PID before acting; do not kill a possibly used window or test the old binary as if it contained the latest fixes.
- A later build emits the current source to `target/local/atlas-app-validation.exe`, leaving that old executable alone. Its revised `--smoke` check passed in the normal desktop context. Eleven new regressions also passed in an 80-test app/platform run, with focused Clippy and formatting passing. A fresh validation window is open, but interactive inspection awaits computer-use app-access approval. This does not resolve the earlier crash's cause or complete the GUI gate; see the updated preview report.
- The dependency additions have a completed audit and exact licence approval in `docs/integrated-preview-dependencies.md`. Product metadata contains no registry package/version absent from that proposal. No GPUI idle patch or passing D48 search benchmark exists.
- A further review fixed palette reply invalidation, queued relative shortcuts, persistent failure reporting and result selection. The app now has 11 passing tests, focused Clippy passes, and a rebuilt `atlas-app-smoke.exe` passed startup/shutdown. The existing open validation window predates these changes; neither interactive acceptance nor the full latest workspace gate is claimed.
- The later P1-05 ASCII-presence experiment has **34 passing search tests and two failing differential tests**. Both the filtered and exhaustive paths fail for a synthetic NUL query because frizbee returns out-of-range highlights. Its diagnostic benchmark preserves all 1,040 sample results but passes only 2/26 latency cases, with a 378.0657 ms worst sample. These current failures supersede earlier search correctness passes for the changed working tree; no completed performance fix is claimed.
- Query errors now use request-scoped replies and reach the palette without letting stale failures affect current state. All 12 app tests passed after independent review (`target/integrated-preview-query-errors-final-tests.log`). The narrow frizbee source correction in `docs/p1-05-frizbee-maintenance-review.md` awaits owner approval; it has not been applied.
- Unloaded tabs now retain the latest explicit address across stale snapshots; independent review and Clippy passed. Windows initially blocked the 17-test executable, but rebuilding after the bounded search integration produced a successful run of all 17 tests (`target/integrated-preview-bounded-search-tests.log`). The older unsigned `atlas-app-preview.exe` passed its fresh-profile smoke check. Interactive validation remains pending.
- Exact zero/one-deletion membership and conservative bounded ranking are now integrated experimentally. The full search suite passed 45 of 47 tests; the same two NUL-highlight regressions fail. New bound, saturation, scope/count and membership regressions passed, as did independent review and focused Clippy. Windows blocked the rebuilt benchmark executable in both execution contexts, so there is no timing result for this source. `docs/p1-05-search.md` preserves the evidence and unresolved D48 gate.
- The newest `atlas-app-preview.exe` now includes bounded search and passed a fresh normal-desktop startup/shutdown check. It was then launched for the owner in a new synthetic profile as PID 36164. The process responds, but visible-window and interactive behavior remain unverified; do not replace or terminate a possibly used window. Build evidence: `target/integrated-preview-build-bounded-search.log`.
- Subsequent source now resolves navigation after queued tab selection on the session worker and tightens search bounds using cached field metadata. All 19 app tests pass; 47 of 49 search tests pass, with the same two NUL failures. Independent reviews, focused Clippy and formatting passed. The open preview predates these changes. The new benchmark finished: all 1,040 results equal the baseline, but only 1/26 timing cases passes, with a 382.8664 ms worst sample. See `target/integrated-preview-field-bounds-navigation-tests.log` and the two detailed reports.
- That source built unsigned as `target/local/atlas-app-navigation.exe` and passed a fresh normal-desktop startup/shutdown check. Interactive acceptance remains pending. The separate page-close correction subsequently received explicit API/vendor approval and is implemented and verified as recorded at the top of this ledger.

The later recovery-notice correction passed all 19 app tests, focused Clippy
and independent review. The binary above predates it. A private synthetic
search-stage diagnostic also passed exhaustive-result comparison and Clippy;
it attributes most short/Unicode query time to all-scope membership. This
adds one passing test, not a rerun of the full suite or a D48 pass. See
`target/p1-05-stage-profile-1.log` and `docs/p1-05-search.md`.
The subsequent substring experiment preserved all 1,040 benchmark results
but still passed only 1/26 timing cases and regressed several cases. It was
removed. The restored production path passes 49/51 search tests, with only
the same two NUL-highlight failures; no tests are ignored or filtered.
The new stage profiler and semantic regression remain. No browser crash fix
or product completion follows from this search work.

The current runtime evidence is in `docs/integrated-browser-preview.md`; the historical failed search measurements remain in `docs/p1-05-search.md`. Older blocked-test statements in earlier checkpoints describe earlier executables: the 178-test result supersedes those execution blocks for this build, not the outstanding performance or GUI checks.

## Reading the status

**Merged/tested** means the named library slice passed its recorded gate and owner-approved merge. **Partial** means some product source or tests exist, with the listed requirements still open. **Missing** means the required implementation/evidence is absent; a dependency offering an API is not delivery. A rendered window is evidence only for what was actually observed.

## Plan 1 — engine, browser core and palette

The complete requirements and blocking edges remain in `docs/plans/Atlas v1 — Plan.md` and its matching readiness-report map. Every approved ID is accounted for below.

| Phase / slice | Status and existing evidence | Remaining exit work |
| --- | --- | --- |
| Phase 0 | Conditional GPUI go; `docs/research/09-phase-0-verdict.md`. Passkey/Workspace, YouTube and Disney+ passed in throwaway spikes. | D37 idle condition and remaining owner site checks still apply to the product. Spike results do not certify this implementation. |
| 1 / P1-01 scaffold | **Merged/tested.** Cargo workspace, `crates/xtask`, dependency rules, local gate and development guide. | Preserve the same checks as crates are added. |
| 1 / P1-02 tab model | **Merged/tested.** `atlas-core`: checked identities, saved/live association, ordering, selection and 20-entry web closure stack. | Later UI/folders must preserve D42/D43; plugin kinds do not implement a plugin runtime. |
| 1 / P1-03 storage | **Merged/tested.** `atlas-store` and native directory lease: serialized writes, migration, snapshots, revision checks, identity/path protection and long-path coverage. | Later schemas/features need their own persistence and recovery tests. |
| 1 / P1-04 backup/recovery | **Merged/tested.** Clean/dirty lifecycle, daily idle snapshot mechanism, seven-copy rotation, validated owner-confirmed/resumable recovery and pure deletion-record contract. | Preview scheduling/shutdown needs real integration verification. No recovery confirmation UI exists yet. Actual history-deletion persistence/replay belongs to P1-27; schema 1 refuses unsupported deletion state. |
| 1 / P1-05 tier 1 search | **Partial.** `atlas-search` correctness tests and reproducible 100,000-row complete-query benchmark exist. | D48 fails: the latest narrowed-projection run passes 10/26 cases. Profile and improve without changing the corpus, `safe_read`, counts/highlights or 5 ms p95 limit; then obtain three passing fresh-process runs. |
| 1 / P1-06 GPUI idle | **Partial, on branch `claude/p1-06-gpui-idle-v2` (not merged).** Approved demand-driven repaint patch vendored as `vendor/gpui-pre-windows-0.3.8` with provenance and upstream-ready diff. Idle `WM_PAINT` 59/s to 0/s; idle CPU 1.66–2.00% to 0.23–0.46% of one core (harness and fresh-profile `atlas-app`, 300 s, two runs each, on a busy machine): a provisional pass only, 0.46% being 8% under the bar. Evidence in `docs/evidence/p1-06/`. Two-window behaviour harness and eight latch unit tests pass. See `docs/p1-06-gpui-idle-results.md`. | Quiet-machine idle reruns, eight-suspended-tab measurement after P1-15, five-launch first-page startup comparison, owner window/touchpad inspection, owner merge approval. Public submission requires permission. |
| 1 / P1-07 design tokens | **Merged into the integration branch 2026-10-12; palettes owner-approved.** `crates/atlas-ui` holds the five canvas palettes, accent states, Reduce-transparency fallback, metrics, PROPOSED D44 width states/thresholds and a WCAG audit; `atlas-app` colours come from Graphite unchanged; `--palette-sample` renders palettes and width states. The owner reviewed the sample window on 2026-10-12 and approved the palettes, and accepted the 16 recorded contrast shortfalls for now. See `docs/p1-07-design-tokens.md`. | D44 drag thresholds and peek edge stay PROPOSED until the owner tries them in a draggable sidebar (P1-08). `ui/recovery.rs` colours move to tokens with their slice; `SIDEBAR_WIDTH` was replaced by the `atlas_ui::sidebar` tokens on the P1-08 sidebar branch. |
| 2 / P1-08 sidebar width states | **Partial (merged into the integration branch 2026-10-12 with P1-28/29; awaiting owner window check).** The edge between sidebar and page card drags through D44's wide (264 px), favicon-only (52 px) and hidden states and back, snapping at 158/26 px with labels shown exactly when a release would settle wide (`atlas_ui::SidebarSizing`). A release outside the window snaps where the button came up; window deactivation settles a drag in progress. The page card's physical bounds come from one tested function (`ui/page_card.rs`) on every drag step, resize and DPI change, and the drag strip and card share one edge calculation, tested not to overlap from 0 to 264 px; `SIDEBAR_WIDTH` is gone. Only the shown page follows drag steps; hidden pages take the card's bounds when shown (`atlas-platform/src/browser/placement.rs`). The edge is a focusable splitter whose Left and Right keys step between states; rows are tree items with levels, and favicon-only rows keep their titles as accessible labels. Hidden keeps the selection. Review fixes landed 2026-10-12. The settled state persists in P1-29's `sidebar.width_state` setting and is restored at start-up (DW-2 Done; tested from settle to restore). P1-29's Favorites render as the four-column grid when wide, one column of named icon tiles in favicon-only and not at all when hidden. See `docs/p1-08-sidebar-states.md`. | Owner's manual checks in that note (no agent window inspection), including Favorites in favicon-only and the width surviving a restart. Ctrl Alt S and peek belong to P1-17 (DW-3); peek must share the hidden handle's left-edge zone. Shared-environment, guardrail and runtime-disclosure items below still apply. |
| 2 / P1-09 saved rows and folders | **Partial.** Saved, open and ordinary rows render in wide and favicon-only widths from the core snapshot. P1-21's folders render in layout order with depth indentation and in-memory expand/collapse (`ui/sidebar_rows.rs`, tested); no folder editing UI. | Prove repeated lifecycle changes create no duplicate or leaked views; compact-folder affordances need owner review (D44); folder expansion is not persisted. |
| 2 / P1-08–P1-10 page/sidebar/navigation | **Partial.** `atlas-platform/src/browser*` and app session/UI source connect shared environment, page bounds, model/store/search, saved/open rows, navigation, shortcuts and owned entry popup. The current gate passes 54 app tests. The owner reports the `about:blank` text/stable Go to/Enter-close check and one local-fixture page-focused Ctrl K, palette typing and Escape focus-return cycle pass. | Verify repeated focus reliability, Enter focus return in other destinations, broader page behavior, persistence, close/reopen and no duplicate/leaked views. D44 dragging and resize/DPI following are now in the P1-08 row above (owner window check pending); UI reordering is absent. Verify page guardrails and runtime disclosure in the running product. |
| 2 / P1-11 popup adoption | **Missing.** Preview explicitly denies new-window requests. | Same-environment/profile unnavigated target adoption, opener relationship and synthetic focus tests. OAuth popup compatibility cannot yet be claimed. |
| 2 / P1-12 permissions | **Partial defaults only.** Pinned wry defaults do not auto-grant clipboard; no complete Atlas permission/notification flow. | Decisions, prompts, persistence, notifications and disposable-profile verification. |
| 2 / P1-13 essentials | **Missing integration/acceptance.** | Find, downloads/Ctrl J, print, context menu and explicit zoom controls, with focused-page keyboard tests. Engine defaults alone are insufficient. |
| 2 / P1-14 security display | **Missing.** | Truthful scheme/error state and invalid-certificate tests; HTTPS alone must not be labelled secure. |
| 2 / P1-15 suspension/fullscreen | **Missing.** Preview hides inactive views but does not implement suspension/fullscreen policy. | Fullscreen layout, suspend/resume, hidden-state ordering and late-callback tests. |
| 2 / P1-16 process recovery | **Partial.** Structural process-failure events and worker-termination reporting exist. | Separate renderer/shared-browser recovery, fault tests and proof of no duplicate recovery or lost tabs. Reporting a failure is not recovering it. |
| 2 / P1-17 peek | **Missing.** | Owned hover peek, Ctrl Alt S, hidden title-bar controls and focus/stacking/Alt Tab/window-lifecycle checks. |
| 2 / P1-18 acceptance | **Missing.** | Owner site/sign-in checks, one crash-free day, product idle/startup and complete shortcut/lifecycle evidence. |
| 3 / P1-19 session restore | **Implemented on `claude/p1-19-20-restore-history`, awaiting review/merge (2026-10-12).** A subprocess test ends a real session worker with `process::exit` (no store shutdown, WAL checkpoint or clean marker), restarts it and proves `Unclean` detection, acknowledged order/associations/titles, unloaded saved rows staying unloaded, all-or-nothing for the queued change and fixed saved targets; the UI requests only the active page from a restored snapshot. See `docs/p1-19-20-restore-history.md`. | Owner's running-app checks: normal restart and a Task Manager kill with several tabs and saved rows. Back/forward lists are not restored (not promised). |
| 3 / P1-20 history | **Implemented on `claude/p1-19-20-restore-history`, awaiting review/merge (2026-10-12).** Schema version 4, after P1-29's preferences version 3 (`history_urls`, `history_visits`, index) with upgrade/rollback/refusal tests; worker records a visit only when a live web tab reports a new http(s) address, ignoring replayed/stale callbacks and restored reloads; tier-1 catalogue updated row by row through the existing public API (no `atlas-search` change); palette History results open a new tab; Ctrl H/toolbar history list of 200 recent visits; `Store::apply_history_deletions` is the D47 participant, proved by replaying a cleared range onto an older copy. | Owner's running-app check of the list and palette results. Ctrl H while a page has focus needs a one-line accelerator in `atlas-platform/src/browser/page.rs`. Clearing UI, journal persistence, catalogue removal on clear and recovery replay remain P1-27. |
| 3 / P1-21 folders | **Partial (headless done, merged into the integration branch 2026-10-12).** `atlas-core` folder create/rename/move/reorder/remove with cycle rejection and unchanged live-page association; `atlas-store` schema version 2 migration (`saved_folders`, `saved_layout`, `saved_folder_state`) with upgrade, rollback and restart tests. Owner decision 2026-10-12: removing a folder moves its contents up by default; deleting contents asks for confirmation; empty folder names are refused. See `docs/p1-21-folders-core.md`. | Sidebar/session UI, compact-folder controls (D44), refusing empty names in the model; search treatment of folders; running-app restart check. |
| 3 / P1-22 import | **Partial (parser merged into the integration branch 2026-10-12).** Bounded, dependency-free HTML export parser in `atlas-core` (`bookmark_import`): folder tree in export order, toolbar/unfiled roles, http(s)-only addresses with typed skip reasons, duplicate/truncation/unclosed counts, UTF-8 only. Synthetic Chrome/Edge/Firefox/malformed fixtures, bound tests and 3,000 seeded generated/mutated inputs; see `docs/p1-22-import-parser.md`. Owner decisions 2026-10-12: toolbar folders import as ordinary folders, UTF-8 only, default limits kept, `atlas-core` may use `url`. | Preview/confirm UI and file selection; transactional write into P1-21's folders as unloaded saved rows, re-checking every address with navigation's rule through one shared function (DW-1). |
| 3 / P1-23–P1-27 full text/retention/clear | **Missing.** Only the pure deletion contract exists. | FTS5/Tantivy bake-off and reviewed choice; adapter; approved extraction proposal and synthetic proof; excluded-site/no-store enforcement; seven-day cache outside backups; coordinated native/Atlas clearing and durable deletion replay. |
| 3 / P1-28–P1-29 home/settings | **Partial (branch `claude/p1-28-29-home-settings`, awaiting review and owner window checks).** Canvas home page (Customize, search pill opening the palette, 8-per-row Quick access with add/edit/move/remove, unavailable plugin cards); separate square sidebar Favorites (D45) with the same editor; plugin targets stay dimmed and report "not available yet"; `atlas-core` destinations model; schema version 3 (`settings`, `quick_destinations`) with round-trip, upgrade, rollback, damaged-row and worker-restart tests; Customize persists all five palettes, preset/custom accent and Reduce transparency and re-renders live. See `docs/p1-28-29-home-settings.md`. | Owner's manual window checks in that note; the header's local date is not shown yet (needs a local-calendar helper in `atlas-platform`); Glass Acrylic via `atlas-platform`; whether Reduce transparency follows the Windows setting (owner question); accessible names (P1-36). |
| 4 / P1-30–P1-32 full palette | **Partial.** Owned input/results source, local tab/saved search, stale request rejection and explicit navigation/web-search parser; popup rendered. | Command registry/prefixes, scope chips, displayed highlights, bounded tier 2 previews, complete shared activation and keyboard/focus verification. No full command or plugin-action implementation exists. |
| 4 / P1-33 palette acceptance | **Missing.** | One-frame opening, D48 composed ranking and live overlay/focus/stacking measurements. |
| 5 / P1-34–P1-40 daily driver | **Missing.** | Single-instance link forwarding; runtime-change notice/open-in-Edge; F6/accessibility/NVDA; installer and registration; local signed-update verification then approved delivery; one default-browser week and D6 memory/startup evidence. |

Phase 1 remains open despite D49 integration. D49 authorizes this preview exception, not automatic completion or cancellation of P1-05–P1-40.

## Plan 2 — plugin runtime and Fledge demonstration

No `atlas-plugin-api`, plugin host, `fledge-client` or plugin crates exist. The three “coming soon” rows in the preview are not implementations. `docs/plans/Atlas v1 — Plugins and Fledge — Plan.md` is approved; its 12-slice map is now approved, with predecessor gates unchanged.

| Phase | Status / required exit |
| --- | --- |
| 1 contract/test plugin | **Missing.** Owned serializable API, registration/version/duplicate checks, scoped Host services, command ownership and synthetic lifecycle tests. Existing manifest-boundary checks are only one foundation. |
| 2 real palette command | **Missing.** Declarative forms/lists, Plugin actions, pending/errors, validation/draft retention, no duplicate submissions, permanent plugin rows and correct live-page focus. |
| 3 Fledge demo | **Missing.** Session-only synthetic leads in normal app, clear demo label, Name required, optional Company/Source/value in demo USD; host-adapted stand-in with list/detail/create and failure/idempotency tests. |
| 4 search/lifecycle/acceptance | **Missing.** Replay/deletion consistency, restart clears demo records from both search tiers, subscription cleanup, activation/idle/ranking measurements and owner acceptance. |

Real OAuth/REST integration is outside this plan until Fledge Stage C, a current authorized contract and separate approval. Do not inspect Fledge repositories or call its services.

## Plan 3 — Notes

No Notes implementation, vault service, document bridge or CodeMirror bundle exists. `docs/plans/Atlas v1 — Notes — Plan.md` is approved; its 16-slice map is now approved; implementation waits for the predecessor gates.

| Phase | Status / required exit |
| --- | --- |
| 1 vault/index | **Missing.** One-vault existing/new picker, bounded files/indexer, navigation, both search contributions, cancellation and malformed/unsupported-file handling on synthetic vaults. |
| 2 editor/save | **Missing.** Shared local CodeMirror bridge, live preview/source/outline, checksummed revision protocol, autosave/Ctrl S, watcher conflicts, recovery drafts and safe saves; existing local Markdown images/attachment links without remote preview loads. |
| 3 links/rename | **Missing.** Wikilinks, heading navigation, backlinks, guarded rename preview/rewrites and recoverable partial failures; preserve aliases and case-only/ambiguous paths. |
| 4 properties/Bases | **Missing.** Typed frontmatter table, one sort, AND filters and `.base` round trip; unsupported constructs labelled, unchanged and source-readable; edits share the document/conflict/undo service. |
| 5 compatibility/acceptance | **Missing.** Disposable Obsidian interoperability, recorded synthetic performance scales, accessibility/five-palette review and owner acceptance. Personal vault inspection is neither required nor authorized for agents. |

Property-type interoperability and vault performance workloads need implementation evidence, not invented compatibility promises. Paste/drop importing, special embeds, advanced Bases and graph/WYSIWYG remain outside v1.

## Plan 4 — Forge

No Forge implementation, grammar bundle, terminal, Git service, LSP client or task runner exists. `docs/plans/Atlas v1 — Forge — Plan.md` is approved; its 21-slice map is now approved; implementation waits for Plans 1–3 and the retained predecessor exits.

| Phase | Status / required exit |
| --- | --- |
| 1 project/editor/highlighting | **Missing.** One project, lazy tree, shared Notes document service, explicit save/recovery, location history and Rust Tree-sitter highlighting for CSS, JS, HTML, TS, Svelte, Rust, GDScript and SQL; synthetic round trips and layout/key review. |
| 2 terminal | **Missing.** Audited ConPTY/terminal transport, bounded output through renderer acknowledgments, resize/input/interrupt and explicit Stop with child cleanup; flood responsiveness measured. |
| 3 Git | **Missing.** Audited in-process reads; installed Git for selected file status/diff/log/history, staging/commit/branches/fetch/pull/push and visible merge conflicts. Literal paths, hooks, external changes and failures need disposable-repository tests. |
| 4 language servers | **Missing.** Installed/configured servers; completion, diagnostics, hover, definition, references, rename, formatting and inlay hints when supported. Prove revisions/position encodings, stale/unversioned edit safety, stdio/Godot loopback lifetimes and no implicit install/fetch. |
| 5 tasks/splits/context | **Missing.** Declared Cargo/npm/just/make and installed Python entry-point discovery without incidental execution; save all dirty documents before run; shared-buffer split undo/save; outline/Notes/Git rail and Stop/restart behavior. |
| 6 workflow/acceptance | **Missing.** End-to-end workflow, resource cleanup, keyboard/accessibility/five palettes and agreed synthetic latency/large-file/terminal budgets, then owner acceptance. |

Debuggers, special Python task adapters, runtime grammar downloads and a three-way merge UI are not implied by this plan. Ctrl W closes the focused editor; whole-project close and running-task/dirty-buffer decisions must follow the approved contract.

## Remaining approval and operator gates

The owner approved the exact breakdown in
`docs/plans/Atlas v1 — Plans 2–4 slice proposals.md`: 12 Plugin/Fledge slices,
16 Notes slices and 21 Forge slices. The issue maps now carry those approved
boundaries and dependencies. No later-plan feature is implemented by this
approval; the original plan order and predecessor exits remain in force.

The approved page-close patch has native fixture evidence in
`docs/page-close-event-review.md`. A separate failed-construction ownership
observation is recorded in `docs/wry-creation-lifetime-review.md`; it grants no
additional dependency-patch authorization and does not diagnose the breakpoint.

- Continue D49's authorized preview now. Plans 2–4 now have approved slice tables and blocking edges; their predecessor exits still gate implementation. Use independent subagent reviews, one approved slice branch and owner-approved merges. Commit only on request; no remote, push or publication.
- The owner alone performs real sign-in/DRM tests: remaining password plus phone prompt, signed-in persistence, Hulu and daily-site acceptance; Workspace/admin device-policy checks also belong to them. Repeat relevant product tests rather than relying solely on the spike. Record only pass/fail, never account contents.
- The owner reviews D44 widths/thresholds/compact-folder details, the implemented Notes/Forge workflows and later acceptance use. Owner-operated bookmarks import, one-day browsing and one-week default-browser use cannot be replaced by generated fixtures.
- The page-text extraction method needs a concrete approved proposal before guarded page access. Its exclusion/retention tests must precede real capture. D39 analytics direction is approved, but AN-01/AN-02's schema/control/slice proposal is still outside the approved 40-row map; no collector or model upload exists.
- New dependencies require exact maintenance/security/licence audit. Keep the approved GPUI/wry/WebView2 pins and narrow exceptions. No GPL; repository licence/public release, GitHub creation, upstream publication, updater host/network/install policy, software installations and Windows registration remain owner decisions/actions.
- Keep Atlas unsigned and Windows security unchanged. Prior error 4551 and the preview breakpoint are observations, not grounds for a signing prerequisite or bypass. Never use integration apps without permission, real browser profiles/personal fixtures, or Fledge production. Spike cleanup needs explicit approval.
- The later mobile-sync outline is outside these four v1 plans and does not authorize mobile, remote sync or earlier work to be skipped.

## Shortest next step

The latest product gate passes all 376 tests and application smoke. The owner
reports that `about:blank` appears while typing, its Go to row stays selected
after two seconds, and Enter closes the palette. Continue the approved fresh-
window inspection. The owner also confirms one local-fixture page-typing,
page-focused Ctrl K, palette-typing and Escape focus-return cycle. Remaining
checks include repeated focus reliability, Enter focus return in other
destinations, other page-focused shortcuts, saved-target independence, unload/reopen, resize,
persisted restart and shutdown. Preserve older windows and all native failures;
the original breakpoint remains unresolved, and the page-host prototype is
unpromoted. The narrowed ASCII projection is implemented, but only 10/26 D48
cases pass; do not promote the test-only index or relax D48. Re-run the required
gate after implementation changes and retain independent review before any
requested commit or approved merge. This narrow manual pass does not complete
browser acceptance or a later plan.
