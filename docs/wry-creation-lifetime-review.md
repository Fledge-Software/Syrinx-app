# Review failed native page creation before proposing another dependency patch

Status: source observation on 2026-10-10, independently reviewed. The owner
subsequently approved the exact failed-construction patch, including its
documented limitations; it is applied with byte identity verified. Ordinary
native lifecycle and all 30 cases in the bounded construction-fault subset
passed through the integration chat, as recorded in the patch review. This is not a
diagnosis of the owner's WebView2 breakpoint (`0x80000003`).

The subsequent concrete proposal is in
`docs/wry-failed-construction-patch-review.md`, with candidate source and an
exact diff under ignored `target/wry-failed-construction-proposal/`. It is
based on the current vendor source, including the approved close callback,
and was applied only after that separate approval. It additionally accounts for controller
completion after a canceled wait and destruction during message pumping.
Machine validation remains assigned to the integration chat.

The page-close callback fixes successful pages that ask to close themselves.
A separate failure path in pinned wry 0.57.0 can leave a native child window
without its normal cleanup owner. Keeping these findings separate prevents
the approved two-file callback patch from acquiring unrelated changes.

## Evidence in the pinned source before this correction

In `vendor/wry-0.57.0/src/webview2/mod.rs`, `new_in_hwnd` calls
`create_container_hwnd` before creating the controller and configuring the web
view. Both subsequent operations return through `?` on error. The resulting
`InnerWebView`, whose destructor closes the controller and destroys a child
window, is constructed only after those operations succeed. The intermediate
raw window handle has no temporary cleanup guard. A failure in that interval
therefore bypasses the destructor that normally owns child destruction.

The guard also needs to cover the helper itself: `create_container_hwnd`
calls fallible `SetWindowPos` after `CreateWindowExW`. Beginning cleanup
ownership only after that helper returns would leave this earlier gap open.
Syrinx's child path does not install wry's parent subclass; a generic upstream
correction must still account for the non-child path's subclass ownership.

Syrinx supplies its shared environment, so wry's fallback environment-creation
branch is not reached in ordinary Syrinx page creation. Controller creation
and `init_webview` remain fallible. `crates/syrinx-platform/src/browser/page.rs`
maps a failed builder result to `BrowserError::CreatePage`; the browser worker
reports `CreationFailed` and can continue processing commands. Syrinx has no
owned `Page` to drop when the builder has not returned one. Parent destruction
can eventually clean up child windows, but does not provide per-attempt
cleanup while the browser remains open.

This source observation does not establish how often any such operation fails,
whether a child leaked in the reported run, or whether it could explain a
breakpoint. No fault was induced, crash dump read, or personal profile inspected.

The earlier, separate COM observation also remains open: wry calls
`CoInitializeEx` for each construction attempt without a matching call in this
module. Syrinx balances its own apartment initialization and normally drops
pages and the shared environment before its apartment guard. Do not guess an
extra teardown count or alter that ownership without a dedicated review.

## Original recommendation and subsequent approval

The recommendation below led to the exact patch now recorded in
`docs/wry-failed-construction-patch-review.md` and
`vendor/wry-0.57.0.failed-construction.patch`. Applying it is approved;
compilation and native fault-injection acceptance are separate evidence.

Prepare a small scoped proposal for temporary ownership of a successfully
created container and controller until `InnerWebView` takes them. It must
handle both controller-creation and configuration failures, preserve parent
subclass cleanup, avoid double destruction, and report cleanup failures without
adding Syrinx-authored lint suppression or swallowed errors. Review the exact
published baseline and a complete diff before requesting owner authorization.

The main alternative is Syrinx-side cleanup after `build_as_child` fails. That
caller never receives the newly created native handle, so searching the parent
for a child would guess ownership and risks affecting another page. Cleanup
belongs at the point that owns the resource, subject to the narrow dependency
patch approval required by `AGENTS.md`.

Before acceptance, use a disposable native harness with controlled failure
points after container creation, controller creation and configuration. Verify
child counts return to baseline, successful pages remain intact, retry works,
and shutdown completes. Keep these injected test failures outside ordinary
page content and outside owner profiles. A passing harness would establish
cleanup behavior; reproducing and diagnosing the original breakpoint remains
a separate requirement. The existing page-close approval grants none of these
additional source changes.
