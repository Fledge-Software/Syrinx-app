# Reproduce the reported WebView2 breakpoint before selecting a fix

The owner's earlier `msedgewebview2.exe` breakpoint dialog (`0x80000003`)
occurred during a restricted-context smoke run that nevertheless returned
zero. That run remains a failure. Current successful tests do not establish
its cause. This source-only investigation follows the owner's instruction to
continue with subagents, focus on the crash and make the app usable. The
integration chat owns every build, native run and machine interaction;
this investigation changes no product or vendor source.

## What the evidence distinguishes

| Observation | Supported conclusion | Missing evidence |
| --- | --- | --- |
| Original dialog, recorded in `docs/integrated-browser-preview.md` and `docs/handoff-integrated-preview-2026-10-10.md` | The original smoke cannot count as successful despite its exit code. | Exception location, failing runtime process role and lifecycle stage are absent from the preserved structural evidence. |
| Later restricted-context watchdog and native-event timeouts | Those launches stalled. | They do not reproduce the breakpoint code or establish the same cause. |
| Ordinary native lifecycle and application smoke pass on the normal desktop | Current normal creation, navigation and orderly teardown work in those runs. | Real page-focused keyboard input and the original context-dependent failure are not covered. |
| All 30 v5 failed-construction cases pass | The bounded cleanup assertions in `docs/wry-failed-construction-patch-review.md` hold. | The omitted cases and original breakpoint remain unproved. |
| V4 loader failure `0xC0000139`, followed by manifested v5 success | The disposable fault harness required its Common Controls v6 activation manifest. | This is neither product crash causality nor evidence of product activation-context equivalence. |
| Five `target/continuation-crash-smoke-repeat-*.result.json` records | Each reports exit zero and empty stderr, taking 3.55–4.18 seconds. The integration launcher confirmed the frozen binary and normal-desktop context below. | The runs do not reproduce the original context or exercise real keyboard input. |

The integration chat confirmed terminal exit zero for the serial five-run
launcher and its pre-run SHA-256 check of
`target/local/syrinx-app-parallel-preview.exe`:
`3ca0bf045fc0826e7300a647307845733fb5249b085f52feeadbebf0abd4c276`.
It used normal-desktop execution with `Start-Process --smoke`, hidden startup
and each run's built-in fresh synthetic profile. A separate non-smoke launch
that lacks a targetable window is a visibility investigation until there is
specific failure evidence; it must not be relabelled as the original crash.

An earlier scoped Windows Application log check found no matching runtime
record; absence of a record does not disprove the dialog. No personal profile
or crash dump is needed or authorized for this plan.

## Independent source findings

Three subagents reviewed thread ownership, shutdown, and callback/evidence
paths independently. None found a demonstrated cause of the original
breakpoint. The following findings guide discriminating tests rather than
justify a speculative patch.

Normal shutdown retains the parent window. In
`crates/syrinx-app/src/main.rs`, the window-close callback vetoes immediate
destruction. `ui.rs::request_close` queues browser shutdown first.
`crates/syrinx-platform/src/browser.rs` releases `BrowserState` pages and its
environment before Syrinx's apartment guard, then `HostCompletion` emits
`Stopped`. The UI joins that completed browser worker, stops the session
worker, and quits after both stop. No normal premature-parent destruction
was identified in this path.

Creation can delay shutdown. Environment and controller creation use nested
message-pumping waits. A queued shutdown command is not applied until the
current creation returns, even if its wake message was consumed by that wait.
A callback that never completes could therefore stall shutdown. This is a
liveness hypothesis, not evidence of the breakpoint or permission to add
guessed COM teardown calls.

Syrinx confines controllers and their calls to the message-pumped browser
thread. Accelerator callbacks in `browser/page.rs` read key arguments, mark
handled keys, reject repeats and queue commands; they do not directly close,
navigate or focus a page or start a nested event loop. This follows
[WebView2's threading and reentrancy requirements](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/threading-model).
The smoke path already requests programmatic focus and waits through a short
health interval. It does not exercise actual page-focused keystrokes.

Two concrete parent-handle concerns remain. First,
`browser/page.rs::Parent::window_handle` constructs a typed Win32 handle for
the GPUI thread's window on the browser thread. Pinned
`raw-window-handle 0.6.2`, `src/windows.rs`, documents that the handle is
assumed to belong to the current thread; the present safety comment proves
lifetime only. This contract mismatch does not mean every Win32 child window
with a foreign-thread parent is prohibited, and does not prove a crash.

Second, `vendor/wry-0.57.0/src/webview2/mod.rs::InnerWebView::drop` calls
`dettach_parent_subclass` even for child views, although their construction
never attached that parent subclass. The helper synchronously sends a message
to the UI parent and then calls `RemoveWindowSubclass` from the browser thread.
Microsoft documents the
[cross-thread subclassing restriction](https://learn.microsoft.com/en-us/windows/win32/api/commctrl/nf-commctrl-removewindowsubclass).
This unnecessary operation is an actionable source concern, already outside
the approved failed-construction patch's scope. It has not reproduced the
reported failure in the tested close runs. The v5 fault harness creates its
parent on the same apartment thread; its 30 passes do not test Syrinx's
GPUI-parent/browser-worker thread boundary.

The source-only follow-up in `docs/browser-parent-host-review.md` proposes
one browser-thread-owned intermediary per page to correct the typed-handle
contract while preserving the current architecture. It records independent
review, remaining cross-thread input coupling and a bounded validation plan.
No intermediary implementation or evidence of crash causality is claimed.

Current diagnostics also have limits. `ProcessFailed` registration follows
construction, so it cannot report every early process failure. Creation and
focus errors are mapped into structural `BrowserError` categories, losing
the originating HRESULT. Do not interpret silence as proof that no runtime
failure occurred before registration.

## Smallest next evidence sequence

1. Finish the integration chat's five frozen-binary smoke records and enhanced
   native lifecycle run. Record executable hash, launch context, terminal
   status and structural failure category. Preserve every failure and avoid
   more identical repetitions when they add no distinguishing observation.
   The enhanced native fixture can assert parent-owned-by-caller,
   child-owned-by-browser-thread and parent-alive-through-retry booleans using
   window thread ownership. This confirms the reviewed topology without
   moving COM calls across threads or logging handle values.
   A focused liveness comparison can queue shutdown before `Ready` or during
   pending creation, retaining the parent through `Stopped`, and compare it
   with shutdown after completed creation. A passing run covers the observed
   timing only; it cannot force an indefinitely missing runtime callback.
2. Verify real page-focused `Ctrl K`, `Esc`, typing and `Ctrl W` with synthetic
   content through the authorized interaction work. Directly calling a focus
   or close command does not verify accelerator delivery. Navigation/UI work
   remains with its existing team; this investigation performs no input.
3. If a stall or breakpoint recurs, stop the sequence and retain the first
   failing case. Use a disposable instrumented copy to record only named
   lifecycle boundaries: creation entered/completed, callback entered/exited,
   focus or close queued/applied, controller close, child destruction, worker
   stopped and parent destruction. Thread ownership checks should report
   same/different-thread and parent-alive booleans, not user content or a
   detailed real-usage timeline. Record raw HRESULT only for the synthetic
   failing operation. No page scripts, debugging port or security changes.
4. Compare the original launch method with the normal desktop only if the
   integration chat can reproduce that exact method within its authorized
   execution scope. Use the same executable and fresh synthetic profiles.
   A timeout in one context is not a reproduced breakpoint in that context.
5. If evidence implicates parent teardown, prepare a disposable comparison
   that changes only whether successful child destruction attempts to detach
   an unowned parent subclass. If evidence implicates handle thread ownership,
   evaluate that separately; do not combine it with a broad threading rewrite.
   Any additional vendor correction, public API or architecture change needs
   a concrete reviewed proposal and separate authorization before application.

The first candidate correction, if separately approved and supported by the
teardown evidence, is to detach a parent subclass only for the path that
installed it. Moving all WebView2 ownership onto GPUI's thread or adding a
browser-owned intermediate parent would be broader alternatives with their
own focus, resizing and destruction consequences. Neither is selected by
this review. Existing successful paths and the frozen patch stay unchanged.

Success for this investigation means a reproducible failure tied to a specific
boundary, an independently reviewed minimal correction, and a discriminating
before/after test with normal lifecycle regression checks. Until then, report
the exact usability behaviors verified and keep crash causality unresolved.
No additional construction-fault matrix, commit or merge is part of this work.

## Next authorized test: shutdown while native creation is pending

The owner explicitly requested this test next. The integration chat retains
all execution and the existing lifecycle fixture. A separate disposable copy
under `target/wry-pending-shutdown-proposal/` will exercise the real browser
worker and a parent on another message-pumping thread, without changing live
product sources or the previous fault harness.

The test must distinguish a shutdown queued before environment completion,
a shutdown queued before controller completion, and an ordinary completed
creation followed by shutdown. For each pending case, the native initiation
call must have returned successfully and its real completion callback must
still be unentered when shutdown is queued. This proves pending callback
delivery, not whether the runtime has finished its internal work. If the
callback already ran, that attempt cannot pass as pending-creation coverage. Merely queueing Create
then Shutdown does not prove this boundary.

The reviewed mechanism briefly gates the browser thread after initiation and
before it pumps completion. The parent thread keeps pumping, queues Shutdown
through the existing host, then releases that bounded gate. This intentionally
controls callback delivery; it does not measure natural race frequency.
The observer must run outside a WebView2 callback and must neither synthesize
a completion nor change the original command-drain path. Shutdown is expected
to wait for the original native completion, then release resources and report
`Stopped`. A command-consumed marker must establish this order; `Ready` or
`Created` before `Stopped` is expected and does not itself fail the test.
Assertions retain the parent until `Stopped` and successful join,
check that native children are gone before parent destruction, and fail on
structural browser/process errors. Each case needs an independent process
deadline as well as bounded message pumping. This tests eventual shutdown
when completion arrives; an indefinitely missing callback remains outside
the claim. Source and evidence review precede the integration chat's run.

While this test was being prepared, the integration chat's expanded 12-cycle
native lifecycle fixture failed with terminal exit code one after about eight
seconds. `target/continuation-native-stress.stderr.log` reports a synthetic
navigation failure in cycle 5; there was no reported `ProcessFailed` event or
breakpoint. The integration fixture owner is adding stage and synthetic-ID
diagnostics while preserving the failed run and its assertions. This is a
separate observed failure to investigate, not a reproduction of `0x80000003`
or a reason to broaden the pending-creation test.

The three-case disposable source is now frozen for integration execution.
`target/wry-pending-shutdown-proposal/README.md` gives the isolated build
commands and `environment`, `controller`, `completed` process arguments.
Independent source and provenance reviews are saved beside it as
`independent-review.md` and `source-audit.md`. All nine source-manifest entries
match. Of 279 copied files, four contain the expected observational hooks,
two are the new probe and example, and the other 273 are unchanged; dependency
manifests, lockfile, configuration and licences are identical.
The frozen `workspace-manifest.json` SHA-256 is
`1f92eb856d007085ff65a52ca10151a090dc78631aed604934b2490ec3c7b008`,
and `source-identities.json` is
`9e10ff1d31c07416e8a4a8dd8737371cda351b2214b52ea9c6e159076d7af994`.
The integration chat has the reviewed copy for compilation and the three
serial runs. This checkpoint establishes source readiness, not execution.

The integration chat verified those nine frozen entries and started the
isolated build recorded in `target/continuation-pending-shutdown-build.log`.
It also confirmed a separate complete product gate: 369 tests and the smoke
check passed in `target/continuation-crash-usability-gate.log` (SHA-256
`88b153e055d1e64b48feb2b18ff6b34199ebeb650e64cb22600d33e9e288b39d`).
That product executable has SHA-256
`3d8a38e8b542609961a1949cdb9d1f0b21c8ec054cceb257194aa38cc56f15aa`;
it is distinct from the disposable pending-shutdown executable. Gate success
does not resolve the separately reproduced stress navigation failure.

### First pending-shutdown execution

The isolated build completed with terminal exit code zero in 1 minute 49
seconds. Its executable SHA-256 is
`da9d7948184243ca47bc7dd6c542029881442df6f96f56f80cee9c7418b8175b`.
The environment case failed with exit one: callback entry, callback exit and
`Stopped` were observed, followed by `BrowserError::Environment`. There was
no native-return or shutdown-enqueue marker, so this run establishes no
pending environment shutdown coverage. The preserved files use the prefix
`target/continuation-pending-shutdown-environment`.

The integration chat then ran the independently applicable completed baseline
and controller-pending case using the same executable. Both exited zero,
printed their matching `PASS` and had empty stderr. The controller case's
seven observations were native return, shutdown queued, callback entry,
callback exit, wait return, shutdown consumed and worker stopped, in that
order. Both verified successful join and the exact original child-window set
before parent destruction. Logs and result files use the corresponding
`continuation-pending-shutdown-completed` and `-controller` prefixes. These
passes establish deferred shutdown with an outstanding controller completion;
they do not establish environment-pending coverage or fix the original crash.

The environment result needs a narrower observation. The pinned loader binding
directly invokes the native API and converts its HRESULT to a Rust result;
the used handler `create`/`Invoke` path also adds no Rust message pump. In the
current probe, native initiation failure bypasses the return marker, while
successful initiation followed by detection of an already-entered callback
rejects coverage before that marker. Either can produce the saved trace.
Callback status and object presence also need recording before the early error
return. A new disposable diagnostic copy will preserve the original source
and failed logs, adding only fixed call/result/callback/rejection observations.
The seven-step assertions, original error handling and native calls stay
unchanged; no retry loop or manufactured pending state is authorized.

The diagnostic copy is `target/wry-pending-shutdown-proposal/workspace_diagnostic/`.
Its nine-entry manifest SHA-256 is
`fb7545f2e751745cb234a22d73fab0a386536c882c81cbd1d6796b08be36186e`.
Independent review found no blocker: only copied environment/probe diagnostic
statements differ, both source manifests still match, and the seven-step
test remains unchanged. `diagnostic-notes.md` records the interpretation and
single-run scope. Native and callback success are reported as success only:
the binding has already converted away the raw successful HRESULT. Failures
retain numeric HRESULTs without error message text. Use the worker's stderr
diagnostic order for these call boundaries; merged stdout/stderr arrival
order does not establish causality.

### Environment diagnostic outcome

The integration chat built the reviewed diagnostic copy with terminal exit
code zero in 2 minutes 6 seconds, then ran the environment case once. It
exited one as required by the coverage guard. The worker's stderr records,
in order: native call entered; callback success with a non-null environment;
native call returned success; pending coverage rejected because completion
had already entered. Its three original observations remain callback entry,
callback exit and worker stopped. No Shutdown was queued in this case.

This run distinguishes a successful early completion from native
initialization failure: the harness's `Environment` error here came from its
own strict pending-coverage check. It does not prove the first run had the
same native status, since that older run lacked these diagnostics. Both failed
test records remain preserved. Neither is a reproduced WebView2 breakpoint.

The evidence prefix is
`target/continuation-pending-shutdown-environment-diagnostic`; its executable
SHA-256 is `b649237d831d5c138702ac66fb13dca8bbafd6eb503ddb858c727ac4fa581fa4`.
The result JSON SHA-256 is
`cee9892f33c645fea3a11b0c40bb65b12fcfc4b335298630742ca116617819a5`,
and stderr SHA-256 is
`eed7c5c218293353e46f2aae95b7d1d0b9097378f67cc318093fa26d968d815e`.

The requested pending-shutdown investigation therefore has two passing
scenarios, pending controller and completed creation, and no coverage for
pending environment completion under the observed timing. There is no basis
to change product shutdown, fabricate a pending environment or repeat runs
until one passes. The integration chat is continuing the separately
reproducible navigation failure and usability work. Product and vendor source
remain unchanged by this investigation; no commit or merge was made.

## Per-page intermediary prototype: first native outcome

On 2026-10-11, integration expanded the earlier proposal-only scope to
prepare a disposable implementation under `target/browser-parent-host-proposal/`.
The reviewed `prototype_v2/` adds a browser-thread-owned native parent for
each page, preserving live product/vendor source and all dependency features.
Its strengthened fixture checks both the intermediary and wry child.
Independent source reviews and exact differences are recorded beside it in
`independent-review.md`, `host-source-review.md` and `fixture-source-review.md`.
This is the bounded experiment proposed in `docs/browser-parent-host-review.md`;
it has not been promoted.

Integration reported build exit zero. Independent inspection of
`target/continuation-page-host-build.log` confirms the optimized `local` build
finished in 2 minutes 1 second, with one existing vendored-wry dead-code
warning. This was a build, not the full product gate. The resulting executable
hash independently matches the native result record:
`6d5db8f282ecb9fffb50b62dbefb4d01128d0aaf44fccb6cff51792884055050`.

The single native run **failed with exit one**. Its stdout is empty, and its
only stderr line is:

```text
Synthetic lifecycle cycle 2 failed: Synthetic tab 101, fixture creation/navigation wait: Synthetic tab 101 navigation failed.
```

The frozen fixture reports this when it receives a failed navigation
completion for synthetic tab 101. It reached the second repeated cycle,
but did not complete the 12-cycle sequence or its final multi-page shutdown
scenario. This prototype does not record native navigation error status or
HTTP status, so those values cannot be inferred from the failure text.
An independent subagent checked the result against the frozen control flow:
the initial lifecycle checks and first repeated cycle completed before this
failure; cycle two failed before its own child-topology follow-up. The error
path still requests shutdown and joins the worker, but its final retained
three-page identity list has not yet been populated. These are different
checks and must not be reported as completed multi-page shutdown coverage.
There is no reported process-failure event, worker panic or host-cleanup
diagnostic in these logs. That absence does not establish crash causality,
complete failure-path coverage or a general cleanup guarantee. The original
breakpoint remains distinct from this navigation failure.

All 278 source identities still match `prototype_v2-manifest.json`, SHA-256
`53def35c680db3a5b44595fc6c56cf41a71f840ad5175fbe139cb4eb0f58d736`.
The preserved native records use the prefix
`target/continuation-page-host-native`. Their SHA-256 identities are:

| Record | SHA-256 |
| --- | --- |
| Build log | `8749a02e0ce6d76677e0841534e8d24813b3b416e9bac502ccfa799a267eecc9` |
| Native result JSON | `f49715a3faac49da6ea3e21fc12ea899483845a3e72faf08588e3a69f96828a2` |
| Native stderr | `8c84387392817c8b04653adf32d92158c425a11c8d82c6534395d76b96eb095b` |
| Empty native stdout | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

Separately, integration's fixed diagnostic-fixture v2/v4 comparison recorded
three failures without server logging and three full passes with server-only
logging added. Those names refer to the separate navigation diagnostic
fixtures, not this intermediary `prototype_v2`. The comparison is preserved
in `docs/integrated-browser-preview.md` and
`target/continuation-native-server-pair{1,2,3}-{v2,v4}.*`. It suggests a
server-work/timing confound in that small sample; it neither proves a TCP
connection-close cause nor establishes that this prototype's failure has
the same cause. Changing the server while changing parent topology would
not isolate the intermediary's effect.

Keep the failed run and prototype source frozen. No next variant is applied,
no repeat is substituted for this failure, and no promotion is supported.
Host failure injection, adapted pending-controller shutdown and real GPUI
typing/focus validation remain outstanding; the owner's manual typing check
is still pending through integration. This outcome update changes only this
report and makes no commit or merge.

### Process-role diagnostic blocked before application execution

On October 11, 2026, the isolated diagnostic build and its single unchanged
metadata-trace build ended with Cargo exit 101 at `gpui_macros` resolution.
A subsequent single explicit-import compiler probe ended with exit 1 and
exposed `LoadLibraryExW` error 4551: Application Control blocked the exact
diagnostic `gpui_macros-1a9aa98a68e2369f.dll` (SHA-256
`0dc1d95fdd74af5015a9a5c59d1b753b73e466bf81251aacc85e1b727c2f741b`).
The diagnostic reports five internal load attempts; integration invoked the
probe compiler once. Evidence is preserved in `target/macro-load-probe/`,
alongside `target/continuation-process-role-build.log` and
`target/continuation-process-role-build-trace.log`.

No diagnostic Syrinx application ran, so process-role and browser-exit
observations remain uncollected. This establishes the probe's policy block,
not which policy rule selected the file, every earlier compiler failure's
cause, or the original WebView2 breakpoint's cause. The earlier empty scoped
CodeIntegrity query does not negate the explicit loader error. This isolated
build path is stopped without security, signing, cache, dependency or DLL
replacement changes.

### Abnormal parent-window lifetime remains a separate coverage gap

A read-only audit, independently reviewed by two subagents, found the ordinary
close veto, startup-window failure and ordinary Rust worker unwinding consistent
with the reviewed browser-first ordering. That is Rust resource ordering, not
proof that every native close succeeded or all COM references were released.

Windows session termination follows a different path in the pinned GPUI source:
it calls application shutdown, clears windows before awaiting quit observers,
and may exit without Syrinx's stopped-event and join protocol. Syrinx's fallback
`BrowserHost` destructor requests shutdown but retains no parent-window owner
and does not wait. This leaves the parent-lifetime guarantee unenforced on that
path; native destruction timing and consequences remain untested. It does not
show that session termination occurred during the original breakpoint or that
this path caused it. No Windows logoff/shutdown test, source fix or policy
change was performed. A blocking GUI join or an ordinary async quit hook alone
does not establish safe ownership because the worker needs message pumping and
GPUI clears windows before awaiting that hook.

Two independent source reviewers then checked the available shutdown hooks.
The pinned `on_app_quit` hook is noncancelable, and `WindowHandle` does not own
the native window. GPUI's 200 ms quit-future waiter parks without pumping
Windows messages or foreground tasks; meanwhile wry's destructor can send a
synchronous message to the parent. Merely moving window destruction later or
joining on the GUI thread can therefore introduce a deadlock. The waiter also
cannot enforce its timeout while a future's poll blocks.

A safe solution needs a contract before native window teardown: retain the
actual parent owner, release application borrows before pumping messages,
coordinate browser shutdown and store draining, and define deadline,
cancellation and reentrancy behavior. This requires a concrete dependency or
public-hook proposal with separate owner approval. Preserve the ordinary close
path; do not apply an `on_app_quit`-only workaround. This is source feasibility
analysis, not a reproduced crash or authorization for a dependency patch.

Any in-process pumping deadline is cooperative between message dispatches;
it cannot interrupt a handler that blocks. A future proposal must not promise
a hard timeout or guaranteed graceful OS-session shutdown, and must explicitly
guard against reentrant or duplicate session-end and quit notifications.
