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
there closes the peek and still starts a drag. Leaving the edge is required
before a dismissed peek can open again.

`ui/peek_state.rs` implements and tests the pure timing/press state machine.
`ui/peek.rs` samples native pointer position every 25 ms only while monitoring
the edge or popup. This detects departure over a native page child, where GPUI
does not receive mouse moves. Opening is no earlier than 250 ms; a busy UI
thread can delay it. Pointer departure, Escape, row/Favorite activation,
window deactivation, movement/resizing, minimization and shutdown retire the
popup. Moving or resizing dismisses it rather than leaving it at stale screen
coordinates. Placement uses the owner's current display and scale. There is
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
`sidebar.last_visible_state`; older profiles without the second key restore
wide when first pinning a hidden sidebar. A popup never changes these settings.

The main window now uses a client title bar with GPUI's native Windows caption
hit areas and the approved 36 px height token. Its caption buttons use text
glyphs because the app does not register the UI library's icon bundle. When the
sidebar is hidden, back, forward, reload and the address button occupy that
bar beside the window controls. Visible states retain their separate toolbar.
`ui/page_card.rs::chrome_height` reserves the correct height and its tests
check the card starts below the controls at five scales. The address button
opens the palette. The normal native close callback still requests durable
shutdown before removing the owner.

## Validation

Logs live under this worktree's `target/`; they contain synthetic test evidence
and are not committed. No dependency pins, vendor source or lockfile changed.

On 2026-10-12, `cargo xtask check` passed with exit code 0; the final log is
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

Native desktop inspection could not run after the restart: the local Computer
Use helper returned "native pipe is unavailable ... (os error 2)" both before
and after resetting its session. Therefore actual popup appearance, hit testing,
Alt Tab, page focus and multi-monitor acceptance remain the owner's checks
below. The gate's smoke launch and the native test of popup ownership are
narrower evidence, not substitutes for those checks.

Shared existing files changed are `crates/atlas-app/src/ui.rs`, `main.rs`,
`palette.rs`, `ui/sidebar_view.rs`, `ui/favorites.rs` and `ui/page_card.rs`;
`crates/atlas-platform/src/browser.rs` and `browser/page.rs`;
`crates/atlas-ui/src/sidebar.rs`; and the implementation ledger, plan's DW-3
row and traps document. The workspace manifests, lockfile and platform
`lib.rs` are unchanged. New files are the peek, timing and title-bar modules,
the platform pointer helper, the synthetic HTML fixture and this note.

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
7. Reopen, minimize, restore, maximize/restore, move the window and resize it.
   Each transition should dismiss the peek, with no detached window. Repeat
   across monitors at different display scales if available. Verify title-bar
   dragging, double-click maximize, and back/forward/reload/address clicks do
   not interfere with each other.
8. Close Atlas while peeking using its close button. Separately, Alt F4 while
   the peek has focus (click Favorites Edit first) should dismiss it; a second
   Alt F4 closes Atlas. Without clicking the hover popup, Alt F4 still closes
   the main window because hover did not take focus.
   There must be no orphan popup or lingering synthetic Atlas process. Start
   again and check that a fresh hover still works after every dismissal path.

Owner acceptance and independent branch review remain separate from the gate.
