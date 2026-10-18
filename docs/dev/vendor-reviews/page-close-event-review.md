# Keep page-requested closes inside Syrinx's tab lifecycle

Status: owner approved the page-close patch and narrow vendor exception on
2026-10-10. The patch is implemented and its native close fixture passed. This approval
does not authorize a commit, merge or upstream publication.

A page that requests its own close must go through the same saved-tab and
storage rules as the sidebar close button. Otherwise its native window can
disappear while Syrinx still shows a live tab. The approved implementation
now routes that request through durable session closure. This is not evidence that it caused the owner's
reported `0x80000003` breakpoint; that crash remains unproved.

## What the unpatched source established

- Pinned wry 0.57.0, `src/webview2/mod.rs:478`, installs its handlers while
  constructing the web view. At lines 683–685, its `WindowCloseRequested`
  handler calls `DestroyWindow(hwnd)` immediately. Its registration token is
  discarded, as are the other lifetime-bound handler tokens.
- Syrinx calls `build_as_child` in
  `crates/syrinx-platform/src/browser/page.rs:83`, then adds metadata,
  navigation, process-failure and shortcut handlers. It has no close event.
  The page remains in the browser worker's `pages` map until a
  `BrowserCommand::Close` arrives.
- `crates/syrinx-app/src/ui.rs` records a created view in `loaded`. That state
  is reconciled with the session model, which likewise receives no native
  close observation. A requested native close therefore bypasses both.
- wry owns the child HWND for its whole `InnerWebView` lifetime. Its `Drop`
  calls controller `Close`, then `DestroyWindow` for a child. Destroying that
  HWND earlier leaves wry's stored handle stale. Windows handle reuse makes
  relying on a later second destruction undesirable; no such reuse or crash
  has been observed in Syrinx.

Microsoft defines `WindowCloseRequested` as a request that the host may act
on, rather than an instruction to destroy a host window immediately. It also
warns that repeated requests need not each produce an event. The documented
contract does not establish that an Syrinx handler registered after wry's
destructive handler will reliably run after that destruction.
[WebView2 close-event reference](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2#add_windowcloserequested).

Registering a second raw callback is therefore not a verified fix. Observing
`WM_NCDESTROY` could detect the disappearing child, but would retain the
premature destruction and stale ownership. The recommendation is one small
wry callback override, with the default behavior unchanged for other users.

## Approved narrow patch

The patch keeps wry at exactly 0.57.0 with its current features. A Windows-only
builder callback replaces wry's automatic destruction when supplied. Syrinx
uses it only to enqueue the page's existing numeric ID. No page script,
inter-process communication bridge, user-agent change or request filter is
involved.

The following is the intended two-file dependency diff. Context is abbreviated;
the verification record below distinguishes source changes from runtime evidence.

```diff
--- wry-0.57.0/src/lib.rs
+++ wry-0.57.0/src/lib.rs
@@ Windows PlatformSpecificWebViewAttributes
+  window_close_requested_handler: Option<Rc<dyn Fn()>>,
@@ Windows PlatformSpecificWebViewAttributes::default
+      window_close_requested_handler: None,
@@ WebViewBuilderExtWindows
+  /// Delegates a page's close request to the host instead of destroying its window.
+  /// The callback must defer closing the web view until after the event returns.
+  fn with_window_close_requested_handler(self, handler: impl Fn() + 'static) -> Self;
@@ impl WebViewBuilderExtWindows for WebViewBuilder<'_>
+  fn with_window_close_requested_handler(mut self, handler: impl Fn() + 'static) -> Self {
+    self.platform_specific.window_close_requested_handler = Some(Rc::new(handler));
+    self
+  }
--- wry-0.57.0/src/webview2/mod.rs
+++ wry-0.57.0/src/webview2/mod.rs
@@ init_webview, after settings are configured
-    unsafe { Self::attach_handlers(hwnd, &webview, &mut attributes, &mut token, env)? };
+    unsafe {
+      Self::attach_handlers(
+        hwnd, &webview, &mut attributes, &mut token, env,
+        pl_attrs.window_close_requested_handler.clone(),
+      )?
+    };
@@ attach_handlers arguments
     env: &ICoreWebView2Environment,
+    close_requested: Option<Rc<dyn Fn()>>,
@@ WindowCloseRequested registration
-      &WindowCloseRequestedEventHandler::create(Box::new(move |_, _| DestroyWindow(hwnd))),
+      &WindowCloseRequestedEventHandler::create(Box::new(move |_, _| {
+        if let Some(handler) = &close_requested {
+          handler();
+          Ok(())
+        } else {
+          DestroyWindow(hwnd)
+        }
+      })),
```

Both files already import `Rc`. Platform attributes derive `Clone`, so a
reference-counted callback preserves their existing ownership contract. The
callback captures only Syrinx's event sender and the original page ID, not a
controller, web view, HWND owner or session model. It introduces no reference
cycle to those owners. Existing teardown releases it with the web view.

Syrinx's accompanying changes are limited to the existing lifecycle:

| File | Proposed change and reason |
| --- | --- |
| `crates/syrinx-platform/src/browser.rs` | Add public `BrowserEvent::CloseRequested { id: u64 }`. Keep content-redacted diagnostics and existing browser commands. |
| `crates/syrinx-platform/src/browser/page.rs` | Supply the new builder callback before `build_as_child`. It only emits that event and returns; it does not close the controller or remove its page. |
| `crates/syrinx-app/src/ui.rs` | Convert a valid ID to `TabId` and send `SessionCommand::ObserveClosed(id)` unless whole-app shutdown has begun. Do not optimistically remove the row or native page. |
| `crates/syrinx-app/src/session.rs` | Add `ObserveClosed(TabId)`, which closes a still-present web tab through `TabModel::close` and is a no-op for an absent ID. Preserve the existing save-before-State acknowledgement. |
| `crates/syrinx-app/src/session/worker_tests.rs` and existing browser/UI test modules | Exercise real queued closes and event routing with synthetic state. Keep native event delivery as a separate runtime check. |

After storage acknowledges the updated session, existing `sync_pages` sends
`BrowserCommand::Close`. The browser worker removes its `Page`, and wry's
ordinary destructor owns the sole child-window destruction. Closing happens
outside the WebView2 event callback, as required by its serial callback model.
[WebView2 threading guidance](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/threading-model),
[controller Close reference](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller#close).

## Identity, repeats and failures

The event carries the live `TabId`, never a saved-page ID or a selected-tab
guess. Syrinx does not reuse live IDs. Repeated requests, a queued sidebar
close followed by a page close, and late callbacks after close are harmless
at the authoritative session worker: only the first still-present instance
is closed. A reopened saved row has a fresh live ID and cannot be closed by
the old event.

D42 and D43 remain unchanged. Closing an open saved page keeps its durable
saved row and fixed destination; its live instance enters the existing
20-entry web closure stack once. The right-then-left selection rule and final
Home state remain core-model decisions. Native callbacks do not remove saved
records, create tabs or choose the next active tab.

A close request may arrive during initial navigation before `Created` reaches
the UI. The same ID-based path must work then: the committed model removes
the pending page, and the existing create/close browser command ordering
performs cleanup. A later `Created` acknowledgement must not resurrect it.

On a persistence failure the UI retains its visible error and the native view
has not already been destroyed. Callback channel failure follows the existing
structural worker-stop behavior. Whole-app shutdown continues to close the
browser worker before stopping the session; late page requests must not queue
new model work during that shutdown.

## Smallest meaningful validation

1. Queue duplicate `ObserveClosed`, manual-close then observed-close, and an
   old-ID close after reopening a saved row. Assert no error, one history
   entry, the saved destination unchanged and the new live instance intact.
2. Run the real session worker against a fresh synthetic temporary database.
   Close a saved instance and the final ordinary tab, shut down, and reopen
   the store. Assert persisted unloaded saved rows and Home selection.
3. Test event routing and create acknowledgement ordering, including
   close-before-`Created`, absent IDs and whole-app shutdown. The callback must
   enqueue only, and the native page must remain owned until model acceptance.
4. After the source audit and normal gate, run a disposable native harness
   with a synthetic HTTP page served only on loopback. Trigger a legitimate
   content `window.close()` request and verify one logical close, native
   destruction after acknowledgement, saved-row preservation and safe exit.
   The script belongs to the synthetic test page; no script is injected into
   ordinary browsing pages. If Chromium refuses that test page's close, report
   the missing event evidence rather than treating the absence as a pass.

Pure tests cannot establish native callback delivery, controller teardown,
or the original breakpoint's cause. UI interaction still requires the
owner's permission. No runtime result is claimed by this proposal.

## Approval and dependency provenance

`AGENTS.md` requires explicit approval before “Changing public APIs other
code depends on”. The new cross-crate `BrowserEvent` variant falls under that
rule. D12 allows small pinned wry patches offered upstream, but the concrete
patch still needs its source review and a narrow vendoring exception for
unchanged upstream `TODO`/`FIXME` comments and existing lint annotations. The
original recorded source exception covered SQLite only; the owner has since
approved the narrow wry exception in `AGENTS.md`.

The owner approved this close-event API and two-file pinned wry patch,
including retaining unchanged upstream comments and lint annotations in its
vendor directory, so Syrinx can persist a page-requested close before
destroying its native view. Prepare the upstream diff under D12; obtain
separate permission before sending it publicly.

The checked baseline is the published registry archive, whose SHA-256 matches
`Cargo.lock`: `a819957a01b3119af85e638a38d242af76dbc87d130dca67bfd0441072e21ff0`.
Its `.cargo_vcs_info.json` identifies commit
`792d0359ba6501a4fc360ece17de2ae42329a47c` with `dirty: true`; the published
archive, rather than an assumed clean checkout of that commit, must be the
vendor baseline. Current file hashes are:

- `src/lib.rs`: `f5c9e28fbb1b6a4ddfed7ddface796a9e91689e8663457ed69b651ad00149ba7`.
- `src/webview2/mod.rs`: `4800257d9d251c59b5aa464814ade5f8f041a854f0e4498afbdb036eba5307b8`.

After approval, record the complete baseline inventory and modified hashes,
confirm all other files are unchanged, preserve dependency versions/features,
and audit the narrow source patch. The intended location is `vendor/wry-0.57.0`,
excluded from workspace membership and selected through `[patch.crates-io]`;
the lockfile should change this package's source only, without version churn.
wry declares `Apache-2.0 OR MIT`; retain `LICENSE-APACHE`, `LICENSE-MIT` and
`LICENSE.spdx` unchanged. This requires no new licence-policy exception.
Do not combine it with the separately
pending frizbee patch or the separate COM initialization finding.

## Verification record

The vendored archive and two-file change are recorded in
`vendor/wry-0.57.0.provenance.md`. Its local upstream diff is prepared but
unpublished. The lockfile comparison changes only wry's source/checksum;
versions and dependency edges remain identical. Independent reviews of the
vendor patch, Syrinx lifecycle and native fixture passed.

`target/page-close-app-platform-tests.log` records all 99 relevant tests
passing: 29 application, 48 platform unit, four backup integration and 18
storage integration tests. Six new application regressions cover queued
duplicates, manual then observed closes, stale IDs after reopening, plugin
and absent IDs, saved destination preservation and restart to Home, rejected
persistence with no State acknowledgement, and creation/shutdown ordering.
These tests use disposable synthetic stores only.

Dependency boundaries and workspace Clippy passed; logs are
`target/page-close-dependencies.log` and `target/page-close-clippy.log`.
Formatting initially passed. The final workspace recheck found a newly
changed assertion in the unrelated search profiler
`crates/syrinx-search/src/catalogue/profiling.rs`; that file was left untouched.
`target/page-close-format.log` preserves that failure, while scoped app and
platform formatting passes in `target/page-close-scoped-format.log`.
The vendored wry emits an existing unused
`platform_webview_version` warning with Syrinx's features; its body and lint
settings remain unchanged. No warning suppression was added.

The standalone fixture `crates/syrinx-platform/examples/page_close.rs`
requires actual close-event delivery from its own loopback HTML page. It
checks the native child and controller remain alive until a separate queued
Close, then checks child destruction, a replacement page and orderly shutdown.
Its profile is exclusively created and contains synthetic data. Saved-row
persistence is proven separately by the real session-worker tests above;
this native fixture does not drive the GPUI interface.

The fixture built under the unchanged `local` profile and ran on the normal
Windows desktop with exit zero. It received the actual close event, kept the
native child alive through subsequent navigation, observed its destruction
after Close, created another page and shut down cleanly. Build evidence is
`target/page-close-native-build.log`; runtime output is
`target/page-close-native-run.stdout.log`, with an empty sibling stderr log.
No certificate or Windows security change was involved.

The standard `target/local/syrinx-app.exe` was rebuilt with this patch and
passed a fresh normal-desktop `--smoke` run, exit zero and no stderr. Evidence
is `target/page-close-app-build.log` and
`target/page-close-app-smoke.stdout.log` / `.stderr.log`. Its SHA-256 at
verification was
`c475b1402be7683b4af449926b304aa19e5600bc2f4154f2dca39885193d02a7`.
This checks startup and shutdown, not interactive acceptance of the GPUI UI.

The full workspace gate remains incomplete because the two existing search
NUL-highlight regressions and D48 performance requirement remain unresolved.
No visual acceptance, original breakpoint diagnosis, commit or merge is
claimed by these checks.

### Independent continuation evidence

The continuation preserved a restricted-context failure in
`target/continuation-page-close-native.log`: the fixture timed out waiting
for a native event and reported a shutdown deadline failure, exiting 1.
The same built fixture then passed on the normal desktop, exiting 0 after
close delivery, deferred destruction and shutdown
(`target/continuation-page-close-desktop.log`). Each invocation exclusively
created its own synthetic profile. Neither result explains the execution
context difference or the owner's breakpoint; Windows settings were unchanged.

`target/continuation-initial-gate.log` records a separate failed complete-gate
attempt. Dependency boundaries, formatting and workspace Clippy passed.
Application, core and platform stages plus search's passing tests total 182
passes; the same two NUL-highlight search regressions fail. Cargo stopped
before store/xtask tests, documentation tests and application smoke. That
attempt predates the subsequent Home-palette intent correction and expanded
search profiler; it is not acceptance evidence for those changes.

After those changes, `target/continuation-final-gate-2.log` records passing
dependency checks, formatting and workspace Clippy, then 189 passing tests
and the same two failing search regressions. The application suite is now
36 passing tests. An earlier final-gate attempt stopped at formatting and
is retained in `target/continuation-final-gate.log`; formatting was corrected,
not disabled. The complete gate still stops before the later test/smoke stages.
