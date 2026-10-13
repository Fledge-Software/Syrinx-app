# P1-14: truthful connection labels and certificate rejection

An HTTPS address alone cannot establish that a site is safe. The browser must
also refuse an invalid certificate without silently continuing. This slice
adds native observations and explicit cancellation, without scripts, request
filters, identity changes, dependency changes or another vendor patch.

## What the display means

`crates/syrinx-platform/src/browser/security.rs` observes native navigation
starts and completions. It records the attempted scheme and the native
navigation ID, so an earlier completion cannot mark a later attempt loaded.
Redirects update the destination scheme. Failed observations remain unknown;
certificate rejection survives later completion. The UI reads this state per
live tab, rather than inferring security from saved address metadata.

`crates/syrinx-app/src/ui/security.rs` draws a two-line label beside the address:

- HTTP is labelled unencrypted and has no TLS certificate. Pending or failed
  HTTP navigation is explicitly labelled an attempt too.
- A loaded HTTPS document says “No certificate error reported.” This describes
  an observation, not a promise about identity, mixed content, malware, page
  safety or an independently inspected certificate chain.
- Pending or failed HTTPS navigation says “HTTPS attempt,” with pending,
  failed/unknown or certificate-request-blocked status. Cancellation can leave
  the prior document visible; the attempt label does not certify that document.
- Unavailable observations and non-HTTP(S) schemes make no HTTPS assurance.

There is no “secure” label, green padlock or certificate bypass control.
On `ServerCertificateErrorDetected`, the native handler sets `CANCEL` before
reporting anything, then reads the action back. A callback failure stops the
navigation and reports unavailable state. The event also covers subresources,
so the label says a certificate request was blocked, without claiming that the
main document's certificate was necessarily responsible. Syrinx never selects
`ALWAYS_ALLOW` and never caches a certificate exception.

Microsoft documents [certificate errors and their ordering](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_14?view=webview2-1.0.3856.49)
and [the event's default action](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2servercertificateerrordetectedeventargs?view=webview2-1.0.3537.50).
The runtime's unhandled default is an interstitial, not explicit cancellation;
Syrinx installs the cancellation handler before first navigation. No development
protocol, remote debugging port or certificate-store change is used.

## Validation and fixture design

`target/p1-14-native-4.log` passes all five suites. Independent review found no
remaining fixture blocker after the expected-failure handling was corrected.
`target/p1-14-gate-final-2.log` is green: dependency boundaries, formatting,
Clippy, all 454 tests and the application smoke run passed. No error 4551
was observed. The unchanged upstream wry dead-code warning remains. Pure tests
cover stale navigation completions, redirected schemes, sticky certificate
rejection, unknown observation failures and the display's limited claims.

`target/p1-14-gate-1.log` passed before the last HTTP-attempt refinement.
`target/p1-14-gate-final.log` retained a failed regression: a shell pipeline
replaced the Unicode separator in an edit, leaving the old HTTP label intact.
The label and exact assertion were corrected directly; no assertion was relaxed.

`examples/page_close/security.rs` adds a fifth independent suite. It creates a
fresh profile, loads a synthetic HTTP baseline, attempts a loopback server with
a fresh self-signed certificate twice, and recovers to distinct HTTP documents.
Each attempt requires a native failed navigation and a production callback
report issued only after cancellation readback. Successful invalid navigation,
an invalid-content title or any HTTP request delivered to the TLS server fails
the test. Repeated rejection checks that no certificate exception was cached.
The original suites and their individual 45-second deadlines remain intact;
only the outer watchdog accounts for the additional suite.

All recorded runs set `TEMP` and `TMP` to this worktree's `target/session-temp`
before launching Cargo. The shared fixture uses `std::env::temp_dir()`, so this
override is required to reproduce the same confinement:

```powershell
$env:TEMP = Join-Path (Get-Location) 'target/session-temp'
$env:TMP = $env:TEMP
cargo run --offline --locked --profile local --package syrinx-platform --example page_close
```

Create `target/session-temp` first if it is absent. Retain only synthetic files
there; the fixture reserves a unique child directory for each suite.

The test helper uses the installed Python standard library's TLS server and
PowerShell/.NET certificate generation. A random ephemeral RSA key and fresh
self-signed certificate are generated for each run. Temporary PEM files remain
under the exclusive synthetic directory in this worktree's ignored `target/`;
they are not trusted, committed or printed. The fixture terminates only its own
helper process; a 75-second helper lifetime also bounds abnormal abandonment.
No software is installed and no trust store or Windows policy is changed.

The retained diagnostic logs explain changes to the new fixture:

- `target/p1-14-native-1.log` stopped in setup because a shared helper expected
  an `about:blank` source change on an already blank page. Setup now uses HTTP.
- `target/p1-14-native-2.log` reached HTTP, but Windows SslStream could not serve
  the in-memory certificate with its ephemeral key. Python TLS now serves it.
- `target/p1-14-native-3.log` proved certificate rejection, then awaited a source
  change when navigating to the retained document's unchanged address.
  Recovery now uses distinct HTTP query destinations and still requires source,
  successful completion and HTTP state. Expected certificate-denial completion
  is accepted only in this suite for its own tab; shared error checks stay intact.

The known original lifecycle navigation timeout remains intermittent and
undiagnosed; see `docs/fixture-transport-timing-review.md`. Later passing runs do
not claim to fix it. The owner explicitly permits these slices to continue.

## Owner visual checks

The gate runs the app, but desktop inspection is unavailable in this session.
Visual acceptance remains open:

1. Launch `cargo syrinx -- --test-data-dir <new-absolute-directory-under-target>`.
   Use a disposable profile only. Open the P1-13 local HTTP page and verify
   “HTTP · unencrypted.” If checking ordinary HTTPS manually, `https://example.com`
   should say “No certificate error reported” after loading, never “secure.”
2. In another PowerShell terminal in this worktree, create a new directory under
   `target/`. Set `SYRINX_SYNTHETIC_CERT_DIRECTORY` to its absolute path and
   `SYRINX_SYNTHETIC_CERT_GENERATOR` to the absolute path of
   `crates/syrinx-platform/examples/page_close/certificate_server.ps1`. Run
   `python crates/syrinx-platform/examples/page_close/certificate_server.py`.
   Read the published port from that directory's `port` file in a second terminal.
   The server lasts 75 seconds; prepare the app first, or repeat with a fresh directory.
3. Open `https://127.0.0.1:<port>/invalid`. Verify “HTTPS attempt” and
   “Certificate request blocked”; the invalid server's content must not appear.
   No bypass button is offered. The previous document can remain visible.
4. Reload/retry the invalid address, switch to another tab and return. The
   appropriate per-tab status must remain. Navigate to a distinct local HTTP
   address and verify the label changes to unencrypted. Check narrow-window
   layout, long addresses and DPI scaling.

## Integration handoff

P1-14's ledger row records delivery. P1-11 retains its historical partial status;
its stale “not started” continuation sentence now points to the later evidence.
The shared `syrinx-app/src/ui.rs`
change is limited to module/state/event/display wiring and cleanup. The workspace
manifest, lockfile and platform `lib.rs` are unchanged. No deferred-work marker
was introduced. P1-12's schema-2 permission migration still needs renumbering at
integration; Ctrl H still needs the parallel history-list handler described in
the P1-13 report. No branch is merged or pushed.
The earlier P1-12 report also receives a quotation-mark typo correction.
