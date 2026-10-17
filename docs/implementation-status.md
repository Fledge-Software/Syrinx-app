# Syrinx implementation and evidence ledger

This ledger prevents tested foundations from being mistaken for a finished browser. It records implementation evidence against all four approved plans; it changes no requirement, decision or approval. Update it when a slice gains code, verification or an approved merge. The source of authority remains `docs/decisions.md`, the spec, the design and the active plans, in the order defined by `AGENTS.md`.

The product was renamed from Syrinx to Syrinx on 2026-10-12. Evidence recorded before then keeps its original file names: executables such as `target/local/syrinx-app.exe` and logs under `target/` are named as they were built, and their hashes belong to those files. The executable is now `target/local/syrinx.exe`.

## Current checkpoint — 2026-10-14

**Where main is.** `main` is at `5a0ac2c`. Plan 1's reviewed slices reached it
through the integration-branch merge `681d992` (2026-10-12); since then the
peek fixes, DW-90's single palette popup, the Syrinx rename (`9b74aa9`), the
branding (`96c5f86`), P1-05's partitioned complete queries (`0723eed`),
P1-23's full-text bake-off harness (`5a40b3d`), DW-61 (`7cd750e`), DW-7
(`0f097af`), the fixture-timeout diagnosis (`a49ef28`), DW-64 and DW-65
(`1de8676`), P1-39a (`4307009`), P1-39b (`ef933d9`), P1-39c (`8809cd7`) and
the gate on every push and pull request (PR #4, `b44c19a`) have merged. On
2026-10-14 the rows below were corrected: every slice that an older row
described as "merged into the integration branch" or "awaiting merge" is on
main, with the merge that brought it there named in its row.

**Latest recorded gate.** On 2026-10-14 `cargo xtask check` passed on `main`
at `08cf323` (after the design-polish merge and the hand-off split) on
EZ-LAPTOP-01: 1,387 tests passed, none failed or ignored, and the
application smoke run passed (log: `target/gate-main-08cf323.log` in the
main checkout). Each later merge records its own gate in its row.

**Open Plan 1 work.** Phase 1 stays open.
- *Merged on 2026-10-14, each after independent review and a green gate:*
  AN-01, the local usage counter, which the owner allowed to start before
  P1-39d because it needs neither the release keys nor the host (its
  quiet-machine overhead and file-load run remains), and DW-68
  (`check-deps` reading the resolved `windows` features). DW-62 (the focus
  ring only after keyboard input, through a Syrinx-side keyboard-or-pointer
  flag the owner chose on 2026-10-14) has merged and waits only on the
  owner's visual check 12. DW-66 ("Reset
  update records") has merged; see the P1-39b row. DW-60 (the palette's
  selection announced to screen readers) has merged and waits only on the
  owner's NVDA check 7; see the P1-36 row.
- *Merged on 2026-10-15, each after independent review (re-reviewed after
  every fix commit) and a green gate:* DW-69, which makes every discard of a
  staged update write the staging mark first so the next start finishes an
  interrupted one, keeps the recorded update outcome until a start's
  recovery succeeds, and has the update helper record a pre-swap failure
  before it releases the update lock (`docs/p1-38-update-format.md`,
  "DW-69"); AN-02, the usage report's preview, Save report… and Copy
  report with the approved switch note restored, which stays open until
  the owner looks at the preview in the running app
  (`docs/an-02-report-export.md`); and DW-70,
  which gives the find bar, folder name, tile name and address and custom
  accent fields a node that reports focus to screen readers through one
  shared container (`crates/syrinx-app/src/text_field.rs`), the palette
  included; it waits only on the owner's NVDA check 14
  (`docs/p1-36-accessibility.md`, "DW-70"). Its probe found DW-71, the find
  bar losing keyboard focus after a few characters on a page with matches,
  which predates DW-70 and is open under P1-13. DW-63 followed once the
  owner chose Ctrl W: on a focused sidebar row it unloads a live saved page
  or closes an open tab, and it waits only on the owner's check 15.
- *P1-39d:* the owner reported P1-39 finished on 2026-10-14, with release
  bugs still being worked out; the pinned keys (DW-30) are not yet on main.
- *Blocked:* P1-24 and P1-26 on P1-23's quiet-machine runs and engine choice
  (which must also resolve the lru advisory); DW-5 belongs to Plan 2; DW-67 waits until after P1-40.
- *Owner checks and runs:* the quiet-machine runs (P1-23 first, then D48's
  for P1-05 and P1-06's idle and suspended-tab measurements), P1-33's three
  foreground palette runs, the manual window and NVDA checks in each row,
  and the owner-only slices P1-18 and P1-40.
- *Undiagnosed:* the owner's original WebView2 breakpoint crash; a passing
  run is not a diagnosis.

## Checkpoint — 2026-10-13 (historical)

This section was written on 2026-10-13 and extended on 2026-10-14 as that
day's slices landed (DW-64/65, P1-39c and the DW-66 to DW-68 rows); the
current checkpoint above supersedes it.

**Where main was.** `main` was at `6062d8c`. Plan 1's reviewed slices reached it
through the integration-branch merge `681d992` (2026-10-12); since then the
peek fixes, DW-90's single palette popup, the Syrinx rename (`9b74aa9`), the
branding (`96c5f86`), P1-05's partitioned complete queries (`0723eed`) and
P1-23's full-text bake-off harness (`5a40b3d`) had merged.

**How work reaches main now.** Each slice is built on its own branch in a local
worktree and merges to `main` with `--no-ff` once an independent review
approves it and `cargo xtask check` passes after the merge (`docs/decisions.md`,
2026-10-12). The integration branch `codex/integrated-browser-preview` is no
longer the route. Since 2026-10-13 the repository is on GitHub at
`Fledge-Software/Syrinx-app`; an agent pushes only its assigned branch, and
publication, the licence and moving the registers into issues stay the owner's.

**Latest recorded gate.** On 2026-10-13 `cargo xtask check` passed on `main`
at `9e22f9b`, after the P1-05, P1-23, P1-39-approval and licence merges and
the owner's history rewrite: 1,050 tests passed, none failed or ignored, and
the application smoke run passed. This is the first gate run on the second
development machine (EZ-LAPTOP-01, Rust 1.97.1, Windows SDK 10.0.26100.0),
from a clean `target/`. It is also the first run of DW-7's three Store-level
tests in `crates/syrinx-store/src/engine/tests/permissions.rs`, which had
been merged without the gate; all three pass. The previous recorded gate
passed 1,014 tests after the branding review.

**Open Plan 1 work.** Nothing below is finished, so Phase 1 stays open.
- *Blocked on P1-23's quiet-machine runs and review:* P1-24 (the full-text
  adapter) and P1-27's full-text participants wait for the measured engine
  choice, which also has to resolve the lru advisory. P1-26 (the page-text
  cache) is blocked by the plan on P1-25 (merged as `8737677`), the review of
  its evidence (its Q2 dependency is settled by D40); it also writes into the index
  P1-24 builds, so in practice it waits for P1-24 too.
- *Owner checks and quiet-machine runs:* D48's three fresh-process runs for
  P1-05; P1-06's quiet idle reruns and eight-suspended-tab measurement; P1-23's
  three runs per engine and corpus; P1-33's three foreground palette-acceptance
  runs; and the manual window checks listed in each slice's row.
- *Owner-only slices:* P1-18 (site and sign-in acceptance, a crash-free day) and
  P1-40 (a default-browser week with memory and start-up evidence).
- *Approved, waiting on the owner's keys:* P1-39's update delivery
  (`docs/proposals/p1-39-update-delivery.md`, approved 2026-10-13 and split
  into P1-39a to P1-39d). P1-39a to P1-39c can be built and tested with
  synthetic keys; production use waits on DW-30's release keys and the host
  set-up, both owner actions in P1-39d.
- *Analytics:* the owner approved
  `docs/proposals/an-01-02-local-analytics.md` on 2026-10-12
  (`docs/decisions.md`). AN-01 (the collector, off until switched on) is
  implemented on `claude/an-01-local-analytics` and awaits re-review (the
  owner approved its frozen command list on 2026-10-14); AN-02 (preview and export)
  precedes P1-40 and is not built.
- *Undiagnosed:* the owner's original WebView2 breakpoint crash, which the
  historical checkpoints below still record as unresolved; a passing run is
  not a diagnosis. The intermittent permission and certificate fixture
  timeouts (5 of 15 untraced runs) were diagnosed on 2026-10-13 as the old
  hidden-page suspension timer; see the P1-11 row.
- *Deferred rows still open:* DW-5, DW-30, DW-60, DW-61 (wired; awaiting
  only the owner's NVDA check), DW-62, DW-63 (Ctrl W on a focused sidebar
  row; merged, awaiting only the owner's check 15), DW-66 (a "Reset update records" action for an
  unreadable `Updates\state`), DW-67 (the "Last checked" line), DW-70
  (merged; awaiting only the owner's NVDA check 14) and DW-71 (the find
  bar losing focus while typing), in the
  plan's "Deferred work" register. DW-68 (checking the resolved `windows`
  features for networking) is done on
  `claude/dw-68-resolved-windows-features`; see the P1-39c row. DW-64 (a
  fail-fast certificate wait in the native fixture) and DW-65 (deleting the
  fixture's synthetic profiles after a fully passing run) are done on
  `claude/dw-64-65-fixture-failfast-cleanup`; see the P1-11 row. DW-7
  (forgetting a remembered site permission from Customize) is merged to
  `main`; see the P1-12 row.

## Checkpoint — 2026-10-11 (historical)

The atomic startup and retained-handle correction now pass the complete
normal-desktop gate: 436 tests, dependency boundaries, formatting, Clippy,
documentation tests and synthetic application smoke. Evidence is
`target/continuation-atomic-startup-desktop-gate.log`, SHA-256
`5654ffa30e3f5af2b8711410ea862db2d5dcb8c42b474450b294048f49ccad79`.
The current `target/local/syrinx-app.exe` SHA-256 is
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
Current `target/local/syrinx-app.exe` SHA-256 is
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
The latest verified `target/local/syrinx-app.exe` SHA-256 is
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
`a94500a`, with no merge or push. New investigation and corrections follow it.

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
The latest verified `target/local/syrinx-app.exe` SHA-256 is
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
five internal loader retries, not five agent compiler invocations. No Syrinx
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
Current `target/local/syrinx-app.exe` SHA-256:
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
historical evidence, not a Syrinx black-window diagnosis. On October 11 the
owner explicitly authorized continuation, confirmed Windows unlocked and Syrinx
visible/responsive. The integration agent observed Home and the palette in
synthetic `syrinx-app-focus-20261010` process 46216, window 6097856. Clicking edit
3 showed a caret/popup focus; setting its value failed because the element was
unavailable in cached app state, and the refreshed list exposed only the main
window. Manual `about:blank` entry, a two-second wait and Enter were requested
to verify typing, stable Go to selection and palette closure. That initially
pending check is now superseded by the owner's “All three worked” confirmation.
Unlock is no longer required. This tool limitation does not prove a Syrinx
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
current `target/local/syrinx-app.exe` has SHA-256
`3d8a38e8b542609961a1949cdb9d1f0b21c8ec054cceb257194aa38cc56f15aa`.
No security setting changed. Separate expanded native stress runs failed
navigation during repeated local-fixture creation; diagnosis and the original
breakpoint remain open. The owner-approved stable address row is being built
after this checkpoint and is not yet validated. See the current handoff.

The disposable v5 failed-construction harness now passes all 30 synthetic
cases, with matching exit status, PASS markers and expected cleanup diagnostics.
Its per-executable Common Controls v6 manifest corrects the v4 harness loader
failure; it does not change Syrinx's release configuration. This is partial
runtime evidence for the approved wry ownership patch, not a diagnosis of the
original crash or a passing full gate. The aggregate is
`target/continuation-wry-fault-v5-results.json`, SHA-256
`384902be7ab55637cc496b05d89028a557547e4bd7269b63af7bd7d2a10c72a0`.
The handoff records source identities and remaining coverage limitations.

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

### Earlier in-place mutation checkpoint

The current binary is `target/local/syrinx-app-digit-counts.exe`; its awaited
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

The current binary is `target/local/syrinx-app-highlight-traces.exe`; its awaited
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

The current binary is `target/local/syrinx-app-group-scores.exe`; its awaited
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
`target/local/syrinx-app-grouped-search.exe`. Its awaited fresh synthetic
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
The rebuilt standard `target/local/syrinx-app.exe` passed a fresh desktop
smoke check. See `docs/page-close-event-review.md` for its binary hash and
logs. A final workspace formatting check found a concurrent change in the
unrelated search profiler; it was left untouched. Search correctness and
performance, interactive acceptance and the original breakpoint cause remain
open. No commit, merge or upstream publication has been made.

Latest evidence supersedes the historical checkpoints below: the final
watchdog/profile-reservation changes pass all 23 app tests, focused Clippy
and independent review. `target/local/syrinx-app-search.exe` passed a fresh
normal-desktop smoke run with exit zero. Interactive acceptance and the
breakpoint diagnosis remain open. Unicode-query matching against cached ASCII
fields and short-ASCII bitsets improve relevant benchmark cases while preserving
all 1,040 results. The full search suite passes 53/55 tests with the same two NUL
failures, and only 1/26 timing cases passes D48 (worst 382.4652 ms). No test,
build, benchmark or smoke job is still running. See the detailed preview and
search reports; no full gate, slice completion, commit or merge is claimed.

- All four plans and Plan 1's revised 40-slice map are approved. The full four-plan goal remains unfinished. D49 explicitly brings the integrated browser preview forward while preserving unfinished features and performance gates.
- Current branch: `codex/integrated-browser-preview`, checkpoint base `c55b2de`. P1-01 through P1-04 were independently reviewed and owner-approved for merge; product commits are `c30e4e7`, `0641826`, `a30ef72` and `6fbfc97`. On October 11 the owner authorized committing all current work after completion and the full gate. P1-05 and the preview still lack merge approval. Check Git for the resulting commit state; no remote or push is authorized.
- `target/integrated-preview-tests.log` records **178 passing tests**, zero ignored/filtered cases, and completed documentation-test stages: app 7, core 30, platform 62, search 26, store/engine 39 and xtask 14. Formatting, dependency boundaries and workspace Clippy passed. A subsequent focused Clippy pass covers the pending-page navigation correction. These results do not constitute the final application smoke/interactive gate.
- The unsigned executable rendered Home and Ctrl K in the normal desktop context. Typing, real page navigation, page-focused shortcuts, save/unload/restart and final shutdown are **not yet verified**. An earlier restricted-context run produced a WebView2 breakpoint dialog; its cause remains unproved. Do not count its zero process exit as success.
- At this check, old preview process **37612** is still running. `target/integrated-preview-build-4.log` shows Windows refusing to replace its open executable. The owner has been asked to close it. Recheck the PID before acting; do not kill a possibly used window or test the old binary as if it contained the latest fixes.
- A later build emits the current source to `target/local/syrinx-app-validation.exe`, leaving that old executable alone. Its revised `--smoke` check passed in the normal desktop context. Eleven new regressions also passed in an 80-test app/platform run, with focused Clippy and formatting passing. A fresh validation window is open, but interactive inspection awaits computer-use app-access approval. This does not resolve the earlier crash's cause or complete the GUI gate; see the updated preview report.
- The dependency additions have a completed audit and exact licence approval in `docs/integrated-preview-dependencies.md`. Product metadata contains no registry package/version absent from that proposal. No GPUI idle patch or passing D48 search benchmark exists.
- A further review fixed palette reply invalidation, queued relative shortcuts, persistent failure reporting and result selection. The app now has 11 passing tests, focused Clippy passes, and a rebuilt `syrinx-app-smoke.exe` passed startup/shutdown. The existing open validation window predates these changes; neither interactive acceptance nor the full latest workspace gate is claimed.
- The later P1-05 ASCII-presence experiment has **34 passing search tests and two failing differential tests**. Both the filtered and exhaustive paths fail for a synthetic NUL query because frizbee returns out-of-range highlights. Its diagnostic benchmark preserves all 1,040 sample results but passes only 2/26 latency cases, with a 378.0657 ms worst sample. These current failures supersede earlier search correctness passes for the changed working tree; no completed performance fix is claimed.
- Query errors now use request-scoped replies and reach the palette without letting stale failures affect current state. All 12 app tests passed after independent review (`target/integrated-preview-query-errors-final-tests.log`). The narrow frizbee source correction in `docs/p1-05-frizbee-maintenance-review.md` awaits owner approval; it has not been applied.
- Unloaded tabs now retain the latest explicit address across stale snapshots; independent review and Clippy passed. Windows initially blocked the 17-test executable, but rebuilding after the bounded search integration produced a successful run of all 17 tests (`target/integrated-preview-bounded-search-tests.log`). The older unsigned `syrinx-app-preview.exe` passed its fresh-profile smoke check. Interactive validation remains pending.
- Exact zero/one-deletion membership and conservative bounded ranking are now integrated experimentally. The full search suite passed 45 of 47 tests; the same two NUL-highlight regressions fail. New bound, saturation, scope/count and membership regressions passed, as did independent review and focused Clippy. Windows blocked the rebuilt benchmark executable in both execution contexts, so there is no timing result for this source. `docs/p1-05-search.md` preserves the evidence and unresolved D48 gate.
- The newest `syrinx-app-preview.exe` now includes bounded search and passed a fresh normal-desktop startup/shutdown check. It was then launched for the owner in a new synthetic profile as PID 36164. The process responds, but visible-window and interactive behavior remain unverified; do not replace or terminate a possibly used window. Build evidence: `target/integrated-preview-build-bounded-search.log`.
- Subsequent source now resolves navigation after queued tab selection on the session worker and tightens search bounds using cached field metadata. All 19 app tests pass; 47 of 49 search tests pass, with the same two NUL failures. Independent reviews, focused Clippy and formatting passed. The open preview predates these changes. The new benchmark finished: all 1,040 results equal the baseline, but only 1/26 timing cases passes, with a 382.8664 ms worst sample. See `target/integrated-preview-field-bounds-navigation-tests.log` and the two detailed reports.
- That source built unsigned as `target/local/syrinx-app-navigation.exe` and passed a fresh normal-desktop startup/shutdown check. Interactive acceptance remains pending. The separate page-close correction subsequently received explicit API/vendor approval and is implemented and verified as recorded at the top of this ledger.

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

The complete requirements and blocking edges remain in `docs/plans/Syrinx v1 — Plan.md` and its matching readiness-report map. Every approved ID is accounted for below.

| Phase / slice | Status and existing evidence | Remaining exit work |
| --- | --- | --- |
| Phase 0 | Conditional GPUI go; `docs/research/09-phase-0-verdict.md`. Passkey/Workspace, YouTube and Disney+ passed in throwaway spikes. | D37 idle condition and remaining owner site checks still apply to the product. Spike results do not certify this implementation. |
| 1 / P1-01 scaffold | **Merged/tested.** Cargo workspace, `crates/xtask`, dependency rules, local gate and development guide. | Preserve the same checks as crates are added. |
| 1 / P1-02 tab model | **Merged/tested.** `syrinx-core`: checked identities, saved/live association, ordering, selection and 20-entry web closure stack. | Later UI/folders must preserve D42/D43; plugin kinds do not implement a plugin runtime. |
| 1 / P1-03 storage | **Merged/tested.** `syrinx-store` and native directory lease: serialized writes, migration, snapshots, revision checks, identity/path protection and long-path coverage. | Later schemas/features need their own persistence and recovery tests. |
| 1 / P1-04 backup/recovery | **Merged/tested.** Clean/dirty lifecycle, daily idle snapshot mechanism, seven-copy rotation, validated owner-confirmed/resumable recovery and pure deletion-record contract. | Preview scheduling/shutdown needs real integration verification. No recovery confirmation UI exists yet. Actual history-deletion persistence/replay belongs to P1-27; schema 1 refuses unsupported deletion state. |
| 1 / P1-05 tier 1 search | **Partial; the D48 work merged to main as `0723eed` (2026-10-13).** Branch `codex/p1-05-d48` partitions complete queries and shares equivalent field scores within each query. Seven new regressions and all 1,040 full-result comparison pairs pass. The full gate passes 445 workspace tests and application smoke in `target/d48-gate-20261012-jobs2.log`. Review corrections then moved the 100,000-row timing diagnostics behind the opt-in `acceptance-diagnostics` feature, so the gate no longer runs them, and recorded refused partition workers instead of discarding the error; that gate passed 444 tests (`target/d48-review-gate-20261012.log`). The dated design and diagnostic evidence are in `docs/p1-05-search.md`. | D48 remains unverified for this implementation: no build-free window has been available. Retain 100,000 rows, `safe_read`, exact counts/highlights and the 5 ms p95 limit. On a quiet machine, run the preserved baseline executable three times (stop if it differs materially from the historical 10/26), then obtain three passing fresh-process `cargo xtask bench` runs of the candidate, recording the logical processor count beside them. |
| 1 / P1-06 GPUI idle | **On main (integration merge `d3bbb8c`, reaching main through `681d992` on 2026-10-12); owner checks pending; provisional D37 pass.** Built on branch `claude/p1-06-gpui-idle-v2`. Approved demand-driven repaint patch vendored as `vendor/gpui-pre-windows-0.3.8` with provenance and upstream-ready diff. Idle `WM_PAINT` 59/s to 0/s; idle CPU 1.66–2.00% to 0.23–0.46% of one core (harness and fresh-profile `syrinx-app`, 300 s, two runs each, on a busy machine): a provisional pass only, 0.46% being 8% under the bar. Evidence in `docs/evidence/p1-06/`. Two-window behaviour harness and eight latch unit tests pass. See `docs/p1-06-gpui-idle-results.md`. | Quiet-machine idle reruns and the eight-suspended-tab measurement (P1-15 is now integrated) are still pending before D37 passes outright; five-launch first-page startup comparison and owner window/touchpad inspection. Public submission requires permission. |
| 1 / P1-07 design tokens | **On main (integration merge `af4f93c`, reaching main through `681d992` on 2026-10-12); palettes owner-approved.** `crates/syrinx-ui` holds the five canvas palettes, accent states, Reduce-transparency fallback, metrics, PROPOSED D44 width states/thresholds and a WCAG audit; `syrinx-app` colours come from Graphite unchanged; `--palette-sample` renders palettes and width states. The owner reviewed the sample window on 2026-10-12 and approved the palettes, and accepted the 16 recorded contrast shortfalls for now. See `docs/p1-07-design-tokens.md`. | D44 drag thresholds and peek edge stay PROPOSED until the owner tries them in a draggable sidebar (P1-08). `ui/recovery.rs` colours move to tokens with their slice; `SIDEBAR_WIDTH` was replaced by the `syrinx_ui::sidebar` tokens on the P1-08 sidebar branch. |
| 2 / P1-08 sidebar width states | **Partial; on main (integration merge `fe42daa`, reaching main through `681d992` on 2026-10-12); awaiting owner window check.** The edge between sidebar and page card drags through D44's wide (264 px), favicon-only (52 px) and hidden states and back, snapping at 158/26 px with labels shown exactly when a release would settle wide (`syrinx_ui::SidebarSizing`). A release outside the window snaps where the button came up; window deactivation settles a drag in progress. The page card's physical bounds come from one tested function (`ui/page_card.rs`) on every drag step, resize and DPI change, and the drag strip and card share one edge calculation, tested not to overlap from 0 to 264 px; `SIDEBAR_WIDTH` is gone. Only the shown page follows drag steps; hidden pages take the card's bounds when shown (`syrinx-platform/src/browser/placement.rs`). The edge is a focusable splitter whose Left and Right keys step between states; rows are tree items with levels, and favicon-only rows keep their titles as accessible labels. Hidden keeps the selection. Review fixes landed 2026-10-12. The settled state persists in P1-29's `sidebar.width_state` setting and is restored at start-up (DW-2 Done; tested from settle to restore). P1-29's Favorites render as the four-column grid when wide, one column of named icon tiles in favicon-only and not at all when hidden. See `docs/p1-08-sidebar-states.md`. | Owner's manual checks in that note (no agent window inspection), including Favorites in favicon-only and the width surviving a restart. Ctrl Alt S and peek belong to P1-17 (DW-3); peek must share the hidden handle's left-edge zone. Shared-environment, guardrail and runtime-disclosure items below still apply. |
| 2 / P1-09 saved rows and folders | **Partial.** Saved, open and ordinary rows render in wide and favicon-only widths from the core snapshot. P1-21's folders render in layout order with depth indentation. From `claude/p1-09-21-folder-ui`, reviewed and on main (integration merges `6fd4272` and `c5bdd1c`, reaching main through `681d992` on 2026-10-12; the owner's window check is pending): rows are focusable tree items (Enter/Space activate through `activate()` or toggle), folder expansion is persisted as the `sidebar.expanded_folders` setting (open folders only, so imports and unnamed folders start collapsed), and the peek draws the same rows and controls. See `docs/p1-09-21-folder-ui.md`. | Prove repeated lifecycle changes create no duplicate or leaked views; compact-folder affordances need owner review (D44, owner question 1 in `docs/p1-09-21-folder-ui.md`). |
| 2 / P1-08–P1-10 page/sidebar/navigation | **Partial.** `syrinx-platform/src/browser*` and app session/UI source connect shared environment, page bounds, model/store/search, saved/open rows, navigation, shortcuts and owned entry popup. The current gate passes 54 app tests. The owner reports the `about:blank` text/stable Go to/Enter-close check and one local-fixture page-focused Ctrl K, palette typing and Escape focus-return cycle pass. | Verify repeated focus reliability, Enter focus return in other destinations, broader page behavior, persistence, close/reopen and no duplicate/leaked views. D44 dragging and resize/DPI following are now in the P1-08 row above (owner window check pending); UI reordering is absent. Verify page guardrails and runtime disclosure in the running product. |
| 2 / P1-11 popup adoption | **On main (integration merge `e301c13`, reaching main through `681d992` on 2026-10-12); owner checks pending.** Slice report as written before the merge: **Partial checkpoint on `codex/p1-11-popups`.** The 2026-10-12 full gate passed all 439 tests and application smoke without error 4551. Final-source synthetic popup adoption, opener/message exchange, document focus, focus-command completion and active-child visibility pass even when Windows denies foreground activation. See `docs/p1-11-popups.md`. | The original lifecycle suite's intermittent timeout (tab 1 created with its source changed, but no navigation completion or close within 45 seconds) is diagnosed on `claude/diagnose-lifecycle-timeout`: the fixture's hidden close page was suspended by P1-15's five-second hidden-page grace while still loading. A 6 s held response reproduced it 2/2. The fixture now shows each close page once created, adds `--stall-first-close` as the regression check and asserts P1-08's hidden-page placement contract instead of the obsolete survivor-bounds check; no deadline was changed. See "Diagnosis of the lifecycle timeout" in `docs/p1-11-popups.md`. The intermittent permission and certificate suite timeouts seen in 5 of 15 untraced runs after that fix are diagnosed on `claude/fixture-timeout-diagnosis` (2026-10-13; see `docs/fixture-timeout-diagnosis-plan.md`): the same old five-seconds-from-navigation-start suspension froze the two pages those suites never show, tab 2003 and tab 4000. Step 0 ran the unmodified fixture 20 times on `main` at `6a23836` on EZ-LAPTOP-01, and all 20 exited 0 with all 11 pass lines in 31.6–35.8 s (median about 32.2 s); twenty passes would happen by chance about 0.03% of the time at the recorded rate, though the recorded runs were on the original machine. A disposable `--stall-hidden-pages` reproduction, holding tab 2003's page and the first TLS handshake for 6 s, failed both suites 3 of 3 times on `bbe59d3` at the labelled waits, with `Suspended … true` five seconds after each navigation began, and passed 3 of 3 on `6a23836`. A separate mechanism experiment showed that an HTTPS attempt failing without a certificate event also becomes a bare 45 s timeout; the owner approved a fail-fast wait (at most 5 s after `Failed`; DW-64) and deleting the synthetic profiles only after a fully passing run (DW-65). Both were built on `claude/dw-64-65-fixture-failfast-cleanup` and merged to main as `1de8676` (2026-10-14): the certificate suite now fails about 5 s after an attempt's `Failed` state with a message naming the attempt, the last connection state and `server.log` (proved with a disposable listener-closing server: 5.0 s after `Failed`, all seven directories kept and listed), and a fully passing run deletes exactly the directories it created (three unchanged runs passed and left none). The 45 s deadline and every other assertion are unchanged. The five original failures were untraced, so their link to this cause rests on matching symptoms. No product code, assertion or deadline changed. Following the owner's 2026-10-12 decision, a hidden page's five-second suspension countdown now starts when its navigation finishes, with a 60-second limit for loads that never finish (`browser/hidden_timer.rs`; see `docs/p1-15-suspension-fullscreen.md`). A green combined native fixture, independent review and owner visual/sign-in checks remain. Later passing runs are recorded in the P1-12–P1-14 reports; they do not diagnose the original intermittent failure. |
| 2 / P1-12 permissions | **On main (integration merge `e301c13`, reaching main through `681d992` on 2026-10-12); owner checks pending.** Slice report as written before the merge: **Implemented and independently reviewed on `codex/p1-12-permissions`; owner visual acceptance pending.** Syrinx consent bar, explicit allow/deny/remember, schema-2 origin/kind persistence, navigation/close cancellation and default notification delivery. All 445 tests and smoke pass in `target/p1-12-gate-final.log`; synthetic native permission/persistence/notification, lifecycle and popup suites pass in `target/p1-12-native-3.log`. See `docs/p1-12-permissions.md`. | Owner visual checks listed in the report; permissions are schema version 6 after integration (migration appended after P1-27's version 5). DW-7 (2026-10-13, `claude/dw-7-forget-permission-ui`, merged to main as `0f097af` on 2026-10-14): Customize → Site permissions → Manage lists every remembered choice and forgets one so the site asks again; a Forget click acts only on the row as drawn and only while the choice is still saved with that value. Owner check: open Customize on Home, remember a choice on a test site, forget it and see the site ask again. The original intermittent lifecycle navigation timeout remains known and undiagnosed; owner permits continuation. |
| 2 / P1-13 essentials | **On main (integration merge `e301c13`, reaching main through `681d992` on 2026-10-12); owner checks pending.** Slice report as written before the merge: **Implemented and independently reviewed on `codex/p1-13-essentials`; owner visual acceptance pending.** Syrinx find bar with native matching, native download list/print/context menu, visible zoom and page-first keys: Ctrl F, Ctrl H, Ctrl P and the zoom keys reach a focused page first, and Syrinx captures only Ctrl J among them (owner decision 2026-10-12). All 448 tests and smoke pass in `target/p1-13-gate-final.log`; native suites pass in `target/p1-13-native-final.log`. See `docs/p1-13-essentials.md`. | Owner typing/layout, actual download and print-preview checks are listed in the report. Integration folded the seven commands into the one registry; History opens P1-20's list. The known original lifecycle timeout remains undiagnosed; owner permits continuation. |
| 2 / P1-14 security display | **On main (integration merge `e301c13`, reaching main through `681d992` on 2026-10-12); owner checks pending.** Slice report as written before the merge: **Implemented and independently reviewed on `codex/p1-14-security-display`; owner visual acceptance pending.** Native scheme/navigation state, truthful attempt/error labels and explicit certificate cancellation with readback. All 454 tests and smoke pass in `target/p1-14-gate-final-2.log`; all five native suites, including repeated self-signed rejection and recovery, pass in `target/p1-14-native-4.log`. See `docs/p1-14-security-display.md`. | Owner visual checks in the report remain. HTTPS is not a site-safety assurance. The known original lifecycle timeout remains undiagnosed; owner permits continuation. |
| 2 / P1-15 suspension/fullscreen | **On main (integration merge `e301c13`, reaching main through `681d992` on 2026-10-12); owner checks pending.** Slice report as written before the merge: **Implemented on `codex/p1-15-suspension-fullscreen`; independent review and owner visual acceptance pending.** Native fullscreen events, whole-viewport page layout, Escape/F11/page exit, five-second hidden grace, native suspension/resume readback and controller/request callback guards. All 456 tests and smoke pass in `target/p1-15-gate-final.log`; all six native suites pass in `target/p1-15-native-final.log`. The application measurement mode confirms eight genuinely suspended tabs. See `docs/p1-15-suspension-fullscreen.md`. | Owner GPUI fullscreen/DPI/restoration checks and independent review remain. P1-06 owns the five-minute idle CPU and startup measurement. The original intermittent lifecycle timeout remains undiagnosed; its passing run does not establish a fix. |
| 2 / P1-16 process recovery | **On main (integration merge `e301c13`, reaching main through `681d992` on 2026-10-12); owner checks pending.** Slice report as written before the merge: **Implemented on `codex/p1-16-process-recovery`; independent review and owner visual acceptance pending.** Page-local renderer crash/Reload, shared environment/controller reconstruction, retired-generation deduplication and retained retry records. Synthetic faults on real controllers prove no duplicate recovery or lost live IDs, preserve healthy siblings, and cover environment/controller retry. All 461 tests and smoke pass in `target/p1-16-gate-2.log`; all six native suites pass in `target/p1-16-native-final.log`. See `docs/p1-16-process-recovery.md`. | Owner crash-card, reload/focus and recovery presentation checks plus independent review remain. Fault injection is test-only; no real process was terminated. The original intermittent lifecycle timeout remains undiagnosed. |
| 2 / P1-17 peek | **On main (integration merge `7ea8276`, reaching main through `681d992` on 2026-10-12; the peek fixes merged to main as `e8fb77f` and `9a46aec` on 2026-10-13); owner checks pending.** Built on `codex/p1-17-peek`. Owned popup shares the full sidebar, Favorites and search; 250 ms hover and press-to-drag disambiguation; Ctrl Alt S from chrome and page focus restores the last visible width, including persisted restoration (DW-3 Done). Hidden navigation/address controls occupy the client title bar. Independent source review completed; `cargo xtask check` passed on 2026-10-12 (599 tests and application smoke), logged in `target/p1-17-gate-complete.log`. Pure hover/drag, shortcut, persistence and layout tests plus native owner/topmost/destruction test pass; see `docs/p1-17-peek.md`. **Peek crash fixed on `claude/fix-peek-double-lease` (2026-10-12):** opening the peek from the hover sampler panicked because GPUI's `open_window` draws synchronously and the peek's render updates `BrowserView`; the window now opens from a deferred callback outside every update, with generation rechecks. A drag-to-hidden plus edge `WM_MOUSEMOVE` script on a synthetic profile reproduced the panic before and showed the peek without panic after (four cycles). Activation changes no longer close the peek: GPUI reports bounds on every activation change, so the watch now retires only when the window's bounds, viewport or scale changed; posted click and reactivation keep the peek open, and a real move still closes it. | Owner manual checks of live-page focus/stacking, Alt Tab, drag, title-bar hit testing, minimize/restore/close and multi-monitor placement. Desktop inspection was unavailable after restart because the local Computer Use native pipe could not connect; smoke is not visual acceptance. |
| 2 / P1-18 acceptance | **Missing.** | Owner site/sign-in checks, one crash-free day, product idle/startup and complete shortcut/lifecycle evidence. |
| 3 / P1-19 session restore | **On main (integration merge `30b41a2`, reaching main through `681d992` on 2026-10-12); owner checks pending.** Built on `claude/p1-19-20-restore-history`. A subprocess test ends a real session worker with `process::exit` (no store shutdown, WAL checkpoint or clean marker), restarts it and proves `Unclean` detection, acknowledged order/associations/titles, unloaded saved rows staying unloaded, all-or-nothing for the queued change and fixed saved targets; the UI requests only the active page from a restored snapshot. See `docs/p1-19-20-restore-history.md`. | Owner's running-app checks: normal restart and a Task Manager kill with several tabs and saved rows. Back/forward lists are not restored (not promised). |
| 3 / P1-20 history | **On main (integration merge `30b41a2`, reaching main through `681d992` on 2026-10-12); owner checks pending.** Built on `claude/p1-19-20-restore-history`. Schema version 4, after P1-29's preferences version 3 (`history_urls`, `history_visits`, index) with upgrade/rollback/refusal tests; worker records a visit only when a live web tab reports a new http(s) address, ignoring replayed/stale callbacks and restored reloads; tier-1 catalogue updated row by row through the existing public API (no `syrinx-search` change); palette History results open a new tab; Ctrl H/toolbar history list of 200 recent visits; `Store::apply_history_deletions` is the D47 participant, proved by replaying a cleared range onto an older copy. | Owner's running-app check of the list and palette results. Ctrl H while a page has focus needs a one-line accelerator in `syrinx-platform/src/browser/page.rs`. Clearing UI, journal persistence, catalogue removal on clear and recovery replay remain P1-27. |
| 3 / P1-21 folders | **Partial; headless model and sidebar UI both on main (integration merges `f59c0ca` and `6fd4272`, reaching main through `681d992` on 2026-10-12); the owner's window check is pending.** `syrinx-core` folder create/rename/move/reorder/remove with cycle rejection and unchanged live-page association; `syrinx-store` schema version 2 migration (`saved_folders`, `saved_layout`, `saved_folder_state`) with upgrade, rollback and restart tests. Owner decision 2026-10-12: removing a folder moves its contents up by default; deleting contents asks for confirmation; empty folder names are refused. The branch adds the model refusal (`TabError::EmptyFolderTitle` in create, rename and import append), `SessionCommand::Folder` with worker restart tests, and the wide sidebar's New folder form, rename (✎/F2), removal card with a separate confirmed delete, drag-and-drop and Alt-arrow moves (`ui/saved_moves.rs`, tested against the model). See `docs/p1-21-folders-core.md` and `docs/p1-09-21-folder-ui.md`. | Owner questions on compact folders (D44) and control placement; search treatment of folders; running-app restart check by the owner. |
| 3 / P1-22 import | **On main after two reviews (integration merge `0935704`, reaching main through `681d992` on 2026-10-12); awaiting the owner's manual check.** Review fixes: imported and restored folders start collapsed; the store keeps unchanged saved rows and the catalogue syncs only changes, so a change after a 50,000-entry import takes about 43–48 ms instead of about 1 s; Enter/Escape in the preview; Alt+Enter searches. Parser (merged 2026-10-12) plus: DW-1 done (`syrinx_core::web_address`, `url` 2.5.8 already in the lockfile, shared by navigation and import); `commit_bookmark_import` appends one new top-level folder named after the file, of unloaded saved rows in export order, re-checking every address and reporting refusals; the session worker writes it with one store save, so an interrupted import writes nothing (trigger-injected store tests). `>import bookmarks` in the palette opens Windows' file dialog (`syrinx-platform::file_dialog`, existing bindings) and an owned preview window (`ui/import.rs`, syrinx-ui tokens) with counts, depth, duplicates and skipped entries by reason, then Import or Cancel. See `docs/p1-22-import-parser.md`. | Owner runs the import on their own export and checks the preview, the new folder and a restart. The sidebar is not virtualized, so expanding a very large folder still builds its rows. Moving `>import bookmarks` into the command registry belongs to the palette merge. Search of imported rows follows the existing catalogue sync. |
| 3 / P1-23 full-text bake-off | **Partial: the two-engine harness merged to main as `5a40b3d` (2026-10-13) as `crates/fulltext-bench`; paired measurements not yet run.** Built on `codex/p1-23-fulltext-bakeoff`. An independent review's corrections (memory measured per process, expiry compared on equal terms, reopen and verification timed apart) passed the gate in `target/p1-23-review-gate-20261012.log`; earlier smokes and the FTS5-only baseline used the old protocol and are not comparable. `cargo xtask check-deps` quarantines Tantivy: only `fulltext-bench` may depend on it, and nothing may depend on the benchmark. The owner-approved Tantivy 0.26.2 graph adds 36 audited packages without moving existing pins; the scoped lru advisory remains documented. Both engines share the original and broader synthetic corpora, query-count checks and expiry/recovery/replay protocol. The final 440-test gate and both 2,000-document recovery smokes pass (`target/p1-23-final-gate.log`). A twenty-minute quiet-window wait expired before launching any full-size comparison; its load log is `target/p1-23-comparison-20261012-load.csv`. See `docs/p1-23-fulltext-audit.md` and `docs/p1-23-fulltext-bakeoff.md`; the older FTS5 measurements remain historical evidence. | Finish the requested three fresh-process runs per engine/corpus with the corrected harness (`run-comparison.ps1`) when Cargo and rustc are absent, review the measured engine recommendation (FTS5 stays the control), and resolve the lru advisory (RUSTSEC-2026-0253 in `lru 0.16.4`) in an audited upstream graph before any production Tantivy integration. P1-24 remains unstarted. |
| 3 / P1-24 full-text adapter | **Missing.** | Waits for the reviewed P1-23 choice; then the adapter. |
| 3 / P1-25 page-text extraction | **On main (built on `claude/p1-25-page-text` with review fixes and the owner's 2026-10-12 decisions; integration merge `8737677`, reaching main through `681d992` on 2026-10-12); the review of its evidence that P1-26 needs is still open.** `syrinx_core::judge` runs the approved eligibility steps before access (Google, banking/payment and owner lists, redirect chain, every exit from an excluded site Syrinx did not start (with carried taint and popup seeding) and every form `POST`, response status/type/`no-store`, generation) and alone creates the `EligibleDocument` the capture requires; `syrinx-platform` observes navigation and response headers (no filter, no body) and makes one `DOMSnapshot.captureSnapshot` call, parsing off the page thread and keeping only the root document's visible, non-editable text. The app never switches capture on. Synthetic native harness `examples/page_text_feasibility.rs`: F1, F3–F13 and F15–F28 pass (F15 identical, so no fallback; F8 500 races, zero bank calls; F21–F28 exits, `POST` redirects, carried taint, popups and Back into a tainted entry); F14 recorded. See `docs/p1-25-page-text.md`. | **F2 fails as an accepted limit** (owner, 2026-10-12): WebView2 shows only the first line of a repeated `Cache-Control` header. Owner review of the starter banking list and the six added Google domains; the size check runs after UTF-16 conversion (direct `windows-core` dependency not approved). Interface-frame cost, F12 DevTools key and a suspension requested during an in-flight call unverified. Popups are never read (no observed first response). P1-26 must re-check the setting on `Captured` and decide taint across session restore (not saved, so restored tabs start untainted). Storage, setting and expiry are P1-26. |
| 3 / P1-26 page-text cache | **Missing.** | Seven-day cache outside backups; capture setting; excluded content never stored. |
| 3 / P1-27 clear browsing data | **On main after three reviews (built on `claude/p1-27-clear-data`; integration merge `4d45931`, reaching main through `681d992` on 2026-10-12); owner checks pending.** Schema version 5 (`history_deletion_state`); durable `syrinx.deletions` journal held by the session; clear dialog from the history list (kinds, five ranges, per-part outcome); coordinated Syrinx history, live catalogue/recorder and WebView2 profile clearing; startup reconcile; backups record their generation and recovery replays the journal onto the staged copy. See `docs/p1-27-clear-data.md`. | Owner's running-app checks in that note. Full-text participants arrive with P1-23–P1-26. The palette's "Clear browsing data…" command (registered in `syrinx-core`'s registry on `claude/p1-32-address-bar`) opens the same dialog. DW-4 done: visits older than 90 days expire at start-up and at most daily through the journal. P1-12's permissions follow as schema version 6. |
| 3 / P1-28–P1-29 home/settings | **On main (integration merge `124f273`, reaching main through `681d992` on 2026-10-12); owner checks pending.** Built on `claude/p1-28-29-home-settings`. Canvas home page (Customize, search pill opening the palette, 8-per-row Quick access with add/edit/move/remove, unavailable plugin cards); separate square sidebar Favorites (D45) with the same editor; plugin targets stay dimmed and report "not available yet"; `syrinx-core` destinations model; schema version 3 (`settings`, `quick_destinations`) with round-trip, upgrade, rollback, damaged-row and worker-restart tests; Customize persists all five palettes, preset/custom accent and Reduce transparency and re-renders live. See `docs/p1-28-29-home-settings.md`. | Owner's manual window checks in that note; the header's local date comes from `syrinx_platform::calendar` (`GetDateFormatEx`), merged with P1-32; Glass Acrylic via `syrinx-platform`; whether Reduce transparency follows the Windows setting (owner question); accessible names (P1-36). |
| 4 / P1-30 command palette | **On main (integration merge `a29488b`, reaching main through `681d992` on 2026-10-12); owner checks pending.** Built on `claude/p1-30-31-palette`. `syrinx-core` command registry (identifiers, scope, enablement, action enum, shortcut conflicts rejected) holding the eleven browser commands (Ctrl H history included) with their existing shortcuts; `ui::bind_keys` derives key bindings from it. P1-10's owned popup now lists registry-backed command rows, reads `>` commands, `@` plugins (empty "No plugins yet" state) and `/` open tabs, and keeps keyboard selection, Enter, Esc and focus restoration. See `docs/p1-30-31-palette.md`. | Owner's manual checks in that note. Plugin commands need Plan 2's host. The palette sample window is not a command (it quits the app). |
| 4 / P1-31 chips, highlights, previews | **On main (integration merge `a29488b`, reaching main through `681d992` on 2026-10-12); owner checks pending.** Built on `claude/p1-30-31-palette`. Scope chips with counts (History rows and counts from P1-20), multibyte-safe highlight conversion, bounded metadata previews from `syrinx-ui` tokens, one `syrinx-core` activation route (`activation.rs`) used by the palette and sidebar so a saved page focuses its live tab, collapsed duplicate rows and staged command rows that cannot outlive their request. | Tier 2 (page-text) previews wait for P1-23–P1-27; Favorites join the same route with P1-28. Running-app verification of highlights, focus return and de-duplication. |
| 4 / P1-32 address bar | **On main: the parser parts with P1-30/31 (integration merge `a29488b`) and the rest from `claude/p1-32-address-bar` (integration merge `dc55035`), both reaching main through `681d992` on 2026-10-12; owner checks pending.** The address-bar rule now lives in `syrinx-core` (`address_entry.rs`, on top of DW-1's `web_address`): bare hosts, `localhost` and loopback addresses (opened over `http`), IPv4 and bracketed or bare IPv6 navigate; text with spaces, no plausible top-level label or an `@` is a Google search; Alt Enter always searches. `syrinx_core::readable_address` shows the page's address decoded where safe, host kept as punycode. D9 is checked structurally by `cargo xtask check-deps`: no Syrinx crate may declare a network client or a Windows networking feature, and the headless crates cannot reach one through any dependency. See `docs/p1-32-address-bar.md`. | Owner's checks in that note; independent review. Bare names like `node.js` still open as addresses (no public suffix list). |
| 4 / P1-33 palette acceptance | **Harness on main (integration merge `594415f`, reaching main through `681d992` on 2026-10-12; DW-90 merged to main as `a184730` on 2026-10-13).** It was built on `claude/p1-33-palette-acceptance`, revised after independent review, and run five times on a busy machine (only the last with Syrinx in the foreground); provisional results fail acceptance. `cargo xtask palette-acceptance` drives the real app on a fresh synthetic profile against loopback-only synthetic pages and writes JSON plus a p50/p95/max summary under `target/p1-33/`. It measures opening stages (Ctrl K action, `open_window` called, window created, first render, first presented frame), and per-keystroke worker, round-trip, echo and composed key-to-rows latency at 1k/10k/100k in-memory synthetic rows. It adds back-to-back bursts at 80 ms per key, reporting last-key-to-rows and superseded requests, and checks owner/topmost/z-order, Esc/Enter focus and stale refocus. Verdicts use the owner's 2026-10-12 decisions: opening within one refresh interval at p95, and composed key-to-rows within two intervals at p95, with every maximum reported. Latest run `1791855053`: opening p95 525 ms, with about 0.01 ms before `open_window` and about 510 ms by window creation (provisionally inside GPUI's window creation; DW-90 approved as a later slice). D48 in the app passes at 1k (p95 0.95 ms) and fails at 10k (8.8 ms) and 100k (40.6 ms, max 106.5 ms). Composed p95 is 28.8/28.8/58.1 ms against 33.3 ms, so it passes at 1k and 10k and fails at 100k. Burst last-key p95 is 27/27/70 ms, with 2 of 78 requests superseded at 100k. Ownership, not-topmost, z-order and popup reuse pass. **The harness found a palette bug:** Enter pressed before the current query's local reply fell back to a Google search. Because of it, two agent runs sent the synthetic text "enter-target" to www.google.com before any loopback guard existed. No personal data was involved; the owner had both synthetic profiles deleted on 2026-10-12. The palette now waits for the reply and opens its top row, searching only when nothing matched. The harness waits for the full query's reply before Enter, and aborts on any destination outside its loopback pages. Run `1791856583`, after the fix and with Syrinx in the foreground: Enter activation passes; opening p95 446 ms; composed p95 27.9/28.7/60.3 ms. Page focus after Esc still failed 0/20, so foreground denial no longer explains it. The gate passed with 940 tests. **DW-90 (2026-10-12, `claude/dw-90-palette-reuse`):** the palette popup is now created once, hidden, and shown, reset and hidden per Ctrl K; the opening p95 fell from 556.5 ms (`1791858770`) to 5.9 ms (`1791860299`), which passes the one-frame budget. The same slice diagnosed Esc's focus failure: GPUI's `activate_window` sets Win32 focus to the main window from a later task, after Syrinx had focused the page. `focus_page` now activates natively first when Syrinx is in front; Esc returns focus 20/20 and every focus and stacking check passes. Review fixes the same day: Syrinx never takes the foreground from another application (owner decision, `docs/decisions.md`), so with Syrinx behind, the page's focus waits for the owner to return and Ctrl K leaves the palette closed; each close replaces the palette's input so its caret blink no longer wakes the hidden popup, and the harness now counts those wake-ups and Syrinx's idle CPU with the popup hidden. See `docs/p1-33-palette-acceptance.md`. | Owner's three foreground quiet-machine runs and manual D7 liveness and real-keyboard checks; the peek's Esc and a second-monitor palette check by hand; D48 work stays with P1-05. |
| 5 / P1-34 single instance | **On main (built on `claude/p1-34-35-instance-runtime`; integration merge `0428783`, reaching main through `681d992` on 2026-10-12); owner checks pending.** One running Syrinx per Windows user and profile: a per-user named mutex elects it, and later launches forward one http(s) link (or a plain activation) over an owner-only, local-only named pipe, then exit. The launch verifies that the pipe's server runs as the same Windows user before sending or granting foreground rights. A link is accepted only as the single argument after `--` (P1-37 registers `"syrinx.exe" -- "%1"`), checked with `syrinx_core::web_address` before sending and again on arrival; nothing else is accepted or executed. Start-up failures are shown in a message box. The running Syrinx opens each link in a new tab and comes to the front (the launch grants foreground rights). Tests cover simultaneous launches, a launch during start-up and during shutdown, an abandoned mutex, a silent instance, forged and overlong requests, a squatted pipe name and profile isolation; names are scoped to the user's SID and the profile directory. See `docs/p1-34-35-instance-runtime.md`. | Owner's window checks in that note. No Windows registration (P1-37). A link sent in the instant closing starts is dropped and reported after exit; any local program can deny start-up by taking the predictable pipe name first (documented limit). |
| 5 / P1-35 runtime notice | **On main (built on `claude/p1-34-35-instance-runtime`; integration merge `0428783`, reaching main through `681d992` on 2026-10-12); owner checks pending.** The page host reports the shared environment's WebView2 version; it is stored in the existing `settings` table as `webview2.runtime_version` (no migration). A changed version shows D36's dismissable one-line notice under the toolbar, with the page card moved below it, and an **Open in Edge** button for http(s) pages that opens the current page through `microsoft-edge:` only when clicked. First start stores silently. No sign-in canary, no network call. | Owner's window check of the notice, the card offset and the Edge hand-off (note above). Dismissal is not stored, by design. |
| 5 / P1-37 installer and registration | **Prepared and on main after review (integration merge `1c38e74`, reaching main through `681d992` on 2026-10-12); owner operation pending.** Recommended route: portable zip plus owner-run per-user scripts, because WiX, Inno Setup and a signing certificate are absent and MSIX needs signing. `cargo xtask package` builds the `local` executable into a versioned folder and zip with `syrinx-package.json` (version, commit, dirty flag, `signed: false`, per-file SHA-256). `packaging/` holds install, register and unregister scripts; their `HKCU`-only registry list is rendered from one Rust table, and `cargo xtask check-packaging` (in the gate) refuses drift and parses every script without running it. No script was run by an agent. See `docs/p1-37-installer-design.md`. | Owner runs install, registration, default selection, one upgrade with data intact and one uninstall that keeps data. Links are registered as `"<exe>" -- "%1"`, the form P1-34 accepts, so P1-34 and P1-37 must merge together before the owner registers; `register-syrinx.ps1` warns until then. The installed build uses `%LOCALAPPDATA%\Syrinx\data` (owner decision 2026-10-12); preview runs keep `SyrinxProductPreview`. |
| 5 / Branding (owner decision 2026-10-13) | **Merged to main as `96c5f86` (2026-10-13) after two review rounds; owner checks pending.** Built on `claude/syrinx-branding`. The hawk on a Graphite (`#141519`) rounded tile, with rasters from 16 to 512 px, `syrinx.ico` and a website favicon kit, generated once by `design/brand/tools/regenerate.py` (headless Edge with a throwaway profile, stdlib Python) and committed; see `design/brand/README.md`. `crates/syrinx-app/build.rs` compiles icon 1 and VERSIONINFO with the Windows SDK's `rc.exe` and fails clearly without it (`docs/LOCAL-DEV.md`). `syrinx_platform::app_icon` sets DPI-sized WM_SETICON icons on the main window and the palette, peek and import popups. The registration's five `{exe},0` values and the Start shortcut already point at it (new test). Default accent `#FF612E`; a stored `#FF8A65` loads as the new default (owner decision A). Haze and Glass lift an accent's ink only when it fails the contrast audit there (owner decision B): the default to 70% lightness and the blue preset on Glass to `#7AAFFF`; green and gold keep their colour (see `design/README.md`). Filled buttons use the accent fill with badge text. The icon is re-sized when the main window changes scale. Review fixes followed (UTF-16 resource script for non-ASCII paths, reported SDK read errors, fresh Edge profiles). The gate passed with 1,011 tests before review and 1,014 after it; the rebuilt `syrinx.exe` again showed one icon group and the hawk through ExtractIconEx. A visible `--test-data-dir` launch at 200% scaling returned the hawk from WM_GETICON (64 and 32 px) and from the class icons, and ExtractIconEx found one icon group in `syrinx.exe`. | The owner looks at the taskbar, Alt Tab and title bar, including on a light taskbar. The palette and peek popups' icons were not queried at run time (they use the same code path). The owner chooses the CompanyName (left out for now) and reviews the Haze/Glass ink adjustment. |
| 5 / P1-38 update verification | **On main after two security reviews (built on `claude/p1-38-update-verify`; integration merge `9f21939`, reaching main through `681d992` on 2026-10-12).** New crate `crates/syrinx-update`: canonical signed manifest (version, minimum version, file paths, sizes, SHA-256), 76-byte detached ECDSA P-256 signature checked over the raw manifest before parsing, rollback protection refusing equal or lower versions, Windows-safe path rules, bounded streaming payload checks that return the hashed deny-write handles to install from (`DirectoryPayload` refuses links, junctions and wrong-case names) and typed errors. Hashing and verification use Windows CNG through the existing `windows =0.62.2` pin with one added namespace feature; no package added (`docs/dependency-audit.md`, P1-38). Tests use only synthetic packages and in-process CNG keys. `cargo xtask check-deps` refuses any crate depending on `syrinx-update`. Format and key design: `docs/p1-38-update-format.md`. | Owner creates the release key outside the repository and pins its public point (DW-30); until then every package is refused. Download, network, install, restart and UI are P1-39 and need the owner's host/network approval; a signed expiring latest-version statement against freeze attacks is a P1-39 question. |
| 5 / P1-36 keyboard and accessibility | **On main with its review fixes (built on `claude/p1-36-accessibility`; integration merge `7faa66b`, reaching main through `681d992` on 2026-10-12; DW-61 merged to main as `7cd750e` on 2026-10-14); the owner's NVDA checks are pending.** GPUI 0.3.8 already has a UI Automation provider (AccessKit's Windows adapter, vendored `src/window.rs` 1090–1124 and `src/events.rs` 852–867), so no provider gap; the vendored GPUI is unchanged. F6/Shift F6 cycle address toolbar, runtime notice, permission, find, sidebar and page (pure order in `syrinx_ui::focus`, tested), including from a focused page through the post-only `AcceleratorKeyPressed` handler, and Tab out of a page through `MoveFocusRequested`; Ctrl F/H/P and zoom stay page-first (tested). Tab/Shift Tab reach every chrome control in the main window, palette and import preview, with Enter/Space activation; a 2 px accent-ink ring with a dark inner line from `syrinx-ui` tokens; `GotFocus` and `focus_page` clear chrome focus when the page takes the keys. Roles, names and states on toolbar, sidebar rows/Favorites/edge, bars, Home/Customize/tile form, History, Clear data and palette. See `docs/p1-36-accessibility.md`. | Owner's twelve NVDA checks in that note (no agent window or screen-reader inspection). DW-60 palette selection announcement (merged 2026-10-14 from `claude/dw-60-palette-active-descendant` after two reviews: a focused combo box holds the query and rows, and arrowed rows claim the active descendant; probe rerun; only the owner's NVDA check 7 remains), DW-61 sidebar roving focus (merged as `7cd750e`; only the owner's NVDA check 13 remains), DW-62 ring after mouse clicks (merged 2026-10-14 from `claude/dw-62-keyboard-focus-ring` after two reviews: Syrinx's own keyboard-or-pointer flag, set by every Syrinx key, every page-handed key and every mouse press in Syrinx's windows, hides the ring after a click; the owner's visual check 12 remains); owner questions in the note. |
| 5 / P1-39a update statement | **On main after independent review (built on `claude/p1-39a-update-statement`; merged as `4307009` on 2026-10-14).** `crates/syrinx-update` gains the signed latest-version statement (`syrinx-update-latest 1`: serial, version, manifest SHA-256, issued, expires; canonical text of at most 4 KiB) and `verify_statement`: the 76-byte signature with its own `ALS1` magic is checked against any pinned key before parsing, then the 60-day lifetime cap, expiry (exclusive) and at most one day of early issue against the local clock, and a lower or reused serial; `VerifiedStatement::verify_manifest` then requires the manifest's hash and version to be the stated ones before P1-38's checks. `cargo xtask package` now also writes P1-38's unsigned `manifest` beside `syrinx-package.json`, and `cargo xtask sign-update` signs the manifest and a fresh statement with a key named in Windows key storage (NCrypt), verifying both against `PRODUCTION_KEYS` before writing. Both run the update crate's release tool (`src/bin/syrinx-update-tool.rs`) because `check-deps` still forbids linking the verifier. No dependency, lockfile, network or `check-deps` change; tests use in-memory CNG keys and synthetic packages only. Format: `docs/p1-38-update-format.md`, "P1-39a". On 2026-10-14 `cargo xtask check` passed on the branch with 1,113 tests and the smoke run, and with 1,116 after the security review's fixes (atomic release writes, the `release-tool` feature, doc corrections), and `cargo xtask package` wrote a canonical seven-file `manifest` under `target\package`. | `sign-update` refuses every key until the owner's keys are pinned (DW-30, P1-39d). P1-39b and P1-39c have since merged (`ef933d9`, `8809cd7`); P1-39d (owner keys, host, first real update) is the owner's. |
| 5 / P1-39b update staging and apply | **On main after independent review (built on `claude/p1-39b-staging-apply`; merged as `ef933d9` on 2026-10-14).** "Install update from file…" in Customize's new Updates section and the palette verifies a package folder (statement, manifest, payload), copies only its listed files into `Updates\incoming`, verifies that copy and installs from the verified handles into `Programs\Syrinx.new` and `Updates\apply`; the accepted statement serial is stored in `Updates\state` before `pending` is written last. Restart now or Later (install on next quit) first takes an extra D13 snapshot (`Store::backup_before_update`; seven still kept), then `main` hands over to the new `syrinx-apply.exe` (a second `syrinx-update` binary) through a lock file Syrinx holds until it exits. The helper rechecks `Syrinx.new`, renames `Syrinx` to `Syrinx.previous` and `Syrinx.new` to `Syrinx`, and on Restart now relaunches with `--after-update` (no foreground activation) and rolls back if no clean start is recorded within 60 s. Start-up recovery finishes or clears interrupted steps. `check-deps` lets only `syrinx-app` depend on `syrinx-update` and refuses `release-tool` on any dependency; the gate's workspace steps are featureless with separate `syrinx-update --features release-tool` steps. `cargo xtask package` ships the helper. No network, no new package (one `Cargo.lock` line), `PRODUCTION_KEYS` still empty so real installs fail closed. Design: `docs/p1-38-update-format.md`, "P1-39b". `cargo xtask check` passed with 1,229 test results and the smoke run. Window check: the Updates section and the palette command appear and a development build refuses without touching files. **DW-66, merged 2026-10-14 after two reviews:** Customize offers "Reset update records…" only while `Updates\state` is unreadable; after a confirmation that explains the forgotten serial, it renames the record to `Updates\state.unreadable` under the update lock, refusing while any update step runs or a staged update waits, so install from file and Check for updates then behave as on a new install. Following the owner's 2026-10-14 decision, start-up discards a staged update found beside an unreadable record, under the same lock, and tells the owner, so the reset is never blocked by an update the window cannot show (`docs/p1-38-update-format.md`, "DW-66"). `cargo xtask check` passed on the branch with 1,327 test results and the smoke run; a window check with an `installed` build in a synthetic `Programs\Syrinx` showed the action only for a damaged synthetic record, and the reset renamed it (the start-up notice is covered by tests, not a window check). An interrupted discard is DW-69. | Owner: whether Later should also be watched for a failed start; real installs wait on DW-30 keys (P1-39d); foreground behaviour of a real relaunch unverified. |
| 5 / P1-39c check for updates | **On main after independent review (built on `claude/p1-39c-check-for-updates`; merged as `8809cd7` on 2026-10-14).** "Check for updates" in Customize's Updates section and the palette (`browser.check-updates`) runs only when the owner presses it, and a copy that cannot install (every development build, the gate and smoke) refuses before any request. One press reads `latest`, `latest.sig` and the stated `manifest`/`manifest.sig` from the compiled-in `https://syrinxapp.dev/updates/v1/`, verifies them, and asks "Syrinx X is available (N MB). Download and verify it?"; Download fetches the listed files into `Updates\incoming` and P1-39b's staging verifies and stages them for Restart now or Later; Not now and Cancel leave nothing behind. New crate `crates/syrinx-update-net` (WinHTTP through the pinned `windows` crate with only `Win32_Networking_WinHttp`): agent `Syrinx-Update` and nothing else Syrinx-specific, no cookies, credentials or redirects, 200 only, TLS 1.2/1.3 with Windows' certificate validation untouched, size bounds per file, 30 s connect and 60 s receive bounds, cancellation at the next read. Staging now reads through `PackageSource` (`FolderSource` for files), and download failures are fixed `TransferProblem` categories recorded as `network_failed` or `cancelled`. `check-deps` changes 2 and 3 plus a source scan confining WinHTTP to the new crate. `Cargo.lock`: only the new workspace package and two edges. Design and tests: `docs/p1-38-update-format.md`, "P1-39c". Tests use an in-process fake host and 127.0.0.1 servers only (the untrusted-certificate test uses P1-14's Python fixture). `cargo xtask check` passed with 1,300 test results and the smoke run. Window check with a fresh `--test-data-dir`: Customize's Updates section shows the host sentence and the Check for updates and Install from file… buttons, and UI Automation names both; the check was not pressed. | Owner answered 2026-10-14: reading the four signed files before asking is approved; revocation checking stays off; DW-67 stays deferred past P1-40. Review follow-ups made: feature-table entries checked by `check-deps`, a wider WinHTTP source scan, a late Cancel never offers or deletes a staged copy, a per-file overall deadline, and no request while `PRODUCTION_KEYS` is empty; the gate then passed with 1,305 test results. DW-68, the review's last follow-up, merged on 2026-10-14 after three reviews: `check-deps` now reads the features Cargo resolves on every `windows`, `windows-sys` and `winapi` version and refuses any networking or remote-call feature beyond WinHTTP on the download's own build and GPUI's existing network features (eight entries, RPC included), which the owner approved that day pinned to their requesting crates and versions (`docs/decisions.md`); a new requester or a stale entry fails, and the source scan refuses socket names in every Syrinx crate. Real checks wait on P1-39d (keys and a host serving its full chain). |
| 5 / AN-01 local usage counting | **On main (built on `claude/an-01-local-analytics` and merged 2026-10-14 after three reviews: changes required, then approve with minor fixes twice, all applied); the quiet-machine overhead and file-load run remains.** The owner started it ahead of P1-39d and approved the frozen command list as built (both 2026-10-14, `docs/decisions.md`). `crates/syrinx-core/src/usage*`: the approved version 1 schema as enums (35 feature counters, 20 frozen built-in command counters plus `command.other`, eight histograms, 13 error categories), a 159-slot relaxed-atomic recorder that takes only enums and durations (two `compile_fail` doc tests), 30-day periods with at most two kept, and one fixed JSON shape for `usage.json` and the report with a strict reader of its own (no new dependency). `crates/syrinx-app/src/session/usage.rs`: the file beside `syrinx.db`, existing only while counting is on, written by the session worker through a replacement file at most every five minutes after a change (on its existing wake-ups) and at exit, never in backups; damaged files set aside and counted once. Customize's Usage report section (`ui/usage_report.rs`): the approved switch text with the owner's shortened note until AN-02, off by default, Reset, and confirmations before off (which deletes the counts) and Reset. Recording calls at every existing call site; five fields stay zero until P1-24/P1-26 or by construction. Note: `docs/an-01-local-analytics.md`. Review fixes: switching off writes a `usage.off` marker before deleting, so a failed delete can never resume counting; every status event carries the real state with its problem; the tab-switch timer waits for its own tab; a completed period ends on the day it closed; plain Enter counts by outcome, cancelled navigations and palette commands are not counted as failures or keys, and start-ups that waited on a prompt are not timed. `cargo xtask check` passed with 1,338 test results (after the re-review's minor fixes: an unrecordable switch-off now says counting may resume, and leftover copies are deleted at start-up) and the smoke run, which fails if anything was counted. `cargo xtask bench` (busy machine, provisional): update p99s 6.25–12.5 ns over 16-update batch means, largest-file load p95 0.173 ms. Window check with a fresh `--test-data-dir`: the section, switching on (file created, "Counting since"), the off confirmation and off (file deleted) all seen; not the owner's visual acceptance. | Done also needs the quiet-machine overhead and file-load run. Owner: visual check. AN-02 (which restores the approved switch note) (preview, Save report…, Copy report) precedes P1-40. |
| 5 / P1-39d–P1-40 daily driver | **P1-39d: the owner reported on 2026-10-14 that P1-39 is finished and that only release bugs are being worked out; at the time of writing `PRODUCTION_KEYS` on main (`crates/syrinx-update/src/keys.rs`) is still empty, so DW-30's pinning has not reached main. P1-40: missing.** | P1-39d: the pinned keys (DW-30) and the release fixes reach main, and the first real update is recorded in a P1-39 note. P1-40: one default-browser week and D6 memory/startup evidence. Installer and registration are P1-37's row. |

Phase 1 remains open despite D49 integration. D49 authorizes this preview exception, not automatic completion or cancellation of P1-05–P1-40.

## Plan 2 — plugin runtime and Fledge demonstration

No `syrinx-plugin-api`, plugin host, `fledge-client` or plugin crates exist. The three “coming soon” rows in the preview are not implementations. `docs/plans/Syrinx v1 — Plugins and Fledge — Plan.md` is approved; its 12-slice map is now approved, with predecessor gates unchanged.

| Phase | Status / required exit |
| --- | --- |
| 1 contract/test plugin | **Missing.** Owned serializable API, registration/version/duplicate checks, scoped Host services, command ownership and synthetic lifecycle tests. Existing manifest-boundary checks are only one foundation. |
| 2 real palette command | **Missing.** Declarative forms/lists, Plugin actions, pending/errors, validation/draft retention, no duplicate submissions, permanent plugin rows and correct live-page focus. |
| 3 Fledge demo | **Missing.** Session-only synthetic leads in normal app, clear demo label, Name required, optional Company/Source/value in demo USD; host-adapted stand-in with list/detail/create and failure/idempotency tests. |
| 4 search/lifecycle/acceptance | **Missing.** Replay/deletion consistency, restart clears demo records from both search tiers, subscription cleanup, activation/idle/ranking measurements and owner acceptance. |

Real OAuth/REST integration is outside this plan until Fledge Stage C, a current authorized contract and separate approval. Do not inspect Fledge repositories or call its services.

## Plan 3 — Notes

No Notes implementation, vault service, document bridge or CodeMirror bundle exists. `docs/plans/Syrinx v1 — Notes — Plan.md` is approved; its 16-slice map is now approved; implementation waits for the predecessor gates.

| Phase | Status / required exit |
| --- | --- |
| 1 vault/index | **Missing.** One-vault existing/new picker, bounded files/indexer, navigation, both search contributions, cancellation and malformed/unsupported-file handling on synthetic vaults. |
| 2 editor/save | **Missing.** Shared local CodeMirror bridge, live preview/source/outline, checksummed revision protocol, autosave/Ctrl S, watcher conflicts, recovery drafts and safe saves; existing local Markdown images/attachment links without remote preview loads. |
| 3 links/rename | **Missing.** Wikilinks, heading navigation, backlinks, guarded rename preview/rewrites and recoverable partial failures; preserve aliases and case-only/ambiguous paths. |
| 4 properties/Bases | **Missing.** Typed frontmatter table, one sort, AND filters and `.base` round trip; unsupported constructs labelled, unchanged and source-readable; edits share the document/conflict/undo service. |
| 5 compatibility/acceptance | **Missing.** Disposable Obsidian interoperability, recorded synthetic performance scales, accessibility/five-palette review and owner acceptance. Personal vault inspection is neither required nor authorized for agents. |

Property-type interoperability and vault performance workloads need implementation evidence, not invented compatibility promises. Paste/drop importing, special embeds, advanced Bases and graph/WYSIWYG remain outside v1.

## Plan 4 — Forge

No Forge implementation, grammar bundle, terminal, Git service, LSP client or task runner exists. `docs/plans/Syrinx v1 — Forge — Plan.md` is approved; its 21-slice map is now approved; implementation waits for Plans 1–3 and the retained predecessor exits.

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
`docs/plans/Syrinx v1 — Plans 2–4 slice proposals.md`: 12 Plugin/Fledge slices,
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
- The page-text extraction method needs a concrete approved proposal before guarded page access. Its exclusion/retention tests must precede real capture. D39's AN-01/AN-02 proposal is approved (2026-10-12) and adds two rows to the map; AN-01's local collector is built (off by default), AN-02's export is not, and no model upload exists or is planned.
- New dependencies require exact maintenance/security/licence audit. Keep the approved GPUI/wry/WebView2 pins and narrow exceptions. No GPL; repository licence/public release, GitHub creation, upstream publication, updater host/network/install policy, software installations and Windows registration remain owner decisions/actions.
- Keep Syrinx unsigned and Windows security unchanged. Prior error 4551 and the preview breakpoint are observations, not grounds for a signing prerequisite or bypass. Never use integration apps without permission, real browser profiles/personal fixtures, or Fledge production. Spike cleanup needs explicit approval.
- The later mobile-sync outline is outside these four v1 plans and does not authorize mobile, remote sync or earlier work to be skipped.

## Shortest next step

For an agent: review and merge the 2026-10-14 branches listed in the current
checkpoint (AN-01, DW-60, DW-62, DW-66, DW-68), each after an independent
review and a green gate, and record the new test count. Then take the
earliest open Plan 1 work that does not wait on the owner, on its own branch
with an independent review. P1-24 cannot start until P1-23's engine
choice is measured and reviewed; P1-26 also needs P1-25's evidence reviewed
(Q2 is already settled by D40).

For the owner: the quiet-machine runs that unblock that work, P1-23's bake-off
first, then D48's runs for P1-05, P1-06's idle reruns and P1-33's palette runs,
followed by the manual window checks in each slice's row. These runs decide
whether the search and idle targets pass; a passing gate cannot.
