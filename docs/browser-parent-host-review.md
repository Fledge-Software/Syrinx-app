# Give wry a browser-thread-owned parent

This is a source-only proposal dated 2026-10-11. The integration chat asked
for a minimal correction to the typed parent-handle contract while it owns
all builds, native runs and desktop validation. No implementation in this
proposal has been applied or tested. The already approved failed-construction
patch remains frozen; see `docs/wry-failed-construction-patch-review.md`.

Recommend a disposable prototype with **one private intermediary child
window per page**, owned by the browser thread. This can correct a concrete
handle-contract mismatch while preserving GPUI, the browser worker and the
public platform API. It is not an established fix for the owner's breakpoint
or typing/focus problems. Applying the prototype needs a separate go-ahead
because the current coordination request explicitly limits this step to a
proposal.

## The source concern and the limit of this correction

`crates/atlas-platform/src/browser/page.rs::Parent::window_handle` wraps the
GPUI thread's HWND on the browser thread. Its safety comment establishes
lifetime, but pinned `raw-window-handle 0.6.2` documents that
[`Win32WindowHandle`](https://docs.rs/raw-window-handle/0.6.2/raw_window_handle/struct.Win32WindowHandle.html)
assumes a window owned by the calling thread. A new window created on the
browser thread can satisfy that assumption. The GPUI HWND would remain the
raw Win32 parent of that window, without being presented to wry as a typed
current-thread handle.

There is a second, narrower consequence. In
`vendor/wry-0.57.0/src/webview2/mod.rs`, `InnerWebView::drop` unconditionally
calls `dettach_parent_subclass`, including for child views whose construction
never attached that subclass. The helper sends a synchronous message to the
parent and attempts subclass removal. An intermediary makes that target a
window owned by the same browser thread. It does not remove the unnecessary
operation or change vendor code. Microsoft's
[subclass helper documentation](https://learn.microsoft.com/en-us/windows/win32/api/commctrl/nf-commctrl-removewindowsubclass)
restricts cross-thread subclassing.

The GPUI-to-intermediary edge still crosses threads. Microsoft documents
that parent-child relationships
[attach input queues transitively](https://devblogs.microsoft.com/oldnewthing/20130412-00/?p=4683).
The proposal therefore provides neither input isolation nor a general
deadlock guarantee. No preserved result establishes either source concern
as the cause of the original `0x80000003` dialog. Earlier navigation failures
and later passing fixture runs must remain separate evidence.

## Smallest implementation shape

The proposed topology is:

```text
GPUI main window                         UI thread
  page A host -> wry child/controller    browser thread
  page B host -> wry child/controller    browser thread
GPUI palette, owned by main window       UI thread
```

Keep ownership inside `Page`. Add a private native-host module under
`crates/atlas-platform/src/browser/`, replace the borrowed `Parent` adapter,
and declare `Page` fields in the order `view`, then `host`. A host must be
non-Send and non-Sync, created and destroyed on the browser thread, and must
check its owning thread and live identity before lending a typed handle.
Use `CreateWindowExW` with the raw GPUI HWND as parent. Start hidden and
without taking focus. Its window procedure must not forward focus blindly
to a first child or perform browser lifecycle actions.

Construct and own the host before calling `build_as_child(&host)`. On a
successful build, retain it until the view has been dropped. On a subsequent
settings or event-registration failure, local variable ownership must drop
the view before the host. An error from wry's constructor must also release
the host, subject to the existing frozen patch's documented late-callback
limitations. This proposal does not expand those guarantees.

The existing `BrowserState` pages-before-environment order stays intact.
The apartment guard is released afterward, and `Stopped` is emitted only
after worker state is released. Keep the GPUI parent alive and pumping
through `Stopped` and the completed worker join. Windows requires
[window destruction on the creating thread](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-destroywindow),
and a window-owning thread must continue
[servicing its messages](https://learn.microsoft.com/en-us/windows/win32/procthread/creating-windows-in-threads).

The native owner needs a stable identity cleared by its own `WM_NCDESTROY`
handler; `IsWindow` alone cannot distinguish a recycled handle. Ownership
must become effective immediately after successful creation, including
rollback from later initialization failures. Unexpected ancestor destruction
must be recorded as a violated lifetime assumption. Tracking host liveness
cannot make wry's existing successful-view destructor safe after its native
child was destroyed externally. Do not promise that recovery.

Cleanup reporting must be settled in the prototype before promotion. Report
native cleanup failure structurally while preserving the original creation
error, and do not retry against an invalidated identity or silently discard
`DestroyWindow` failure. A private structural diagnostic may suffice. If the
chosen product reporting policy needs a new public error variant or event,
review that API change separately. Do not disguise cleanup failure as a
navigation or creation failure merely to retain the API.

## Geometry, visibility and focus

`ui.rs::resize_pages` supplies physical page-card coordinates. Place each
host at the existing `(x, y, width, height)` and its wry child at local
`(0, 0, width, height)`. Preserve `PageBounds` validation and physical units;
applying the original offsets twice would displace the page. Resize both
windows with checked results. A partially completed resize is a real error,
not an atomic operation, and must have a tested recovery or explicit failure
policy before product use.

Couple host visibility to `Page::show`: hide the outer host before hiding
the controller; show the controller successfully before exposing its host.
Handle and report partial failures. The existing `ShowActive(None)` loop
then hides every host, and closing a page destroys its own host. Newly
constructed hosts stay hidden until selected. A visible full-client host
would cover Home and browser controls, so keep it confined to the page card.
Check the Win32 return semantics individually; for example, `ShowWindow`'s
return value describes previous visibility rather than ordinary success.

Keep the palette's owner as the GPUI main HWND. Preserve Atlas's direct
controller focus requests and queue-only accelerator handlers. No manual
input-queue attachment, foreground workaround, cross-thread subclass or
callback-driven destruction belongs in this prototype. Atlas currently does
not call wry's `focus_parent`; its target would change to the intermediary.

Source review found GPUI's DirectComposition target configured above child
windows and Atlas's undrawn active-page region compatible in principle with the proposed
topology. That is not proof of painting or hit testing. The existing Phase 0
overlay evidence covers the old topology. Nested native children, ordinary
page-native popup placement after moving the main window, and actual typing
must be tested with GPUI on an unlocked desktop.

## Alternatives and approval boundaries

A single shared host saves native windows, but adds host lifetime, active
page, hide-on-close and shared-resize coordination to `BrowserState`. Per-page
ownership follows the current map and failure paths with a smaller change;
one additional HWND per page is its explicit cost. Moving controllers onto
GPUI's thread would be a broader architectural change. A further vendor
patch just to skip child-mode parent detachment would leave the typed-handle
contract mismatch in place.

The prototype should preserve `BrowserHost`, `BrowserCommand`, `BrowserEvent`
and `PageBounds`, all dependency pins and features, security configuration,
and the approved GPUI/browser-thread architecture. Audit the needed Windows
bindings before implementation; do not silently add manifest features.
Private implementation changes are not inherently a new owner decision, but
this source-only request authorizes no implementation. Any vendor changes,
public API additions, COM accounting changes or ownership move to GPUI need
their own reviewed scope. No commit, merge, publication or upstream message
is authorized.

## Validation handoff to integration

Freeze baseline and prototype identities and change only the intermediary
in the comparison. Keep prior failures and all runs, including failures.
Integration owns execution; this chat and its subagents review source and
results. Use fresh synthetic profiles and the existing local test fixtures.

1. Verify exact parent-host-wry-child identities and creating thread IDs.
   Assert the host is current-thread-owned when its typed handle is lent.
   Check page-card screen rectangles after resize, maximize/restore and
   available DPI changes, without changing Windows settings.
2. Exercise host creation and initialization failure, wry failed construction,
   settings failure, retry, and repeated page closure. Check exact surviving
   sibling identities, complete host/child removal and view-before-host
   destruction. Do not accept a matching child count alone.
3. Repeat the existing local navigation and page-close fixture against both
   frozen builds. Record structural callback/navigation outcomes and preserve
   failures. Passing this fixture alone does not demonstrate GPUI usability.
4. On the unlocked desktop, test real GPUI painting and clicks on the sidebar,
   toolbar and Home after hiding or closing pages. On a synthetic editable
   page, type, open the palette through a page accelerator, type in the
   palette, dismiss it and type in the page again. Repeat between two pages
   to catch focus reaching a hidden sibling. Check overlays and native popup
   placement after moving/reactivating the main window.
5. Adapt the pending-controller shutdown test to the extra ancestor and
   verify teardown before parent destruction. Retain the completed-creation
   baseline and an independent deadline. The existing pending-environment
   case remains uncovered because its callback completed before the pending
   boundary; do not manufacture a pending state or weaken that assertion.
6. Only after focused native and UI checks support promotion, run the normal
   `cargo xtask check` gate through integration. Report observed usability
   and remaining uncertainty separately from the unresolved original crash.

Three subagents reviewed ownership, lifecycle and UI implications. Their
source review supports this bounded experiment, not production readiness.
The pending-shutdown results and original crash evidence remain in
`docs/webview2-breakpoint-investigation.md`.
