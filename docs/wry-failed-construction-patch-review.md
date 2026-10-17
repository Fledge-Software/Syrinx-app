# Keep failed WebView2 construction owned until handoff

Status on 2026-10-10: the owner approved applying the frozen patch, including
its documented limitations, and directed validation through the integration
chat with subagent review and no commit or merge. The exact patch is applied
and independently checked against its frozen bytes. Ordinary native lifecycle
and the bounded 30-case fault subset passed; coverage limits remain below.
This review does not diagnose the original `0x80000003` report.
No Cargo command, native program, benchmark or UI automation was run by this
session. The integration chat owns machine validation.

Failed page creation must release its own native resources while the browser
continues running. Waiting for the parent window to close leaves failed
attempts resident during retries. The current pinned source has no temporary
owner between creating the container and constructing `InnerWebView`.
`docs/wry-creation-lifetime-review.md` records the original source finding;
this document specifies its separate dependency-patch review.

## Frozen source and approval boundary

The candidate is based on the **current** `vendor/wry-0.57.0`, including the
already approved close-request callback. It is not based directly on the
published unpatched archive. Exact byte identities at proposal start are:

| Input | SHA-256 |
| --- | --- |
| Current `src/webview2/mod.rs` | `2edff46017af862fe60e2ef685af8093f1201cf4f94f2e374a788db98e7f8f45` |
| Current `src/lib.rs` | `6044247c8411139d68d7c778fc8971084c89fd544d65219b01abdbde7cd3dd93` |
| Workspace `Cargo.toml` | `eaaccc44730585a10417cc4fc00b490c7bcb49d0408c9d90c1ef6a7507b1a56f` |
| Workspace `Cargo.lock` | `55182fb69cefb14b501b03bffd3c8624ac5177ce2189960841b838ad25f8e2ac` |

The full current package inventory is saved in
`target/wry-failed-construction-proposal/vendor-source-inventory.json`.
The published archive SHA-256 is
`a819957a01b3119af85e638a38d242af76dbc87d130dca67bfd0441072e21ff0`.
Its packaged revision is `792d0359ba6501a4fc360ece17de2ae42329a47c`, with
`dirty: true`; `vendor/wry-0.57.0.provenance.md` records all published files
and the approved two-file delta. That archive provenance is retained evidence,
not a new download or independent archive re-audit in this session.

The approved application changes only the two vendor files described below.
Exact `=0.57.0` and
`=0.39.1` pins, features, manifests, lockfile, licences and existing callback
behavior remain inputs. Default page identity, absence of page IPC and
injected scripts/request filters/debugging, SmartScreen and local crash dumps
remain unchanged. A proposal-copy formatting check is not a workspace gate.

## Ownership evidence

Line references below identify the frozen input, before the candidate changes.

| Resource or boundary | Source evidence | Required ownership |
| --- | --- | --- |
| Container HWND | `create_container_hwnd`, lines 266–291, calls `CreateWindowExW`, then fallible `SetWindowPos`. Both modes create a `WS_CHILD` container. | Begin ownership inside this helper, immediately after acquisition. A guard only in its caller misses this failure. Destroy this attempt's container in either mode on failure. |
| Environment | `new_in_hwnd`, lines 133–137, clones the supplied environment or creates one. | Rust COM references retain ordinary ownership. Syrinx supplies its environment; the fallback still needs the container guard. |
| Controller | `create_controller`, lines 380–431, receives completion through a channel. | Own every non-null controller before inspecting completion status. Move the temporary owner through the channel; a discarded completion must explicitly attempt `Close`. |
| Canceled wait | Pinned `webview2-com 0.39.1`, `src/lib.rs::wait_with_pump`, returns on `GetMessageA` failure or quit. | A controller can arrive after the receiving wait ended. Ownership cannot begin only after a successful return from `create_controller`. |
| Configuration | `init_webview`, lines 443 onward, obtains the web view and performs fallible settings/handler/navigation work. | Keep the controller and container guarded until configuration succeeds. Preserve the original returned error. |
| Parent subclass | `init_webview`, lines 613–616, attaches before fallible visibility and extension setup. `attach_parent_subclass`, lines 1368 onward, uses one fixed ID and a boxed controller clone. | A failed attempt must not leave its callback data registered or remove another live attempt's registration. Mere `!is_child` is not ownership proof. |
| Existing handoff | `new_in_hwnd`, lines 164–183, builds `InnerWebView` before final bounds; its existing destructor then handles bounds errors. | Transfer each resource exactly once, including final placement errors. Do not overlap guard destruction with `InnerWebView::drop`. |

The container handle alone is not identity proof after messages are pumped.
The parent can be destroyed while waiting; Windows can later reuse a handle.
Checking `IsWindow` only answers whether that numeric handle is valid now.
Temporary cleanup therefore needs destruction observation tied to the exact
created container, with safe ownership of any callback data used to observe it.
Cleanup itself must account for reentrant destruction during controller close.

## Concrete candidate and cleanup policy

The candidate changes `src/webview2/mod.rs` and adds the private module
`src/webview2/construction.rs`. It adds no public API. Its exact diff is
`target/wry-failed-construction-proposal/candidate/failed-construction.patch`;
the sibling `baseline/src/webview2/mod.rs` is the frozen current input and
`src/webview2/` holds the candidate files. Do not apply the diff to the
published archive without first applying the separately approved close patch.

`PendingContainer` takes authority immediately after `CreateWindowExW` and
installs a private native subclass observing `WM_NCDESTROY`. Its shared alive
flag is cleared before forwarding destruction, so its destructor does not
destroy a later window that happens to reuse the handle. This is a native
container callback, not a page script or IPC bridge. Failure to install the
observer returns a new `E_FAIL`; detected destruction at a phase boundary
returns a new `E_HANDLE`. These are candidate-added failure paths. Existing
operation errors retain their original variants/statuses when they occur.

`PendingController` starts in the completion callback before status is
interpreted. The channel carries status and ownership together. For ordinary
delivery, the waiter checks status after the callback returns; a failed
status then drops the controller outside that callback. A failed send or a
queued completion discarded with its receiver also drops the owner. A late
completion whose receiver has already disappeared can consequently call
`Close` inside the completion callback. The available local bindings do not
establish that callback-context behavior as runtime-safe; it is an explicit
acceptance test, not a demonstrated property.

Controller ownership is declared after container ownership, so Rust unwinds
the controller first, then the container. Each attempts cleanup once. The
container's alive flag is checked after controller cleanup, accounting for
destruction reentered during that cleanup. Final sizing now uses shared
helpers before `InnerWebView` exists. Only then is the non-child parent
subclass attached and the two guards disarmed into `InnerWebView`. No failed
construction path calls the existing broad parent-detach helper. This avoids
removing a pre-existing parent's registration during rollback without adding
another parent-callback owner to partial construction.

On cleanup failure, the candidate records a process-local category bit and
writes a fixed structural message to stderr. It never logs the original error
text, a URL, path, HWND or page contents. Failed stderr writes set a separate
bit rather than panicking or replacing the construction error. These private
bits are not a persistent report or a public diagnostic API: if stderr is
unavailable, the caller does not receive the cleanup category through its
original error. That observability limitation must be accepted explicitly as
part of this patch review; the proposal does not claim guaranteed delivery.

If controller close fails, container cleanup still runs. If container
destruction fails, the guard reports it and does not retry a raw handle.
If observer removal fails, its callback allocation remains alive rather than
becoming a dangling pointer. A later successful destruction/removal can
release it; a failure at final destruction may retain the allocation until
process exit. Those cases preserve memory safety at the cost of a reported
resource leak. They are not successful cleanup or permission to silently retry.

The original successful non-child parent-subclass implementation still uses
a fixed procedure/ID, ignores installation/removal failures and has its
existing replacement/reparenting limitations. This proposal avoids acquiring
it on failed construction; it does not repair that separate successful-owner
policy. The existing `InnerWebView::drop` and approved page-close callback are
unchanged. Likewise, phase-boundary alive checks do not prove all operations
safe against reentrant HWND reuse: the existing sizing sequence still calls
COM `SetBounds` before `SetWindowPos` without an intervening identity check.
The new destruction observer proves the temporary cleanup decision only;
general operation/reentrancy and successful-owner teardown remain outside
that claim. Syrinx uses child construction, but both constructor modes belong
in the proposed fault tests.

Delaying attachment also delays the parent's focus/resize callback during
initial visibility, focus and extension setup. Synthetic non-child focus and
bounds checks must establish whether final behavior remains correct; source
review alone does not establish that timing change as harmless.

## COM is a separate investigation

Frozen wry `new_in_hwnd:115` ignores the result of `CoInitializeEx` on every
construction attempt. This module has no matching `CoUninitialize`.
Syrinx's `crates/syrinx-platform/src/browser/environment.rs:24–36` checks its own
initialization result and balances that successful call through `Apartment`.
Its worker retains the apartment while pages and the shared environment live.

These facts do not establish a correct additional teardown count. In
particular, a failed initialization is not permission to uninitialize an
apartment belonging to another owner. Any separate correction must track the
actual result of each initialization, successful and failed construction,
thread affinity, COM reference release and late callbacks. This candidate
adds no `CoUninitialize` and makes no claim that COM imbalance caused the
reported breakpoint.

## Alternatives rejected

Syrinx cannot clean up a failed builder by enumerating the parent's children:
it never receives the failed attempt's owned HWND. Picking a matching class,
the newest child or the last handle risks destroying a successful page.
Ownership belongs inside the component that acquired the resource.

Starting a guard only after `create_container_hwnd` or after controller wait
misses earlier acquisitions. A guard that checks only `IsWindow` fails to
prove identity. Unconditional parent detach can remove another registration.
Replacing the original error with a cleanup error would hide the operation
that actually failed; ignoring cleanup errors would hide remaining resources.
Adding guessed COM teardown combines an unrelated ownership question with
this patch. Changing a page's identity or security settings addresses none
of these ownership gaps.

## Synthetic validation design, not executed

After a concrete patch is authorized, the integration chat should serialize
validation with its existing native lifecycle work. Use exclusively reserved
fresh synthetic profiles, generated fixture state, a retained synthetic
parent window and `about:blank` or the harness's own loopback page. No real
accounts, personal profiles or injected scripts in browsing pages are needed.

Fault hooks belong in a disposable copy of the candidate, compiled only for
the test harness. They must be outside page content and excluded from the
release diff. Wrap native operations and async completion delivery with
deterministic one-shot faults, recording only operation categories and
synthetic sequence numbers. Compare original error variants and numeric
HRESULTs with the uninfluenced failure path, never browsing content.

1. Fail before container acquisition, immediately after acquisition and at
   its first `SetWindowPos`. Check exact created/destroyed identities and
   baseline child count after each attempt. Keep a successful sibling alive.
2. Fail controller options, creation initiation, completion HRESULT, null
   completion and message wait. Deliver a non-null controller with failed
   status; separately cancel the receiver and then deliver success. Verify
   exactly one candidate-owned close attempt for each failed attempt's
   delivered controller and no close for a null completion. Successful
   transfer closes through the existing `InnerWebView` instead. A normal
   lifecycle harness alone cannot prove these paths.
3. Fail the first web-view query, representative settings and handler
   registration, navigation/visibility and final controller/container bounds.
   For non-child construction, exercise every failure before parent callback
   handoff. Verify unrelated parent callback state remains intact.
4. Destroy the synthetic parent while the controller wait pumps, and during
   controller close. Use the destruction token to demonstrate no later
   cleanup `DestroyWindow` targets a reused handle. Do not claim handle-reuse
   coverage unless the
   harness actually observes or deterministically models that reuse.
5. Inject controller `Close` failure, container destruction failure and any
   observer/subclass removal failure. Verify later independent cleanup still
   runs, callback memory remains valid, diagnostics contain only the stated
   categories, and the original construction error remains unchanged. A
   cleanup failure is a failure result, never evidence of zero remaining
   resources. Check diagnostic-sink failure separately.
6. Retry successfully after every ordinary injected failure, then exercise
   hide/show/resize/close and shutdown. Test two independent children and a
   pre-existing non-child callback on the parent. Verify one handoff and one
   normal destructor path after successful construction.

Run scoped compilation and Clippy with no new suppression, then the ordinary
`cargo xtask check` and authorized native checks. Record exact source and
executable hashes, execution context, timeouts and failure logs. Success
would demonstrate these synthetic ownership paths; it would still not
diagnose the original `0x80000003` report. That requires separate causal
evidence, such as a matching synthetic reproduction and failure trace.

## Frozen review record and subsequent authorization

Three subagents handled disjoint source analysis, candidate preparation and
independent review. Their records are
`target/wry-failed-construction-proposal/source-audit.md`,
`target/wry-failed-construction-proposal/candidate-notes.md` and
`target/wry-failed-construction-proposal/independent-review.md`.
The independent source review found no blocking failed-construction ownership
defect in this candidate, subject to the explicit limits above. This is not
compilation, runtime acceptance or merge approval.

The final candidate is frozen at these raw SHA-256 values:

| Candidate artifact | SHA-256 |
| --- | --- |
| `candidate/src/webview2/mod.rs` | `23e0ade313f6fb888854e5f3aca0dfe46807b77a39f41660795520c6a429a512` |
| `candidate/src/webview2/construction.rs` | `c446c9c14056ae838eef05d26cd4e0504d0ed5c228ca10c3983730c377f8aa76` |
| `candidate/failed-construction.patch` | `a87ca514149545576a25a39a390a444234d6db9159bcde751e7806aafc959042` |

Before application, scoped `rustfmt --check` passed for the proposal copies. A read-only
`git apply --check` against the preserved proposal baseline passed; the
patch was not applied. The final inventory comparison found all 84 vendor
files unchanged. Workspace manifest and lockfile hashes also remain equal
to the inputs above. Existing branch and uncommitted work were preserved.
No claim is made that unrelated files owned by parallel sessions are frozen.

The owner subsequently approved the following precise request: apply the
additional pinned wry patch identified
by the SHA-256 above, changing `src/webview2/mod.rs` and adding private
`src/webview2/construction.rs`, including the limited structural diagnostics,
new observer/liveness failure paths and delayed non-child attachment. Extend
the narrow source exception only as needed to retain unchanged upstream
comments and lint annotations in the existing vendor package. There is no
new suppression, public API, dependency version/feature, COM teardown or
page-security change to approve. Any materially revised diff needs renewed
review rather than being included under this frozen artifact's identity.

The approval is separate from `AGENTS.md`'s earlier close-callback exception,
which by itself “does not cover another dependency patch”. Serialized
synthetic compilation, fault-injection, native checks and
the normal gate must follow the applicable execution authorization before
acceptance. The integration chat handles that execution; this session's
explicit no-machine-work constraint remains honored. Commits, merges,
publication and upstream messages remain separate and unauthorized. D12's
future upstream offer can use a locally prepared diff after review, but this
document sends nothing externally.

## Applied source checkpoint

The owner's explicit instruction was: “I approve applying the frozen wry
failed-construction patch, including its documented limitations. Coordinate
validation through the integration chat, use subagents for review, and don’t
commit or merge.” On 2026-10-10 at 21:51:57 EDT, the checked patch was applied
with `git apply`; both resulting source hashes equal the frozen table above.
The exact patch is also retained outside ignored build output at
`vendor/wry-0.57.0.failed-construction.patch`, with the same SHA-256. A narrow
`.gitattributes` entry preserves its bytes across checkouts.

Independent applied-source and package audits are recorded in
`target/wry-failed-construction-proposal/applied-independent-review.md` and
`target/wry-failed-construction-proposal/applied-source-audit.md`. All 84
original package files were compared: only `src/webview2/mod.rs` changed,
`src/webview2/construction.rs` was added, and the other 83 files are unchanged.
The resulting package contains 85 files and 880,146 bytes. Workspace manifest
and lockfile remain unchanged. `vendor/wry-0.57.0.provenance.md` records the
layered patch identities. No product source, public API, COM accounting or
page-security behavior was changed.

An integration build had already started before coordination reached it.
Its subsequent exit-zero smoke used the prior wry artifact and is explicitly
pre-patch evidence (`target/continuation-browser-parallel-app-smoke.log`).
It cannot validate the applied source. The integration chat has the precise
application time and hashes and will perform fresh serialized validation.
Until those results are recorded, this checkpoint means applied and
source-reviewed, not runtime-accepted or a verified breakpoint fix.

### First serialized patched checks

The integration chat reports that workspace formatting and Clippy pass with
the frozen applied source. The full gate in
`target/continuation-wry-construction-gate.log` records 301 passing tests,
including all 41 application and 160 search unit tests. It then fails because
Windows Application Control prevents `frizbee_padding` from executing
(error 4551). That is an incomplete gate, not a test assertion failure or
permission to skip the executable; later gate stages were not reached.
Windows security settings were unchanged.

The separately rebuilt application smoke check exited zero, as reported by
the integration chat, in `target/continuation-wry-construction-app-smoke.log`.
Its successful startup/shutdown does not establish injected-failure cleanup.
The integration chat also confirmed terminal exit code zero for the patched
ordinary native lifecycle fixture, including close, visibility, bounds, retry
and shutdown. Its log, `target/continuation-wry-construction-native.log`, has
SHA-256 `e6e205ae6af103bf2d28af87d058256ecca3cbfa4d0b751e5f8c774d5b5c5618`.
That result covers the normal lifecycle only; fault-injection results remain
separate.
The existing upstream unused `platform_webview_version` warning remains
unchanged; no suppression was added. Disposable fault-injection source is
prepared under `target/wry-failed-construction-proposal/validation/` and is
kept separate from the approved release source. Its `README.md` names the
authoritative copy and 30 serial case/mode invocations; earlier
copies are retained superseded evidence and must not be executed. The v4
manifest SHA-256 is
`49bb04d969192a84fed6adbdcb7ea03a0e9f0312167430afbc1baf68a3f5030e`.

This first fault-test subset checks original synthetic error identity,
temporary close counts, exact direct-child handle sets, surviving native
window/parent identity and parent-subclass reference
data, child retry on the retained parent after retiring the survivor,
non-child retry on a fresh parent, actual failed completion delivery and
callback-context close, plus forced Close/DestroyWindow failure diagnostics.
Profiles are freshly and exclusively created and environment overrides are
refused. Its late completion test keeps the container alive while pumping;
delivery after constructor return/container destruction is still untested.
Observer removal/installation failure, diagnostic-sink failure, reentrant
destruction and handle reuse are also outside this first runnable subset.
The concrete harness does not turn those remaining design cases into passes.

The v3 overlay initially passed independent source review in
`target/wry-failed-construction-proposal/validation-independent-review.md`.
Its seven manifest entries and nine copied crate manifests were checked; no
dependency graph change or release-source drift was found. The reviewed copy,
commands, hashes and strict coverage limits were handed to the integration
chat for compilation and execution. This session has not run that harness;
runtime results must be recorded separately rather than inferred from this
handoff or the existing smoke result. The integration chat's additional
review then found that equal child counts and a working controller getter
could conceal destruction of the survivor window alongside a leaked attempt.
Execution was held before v3 ran. V3 is superseded: v4 must compare the exact
direct-child handle sets, check the survivor's native window and parent,
identify the exact deliberately retained attempt, and check its deadline
inside message draining as well as outside it. Those test-only changes do
not modify the approved release patch. V4 implements these checks: ordinary
failures must restore exactly the baseline handle set, with the attempted
container no longer live; the forced destruction failure may retain exactly
the baseline plus the recorded attempt. It also checks the 20-second deadline
inside each message-drain iteration and retains the independent process
watchdog. These numeric-handle assertions are not proof of forced handle
generation/reuse, which remains outside this subset.

The corrected v4 passed independent source review in
`target/wry-failed-construction-proposal/validation-v4-independent-review.md`.
All seven v4 manifest entries and nine copied crate manifests match their
sources; all seven preserved v3 entries still match its earlier manifest.
The release source and dependency graph remain unchanged. V4 is frozen for
the integration chat to compile and run at that checkpoint.

### Disposable harness loader failure and v5 preparation

The integration chat subsequently built v4 successfully in 1 minute 46 seconds
(`target/continuation-wry-fault-v4-build.log`). Besides the existing upstream
warning, the harness produced two unused-result warnings for informational
Win32 Boolean values. The first `settings child` process exited with
`0xC0000139` before producing any output. Its stdout and stderr logs are
`target/continuation-wry-fault-v4-settings-child.stdout.log` and the matching
`.stderr.log`. No fault case passed, and the other 29 were not run.

Independent read-only inspection of executable imports and resources found
that the harness imports `GetWindowSubclass`, which this machine's System32
common-controls library does not export by name. The installed side-by-side
version 6 library does export it. Neither this executable nor the working
`page_close.exe` embeds an application manifest; the latter does not import
the missing function. This strongly supports a harness loader mismatch,
pending confirmation by a successful manifested launch. It is separate from
the original WebView2 breakpoint and establishes no cleanup-test result.

Version 5 preserves the v4 sources and failed-run evidence. Its disposable
build helper requests Common Controls v6 only for the fault-test executable,
using `cargo rustc --example` and an explicit embedded manifest. Microsoft's
[linker reference](https://learn.microsoft.com/en-us/cpp/build/reference/manifestinput-specify-manifest-input?view=msvc-170)
documents this combination of `/MANIFESTINPUT` and `/MANIFEST:EMBED`.
No release source, Cargo manifest, lockfile, build script, Windows setting or
dependency changes. The harness also converts the two informational Boolean
results without inventing an error condition or suppressing a warning.
The independent review and frozen input hashes are recorded beside the
disposable source. The integration chat owns compilation, inspection of the
embedded manifest, and the first launch before running the remaining cases.
Source preparation alone does not prove manifest activation or warning-free
compilation, and this explicit harness activation context is not evidence
that the product uses the same activation context.

The final v5 source review found no blocker to integration execution and is
saved in `target/wry-failed-construction-proposal/validation-v5-independent-review.md`.
All seven v5 source entries, all seven preserved v4 entries and nine copied
crate manifests were verified. The frozen v5 source manifest SHA-256 is
`f49167620711b7d9a96e3a2a85e1f5fb16e84da3861b62c7f99ab74a2a3b7998`;
its activation-input inventory is
`5f60bdb800e4a6cc1ea98fd747dd342d6419cdffcce15f248eccfc3e63f36d75`.
These reviewed inputs were handed to the integration chat for serialized
execution. The approved release source hashes remain unchanged.

### Executed v5 fault-test subset

The integration chat built the frozen v5 example with terminal exit code zero
in 1 minute 50 seconds. Only the pre-existing upstream
`platform_webview_version` warning remains. It extracted the executable's
embedded manifest with `mt.exe` and confirmed Common Controls v6 and
`asInvoker`. The first three processes and the remaining 27 all exited zero.
The saved aggregate contains all 15 cases in both modes exactly once; each
has its matching `PASS` line. Stderr is empty except for the exact expected
cleanup diagnostic in each forced Close or DestroyWindow failure case.
This chat independently checked those 30 saved result/output combinations;
the integration chat performed all compilation and native execution.

The evidence identities are:

| Artifact under `target/` | SHA-256 |
| --- | --- |
| `continuation-wry-fault-v5-results.json` | `384902be7ab55637cc496b05d89028a557547e4bd7269b63af7bd7d2a10c72a0` |
| `continuation-wry-fault-v5-build.log` | `2f4160deb3efebdc68cff12409c8663a7e3ee78f46e9334700066058e6a8a54d` |
| `continuation-wry-fault-v5-embedded.manifest` | `af51e233d55b37535b2474b05e3163e2af5beb59877090872760578d7ccaf99e` |
| `wry-failed-construction-proposal/validation/workspace_v5/target/local/examples/failed_construction.exe` | `2ff83f312bbec9b507eae7a1b43e669a5420b0eec4a85ac3fbbacaa83d3460f1` |

Per-case stdout, stderr and result files share the
`continuation-wry-fault-v5-` prefix. The successful manifested launch resolves
the disposable harness's startup blocker. These passes establish the bounded
ownership, original-error, survivor, retry and cleanup-diagnostic assertions
described above. They do not establish callback delivery after container
destruction, forced handle reuse, observer or diagnostic-sink failure,
deliberate reentrant destruction, or the other excluded paths. They neither
diagnose the original breakpoint nor establish production activation-context
equivalence. At this checkpoint the full workspace gate was incomplete because
of its separately recorded Windows Application Control block. The integration
chat subsequently reported a complete 369-test gate and application smoke pass
in `target/continuation-crash-usability-gate.log`, without security changes.
Preserve both results; the later pass does not diagnose the earlier block or
the breakpoint. No commit or merge was performed.
