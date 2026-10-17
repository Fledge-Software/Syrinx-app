# Continue the integrated browser without losing the approved foundations

The owner wants the planned browser, including its plugins, usable for their
workflow. D49 approves bringing the integrated browser forward. This checkpoint
preserves the current evidence; it does not call the preview or all four plans
complete. Read `AGENTS.md`, `CLAUDE.md`, `docs/trapstoknow.md`, the decisions,
active plans, `docs/LOCAL-DEV.md` and `docs/implementation-status.md` first.

## Latest continuation checkpoint

### October 11 browser-first continuation

Latest checkpoint: atomic startup and retained-name guards pass the complete
normal-desktop gate with 436 tests and synthetic app smoke in
`target/continuation-atomic-startup-desktop-gate.log` (SHA-256
`5654ffa30e3f5af2b8711410ea862db2d5dcb8c42b474450b294048f49ccad79`).
Current `target/local/syrinx-app.exe` SHA-256 is
`252044439eb668680360e28009a3245e2992a754cb22d82b2dae8b6b8839982d`.
Both initial rename-refusal failures were fixed without weakening assertions;
60 focused storage tests and 23 worker tests also pass. The restricted-context
gate's smoke timed out with watchdog code 124; the unchanged binary passed
normal-desktop smoke and the full desktop gate. Preserve both logs and do not
call the original WebView2 crash resolved. All execution is terminal. Visual
typing/focus acceptance and the blocked isolated diagnostic remain open.
This supersedes the pending implementation/validation notes below. Changes
remain uncommitted; the previous commit authorization was already fulfilled.

The crash chat independently verified the latest gate and binary hashes and
the 436-test total. Its two source audits found no missing startup
acknowledgement or reversed readiness dependency. Exit 124 identifies the
independent watchdog deadline, not the stalled operation. No new crash fix or
diagnostic execution follows from this review; the isolated diagnostic and
Computer Use recovery limits remain unchanged.

The owner has now explicitly approved exactly `Wdk_Foundation`,
`Wdk_Storage_FileSystem` and `Win32_System_IO` on pinned `windows 0.62.2`.
The usability chat has resumed the atomic startup implementation and its
remaining regressions. This supersedes the pending-amendment checkpoint below;
it grants no other dependency or security changes. Integration must wait for
the reviewed implementation freeze before formatting, targeted tests and the
full gate. The last verified binary remains the 411-test checkpoint.

Owner approval update: the owner answered Yes to the exact missing-primary
startup safeguard in `docs/missing-primary-startup-review.md`. The usability
chat now owns its platform/native/recovery implementation, minimal app session
wiring and synthetic regressions, with subagent review. This approval includes
only that additive API and conservative initialization policy; other pending
GPUI, security, dependency and version-control limits remain unchanged.
Integration retains all execution; wait for implementation freeze before tests.

Implementation review found that directory creation followed by opening its
path cannot prove the opened object is the one created. A separate owner
question is pending for exactly `Wdk_Foundation`, `Wdk_Storage_FileSystem` and
`Win32_System_IO` on the existing pinned `windows 0.62.2` dependency, to expose
typed atomic directory creation. No manifest change is authorized yet. The
usability chat has frozen the unaffected retained-offer/session work and its
13 new regressions after independent source review. The fresh-start branch
deliberately fails closed until the amendment is approved and implemented.
The exact 13-file handoff and remaining acceptance checks are recorded in
`docs/missing-primary-startup-review.md`. Do not build or promote this partial
state as a usable replacement. Startup source remains unvalidated; the latest
verified binary remains the 411-test checkpoint below. Do not call that gate
validation of these later startup changes.

Latest checkpoint supersedes the in-progress notes below: the corrected palette
shortcut routing and queued-refocus race pass the complete 411-test gate and
synthetic smoke in `target/continuation-palette-refocus-gate.log`. Current
`target/local/syrinx-app.exe` SHA-256 is
`4c437c976290615a00ee06287b5338b94b26267e1546002d8b307674d55cdfde`.
All execution is terminal. Native focus/caret acceptance and the original
WebView2 crash remain open. Missing-primary startup API/policy is now approved;
only the three-feature atomic-creation amendment above remains pending. The search
candidate stays test-only after the full comparison rejected promotion.

The first palette-shortcut gate passes 409 tests and smoke in
`target/continuation-palette-shortcuts-gate.log`, but independent review found
a queued Ctrl K/L then Enter/Escape race that the test did not cover. That
version is not accepted. The usability chat owns its narrow correction and
regression; wait for its new freeze before running the next gate. All earlier
evidence stays preserved. The full search comparison was rejected for
production promotion after independent timing review; see `docs/p1-05-search.md`.

Execution update: the shared private benchmark workload extraction passes all
seven focused xtask benchmark tests in
`target/continuation-shared-workload-tests.log`. The reviewed full 26-case
Unicode-selected comparison finished successfully in 19.96 seconds, with exact
result parity, in `target/continuation-unicode-full-comparison.log`. Session
`19312` is terminal. The navigation chat is independently analyzing timings;
production search remains unchanged. The usability chat is implementing the
reviewed private routing of the 11 existing shortcuts in the palette; wait for
its freeze before validating. Missing-primary startup API/policy approval is
pending in the owner question; no such implementation is authorized yet.

Latest validation: the independently reviewed late-action shutdown fix passes
the full 407-test gate and synthetic smoke in
`target/continuation-shutdown-actions-gate.log`. Current executable SHA-256 is
`2e3bc975b0b1ea894c12a4c94f5171401635324712b1e0d378f72cf558b47b9e`.
The Unicode-selected experiment now passes two focused tests; timing review
is assigned to the navigation chat. The usability chat is preparing a
review-only palette shortcut-routing proposal. The crash chat is auditing
existing process-failure handling; blocked diagnostics and GPUI changes remain
stopped. No Cargo/native process remains active at this checkpoint.

The owner now prioritizes a usable browser and explicitly defers plugins,
Fledge, Notes and Forge. The preceding work was committed at their request
as `a94500a`; later changes remain uncommitted. No merge, push or security
change is authorized. Three existing chats and their subagents continue
crash investigation, navigation/search reliability and browser recovery;
the integration chat alone executes Cargo and native checks.

The corrected synthetic socket fixture passes its regression and one native
12-cycle lifecycle run. Send failures now remain visible, and an open palette
refreshes after committed tab changes. Their full gate passes 382 tests and
smoke in `target/continuation-browser-first-formatted-gate.log`. Application
SHA-256 is `0da392f0800e5e3b739bfeabe8122903111d38301e41a6d610168e0b882debd3`.
Computer Use could not activate the fresh synthetic window on either allowed
attempt; automation stopped, so current visual acceptance remains open.

The next reviewed increment adds explicit failed-start recovery confirmation
and a watchdog that cannot block on stderr. All 70 application tests pass in
`target/continuation-recovery-watchdog-tests.log`. Missing-primary preflight,
visual recovery checks remain open. This increment's full gate subsequently
passes all 400 tests and smoke in
`target/continuation-recovery-watchdog-full-gate.log`; the latest executable
SHA-256 is `9b2fded8f7dc79d436ddbfe15af63133a1be005645c188c98750f17fcfb36119`.
The cached-first search experiment passes correctness but is rejected for
unconditional production adoption because paired measurements regress; production
still passes only 10/26 D48 cases. Check execution status before starting any
build or benchmark.

The isolated process-role diagnostic has not executed. Two builds fail while
loading `gpui_macros`; the second preserves a compiler trace at
`target/continuation-process-role-build-trace.log`. One explicit-import probe
exposed Windows Application Control error 4551 loading that exact macro DLL;
evidence is preserved in `target/macro-load-probe/`. That diagnostic build path
is stopped. Do not retry the full build without new evidence, change security,
replace cached DLLs or treat the normal metadata fallback as a proven cause.
The original WebView2 breakpoint remains unresolved. The implementation ledger
contains the fuller evidence and limitations; older checkpoints below remain
historical.

After the 400-test gate, the navigation chat added a separately reviewed,
test-only Unicode-selected membership experiment. It is not included in that
gate and has not run. Its queued command is
`cargo test -p syrinx-search --profile local --offline --locked catalogue::range_index::tests::membership_order::unicode_selected -- --nocapture --test-threads=1`.
The diagnostic launcher now passes 31 pure parser cases and six pure cleanup
cases plus independent review; its final SHA-256 is
`82eaf67d6d0d762ad2234b3f59abfcf5feab0b55c1ad0c0f3ebeec26880eab74`.
It has not launched the blocked diagnostic. No Cargo/native job remains active
at this checkpoint. The owner's requested continued coordination is scheduled
in this chat every 30 minutes as `continue-syrinx-browser-work`, preserving all
approval limits and reporting only meaningful changes or required input.

### Stable address row and current crash evidence

The owner-approved stable visible “Go to …” row is implemented and independently
reviewed. Explicit address selection survives asynchronous local results. The
full gate `target/continuation-socket-close-commit-gate.log` exited zero and ends “Local gate
passed”: dependency checks, formatting, Clippy, all 376 tests (including 54 app
tests, 164 search unit tests and five public matcher regressions), documentation
tests and application smoke pass. None are ignored or filtered. The gate log's
SHA-256 is
`ca28dc0a417a3e00929ece8c652c21dc8b7b2e58da6acd820fec72af0a03ca62`.
The preceding successful 376-test Go to gate remains preserved as
`target/continuation-go-to-gate.log`, SHA-256
`bda488828169ae54deb6803b76ec06e98a12ae1919be53fb29f4e8ab89173cb0`.
The latest `target/local/syrinx-app.exe` has SHA-256
`0fe3b8daad72ddf3383f025756a5174b39f098d2fa7355d81bfd45eaec89c858`.
On October 11 the owner reported that all three requested manual checks passed
on this synthetic build: typed `about:blank` appeared, “Go to about:blank” stayed
selected after two seconds, and Enter closed the palette. This is owner-reported
evidence for that path only. The additional single-cycle focus check below
also passed; broader destination coverage and the original breakpoint remain open.

On the same live `0fe3b8da…` synthetic build, the owner then opened the local
fixture through Ctrl K, `http://127.0.0.1:64751`, Enter. They clicked its Typing
area and typed `before`, pressed Ctrl K from the page, typed `about:blank` into
the palette, pressed Escape, and typed `after` without clicking the page again.
The owner confirmed “Yes, beforeafter appeared”. This is an owner-reported pass
for page typing, page-focused Ctrl K, palette typing and Escape focus return
in that single cycle. It does not establish repeated reliability, Enter focus
return in other destinations or a crash fix. The possible ordering race remains
a source concern that this check did not reproduce; no focus fix was applied.

The earlier October 10 visual attempt used this binary and fresh synthetic profiles.
Normal-start launches produced a black capture and failed activation twice;
a direct launch then showed the Windows lock screen in Computer Use. The
integration agent stopped Computer Use and asked the owner to unlock Windows.
These observations do not establish that Syrinx rendered a black window.
On October 11 the owner explicitly said “GO AHEAD”, confirmed Windows was
unlocked, and reported Syrinx visible and responsive. The integration agent
observed Home and the palette rendered normally in the latest synthetic
`syrinx-app-focus-20261010` process 46216, window 6097856. Clicking edit element
3 showed a caret and popup focus. Setting its value then failed with the exact
tool error “element 3 is not available in cached app state for
syrinx-app-focus-20261010.exe”; a refreshed window list exposed only the main
window. This is a tool-targeting limitation, not proof that Syrinx rejects typing.
The integration agent asked the owner to type `about:blank`, wait two seconds
and press Enter to verify text entry, the stable Go to row and submission.
The check was initially pending; the owner's subsequent “All three worked”
confirms those three observations. Windows unlock is no longer the blocker.
The possible focus-ordering issue remains a source finding, with no new fix
applied and no broader focus acceptance claimed.

The disposable pending-creation shutdown test records exit zero for `controller`
and `completed`, including ordered completion, actual queued/consumed Shutdown,
Stopped, join and the exact child baseline. The original `environment` case
exited one with `BrowserError::Environment`; its trace contains completion and
Stopped but no successful native-return or queued-shutdown marker. This does
not establish a shutdown defect. Preserve the three
`target/continuation-pending-shutdown-*.result.json` records and stdout/stderr.
The diagnostic build in `target/continuation-pending-shutdown-diagnostic-build.log`
then exited zero. Exactly one diagnostic environment run exited one, recorded
under `target/continuation-pending-shutdown-environment-diagnostic.*`. Its trace
shows native-call entry, a successful callback with an environment object,
successful native return, then `pending_rejected=completion_already_entered`.
Initialization succeeded; the callback arrived too early to cover the intended
pending-callback shutdown state. This is a failed coverage precondition, not
evidence of native-initiation failure or a shutdown defect. The controller and
completed passes remain valid; pending-environment coverage remains open.
The integration agent also completed the native v2 diagnostic build and its
single run described below. These diagnostic jobs are terminal.

The first disposable native-status overlay also failed: cycle 4, synthetic tab
103, navigation 21 reported `success=false`, `web_error=0` while waiting for the
local fixture. Zero denotes UNKNOWN, not success. The preceding eleven
completions succeeded. Its exit-one record and log are
`target/continuation-native-error-category.result.json` and
`target/continuation-native-error-category.stderr.log`. Independently reviewed
`target/native-stress-diagnostics/workspace_v2` adds HTTP status and a shared
navigation-completion/close-callback entry counter only in its copied page
module. All 277 copied-file hashes were checked; the fixture, assertions and
dependency inputs are unchanged. The frozen v2 source built successfully in
`target/continuation-native-ordering-build.log`. Its single native run exited
one, preserved in `target/continuation-native-ordering.result.json` and the
matching stdout/stderr logs. The only native callback line is sequence 1,
tab 1, navigation 2, `success=false`, `web_error=0`, `http_status=0`; the fixture
creation/navigation wait then fails. No close-request callback was recorded.
These values do not establish a transport cause, a close-timer race or the
original breakpoint's cause. All earlier failures remain preserved; none of
the narrower passing checks resolves the owner's original WebView2 breakpoint.

The independently reviewed v3 disposable overlay adds navigation-start
classification and server progress counters without changing the fixture,
transport, assertions or deadlines. Its first run and all three bounded fresh-
process repeats exit zero: four runs each complete 12 lifecycle cycles and
multi-page shutdown. Evidence is `target/continuation-native-correlation.*`
and `target/continuation-native-correlation-repeat-{1,2,3}.*`; all result
records match diagnostic executable SHA-256
`942fc597df9cbef496a570d09658448aad1e5b8c62fc6aae92ee1921bb932fea`.
Independent review pairs all 31 starts/completions in each run by tab and
navigation ID: 13 fixture completions succeed with HTTP 200, 18 about:blank
completions succeed, and 13 close callbacks are recorded. All four server
summaries have 13 complete headers and 13 written responses, zero EOF/partial
EOF and `failed=false`. Accepted connections/empty timeouts are 37/24, 44/31,
32/19 and 27/14 respectively.

This strengthens synthetic lifecycle evidence for this instrumented copy; it
does not explain the earlier failures or prove the original breakpoint fixed.
Instrumentation can affect timing, server writes do not prove renderer receipt,
and the hidden native harness does not establish visible app usability. Preserve
v1/v2 failures and their sources. All four v3 processes are terminal.

The fixed three-pair v2/v4 comparison is now complete, preserving every
predeclared run under `target/continuation-native-server-pair{1,2,3}-{v2,v4}.*`.
V4 keeps the v2 page callbacks byte-identical and adds only the reviewed v3
server counters/logging; its fixture, transport, assertions and deadlines are
unchanged. Independent review checked all six result records and executable
hashes. V2 remains
`b7d019e931a16fbb63ccc433283d2764b67551e119edf716c01253e25ccf67ed`;
v4 is `84766b45fc38577c12293ffb25d67a769f81545dd22a1feb6d937ec977b381eb`.

| Pair | V2 result | V4 result |
| --- | --- | --- |
| 1 | Exit 1 at initial tab 1, navigation 2: unsuccessful, native error 0, HTTP 0. | Exit 0; all 12 cycles and multi-page shutdown pass. |
| 2 | Exit 1 at cycle 5, tab 104, navigation 24: unsuccessful, native error 0, HTTP 0. | Exit 0; all 12 cycles and multi-page shutdown pass. |
| 3 | Exit 1 at cycle 8, tab 107, navigation 33: unsuccessful, native error 9, HTTP 0. | Exit 0; all 12 cycles and multi-page shutdown pass. |

Pinned `webview2-com-sys 0.39.1` identifies native error 9 as
`CONNECTION_ABORTED`; error 0 is UNKNOWN. Each v4 run has 31 successful
completions, 13 fixture HTTP 200 responses, 13 close callbacks and a server
summary of 13 complete headers/13 written responses with `failed=false`.
Accepted connections/empty timeouts are 41/28, 42/29 and 39/26.

Server-only instrumentation accompanies a 3/3 pass versus 0/3 pass difference
in this small fixed sample. Its synchronous logging changes timing, including
work after response writing and before stream drop. This narrows a testable
timing hypothesis; it does not identify the responsible operation or prove
that server connection closure caused the native failure. No delay, relaxed
assertion or product fix is justified by this result alone. Earlier failures,
the unresolved original breakpoint and interactive acceptance remain separate.
All six comparison processes are terminal.

The owner-authorized socket-lifetime comparison is also complete in its fixed
A/B/B/A order (v4/v5/v5/v4). All four runs exited zero and reported all 12
lifecycle cycles and multi-page shutdown passed. Each has 31 successful native
completions (13 HTTP 200, 18 HTTP 0), 13 expected close callbacks, contiguous
callback sequence numbers 1–44, and no native error. Each server reports 13
complete headers/13 written responses, zero empty/partial EOFs and `failed=false`.
Accepted connections/empty timeouts are 39/26, 35/22, 38/25 and 35/22.
A retains the stream through the final response log; B's sole source change
drops it immediately before that log. Verified executable hashes are
`84766b45fc38577c12293ffb25d67a769f81545dd22a1feb6d937ec977b381eb` (A) and
`00fcca09f5313592e111f5dac950d811ec455dd066f5595e0531561bf0b5a28d` (B).
Preserve `target/continuation-socket-close-series.json` (SHA-256
`b40081339240a797f4c12720b2bc4460ec487bd270011da0f771cec4008a7c21`)
and `target/continuation-socket-close-{01-A,02-B,03-B,04-A}.*`.
`docs/fixture-transport-timing-review.md` records independently checked source,
result and log identities. Retention through the final log was not necessary
in these runs. This does not identify a TCP mechanism, explain prior failures,
prove reliability or resolve the original breakpoint. Both copies remain
disposable diagnostics; no product transport change or prototype promotion follows.

The isolated browser-thread page-host prototype has its first result. Reviewed
`target/browser-parent-host-proposal/prototype_v2` gives each wry page an
intermediate host owned by the browser thread; it remains unpromoted. Its
278-file manifest has SHA-256
`53def35c680db3a5b44595fc6c56cf41a71f840ad5175fbe139cb4eb0f58d736`.
After source verification, `target/continuation-page-host-build.log` built
successfully (SHA-256
`8749a02e0ce6d76677e0841534e8d24813b3b416e9bac502ccfa799a267eecc9`).
The one native run exited one: cycle 2, synthetic tab 101 failed while waiting
for fixture creation/navigation. Its stdout is empty and stderr contains only
that navigation failure. Preserve `target/continuation-page-host-native.*`;
the recorded executable SHA-256 is
`6d5db8f282ecb9fffb50b62dbefb4d01128d0aaf44fccb6cff51792884055050`.
No original breakpoint or host-cleanup diagnostic was observed. Their absence
does not make the full host lifecycle test pass or establish a crash fix.
The prototype stays isolated; live source and the `0fe3b8da…` application are
unchanged, with the latest full gate again passing all 376 tests. The owner's later
manual confirmations establish the `about:blank` text/selection/Enter-close
path and one local-fixture page-focused Ctrl K/Escape focus-return cycle.
They do not establish broader navigation reliability or crash acceptance.

The latest search benchmark remains 10/26 D48 cases; the range index and grouped
bound-reuse experiment remain test-only. Existing approvals and predecessor
gates remain in force.

On October 11 the owner explicitly authorized committing all current work
after completion and the full gate. The gate above passed; this authorization
does not approve a merge, remote, push, publication or security change. Check
the repository's actual commit state rather than treating this record as proof
that a commit has already been made.

The following checkpoints are historical.

### Earlier crash and usability continuation

The owner requested continued subagent work focused on the crash and usable
browser behavior, and explicitly reapproved fresh synthetic Computer Use.
Five serial normal-desktop smoke runs of the frozen `3ca0bf0…` binary exited
zero with empty stderr (`target/continuation-crash-smoke-repeat-*.result.json`).
They did not reproduce the original breakpoint.

The expanded native lifecycle test now repeats 12 create/navigate/show/resize/
close cycles and checks cross-thread parent/child ownership and multi-page
shutdown. Its first run FAILED in cycle 5 with a navigation failure. A diagnostic
rerun FAILED in cycle 3, synthetic tab 102, while waiting for initial navigation
to the existing local fixture. Neither emitted a process-failure event. Preserve
both failures: `target/continuation-native-stress.stderr.log` and
`target/continuation-native-stress-diagnostic.stderr.log`. Assertions remain
strict. A disposable diagnostic copy is being prepared to expose the native
navigation error category without adding real-data logging to Syrinx.

The independently reviewed navigation-status correction is implemented in
`ui.rs` and private `ui/navigation_status.rs`: active-page loading/completion
feedback survives metadata snapshots, inactive and stale pages cannot replace
it, and failure remains failure after a late Finished event. All 47 app tests
pass in `target/continuation-navigation-status-tests.log`, SHA-256
`5cdd5627201b0ac6394d3c1f84d11a491c35af2bedec93e2c630d480db610ce1`.
The full gate subsequently PASSED: all 369 tests, dependency checks,
formatting, Clippy and app smoke. The previously blocked public search test
executable ran successfully; no security setting was changed. The gate log
`target/continuation-crash-usability-gate.log` has SHA-256
`88b153e055d1e64b48feb2b18ff6b34199ebeb650e64cb22600d33e9e288b39d`.
The current `target/local/syrinx-app.exe` has SHA-256
`3d8a38e8b542609961a1949cdb9d1f0b21c8ec054cceb257194aa38cc56f15aa`.
This supersedes the earlier binary and failed-gate checkpoint below, but does
not supersede the separate native stress failures or diagnose the breakpoint.
The owner explicitly approved the stable visible “Go to …” row, retaining
address selection across asynchronous local results. The navigation team is
implementing that follow-up; it is not included in this passing binary.

Visible synthetic process 34160 rendered Home and Ctrl K. Computer Use saw the
popup input but could not set its cached edit element; typing reactivated the
main window and entered no text. Alt F4 closed that test process, with empty
stderr. Older process 22080 was untouched. Hidden test launches 8288 and 11236
exposed no targetable window and were explicitly stopped after identity checks.
Their fresh profiles and logs are preserved. This is a tool-targeting limitation,
not proof of manual typing failure or success. No breakpoint was observed.

The WebView2 investigation chat now owns a disposable pending-creation shutdown
test, explicitly requested by the owner there. It will distinguish successful
native initiation before callback delivery from merely enqueuing Create and
Shutdown. Root retains all execution. See
`docs/webview2-breakpoint-investigation.md`; parent-thread concerns remain
source findings without demonstrated breakpoint causality. The test-only
search bound-reuse experiment passed its equality tests in the full gate;
no isolated performance result or production promotion is claimed.

### Historical combined browser checkpoint after parallel work

The current binary is `target/local/syrinx-app.exe` (also preserved as
`target/local/syrinx-app-parallel-preview.exe`), SHA-256
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

The independently reviewed disposable v4 harness subsequently built successfully
offline, but its first `settings child` process exited with `0xC0000139`
(-1073741511), with empty stdout and stderr. No fault case passed; the remaining
29 invocations were not launched. Build log
`target/continuation-wry-fault-v4-build.log` has SHA-256
`a22fab5857206c8b4c832bb3fa932d3f5f89371c7cb3859cfb6574e10312655e`.
Its executable SHA-256 is
`3a5c8e991647a24003fa076dc1066e89814f195aba555c33cdd5d58c7c792e5b`.
The import table includes `comctl32.dll!GetWindowSubclass`, which is absent
from the default System32 DLL's exports. A missing Common Controls v6
activation manifest is the current loader-failure hypothesis, not evidence
about the original Syrinx crash. The investigation chat is preparing a fresh
disposable v5 correction and addressing two informational BOOL warnings;
v4 sources and failure evidence remain preserved. No Cargo or native process
remains active from this v4 attempt.

The corrected disposable v5 then built and passed all 30 fault cases (15
stages in child and non-child modes), each in a fresh process. Both launcher
batches exited zero; every case has matching PASS output and the exact expected
stderr, including the two intentional cleanup diagnostics. Extracted executable
resources confirm its Common Controls v6 manifest. Independent evidence review
checked all 30 per-case records against the aggregate, exact output and the
current executable hash and found them consistent. These tests exercise the
documented temporary-owner cleanup, survivor isolation, original error and
retry assertions. They do not establish the original crash cause, production
activation-context equivalence, callback delivery after container destruction,
forced handle reuse or the other documented untested paths.

The v5 aggregate is `target/continuation-wry-fault-v5-results.json`, SHA-256
`384902be7ab55637cc496b05d89028a557547e4bd7269b63af7bd7d2a10c72a0`.
The disposable executable SHA-256 is
`2ff83f312bbec9b507eae7a1b43e669a5420b0eec4a85ac3fbbacaa83d3460f1`;
`target/continuation-wry-fault-v5-build.log` has SHA-256
`2f4160deb3efebdc68cff12409c8663a7e3ee78f46e9334700066058e6a8a54d`.
The build retains one existing unused upstream version-function warning;
the two harness BOOL warnings are fixed without suppressions. The product
binary and failed full-gate status above are unchanged. A test-only grouped
score-bound reuse experiment is being prepared by the search subagent;
production search is unchanged, and that experiment is not yet validated.

The narrowed projection diagnostic passes and independent evidence review
confirms all 1,040 production benchmark samples preserve their exact results.
The latest benchmark passes 10/26 D48 cases (maximum p95 13.9768 ms, worst
sample 17.7388 ms); search acceptance remains open. See
`docs/p1-05-search.md` and the narrowed-profile/benchmark logs under `target/`.

A fresh synthetic window was launched from `syrinx-app-parallel-preview.exe`,
process 22080, TEMP profile
`SyrinxParallelPreview-e88536c4ea344dc7ac2af6e7cbe298f5`.
The owner stopped Computer Use with physical Escape during its first
inspection. No further Computer Use actions were issued in that turn.
The window was left open; do not terminate it or assume interactive acceptance.
The popup reactivation change remains visually unverified. No Cargo job remains
active at this checkpoint; the parallel validation-overlay preparation may
still be active. Preserve uncommitted work, existing windows and all approvals.

### Earlier evidence

### Parallel browser work in this continuation

The owner requested separate sessions using their own subagents. Three local
sessions now share this checkout; their source changes are already visible and
must not be merged or copied over other work. This integration session owns
Cargo, benchmark and native/UI execution and serializes those checks.

- `Syrinx browser navigation reliability`, thread
  `01a1289c-d333-7b43-947b-608525418617`, owns `syrinx-app/src/navigation.rs`,
  `ui/navigation_intents.rs` and `ui/page_lifecycle.rs`. Its reviewed source is
  frozen: case-insensitive supported/refused scheme handling and a saved-page
  late-creation regression. Its report is
  `docs/parallel-browser-navigation-review-2026-10-10.md`.
- `Syrinx WebView2 failure-path investigation`, thread
  `01a1289e-dcc0-7a81-9877-95a8028f63dc`, owns only the failed-construction
  review documents and ignored proposal copies after applying the separately
  approved frozen patch. Its corrected disposable v4 fault harness passed
  independent source review; execution remains with this integration session.
  No further release vendor change is approved. See
  `docs/wry-failed-construction-patch-review.md`.
- `Syrinx browser usability readiness review`, thread
  `01a128b6-c712-7f80-b3fe-e0168cbebfff`, uses two subagents to review browser
  journeys and acceptance evidence. It owns only
  `docs/parallel-browser-usability-review-2026-10-10.md`; it may not change
  product code or run builds, native tests or Computer Use. Its report will
  map the next smallest allowed work to existing slices and predecessor gates.

This session's subagents prepared reviewed popup reactivation, saved-row
non-resurrection coverage and native lifecycle coverage. The native harness
passed in `target/continuation-native-lifecycle.log`, SHA-256
`be028092946f48443bafa97d8f3117bbe6470f475457079ab230a16f01959f5c`.
It checks fresh synthetic create/visibility/resize/rejected-command/retry/
close/reopen/shutdown behavior, not the original breakpoint's cause. The
combined gate is running in `target/continuation-browser-parallel-gate.log`;
do not treat the earlier checkpoint below as validation of the new source.

A separate synthetic `syrinx-app-ui-checkpoint.exe` (same bytes as the binary
below) rendered Home and Ctrl K. It used fresh TEMP profile
`SyrinxUiCheckpoint-c5328ca9d8f649eb96692de6caba9d5d`, process 25656.
Computer-use showed the popup's edit control but `set_value` returned
`element 9 is not available in cached app state`; `list_windows` returned
only the main window. Input activation targets the main window, so this does
not establish native typing or focus failure. Alt F4 closed the synthetic
process, and its stderr is empty. No older window or personal profile was
touched. Native typing/navigation acceptance remains open.

### Preceding verified diagnostics checkpoint

The latest verified binary is `target/local/syrinx-app.exe`, SHA-256
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
`SyrinxApprovedWindow-d8f8830e3b6e4779806d246aa0b6da9c`). The initial tool approval
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

### Preceding in-place mutation checkpoint

The test-only range index now updates eligible members in place when text,
ranking class and target order remain unchanged. Independent review and seven
new regressions pass. The full gate in `target/continuation-in-place-gate.log`
passes dependency checks, formatting and Clippy, then records 277 passes and
the same two NUL failures (141/143 search tests), with none ignored or filtered.

The paired mutation diagnostic passes. At 100,000 rows, eligible timestamp
updates fall from 1,380.850 to 2.350 microseconds; route counters prove all ten
measured updates used the guard. Other mutation paths remain unchanged and
their timings are mixed. Keep the optimization test-only. Evidence and hash:
`target/continuation-in-place-profile.log` and `docs/p1-05-search.md`.
Next investigate position-free range bounds from retained metadata, with exact
bound equality and paired query measurements. Production loading, D48, the
WebView2 crash and all pending approvals remain open. The production binary
below is unchanged; no continuation process remains active.

### Preceding locality checkpoint

The latest continuation improves only the test-only range index. Cached
fallback positions are repaired through every mutation, with independent
review and new lifecycle regressions. The final full gate passes dependency
checks, formatting and Clippy, then records 270 passing tests and the same
two NUL-highlight failures (134/136 search tests), with none ignored or
filtered. See `target/continuation-range-locality-final-gate.log`.

Both isolated diagnostics pass. The mixed CJK paired medians are 1.192 ms
ordinary versus 0.654 ms indexed; mixed broad is 13.059 versus 3.783 ms.
Route counters confirm indexing without frontier restarts. Mixed composed
accent still regresses, and 100,000-row bucket edits cost roughly 0.6–1.4 ms.
Capacity recovery also rebuilds the index. Keep it test-only, reduce avoidable
mutation work and review bulk loading before promotion; D48 remains open.
The logs are `target/continuation-range-locality-profile.log` and
`target/continuation-large-bucket-profile.log`; the search report records
hashes, exact costs and limitations. No continuation process remains active.
Production behavior, the binary below and all pending approvals are unchanged.

### Preceding production checkpoint

The latest binary is `target/local/syrinx-app-digit-counts.exe`, SHA-256
`3ea8af80db81795667d32c7732bb30c2f09e6db1d9da26c360f4d221e91668da`.
It adds original-field digit-count rejection while preserving the exact matcher
results. Its awaited fresh synthetic desktop smoke exits 0 with empty stderr;
build and smoke logs use the `target/continuation-digit-counts-app-` prefix.
Existing windows, binaries and uncommitted work are preserved.

The full gate passes dependency checks, formatting and workspace Clippy,
then records 265 passing tests and the same two NUL-highlight failures
(129/131 search tests). No gate test was ignored or filtered; later stages
remain unreached. See `target/continuation-digit-range-gate.log`.
All 1,040 unchanged benchmark samples preserve exact checksums and counts.
Rare-identity p95 falls from 36.2976 to 7.3758 ms; only 9/26 cases meet D48,
which remains open. The report is `target/continuation-digit-counts-bench.json`.

The bounded partial range index is now implemented as a test-only experiment,
with bulk construction and incremental mutation hooks. Independent review and
all complete-result comparisons pass. It improves several broad cases but
regresses on multiple mixed-data cases, so it is not promoted. Diagnose actual
range admission/restart and fallback work, measure large-bucket mutation costs,
and settle production loading before promotion. `docs/p1-05-search.md` records
the proof, hashes, storage and timing evidence. The original crash, frizbee
patch, window access and Plans 2–4 slice-map approvals remain unresolved.
No integration app, extra dependency patch, commit, merge, push or security
change was used; no continuation build/test process remains active.

## Earlier packed-digit and occupancy checkpoint

The latest continuation adds independently reviewed, test-only packed numeric
transitions and six synthetic index-occupancy fixtures. Production behavior
and the binary below remain unchanged. The final gate is now
`target/continuation-packed-occupancy-final-gate.log`: dependency boundaries,
formatting and workspace Clippy pass, followed by 250 passing tests and the
same two NUL-highlight failures (114/116 search tests). No test is ignored or
filtered in that gate; later stages remain unreached. Both isolated diagnostics
pass, and no continuation build/test process remains active.

Packed numeric matching improves several multi-digit cases but regresses for
`0`; it remains test-only. Occupancy rules out the proposed all-rows index:
the existing corpus is fully grouped, but entirely unique fields group only
1,024 of 100,000 rows. The next experiment should be a bounded partial range
index with explicit fallback rows and ordinary scanning outside its resource
envelope. Measure memory and mutation costs; unique-field performance remains
open. The score-bound arithmetic experiment was rejected and removed after
unconvincing timing evidence. `docs/p1-05-search.md` records all results and
proposed limits. Pending approvals and the original crash remain unresolved.

The latest binary is `target/local/syrinx-app-highlight-traces.exe`, SHA-256
`a84d7dfc9fc834332401e31505d3eda26cad0173c86e8036b35c7d5ff074f9bf`.
It reuses original highlight byte traces within each eligible query while
validating ranges against every original field. Its awaited fresh synthetic
normal-desktop smoke run exited 0 with empty stderr. Build and smoke evidence
uses the `target/continuation-highlight-traces-app-` prefix. Existing binaries,
windows and uncommitted work are preserved; no continuation job remains active.

The preceding production checkpoint's full gate passes dependency boundaries,
formatting and workspace Clippy, then records 246 passing tests and the same two NUL-highlight failures
(110/112 search tests). No test was ignored or filtered. Later gate stages
remain unreached; see `target/continuation-highlight-traces-gate.log`.
All 1,040 benchmark samples retain exact checksums, hit counts and match counts.
Nine of 26 cases meet D48; the complete target remains unmet. Long near-match
p95 is 14.5201 ms versus 14.8109 ms in the preceding report. The report is
`target/continuation-highlight-traces-bench.json`, SHA-256
`e3e4723c1dc634cb2040680fd1f26d82d95c2fad229d052f998f39ebabdf48bd`.

An isolated stage profile reduces near-match highlighting from 3.413 to
0.190 ms, but the complete benchmark shows only modest overall improvement.
The revised numeric-span and bounded direct-ranking experiments preserve
their differential results but remain test-only after mixed timings. The
reviewed partial grouped-count/range-ranking design is unimplemented; use the
recorded occupancy and provisional limits to measure actual index memory and
mutation costs. `docs/p1-05-search.md` records results, proofs and limits.

The original WebView2 breakpoint remains unresolved. The frizbee patch,
window/integration access and Plans 2–4 slice maps still await approval; later
implementation also needs predecessor exits. No integration app, additional
dependency patch, commit, merge, push or security change was used.

## Earlier group-score checkpoint

The latest binary is `target/local/syrinx-app-group-scores.exe`, SHA-256
`03e314bd3a689d30f28a49e8df6f70b530960e4937c391aea83e7dbc636dafa7`.
It adds original-field score reuse through live group IDs and exact narrower
recency arithmetic. Its explicitly awaited fresh synthetic normal-desktop
smoke run exited 0 with empty stderr. Evidence is
`target/continuation-group-score-app-build.log` and
`target/continuation-group-score-app-smoke.*`. Existing windows, binaries and
the whole uncommitted tree are preserved; no continuation job remains active.

The full gate passes dependency boundaries, formatting and workspace Clippy,
then records 235 passing tests and the same two NUL-highlight failures
(99/101 search tests). No test was ignored or filtered. Later gate stages
remain unreached. The final log is
`target/continuation-group-score-gate-final-2.log`. The initial compiler run
was stopped after two test references triggered duplicate generic matcher
kernels; both calls now use `&str`. The interrupted log and the intervening
formatting failure remain preserved.

All 1,040 unchanged benchmark samples retain their exact checksums, hit counts
and match counts. Long near-match p95 improves from 30.78 to 14.81 ms; 7/26
cases meet D48, which still fails overall. The report is
`target/continuation-group-score-bench.json`, SHA-256
`898453853a448898339ed6179ccdca796c743ab56e47cd30e8118e034efd888f`.
The isolated stage report preserves exhaustive results and reduces near-match
heap/scoring from 10.211 to 2.052 ms. It still spends 3.413 ms highlighting.

The numeric-span prototype remains test-only. Every row comparison passes,
but mixed timings and a 13.942 to 46.911 ms long-query regression do not justify
production integration. Its next experiment should restore the baseline's
early length rejection and omit fixed-span work for numeric-only queries.
Bounded direct ranking and shared raw highlight indices have reviewed designs
but no implementation. `docs/p1-05-search.md` records proofs, limits, evidence
and next checks; `target/continuation-numeric-spans-profile.log` preserves the
numeric diagnostic. The earlier positional prototype also remains test-only.

The original WebView2 breakpoint remains unresolved. The frizbee patch,
window/integration access and Plans 2–4 slice maps still await approval; later
implementation also needs predecessor exits. No integration app, additional
dependency patch, commit, merge, push or security change was used.

## Earlier grouped-membership checkpoint

The latest binary is `target/local/syrinx-app-grouped-search.exe`, SHA-256
`2b0c955a07d50b291510aa295dc9c9f25e287a50482ad98182f9b9c695c6f98e`.
It includes the earlier page-close and Home fixes plus query-local equivalent
field score reuse and bounded membership groups. An explicitly awaited fresh synthetic normal-desktop smoke
check exited 0 with empty stderr. Build and smoke evidence is under
`target/continuation-grouped-membership-app-build.log` and
`target/continuation-grouped-membership-app-smoke.*`. Existing binaries and windows
were preserved; no continuation job remains active.

All 1,040 unchanged benchmark samples preserve their checksums, hit counts
and match counts. Long near-match p95 falls from 367.56 to 30.78 ms, and
6/26 cases meet D48; the complete target remains unmet. The complete gate passes dependency checks, formatting
and workspace Clippy, then records 221 passing tests and the same two
NUL-highlight failures (85/87 search tests). It stops before the later gate
stages. Logs are `target/continuation-grouped-membership-bench.json` and
`target/continuation-grouped-membership-gate.log`; `docs/p1-05-search.md` records
the exact semantics, independent reviews and diagnostic results.

The positional membership prototype remains test-only: about 46.7 MiB of
extra retained memory and regressions in common queries do not justify
production integration. All approvals listed below remain pending. The
original WebView2 breakpoint remains unresolved; smoke success is not a
crash diagnosis or interactive acceptance. No integration, commit, merge,
push, dependency patch or security change occurred in this continuation.

The retained field groups use 45,477 bytes of canonical key text and 800,000
bytes of row references on the diagnostic corpus, plus bounded structural
overhead. Queries containing digits and uncached fields keep ordinary matching.
Group-ID score reuse and compiled numeric-span transitions have reviewed
designs but no implementation yet; complete ranking/scoring and numeric scans
remain the next performance work. Later plans still require their pending
slice-map approval and predecessor exits.

## Earlier continuation checkpoint

The newest source is built as `target/local/syrinx-app-continuation.exe`,
including the approved page-close patch and the correlated Home-palette fix.
Its SHA-256 is
`8ba6694077c977af211b157a2d1ed347580b078379c2092aa6fc74b3aff89201`.
A fresh-profile normal-desktop `--smoke` run was explicitly awaited and
exited 0 with empty stderr. Evidence is `target/continuation-app-build.log`
and `target/continuation-app-smoke-waited.stdout.log` / `.stderr.log`.
The first direct PowerShell GUI invocation did not supply an exit code and
is not used as exit evidence. No continuation test/build/smoke job remains
active. Earlier binaries remain available and predate the Home correction.

The Home fix gives each app-private new/reopen/saved action a request ID.
Only its matching committed Home acknowledgement can open entry. Submission,
dismissal or a superseding action cancels intent; matching no-op results
consume it. Independent review rejected the initial boolean-only version
because later real-page actions could rearm an old snapshot. All 36 app tests
now pass, including six ordering regressions and a real worker/restart test.
`target/continuation-final-gate-2.log` records passing dependency checks,
formatting and workspace Clippy, then 189 tests passing and the same two
NUL-highlight failures. The gate stops there; later store/xtask/doc-test and
smoke stages were not reached. Separate smoke evidence is recorded above.

The native close fixture again passed on the normal desktop, after a
restricted-context timeout. Both outcomes remain recorded in
`docs/page-close-event-review.md`. Neither proves the original breakpoint
fixed. A separate source-only failed-construction ownership finding is in
`docs/wry-creation-lifetime-review.md`; it is not another approved vendor
patch. No additional wry, frizbee or Windows security change was applied.

The expanded private search profiler now exercises the long near-match
shape. Complete results match the exhaustive reference; it scores all
10,000 candidates, spending a median 303.480 ms in heap/exact scoring and
21.580 ms in membership/candidate work. See
`target/continuation-search-stage-profile.log` and the final section of
`docs/p1-05-search.md`. Production search and the acceptance benchmark are
unchanged; no new performance pass is claimed.

Plans 2–4 now have an independently reviewed proposed map in
`docs/plans/Syrinx v1 — Plans 2–4 slice proposals.md` (12/16/21 slices).
Map approval, the frizbee padding patch and integration/window access remain
pending. No approval was inferred from silence. Later implementation still
waits for predecessor exits. Preserve the entire uncommitted working tree;
no commit, merge, push, personal-data read or integration use occurred.

The paragraphs below preserve earlier checkpoint evidence; the binary and
test counts above supersede their descriptions of the latest state.

The approved page-close patch is applied and independently reviewed. Use the
rebuilt standard `target/local/syrinx-app.exe` for this fix; it passed a fresh
desktop smoke run. All 99 app/platform tests passed, along with dependency
checks, workspace Clippy and scoped formatting. The synthetic native fixture
received a real page close request and verified deferred child destruction,
a usable replacement and orderly shutdown. See `docs/page-close-event-review.md`
for the binary hash and logs. Final workspace formatting detected unrelated
search-profiler edits, left untouched. The full search and interactive gates
remain open; no original-crash diagnosis or full-v1 completion is claimed.

The recovery-notice fix now distinguishes the first received session snapshot
from database revision zero; its 19 app tests, focused Clippy and independent
review passed. The newest `target/local/syrinx-app-search.exe` now includes
it, queued navigation, both search improvements described below and
a smoke-only independent 30-second deadline. It exclusively reserves a fresh
synthetic directory before arming that deadline. All 23 app tests and focused
app/search Clippy pass; independent review passed after the directory guard
was added. A normal-desktop fresh `--smoke` run exited zero with no stderr.
No verified interactive window or breakpoint diagnosis is claimed. See
`docs/integrated-browser-preview.md` for exact evidence paths.

The private search-stage profiler passed complete exhaustive-result comparison,
independent review and Clippy. It attributes the dominant short/Unicode cost
to all-scope membership; see `target/p1-05-stage-profile-1.log`. A subsequent
literal-substring shortcut passed correctness checks but failed the unchanged
full performance workload (1/26 cases, worst 396.0581 ms). All 1,040 sample
results still matched baseline. The shortcut was removed; the preceding
production membership implementation is restored, while the added semantic
regression and stage profiler remain. Read the final sections of
`docs/p1-05-search.md` before trying another optimization. Do not describe
the rejected candidate as a performance fix or ship its benchmark executable
as the current browser. Window-access and frizbee patch approvals remain
pending. The owner approved the page-close patch and narrow exception;
it is implemented as recorded above. The breakpoint cause is still unproved.
After that restoration, a Unicode-query/ASCII-field specialization was added
and reviewed. It preserves original smart-case pairs and consumes the sole
deletion on an impossible non-ASCII unit before scanning the remaining ASCII
subsequence. The full suite passes 50/52 search tests with the same NUL
failures, none ignored or filtered. The unchanged full benchmark preserves
all 1,040 baseline results; CJK/emoji p95 is about 11 ms instead of 83 ms,
but only 1/26 cases passes D48 and the worst sample is 390.8081 ms. The report
is `target/p1-05-bench-unicode-ascii-1.json`. Keep this targeted improvement
distinct from a passing performance gate. No job remains active.

The short-ASCII reachable-prefix bitset is now implemented and independently
reviewed in `membership/bitset.rs`, retaining long/Unicode/budget fallbacks.
Current full search tests pass 53/55, with only the same NUL-highlight failures
and none ignored or filtered. Its unchanged benchmark retains all 1,040
baseline results; word p95 improves from 67.40 to 40.63 ms, but D48 still fails
at 1/26 cases, worst 382.4652 ms. The report is
`target/p1-05-bench-bitset-1.json`. The latest app rebuild passes all 23 tests,
focused Clippy and a fresh normal-desktop smoke launch; exact logs are in the
preview report. No jobs remain active. Long-query exact scoring and all-scope
scan cost still need work. Do not revive the rejected substring shortcut,
reduce the corpus or change counts/ranking to meet the target.

## Repository and authorization

Work in `C:\Users\<user>\Documents\GitHub\Syrinx-app` on
`codex/integrated-browser-preview`, based on `c55b2de`. Preserve all uncommitted
P1-05 search and integration work. P1-01 through P1-04 have approved merges;
The owner authorized committing all current work after completion and the full
gate on October 11; P1-05 and this preview still lack merge approval. No remote, push,
publication, branch deletion or security-setting change is authorized.

The owner approved the exact dependency additions and licence exceptions in
`docs/integrated-preview-dependencies.md`. Do not shift the pins. Do not use
real accounts, inspect personal profiles, kill a possibly used Syrinx window,
or migrate a daily database. Keep all testing synthetic. No integration apps
without permission; fresh synthetic window testing is explicitly approved.

## What happened and what was verified

The owner reported a WebView2 breakpoint dialog, code `0x80000003`, during an
earlier restricted-context smoke run. That run returned zero but is recorded
as failed. Its cause is unproved; it does not establish a certificate
requirement. Windows security settings remain unchanged and the executable
is unsigned.

Source fixes cover browser-first shutdown, stopped workers, page-creation
acknowledgments, native navigation success, process-failure reporting, queued
navigation during creation, and deferred command-palette focus. An earlier
normal-context build rendered Home and Ctrl K, but the UI tool could not
activate its owned popup. Latest interactive behavior remains unverified.

The standard `syrinx-app.exe` could not be replaced while old process 37612
was running. Instead, this supported Rust compiler output option built current
source separately without altering the running program:

```powershell
cargo rustc -p syrinx-app --bin syrinx-app --profile local --offline --locked -- --emit=link=target/local/syrinx-app-validation.exe
```

The validation executable's revised `--smoke` passed in the normal desktop
context, taking about five seconds. It uses a fresh generated profile, native
navigation-success evidence and a short health interval before shutdown.
One passed smoke check does not prove the original crash fixed.

Evidence under ignored `target/`:

- `integrated-preview-tests.log`: earlier full workspace run, 178 tests passed.
- `integrated-preview-regressions.log`: subsequent 80-test app/platform run,
  including 11 new regressions, passed. No tests ignored or filtered.
- `integrated-preview-regression-clippy.log`: current focused Clippy passed.
- `integrated-preview-build-validation.log`: separate current executable built.
- Formatting and `git diff --check` passed. The complete latest workspace
  gate and interactive acceptance remain open.

A subsequent independent review corrected four more paths: stale replies
across palette lifetimes, repeated relative tab shortcuts reading an old UI
snapshot, normal notices hiding serious failures, and Down selecting past the
last result. `SessionCommand::CloseActive` and `CycleActive` now execute on
the worker's current model. The application suite is now **11 passing tests**,
including queued shortcut coverage, with focused Clippy passing. Logs are
`target/integrated-preview-shortcut-tests.log` and
`target/integrated-preview-shortcut-clippy.log`. Current source was built to
`target/local/syrinx-app-smoke.exe` and its `--smoke` check passed in the normal
desktop context. The already-open validation window predates these final fixes.

## Live windows and pending owner input

Later source changes now route search errors with their request ID and retain
the newest submitted address for an unloaded tab across stale snapshots. The
query-error change passed all 12 then-current app tests, including a failed
query followed by a valid query and clean restart. The five added pending-
navigation tests bring the suite to 17, but Windows blocked that new test
executable with error 4551 in both execution contexts and on an unchanged
retry. At that point those five tests had not run. Focused Clippy and independent
source review passed. Logs use the `integrated-preview-query-errors-final-*`
and `integrated-preview-navigation-intents-*` prefixes under `target/`.

Rebuilding after the bounded-search integration now produced a successful
run of all 17 app tests, with none ignored or filtered. The five navigation
tests are verified in `target/integrated-preview-bounded-search-tests.log`.
Windows settings were unchanged; the earlier refusal remains unexplained.

The app's navigation fixes built unsigned as `target/local/syrinx-app-preview.exe`
and its fresh `--smoke` run exited zero in the normal desktop context. This is
the earlier build. A subsequent build now includes the bounded search
experiment below (`target/integrated-preview-build-bounded-search.log`) and
also passed a fresh normal-desktop `--smoke` run with exit zero. This remains
startup evidence, not interactive acceptance or a diagnosis of the original
WebView2 failure. No processes from that smoke run remain intentionally open.

Recheck processes; these are observations, not durable identifiers. Old process
37612 was still running the earlier `target/local/syrinx-app.exe`. New process
29448 was launched from `target/local/syrinx-app-validation.exe` with a newly
generated synthetic profile. Its returned native window was 12715132, title
`Syrinx · integrated preview`. Do not reuse a stale handle without selection.

The latest `syrinx-app-preview.exe` was launched for the owner as process
36164 using the fresh synthetic directory
`%LOCALAPPDATA%\AtlasIntegrationTest-660e689fb4c24444bebc0a318079f13d`.
It reports responding but has no main-window handle through the process API;
this does not prove a visible window. Do not replace its executable or end
that potentially used process without coordinating with the owner.

The computer-use tool returned **Computer Use app approval timed out** before
inspecting the new window. The pending owner question asks permission to inspect
and test that window using synthetic pages and no sign-ins. Wait for permission
before trying that tool again. Also preserve the earlier request to close the
old window; do not terminate it to unlock the executable.

## Next work

### Latest search experiment: failures must stay visible

The uncommitted ASCII-presence filter in `crates/syrinx-search/src/candidate.rs`
and `catalogue.rs` is experimental. Its initial suite had **34 passing
tests and two failing differential tests**, not a passing gate. The diagnostic
run has finished; no Cargo session remains running. Read
`target/p1-05-mask-diagnostic.log` before changing this code. Both filtered and
exhaustive queries fail on the synthetic NUL query (`\0`) with
`InvalidMatchRange`: frizbee returns highlight indices equal to the field's
byte length, outside its valid indices. For example, a six-byte field produces
index 6. This reproduces without the filter. It is consistent with the source
review's zero-filled SIMD padding hypothesis, but the dependency fix is not
implemented. Do not drop these cases, discard invalid indices silently, or
weaken the existing range validation.

The exact three-file correction and provenance are now proposed in
`docs/p1-05-frizbee-maintenance-review.md`. A pending owner question asks to
apply that narrow vendor patch and retain existing upstream comments/lint
annotations. No approval has arrived and no vendor override was added.

The diagnostic benchmark `target/p1-05-bench-mask-1.json` matches the original
benchmark's counts, hits and checksums for all 1,040 samples. Only two of 26
cases pass the 5 ms target; the worst sample is 378.0657 ms. This filter does
not solve P1-05's performance requirement. The comparison was not a controlled
same-session A/B experiment, and does not replace the three acceptance runs.
`docs/p1-05-search.md` records the measurements and limitations. This is
historical evidence, not the latest source's timing report below.

Exact membership in `crates/syrinx-search/src/membership.rs` is now integrated
into the experimental production search path for zero/one deletion. A heap
scores possible finalists using a conservative bound, preserving all-scope
counts, boosts and Target-order ties. Larger budgets keep the batch matcher.
The exhaustive test oracle bypasses all new pruning. Independent review
passed. Do not use the upstream `18*n + 22` score estimate as a bound: new
tests prove it can underestimate; the implementation uses `20*n + 20`,
capped at 65535. Accepted inputs can saturate without overflowing.

The full 47-test run passed 45 and failed the same two NUL-highlight tests,
with none ignored or filtered. All new regressions passed. Focused Clippy,
formatting and diff checks passed. Logs: `target/p1-05-bounded-tests.log` and
`target/p1-05-bounded-clippy.log`. The benchmark executable built, but Windows
Application Control blocked execution. The attempted bounded report files
contain no timing evidence.

The subsequent field-specific bound now caches each field's actual boundary
count, allows the exact bonus only for full original equality and uses scalar
query length only where the Unicode scorer cannot reach its greedy path.
Independent review passed. The current full suite passes **47 of 49 search
tests**; the same two NUL tests fail. All **19 application tests pass**, now
including the queued selection/navigation correction described below.
Focused Clippy, formatting and diff checks passed. Logs use
`target/integrated-preview-field-bounds-navigation-tests.log` and
`target/integrated-preview-field-bounds-navigation-clippy.log`.

The newly rebuilt benchmark ran, but failed D48 with only **1/26 cases**
below 5 ms p95. All **1,040** results exactly match baseline counts, hits and
checksums. Worst sample: **382.8664 ms**. Long-match p95 was 24.4901 ms,
long-near-match 365.6367 ms, word 67.1408 ms and no-match 13.1374 ms. Do not
describe this as a general improvement. The report is
`target/p1-05-bench-field-bounds-1.json`; no compiler ran alongside it, but
older preview processes remained alive. It is diagnostic, not acceptance.
The benchmark job has finished; no build or benchmark session remains active.

### Latest application correction

`NavigateActive` now resolves explicit addresses on the session worker after
earlier queued selection/new-tab/close commands. `SessionSnapshot.navigate`
acknowledges the target after persistence. The UI applies that committed
destination to a native page or retains it until creation. Two new worker
regressions and independent review passed. The currently open preview at
PID 36164 predates this correction and the tighter field bound.
The latest source builds separately as `target/local/syrinx-app-navigation.exe`
(`target/integrated-preview-build-queued-navigation.log`). Its fresh-profile
normal-desktop `--smoke` run exited zero. It has not been launched as another
persistent owner window or visually inspected. All test/build/smoke/benchmark
jobs in this checkpoint are finished.

### Window report follow-up

A read-only check of the Application log's event 1000 records from the last
day found no matching `msedgewebview2.exe` record. This does not disprove the
owner's screenshot or diagnose its cause. No crash dumps or personal browser
profiles were read. The two previously launched Syrinx processes still existed,
but reported no main-window handle through the process API; that is not proof
of a visible, usable window. Interactive approval remains pending.

A subsequent independent source audit found no proven breakpoint cause.
Normal shutdown retains the parent until workers stop and releases pages and
environment before Syrinx's COM apartment guard. Two separate findings need
scoped follow-up, not an assumed crash fix:

- Each pinned wry page creation calls `CoInitializeEx` again, while its source
  has no matching `CoUninitialize`; Syrinx balances only its own apartment
  initialization. Review ownership and failure paths before proposing a fix.
- wry handles `WindowCloseRequested` by destroying its native child, but Syrinx
  has no corresponding event to close the authoritative logical tab and clear
  the UI's loaded state. `docs/page-close-event-review.md` now gives the
  proposed two-file wry callback override and idempotent session notification.
  The owner approved its API/vendor exception; implementation and the native
  fixture now pass as recorded above. A second raw handler
  after wry's destructive handler is not a documented reliable remedy.

Neither finding demonstrates why the owner's `0x80000003` dialog appeared.
No dependency patch, extra COM teardown, profile inspection or security
change was applied during this audit.

1. Use the approved computer-use scope to inspect the fresh synthetic window
   recorded above, or launch the latest verified binary in a fresh synthetic
   test directory and select it through the computer-use skill. The older
   validation window cannot verify current fixes. Verify Ctrl K typing, explicit page
   navigation, shortcuts while a page has focus, bounds, saved-row unload/reopen,
   restart persistence and shutdown. Do not automate authentication or substitute
   the Phase 0 spike.
2. Investigate any reproduced crash using structural errors and synthetic data.
   Keep the original failure recorded. Do not call startup checks proof of full
   browsing or suppress a failing check.
3. When the old window closes, rebuild the standard executable if needed.
   Validate the applied, owner-approved frizbee correction against the two
   unchanged NUL-highlight regressions. Complete the
   prescribed gate and independent review before requesting any commit/merge.
4. Continue the unfinished requirements in `docs/implementation-status.md` and
   the approved plans. P1-05's 5 ms p95 search target still fails; the GPUI idle
   patch, folders/import, Favorites, draggable sidebar, browser features and
   all three plugins are unfinished. Plans 2–4 now have approved slice maps,
   but their predecessor exits remain required.

Read-only performance reviews are now recorded in `docs/p1-05-search.md`
(matching/scoring bottleneck and exact-semantics constraints) and
`docs/gpui-idle-patch-review.md` (pinned Windows demand hooks, wakeup hazards,
measurements and vendor/order approval boundaries). The GPUI idle patch remains
unimplemented. Retained search optimizations and their unmet D48 target are
recorded in the latest checkpoint above. No worker-count change, additional
dependency patch or relaxed target was introduced by the search continuation.

## Prompt for the next session

Continue Syrinx in `C:\Users\<user>\Documents\GitHub\Syrinx-app`. Read the rules,
this handoff, the implementation ledger and current plans. Preserve all
uncommitted work and existing windows. Use the latest checkpoint above for
the verified binary, active gate and evidence; do not treat older checkpoint
results as current. The exact frizbee patch and vendor exception are approved,
applied and independently reviewed; the latest Go to row gate passes all 376
tests, including both NUL regressions and application smoke. Retain the earlier
restricted-context watchdog and native stress failures as separate evidence.
The current benchmark passes only 10/26 D48 cases, and the partial range index
and grouped bound-reuse experiment remain test-only. Check the current
diagnostic execution lane before starting any Cargo or native process.

Computer-use testing of a fresh synthetic Syrinx window is approved. On October
11 the owner confirmed Windows unlocked and Syrinx visible/responsive; Home and
the palette rendered, and clicking its edit field showed a caret. Tool value
entry failed on a stale cached element. The owner subsequently confirmed all
three manual observations: typed `about:blank` appeared, its Go to row stayed
selected after two seconds, and Enter closed the palette. Preserve this narrow
owner-reported pass. A second owner-confirmed local-fixture cycle produced
`beforeafter` after page typing, page-focused Ctrl K, palette typing and Escape,
without clicking the page again. That proves those observations in one cycle,
not general focus reliability or Enter focus return in other destinations.
Continue within the approved scope. Investigate the
unresolved WebView2 report without changing
Windows security, reading personal profiles or using real accounts. Plans
2–4's 12/16/21 slice boundaries are approved; implementation still follows
predecessor exits. D49 permits the integrated preview first without waiving
unfinished requirements. Use subagents and independent reviews. The owner has
explicitly requested a commit of all current work after completion and the full
gate. Merges still need owner approval, and no remote, push or
publication is authorized.
