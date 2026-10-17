# Browser navigation reliability review — 2026-10-10

The initial review below is historical. The integration chat subsequently
validated the integrated preview with its 376-test gate and committed baseline
`a94500a`. The 2026-10-11 fixture continuation at the end records new work whose
execution remains assigned exclusively to that chat.

This bounded D49 review checks address interpretation and native-page ownership
without disturbing the integration chat's shared checkout or serialized builds.
Source is frozen for integration validation. No Cargo commands, compiled
programs, application windows, timing tests or integration apps ran in this
session. The earlier 347-test gate predates these changes and is not their
validation result.

## Changes and behavior

`crates/syrinx-app/src/navigation.rs` now recognizes the already supported URL
schemes without regard to ASCII case. Previously, submitting
`HTTPS://example.test/Case?Key=Value#Part` returned the preview's unsupported-
scheme error. It now takes the existing URL validation path and preserves the
path, query and fragment case. Mixed-case `javascript:` and `data:` previously
fell through to web search; they now receive the same refusal as their lowercase
forms. `aBoUt:blank` becomes canonical `about:blank`, which the existing Home
logic recognizes. The opaque `blank` portion remains case-sensitive.

Three new tests cover mixed-case HTTP and HTTPS, unchanged userinfo refusal,
malformed hosts, mixed-case refused schemes, Home canonicalization and explicit
web search preserving the original text. Search remains an explicit submission;
the parser makes no network requests. The implementation compares only the
scheme instead of lowercasing the full address, which would change destinations
whose paths or query values depend on case.

`crates/syrinx-app/src/ui/page_lifecycle.rs` has one new regression and no
production changes. A saved page closes while its native creation callback is
delayed, then reopens with a fresh live identity. The test checks that callbacks
for the closed identity cannot consume the replacement's pending ownership or
alter its loaded ownership. The existing pending-ID guard already provides
this behavior; the audit found no demonstrated defect in these helpers.

`crates/syrinx-app/src/ui/navigation_intents.rs` is unchanged. Its audit traced
committed navigation through `ui.rs`, `session.rs` and the browser command queue.
Retained destinations survive delayed readiness and inactive selection; later
navigation follows creation in queue order; closing removes intents for old live
identities. No demonstrated module-local defect required a fix.

## Independent review

Three subagents held disjoint ownership of the three modules. The navigation-
intent auditor independently reviewed the parser implementation and its final
tests. The coordinating agent also reviewed that change and the saved-page
lifecycle regression against the real callers and core reopen implementation.
The final search-query assertion uses explicit owned strings to avoid ambiguous
conversion inference. No actionable source-review findings remain in the changed
files. Source review does not establish that the new tests compile or pass.

## Integration follow-up outside this ownership boundary

`BrowserHost::send` in `crates/syrinx-platform/src/browser.rs` places a command on
its Rust queue before calling `PostThreadMessageW`. A wake failure can therefore
return an error after queue acceptance. `page_command` in
`crates/syrinx-app/src/ui.rs` treats every error as an unsuccessful enqueue. If a
queued Create later executes, the UI can lack pending ownership and ignore its
Created callback; a later synchronization can attempt duplicate creation.

The ordering is confirmed in source, but this review did not reproduce a wake
failure or establish its connection to any reported runtime problem. The
integration owner should assess this with fault-injection coverage before
choosing a delivery-contract change. Both files remain untouched by this review.
A failed native creation followed by late URL metadata was also considered, but
no concrete callback sequence was demonstrated; no speculative retry change was
made. Neither observation diagnoses the original WebView2 breakpoint report.

## Validation and handoff

Scoped `rustfmt --edition 2024 --check` passed for all three owned modules.
Scoped `git diff --check` passed, although these files are already untracked in
the shared preview tree and that command does not validate untracked contents.
The coordinating agent inspected the final source directly. No branch, commit,
merge, dependency, public API or existing shared documentation was changed.

The integration chat must run the following after its other source is frozen:

```powershell
cargo test --package syrinx-app --profile local --offline --locked
cargo xtask check
```

Interactive acceptance should exercise a mixed-case HTTP(S) submission on a
fresh synthetic page and saved-page close/reopen, under the integration chat's
approved window-testing scope. Existing windows and personal data were untouched.

The source fingerprints at handoff are SHA-256:

| File under `crates/syrinx-app/src/` | SHA-256 |
| --- | --- |
| `navigation.rs` | `3eeeb8e380d37513b802b0b55059b3e810d32387d064974d34b5cb1161a27db5` |
| `ui/navigation_intents.rs` (unchanged) | `39161bbf385f457b556d483aa2882f3188909b4a1e30f6409e6bcb093aebff75` |
| `ui/page_lifecycle.rs` | `489aa5c580b377179343845a6bb4d719e0f0bfa0ce799323b775a24f0e60d01e` |

Only `navigation.rs`, `ui/page_lifecycle.rs` and this report were changed by this
session. Source is frozen pending the integration chat's focused tests and full
gate; the change is not yet declared validated or complete.

## Fixture transport continuation — 2026-10-11

The synthetic close-page server must wait for request bytes before deciding that
a speculative connection is idle. In `crates/syrinx-platform/examples/page_close.rs`,
the listener is intentionally nonblocking, but its accepted streams previously
received read/write timeouts without switching to blocking mode. Windows accepts
inherit listener properties, and socket timeouts govern blocking calls. Thus an
empty first read could return `WouldBlock` immediately; `respond` classified that
as harmless completion and dropped the stream before a request arrived. This is
a source-confirmed fixture defect, not a diagnosis of the owner's breakpoint.
Sources: [Microsoft accept semantics](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-accept)
and [socket timeout options](https://learn.microsoft.com/en-us/windows/win32/winsock/sol-socket-socket-options).

The installed Rust 1.97.1 source corroborates this path: the local standard-library
HTML source `sys/net/connection/socket/windows.rs.html` lines 215–221 accepts
without resetting mode, lines 371–382 set timeout options, and lines 406–410
change mode through `FIONBIO`. `sys/io/error/windows.rs.html` lines 78–79 maps
Windows errors 10035 and 10060 to `WouldBlock` and `TimedOut`, respectively.

The correction extracts the existing timeout setup into `configure_stream` and
adds checked `set_nonblocking(false)` before it. Listener polling, response bytes,
request bounds, deadlines and browser lifecycle assertions remain unchanged.
The focused regression keeps a connected synthetic client open without sending
bytes: the inherited mode must produce `WouldBlock`/10035, then the production
configuration must produce `TimedOut`/10060. Both probes have the existing
one-second receive bound; no sleep or elapsed-time threshold decides success.
A guaranteed split-header test would need additional read-boundary coordination;
this patch instead tests the socket-mode defect directly. Partial-header EOF
reporting remains a separate finding and is not changed here.

The preserved v2/v4 pairs failed three of three times without server logging and
passed three of three with it. V4's `empty_timeouts` counter combines `WouldBlock`
and `TimedOut`, so its counts do not establish one-second waits or cumulative
delays. Its acceptance log precedes the first read and could change whether bytes
are available; that causal explanation remains unverified. The four passing
v4/v5/v5/v4 socket-drop comparisons retained that acceptance log, so they do not
test this mechanism. See `docs/fixture-transport-timing-review.md` and preserved
`target/continuation-native-server-pair*` and
`target/continuation-socket-close-*` records.

The callback audit found no preceding host close for failing fresh page IDs
1, 104 and 107. The fixture waits for successful navigation and close request
before its later lifecycle exercise, and shutdown follows the failed wait.
Tab-only event correlation is less precise than native navigation IDs, but it
does not establish a lifecycle cause for those fresh-page failures. No browser
callback, teardown or dependency changes accompany the fixture correction.

The integration chat owns execution and must serialize this focused command with
its other builds, then perform its bounded native validation and full gate:

```powershell
cargo test -p syrinx-platform --example page_close --profile local --offline --locked
cargo xtask check
```

Runtime validation is pending; this continuation has run no Cargo command,
compiled executable or native window. Shared handoff/status documents remain
owned by the integration chat.

Independent source review found no actionable issue in the final correction and
regression. Scoped formatting and diff checks passed. The frozen example's
SHA-256 is `be8f602fa618f75d9f50984c19240c6a4e95cd24160e51c319ff0f9696aa2cf6`.
The explicit example test command is required: the workspace gate's ordinary
`cargo test --workspace` does not select this in-file example test for execution.

### Integration validation received — 2026-10-11

The integration chat subsequently ran the focused example regression: one test
passed, with output in `target/continuation-blocking-fixture-test.log`. The example
build passed in `target/continuation-blocking-fixture-build.log`. One bounded
normal-desktop native run exited zero and emitted the complete twelve-cycle,
multipage-shutdown marker; its records are
`target/continuation-blocking-fixture-native.*`. The tested binary SHA-256 was
`3a8fc36f2723bba0d66a05a31bfcb35d14337eee5873b77c501b7a7966c9f651`.
The integration chat's independent reviewer also accepted the fixture diff.

These results supersede the pending focused/native validation above. The full
gate remains pending until the integration chat freezes its concurrent palette
correction. One passing native run establishes this bounded run's outcome; it
does not establish the original breakpoint's cause or eliminate all intermittent
browser failures. All execution remained serialized in the integration chat.

## Navigation ordering audit — 2026-10-11

The bounded, read-only audit examined Back, Forward and Reload alongside queued
tab switching, closing, restoration and in-flight navigation. It established no
policy-independent wrong-tab or stale-instance defect, so no source fix or new
test is proposed. This is a source-review verdict, not additional runtime
validation.

An unresolved routing distinction remains. In `crates/syrinx-app/src/ui.rs`, tab
selection is queued to the session worker, while history actions immediately
target the currently shown page. A queued switch from A to B followed by Reload
before the selection snapshot arrives can therefore send Reload to A. Typed
`NavigateActive` instead resolves the worker's authoritative selection after
earlier session commands. The native shortcut path in
`crates/syrinx-platform/src/browser/page.rs` and `browser.rs` also delivers a
shortcut without its originating page ID.

These paths have different ordering semantics, but deciding which page a history
action should affect requires an explicit routing rule. Carrying the source page
ID would preserve the originating page; resolving the action through the worker
would follow earlier queued selection changes. Neither is an unconditional fix:
for example, a native switch shortcut and Reload can both originate on A while
the queued selection is moving to B. The audit does not choose between those
rules or classify their difference as a proven defect.

The reviewed native commands retain explicit page IDs and queue order. Closing
removes the keyed page, and stale commands fail for that ID rather than being
redirected. Reopened pages receive fresh IDs. Creation ownership checks and
navigation intents retain live-page targeting and discard closed-page work;
restoration does not promise to restore the native history stack.

Source remains frozen. The usability chat retains ownership of `ui.rs` and
`palette.rs` for the separately identified queued palette-shortcut race. No
source-ID routing change, worker-relative history change, or test that merely
restates the implementation accompanies this report. The integration chat owns
all serialized builds and runtime validation; this audit ran none.
