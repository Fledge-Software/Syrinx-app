# Browser navigation reliability review — 2026-10-10

This bounded D49 review checks address interpretation and native-page ownership
without disturbing the integration chat's shared checkout or serialized builds.
Source is frozen for integration validation. No Cargo commands, compiled
programs, application windows, timing tests or integration apps ran in this
session. The earlier 347-test gate predates these changes and is not their
validation result.

## Changes and behavior

`crates/atlas-app/src/navigation.rs` now recognizes the already supported URL
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

`crates/atlas-app/src/ui/page_lifecycle.rs` has one new regression and no
production changes. A saved page closes while its native creation callback is
delayed, then reopens with a fresh live identity. The test checks that callbacks
for the closed identity cannot consume the replacement's pending ownership or
alter its loaded ownership. The existing pending-ID guard already provides
this behavior; the audit found no demonstrated defect in these helpers.

`crates/atlas-app/src/ui/navigation_intents.rs` is unchanged. Its audit traced
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

`BrowserHost::send` in `crates/atlas-platform/src/browser.rs` places a command on
its Rust queue before calling `PostThreadMessageW`. A wake failure can therefore
return an error after queue acceptance. `page_command` in
`crates/atlas-app/src/ui.rs` treats every error as an unsuccessful enqueue. If a
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
cargo test --package atlas-app --profile local --offline --locked
cargo xtask check
```

Interactive acceptance should exercise a mixed-case HTTP(S) submission on a
fresh synthetic page and saved-page close/reopen, under the integration chat's
approved window-testing scope. Existing windows and personal data were untouched.

The source fingerprints at handoff are SHA-256:

| File under `crates/atlas-app/src/` | SHA-256 |
| --- | --- |
| `navigation.rs` | `3eeeb8e380d37513b802b0b55059b3e810d32387d064974d34b5cb1161a27db5` |
| `ui/navigation_intents.rs` (unchanged) | `39161bbf385f457b556d483aa2882f3188909b4a1e30f6409e6bcb093aebff75` |
| `ui/page_lifecycle.rs` | `489aa5c580b377179343845a6bb4d719e0f0bfa0ce799323b775a24f0e60d01e` |

Only `navigation.rs`, `ui/page_lifecycle.rs` and this report were changed by this
session. Source is frozen pending the integration chat's focused tests and full
gate; the change is not yet declared validated or complete.
