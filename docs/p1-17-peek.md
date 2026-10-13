# P1-17: reaching a hidden sidebar over a live page

D44 lets the owner hide the sidebar, so its rows must remain reachable without
giving the page less space. GPUI content and the native WebView2 child cannot
reliably share that rectangle. The peek therefore uses the palette's existing
`WindowKind::PopUp` and `atlas_platform::browser::set_overlay_owner` mechanism:
set `GWLP_HWNDPARENT` to the browser window and remove topmost status. It does
not hide, suspend, snapshot or resize the page.

## Behaviour and implementation

The hidden edge remains an 8 logical-pixel drag strip. Resting there for
**250 ms** opens a full-width sidebar beside the strip. This short delay avoids
opening an overlay while reaching for the resize handle; immediate opening
would compete with a press. A press cancels the pending hover and starts the
existing drag. The strip remains exposed while the popup is open, so pressing
there closes the peek and still starts a drag. Press suppression survives the
whole drag, even outside the edge or while ineligible. Release starts a fresh
250 ms delay when hidden at the edge. Escape or row dismissal still requires
leaving the edge before another peek.

`ui/peek_state.rs` implements and tests the pure timing/press state machine.
`ui/peek.rs` samples native pointer position every 25 ms only while monitoring
the edge or popup. This detects departure over a native page child, where GPUI
does not receive mouse moves. Opening is no earlier than 250 ms; a busy UI
thread can delay it. Pointer departure, Escape, row/Favorite activation,
window deactivation, movement/resizing, minimization and shutdown retire the
popup. Moving or resizing dismisses it rather than leaving it at stale screen
coordinates. Placement starts at GPUI's window bounds and selects the monitor
under the exposed edge, using that monitor's scale. There is
no slide animation.

The peek renders `sidebar_view::sidebar_at`, the same rows, folder expansion,
Favorites and search as the wide persistent sidebar. It observes the browser
entity for changes. Search transfers into the owned palette. Deferred popup
actions carry a generation number so an old popup cannot close or focus a
replacement. Dismissal returns through the main window and the existing
`BrowserCommand::Focus` / `WebView::focus()` path. Losing activation closes
without activating Atlas again, including when a dismissal was already queued.
Hover opening uses the native no-activation path and leaves keyboard focus in
the page. Clicking the peek can activate it. While it is open, an unmodified
Escape from the page is forwarded with that popup's generation; ordinary page
Escape stays with the page when no peek exists.

An unfinished Favorites edit form closes when entering or leaving the peek.
Its input subscriptions belong to the window that created them, so retaining
them across a popup's destruction would break Enter submission. Saved Favorites
and the main window's Quick access editor are unaffected.

Ctrl Alt S hides a visible sidebar or restores its last visible width. The
WebView2 accelerator callback only handles and queues the shortcut, with its
existing auto-repeat guard; it does no cross-process COM work. GPUI and the
palette forward the same action. No command registry exists on this base.
The existing settings table stores both `sidebar.width_state` and
`sidebar.last_visible_state`. The second key remembers the visible width because
`sidebar.width_state` holds Hidden while hidden. Older profiles without the
second key restore wide when first pinning a hidden sidebar. A popup never changes these settings.

The main window now uses a client title bar with GPUI's native Windows caption
hit areas and the approved 36 px height token. Its caption buttons use
Segoe MDL2 Assets glyphs because the app does not register the UI
library's icon bundle. The middle control switches from maximize to restore
when maximized. GPUI's `WindowControlArea::Max` retains native snap hit-testing.
When the sidebar is hidden, back, forward, reload and the address button occupy that
bar beside the window controls. Visible states retain their separate toolbar.
`ui/page_card.rs::chrome_height` reserves the correct height and its tests
check the card starts below the controls at five scales. The address button
opens the palette. The normal native close callback still requests durable
shutdown before removing the owner.

## Review fixes

The independent approve-with-fixes review found that edge placement and hover
rearming needed more coverage. Placement now converts the GPUI window origin
through physical coordinates to the edge monitor's logical coordinates and
clamps to that monitor before creation. This prevents GPUI's off-monitor
recentering, including when the owner straddles monitors at different scales.
The platform manifest explicitly enables the existing Windows GDI and DPI API
features for this lookup; dependency versions and the lockfile remain unchanged.

Hidden-state settlement, main-window activation and palette completion restart
sampling for a stationary pointer. Eligibility uses a pure predicate, and edge
membership calls `atlas_ui::sidebar::in_peek_zone`. A held press remains exactly
`Suppressed` through ineligible drag samples; explicit dismissal is separate.
Page-focused Ctrl Alt S rejects right Alt, preserving AltGr+S. Native pointer
and monitor failures retain their operation, HRESULT and Windows message.

Regression tests cover straddling, both mixed-scale directions, monitor-edge
clamping, each eligibility blocker, stationary-pointer rearming, drag suppression,
AltGr, native error details and caption state. Navigation-width tests assert
concrete expected values at named widths; queued-dismissal coverage exercises a
replacement popup, shutdown, valid delivery and duplicate delivery.

## Validation

Logs live under this worktree's `target/`; they contain synthetic test evidence
and are not committed. No dependency pins, vendor source or lockfile changed.

The review-fix gate passed on 2026-10-12 with exit code 0 in
`target/p1-17-review-gate-2.log`: dependency boundaries, deferred markers,
formatting, Clippy with warnings denied, **611 tests** (133 app, 78 platform
unit tests, and the rest of the workspace), documentation-test selection and
the application smoke run. No tests were ignored or filtered. The first attempt
caught a missing `gpui::Pixels` import; its failure is preserved in
`target/p1-17-review-gate-1.log`. The import was fixed without weakening checks.
Test temporary directories and smoke profiles were redirected into this
worktree's `target/review-temp/`.

A fresh synthetic Home window was inspected using only its own native handle.
The captures `target/p1-17-review-normal-dpi.png` and
`target/p1-17-review-maximized-dpi.png` show the minimize and close glyphs, the
normal maximize square and the maximized overlapping restore glyph. Native
maximize, restore and minimize state checks passed; the window closed through
its normal close request. `target/p1-17-review-window-dpi-inspection.log`
records those checks. The first capture attempt used DPI-virtualized bounds
and clipped the image; only the inspection thread's DPI awareness was corrected.
No Windows setting changed. Snap-layout hover, page-focus transitions and
physical mixed-monitor placement still require the manual checks below.

The original branch gate on 2026-10-12 passed with exit code 0; its log is
`target/p1-17-gate-complete.log`. Dependency boundaries, deferred markers,
formatting, Clippy with warnings denied, all 599 tests and the application
smoke launch passed. This includes 123 app tests, 76 platform unit tests and
42 UI tests. The native synthetic overlay test verifies ownership, removal
of topmost status and destruction with its owner. The existing vendored wry
dead-code warning remains unchanged.

The restart review exposed three compilation details in the review fixes:
`Window::display` requires the app context, that display lookup must precede
the mutable `open_window` borrow, and the activation observer needs an explicit
`Context<PeekView>` type. Those failures are preserved in
`target/p1-17-gate-restart.log`, `target/p1-17-gate-final.log` and
`target/p1-17-gate-verified.log`; no checks were weakened.

Independent source review found and prompted fixes for hover activation,
window-bound Favorites forms and secondary-display placement. A follow-up
review also checked the conditional Escape routing, generation guards and
activation check before a queued action can restore focus.

Before these review fixes, native desktop inspection could not run after the restart: the local Computer
Use helper returned "native pipe is unavailable ... (os error 2)" both before
and after resetting its session. Actual popup appearance, hit testing,
Alt Tab, page focus and multi-monitor acceptance remain the owner's checks
below. The gate's smoke launch and the native test of popup ownership are
narrower evidence, not substitutes for those checks.

Shared existing files changed are `crates/atlas-app/src/ui.rs`, `main.rs`,
`palette.rs`, `ui/sidebar_view.rs`, `ui/favorites.rs` and `ui/page_card.rs`;
`crates/atlas-platform/src/browser.rs` and `browser/page.rs`;
`crates/atlas-ui/src/sidebar.rs`; and the implementation ledger, plan's DW-3
row and traps document. The workspace root manifest, lockfile and platform `lib.rs` are unchanged.
The review adds only Windows API feature declarations to the platform manifest. New files are the peek, timing and title-bar modules,
the platform pointer helper, the synthetic HTML fixture and this note.

## Fix: opening the peek panicked (2026-10-12)

An independent review found that the first real hover crashed Atlas with
"cannot update BrowserView while it is already being updated". The hover
sampler runs inside a `BrowserView` update and called `cx.open_window`
directly. GPUI's `open_window` draws the new window once before returning
(`gpui-pre-0.3.8/src/app.rs`, lines 1431–1455), and `PeekView::render` updates
`BrowserView` to draw the shared sidebar, so the first draw leased the browser
a second time. The regression came from `a79fbc7`, before any native hover had
been tried.

The sampler now only *requests* a peek. `request_peek` computes the placement,
allocates the popup's generation, records it in `BrowserView::peek_opening` and
defers `open_requested_peek` with `cx.defer`. GPUI runs deferred callbacks
after the outermost update finishes, when no entity is leased, so the popup's
synchronous first draw and every later frame draw read the browser freely.
Before opening, the callback checks that its generation is still awaited; after
opening, `install_peek` checks again and closes a popup that hover, Escape,
shutdown or a newer request superseded meanwhile. Retiring the peek also
withdraws a pending request, and a sample that sees the hover end before the
window exists simply withdraws it. While a request is pending the sampler keeps
watching and never issues a second request.

The rejected alternative was a read-only render. `sidebar_view::sidebar_at`
needs a `Context<BrowserView>` because its rows build `cx.listener` handlers
and Favorites/folder input state on the browser, so a render that does not
lease the browser would mean rewriting the shared sidebar's listeners. Deferral
keeps every render outside browser updates with a change confined to the peek.

Generation and stale-action guards, the queued-dismissal Alt Tab check,
Favorites form dismissal, Escape routing and the shared hover zone are
unchanged. `peek_step` and `accepts_opened_peek` are pure and unit-tested:
one request while opening, withdrawal before appearance, and rejection of
withdrawn, replaced, dismissed and shutting-down requests.

**Native evidence** (synthetic profiles under `target/peek-fix/`, not
committed). A script touching only the synthetic Atlas process moved its main
window so the left edge lay under the stationary cursor, then posted mouse
messages only to that window: a press on the wide sidebar's edge strip, a drag
to the left edge and release there (owner check 9's path), then `WM_MOUSEMOVE`
at the edge every 25 ms. Atlas was already the foreground window; nothing set
the foreground, moved the cursor or used SendInput. The pre-fix binary built
from `e292033` opened the popup and panicked
(`target/peek-fix/before-hover.err`, reproduced twice). The fixed binary showed
the full peek (`target/peek-fix/after-shot-0.png`), retired it when its window
moved, and exited with code 0 and an empty stderr after `WM_CLOSE`, in four
consecutive cycles. A real pointer hover, Alt Tab and click-through remain the
owner's checks below.

After merging `main` (`841d29a`) into the branch, `cargo xtask check` passed on
2026-10-13 with exit code 0: dependency boundaries, deferred markers,
packaging, formatting, Clippy with warnings denied, **995 tests** (none failed
or ignored) and the application smoke run. The log is
`target/peek-fix/gate.log`.

A pointer resting on the edge cannot reopen the peek after an activation change
until it leaves the edge. GPUI calls the window's bounds observers on every
activation change (`gpui-pre-0.3.8/src/window.rs`, line 1953), and the peek's
bounds watch retires and dismisses on each call. Clicking into the peek
deactivates the main window, so by source reading it may also close the peek
(owner checks 4 and 8 will show it). This predates the fix and is
recorded for owner check 9 and as a separate follow-up; it is not changed here.

## Owner's manual checks

Use a fresh synthetic profile, not your everyday Atlas directory:

```powershell
cargo run --package atlas-app --profile local --offline --locked -- --test-data-dir "$PWD\target\p1-17-owner-profile"
```

If that directory already exists, choose a new sibling name. Reuse the chosen
directory only for the restart checks below.

In a second terminal, serve the committed synthetic fixture:

```powershell
python -m http.server 8765 --bind 127.0.0.1 --directory crates/atlas-app/tests/fixtures
```

Open `http://127.0.0.1:8765/peek.html` through Ctrl L. This page has a text
input, viewport dimensions and a continuously changing counter/animation,
positioned to remain visible beside the peek. Its second-destination link
provides back/forward history. Stop the server with Ctrl C when finished.
Never sign in for these checks. Repeat the focus checks several times; one
successful cycle does not prove reliability.

1. From wide, click the page and press Ctrl Alt S. The sidebar disappears;
   the navigation/address controls appear in the title bar and the page stays
   below them. Press it again with page focus: wide returns. Drag to compact,
   toggle hidden and back, and verify compact returns. Hide, close and restart
   the same synthetic profile: Ctrl Alt S should still restore compact.
2. Hide again. Move into the leftmost 8 px below the title bar without pressing.
   Crossing quickly should do nothing; resting about a quarter second should
   open the full sidebar. Move into it and inspect Favorites, search and the
   same saved/open rows. The page's content width, scroll position and live
   counter must stay unchanged while it is open. With the page input focused
   before hovering, type while peeking: text should still enter that input.
3. Move right out of the peek over the live page, then repeat leaving above,
   below and outside Atlas. It should close. In a synthetic page input that
   was focused before peeking, type after dismissal: input should go to the
   page without another click. Repeat with Escape, keeping the pointer on
   the edge: the peek must stay closed until you leave and return.
4. Reopen and activate the current row, another open row, an unloaded saved
   row and a website Favorite. Each should close the peek and focus/show the
   destination. Expand a folder: it should stay open for choosing a child.
   Click search and type in the palette, then Escape; focus must return to
   the page. Repeat Ctrl L, Ctrl K and Ctrl Alt S from inside the peek. Start
   a Favorites edit, dismiss, reopen and edit again: the unfinished form should
   be gone and a new form should submit with Enter.
5. Press the hidden strip immediately and drag right. The sidebar and page
   should resize normally, with no popup during the drag. Repeat after a peek
   is already open, using its still-exposed left strip. Release outside the
   window and use Alt Tab mid-drag; neither should leave a stuck drag.
6. With the peek open, Alt Tab to another app. The peek must disappear and
   Atlas must not steal focus back. It must not have its own Alt Tab entry or
   remain topmost above the other app. Return: the page should be usable.
7. Wide and compact now show the custom Atlas title bar too. In wide, compact
   and hidden, check minimize, maximize, restore and close glyphs; maximized must
   show the overlapping restore glyph and the accessible label Restore. Hover
   maximize/restore to check Windows snap layouts. Reopen the peek, minimize,
   restore, maximize/restore, move the window and resize it.
   Each transition should dismiss the peek, with no detached window. Repeat
   across monitors at different display scales if available, including a window
   straddling their boundary with its left edge on the other monitor. Verify title-bar
   dragging, double-click maximize, and back/forward/reload/address clicks do
   not interfere with each other.
8. Close Atlas while peeking using its close button. Separately, Alt F4 while
   the peek has focus (click Favorites Edit first) should dismiss it; a second
   Alt F4 closes Atlas. Without clicking the hover popup, Alt F4 still closes
   the main window because hover did not take focus.
   There must be no orphan popup or lingering synthetic Atlas process. Start
   again and check that a fresh hover still works after every dismissal path.

9. Keep the pointer on the strip while hiding with Ctrl Alt S, returning after
   Alt Tab, and closing the palette. Each must begin a fresh hover delay without
   leaving and re-entering. Drag to hidden and release on the strip: it must wait
   a full delay after release, with no popup while pressed. On an AltGr layout,
   AltGr+S must type into the synthetic page input without toggling the sidebar.

Owner acceptance and independent branch review remain separate from the gate.
