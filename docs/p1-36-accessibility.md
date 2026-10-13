# P1-36: keyboard navigation and accessibility

Syrinx must be usable without a mouse and readable by a screen reader before
the owner makes it the default browser (Phase 5, plan issue map P1-36). A
compact or hidden sidebar is the worst case: icons without names and edges
that only a drag can move. This slice adds F6 pane cycling, Tab between every
chrome control, a visible focus ring from the `syrinx-ui` tokens, accessible
names and roles for Syrinx's own controls, and the NVDA checks the owner runs.

Branch `claude/p1-36-accessibility`, from `codex/integrated-browser-preview`
at `bfe5bf5`.

## What GPUI exposes to Windows UI Automation

**GPUI 0.3.8 has a UI Automation provider.** The task allowed for the case
where it had none; the evidence says otherwise, so no provider gap is
recorded. What it does, with file and line evidence (the platform crate is the
vendored one; the core crate is the pinned registry package
`gpui-pre-0.3.8`):

- The Windows backend depends on AccessKit and its Windows adapter:
  `vendor/gpui-pre-windows-0.3.8/Cargo.toml`, lines 55 and 59
  (`accesskit`, `accesskit_windows`; `Cargo.lock` pins `accesskit 0.24.1` and
  `accesskit_windows 0.34.0`).
- Every GPUI window creates an `accesskit_windows::Adapter` for its HWND:
  `vendor/gpui-pre-windows-0.3.8/src/window.rs`, `a11y_init`, lines
  1090–1108; tree updates are raised as UIA events in `a11y_tree_update`,
  lines 1110–1124.
- The window procedure answers UIA's `WM_GETOBJECT` through the adapter
  (which returns the root provider with `UiaReturnRawElementProvider`):
  `vendor/gpui-pre-windows-0.3.8/src/events.rs`, line 166 and
  `handle_wm_getobject`, lines 852–867.
- The core crate turns elements into AccessKit nodes each frame
  (`gpui-pre-0.3.8/src/window/a11y.rs`, module documentation, lines 1–99) and
  activates only when a UIA client asks (`gpui-pre-0.3.8/src/window.rs`,
  lines 1615–1656). It is on by default: only
  `Application::new_inaccessible` turns it off (`gpui-pre-0.3.8/src/app.rs`,
  line 195), and Syrinx starts through `gpui_platform::application()`.
- An element appears in the tree only with an id **and** a role
  (`gpui-pre-0.3.8/src/_accessibility.rs`, lines 58–103). The public API is
  `role`, `aria_label`, `aria_description`, `aria_keyshortcuts`,
  `aria_selected`, `aria_expanded`, `aria_toggled`, `aria_value`, `aria_level`
  and more (`gpui-pre-0.3.8/src/elements/div.rs`, lines 1306–1513).
- An element with a click handler gets AccessKit's Click action and a
  focusable one gets Focus (`div.rs`, lines 3594–3599); a focused element
  with a click handler is clicked by Enter or Space (`div.rs`, lines
  3016–3080). Tab order comes from `tab_index` and paint order
  (`gpui-pre-0.3.8/src/tab_stop.rs`), but nothing binds Tab: an app calls
  `Window::focus_next` itself (`window.rs`, lines 2379–2399).
- gpui-component's text field reports `Role::TextInput` and takes
  `aria_label` (`gpui-component-0.7.1/src/input/input.rs`, lines 271 and
  729–757).

Known limits of that provider, each checked in source:

- **Active descendant needs a focused ancestor.**
  `set_active_descendant` (`gpui-pre-0.3.8/src/window/a11y.rs`, line 258)
  ignores the claim unless the focused node is an ancestor of the row. The
  palette keeps focus in its query field, which is the rows' sibling, so NVDA
  cannot hear the arrow keys move the selection. Recorded as **DW-60**.
- **Plain text is not in the tree.** Text without an id is not reported, so
  Syrinx names the parts a screen reader must read (the permission question,
  the runtime notice, the status line) with labelled nodes.
- **Debug assertions panic on a malformed tree.** The `local` profile keeps
  `debug-assertions = true` (`Cargo.toml`, line 55), and the a11y builder
  panics in debug builds on a second focus claim in one frame, a duplicate
  node id, or a stray active descendant (`a11y.rs`, lines 232–243, 411–423,
  513–542 and 564–573). It runs only while a UIA client is attached, which
  includes screen readers and some password managers, so a gate run does not
  build it. The next section shows why Syrinx cannot make two focus claims,
  and records a probe that built the tree.

## Why two nodes cannot report focus

GPUI reports focus from exactly one place: during prepaint, an element that
tracks a focus handle calls `A11y::set_focus` only if **that handle is the
window's focused handle** (`gpui-pre-0.3.8/src/elements/div.rs`, lines
2326–2335: `if focus_handle.is_focused(window)`). `is_focused` compares one
`FocusId` with the window's single `focus` value, not containment. So two
claims in a frame need one handle tracked by two elements in that frame.

- **Nested regions are not a risk.** A pane region (`panes.rs`, `as_pane`)
  and the address field or a sidebar row inside it track *different* handles.
  When the field is focused, only the field's element claims; the region's
  handle is not the focused one, so its element only registers itself as
  focusable (`set_focusable`), which never panics.
- **Each handle is tracked once per frame.** Every `track_focus` in
  `syrinx-app` was checked: the seven pane handles are private to
  `PaneFocus` and reach elements only through `as_pane`, which `render` calls
  once per pane (the toolbar is drawn either in the title bar or under it,
  never both); the window root's `self.focus` is tracked by the fullscreen
  root or the chrome root, which are alternative early returns; the import
  preview tracks its `focus` on the Import button or on the frame, never both
  (`import.rs`); peek, recovery and the palette are separate windows with
  their own trees.
- **Active descendant is not used.** DW-60 removed it, so the two panics
  around it cannot trigger.
- **A GPUI-level unit test is not possible without a dependency change.**
  `A11yNodeBuilder` is `pub(crate)` and GPUI's test harness needs its
  `test-support` feature, which would change the lockfile; the builder's own
  tests (`a11y.rs`, lines 812–823) already pin that a second claim panics.

**The probe.** To build the tree without a screen reader, an agent attached a
UI Automation client (.NET `System.Windows.Automation` from PowerShell) to the
`local` build on fresh synthetic profiles `target\p1-36-uia-1` and
`target\p1-36-uia-2` (logs `target\p1-36-uia-probe-*.log`). Attaching
activates GPUI's AccessKit adapter, so every frame then built the tree with
debug assertions on. Run 2 posted F6 seven times and Tab twelve times to
Syrinx's own window only, then called UIA `SetFocus` on twelve elements,
including the "Address" toolbar region and its buttons. Syrinx stayed up with
an empty stderr, the tree had 35 nodes on Home, and focus followed as
designed: F6 went address field → sidebar "Search · Ctrl K" → Home
"Customize" → address field; Tab went through the toolbar and into the
sidebar. Run 1 also survived, but sent its keys with `SendKeys` to whatever
window was in the foreground, so its key results are not evidence; run 2
replaced that with messages posted to Syrinx's window. Rerun the probe after
changing roles, ids or focus handles: the script is short and is described
here instead of committed, because it drives a window and is not a test.
- **The web page is a separate HWND.** The AccessKit tree covers GPUI's own
  elements. WebView2 joins UI Automation through its own child window
  (`docs/research/02-browser-v1-scope.md`, line 156). Whether NVDA's object
  navigation moves between the two trees, rather than only following focus
  events, is unverified and is an owner check below.

No vendored GPUI file was changed.

## What works now

All of the following passes the gate's unit tests where it is pure logic; the
window behaviour is for the owner to confirm (no agent inspected a window).

**F6 and Shift F6 move between panes** in the order address toolbar, runtime
notice, permission request, find bar, sidebar, page, wrapping at each end.
Closed bars and a hidden sidebar are skipped; the address field and the page
card are always there. The order and skipping are pure functions in
`crates/syrinx-ui/src/focus.rs` (`next_pane`, `OpenPanes`) with eight tests,
including a bar that closes while focused handing focus to its neighbour.

- From chrome, GPUI's `f6`/`shift-f6` bindings run
  `BrowserView::cycle_pane` (`crates/syrinx-app/src/ui/panes.rs`).
- **From a focused page**, WebView2's `AcceleratorKeyPressed` handler maps
  F6 to `BrowserShortcut::FocusNextPane` and Shift F6 to
  `FocusPreviousPane` (`crates/syrinx-platform/src/browser/page.rs`,
  `shortcut_for_modifiers`). The handler is unchanged in shape: it reads the
  key, sets `Handled` and posts; there is no COM call beyond the event
  arguments, and Ctrl F, Ctrl H, Ctrl P and the zoom keys still reach the page
  first (a new test pins that, per the 2026-10-12 decision).
- **Tab past a page's last element** moves to the next pane, and **Shift Tab
  before its first** moves to the previous pane's *last* control, as Tab
  order would: WebView2's `MoveFocusRequested` is marked handled and posted
  the same way (`BrowserShortcut::LeavePageBackward` for Shift Tab); a
  scripted focus move stays in the page (`register_focus_departure`,
  `departure_shortcut`, tested).
- Landing (`syrinx_ui::focus::landing`, tested): the address pane focuses the
  address field itself; a web page is focused natively (`focus_page`); any
  other pane focuses its region and steps to its first (or last) control. If
  the step leaves the region, because the pane has no Tab stops (a Syrinx
  page card with nothing to press), focus stays on the region instead of
  escaping to another pane (`syrinx_ui::focus::settle`, tested).
- **F6 is always Syrinx's.** The page never sees F6; see owner question 5.

**Tab and Shift Tab move between every Syrinx control** in each Syrinx window
(the main window, the palette and the import preview). The main window's
order follows the screen: toolbar, bars, sidebar (search, Favorites, tab
rows, row actions, new tab), the resize edge, then the page card's own
controls (Home, History, Clear browsing data). Enter or Space activates the
focused control. The resize edge was focusable before but could not be
reached with Tab, because a tracked focus handle takes its Tab stop from the
handle rather than from `tab_index`; it now can (new trap below).

**A focus ring from the tokens.** `syrinx_ui::focus` defines a 2 px ring in the
accent ink, which `contrast::audit` already holds to WCAG 1.4.11's 3:1 on the
ground, panel and card, with a 1 px inner line in the accent's on-fill colour
so the ring still reads on accent-filled controls (a new test checks 3:1
against every accent fill). `crates/syrinx-app/src/controls.rs` draws it as an
inset shadow, so it never moves layout and a scrolling sidebar cannot clip it.

**Focus returns predictably.** Closing the palette, peek or import preview
still calls `focus_page`, which now also clears GPUI's focus to the window
root, and a page that takes focus by click reports WebView2's `GotFocus`
(`BrowserEvent::PageFocused`), which does the same. Either way no chrome
control keeps a ring while keys go to the page. Enter on a focused Cancel in
the import preview now cancels instead of importing.

**Names and roles.** Every Syrinx control has a role and a name:

| Area | Role and name |
| --- | --- |
| Toolbar | Toolbar "Address"; Back, Forward, Reload with their keys; address field (button "Search or enter address", value = current address, Ctrl L); Find in page, Downloads, Print, Zoom out, Reset zoom (value = percent), Zoom in, Save page, History |
| Sidebar | Navigation "Sidebar"; Search; Favorites as buttons named by tile ("Edit …" while editing), Add favorite; plugins as labels "… · coming soon"; tree "Saved and open tabs" with tree items (level, selected, expanded for folders); Close tab, Unload page, Remove saved page; New tab; splitter "Resize sidebar" (state as description, vertical, ArrowLeft/ArrowRight) |
| Bars | Status "Runtime notice" with its text; Alert "Permission request" with the question, "Remember for this site" checkbox (checked state), Deny, Allow; Search "Find in page" with a named field, Previous, Next, Close find |
| Page card | Group "Page"; Home's Customize (expanded), Search (Ctrl K), Quick access tiles, Add shortcut, Edit/Done, move and remove buttons named for their tile, plugin cards; Customize dialog with palette and accent radio buttons, Custom accent, Reduce transparency switch, named colour field; tile form fields and target radio buttons; History rows as links; Clear browsing data checkboxes and time-range radio buttons |
| Palette | Dialog "Command palette"; field "Search or command"; scope chips as radio buttons with their counts; results list box with named, selected options described by kind; "Search the web" (Alt Enter) |
| Window | Status line at the bottom; caption buttons Minimize, Maximize/Restore, Close Syrinx (unchanged) |

**Not changed:** page web views' identity. No user agent, IPC handler,
script, request filter or header change; the two new WebView2 handlers are
controller events like the existing accelerator handler.

## Owner checks with NVDA

An agent cannot judge speech output, so these are the owner's. Use a fresh
profile, never the daily one:

```
cargo run --package syrinx-app --profile local --offline --locked -- --test-data-dir target\p1-36-nvda
```

Install or start NVDA yourself (installing software is the owner's call; the
portable copy also works). Then record pass/fail for each:

1. **Window and toolbar.** Start Syrinx with NVDA running. NVDA says the
   window title. Press F6: NVDA announces the address field as a button with
   the current address. Tab through Back, Forward, Reload, the security
   indicator area, Find in page, Downloads, Print, Zoom out, Reset zoom, Zoom
   in, Save page and History; each is announced by name, with its shortcut
   where one is listed above. A visible ring follows each one.
2. **F6 order.** With no bars open, F6 goes address → sidebar → page →
   address. On a web page, open Syrinx's find bar with the toolbar's Find
   button (Ctrl F goes to the page first, by decision) and the cycle includes
   the find bar after the toolbar. Shift F6 runs in reverse.
3. **F6 from a page.** Open `https://example.com/`, click into the page,
   press F6: focus leaves the page for the address field and NVDA says so.
   Shift F6 from the page goes to the sidebar's first control. Tab repeatedly
   inside the page until past its last link: focus comes back to the address
   field. Shift Tab from the page's first link lands on the sidebar's last
   control (New tab).
4. **Page keys still the page's.** In a page, Ctrl F opens the page's find
   (or the engine's) as before, Ctrl P prints, and Ctrl plus or minus zooms;
   none of them is taken by Syrinx.
5. **Sidebar in each width.** In wide, compact (Ctrl Alt S toggles hidden;
   drag or Left/Right on the edge for compact) and hidden: every row and
   Favorite is read by its title, not a letter; folders say expanded or
   collapsed; the active tab says selected. In hidden, F6 skips the sidebar,
   and the address controls in the title bar are still reachable.
6. **Edge.** Tab to "Resize sidebar"; NVDA reads it as a splitter with its
   state; Left and Right change the width.
7. **Palette.** Ctrl K: NVDA reads "Command palette" and the field. Type a
   query; Tab leaves the query field for the scope chips (each reads its
   count and whether it is selected) and "Search the web", and Shift Tab
   returns to the field. This is deliberate: the palette has no inline forms
   yet, so Tab has nothing else to do there; when plan 2 adds forms, Tab will
   be scoped to the form's fields. Known gap DW-60: arrowing through results
   is probably not announced. Record what NVDA actually says.
8. **Permission bar.** On a synthetic page that asks for the camera (any test
   page you trust), NVDA reads the question when you F6 to the bar;
   "Remember for this site" reads as a checkbox with its state.
9. **Home, Customize and History.** On Home, Tab reaches Customize, Search,
   every Quick access tile and Add shortcut, and the plugin cards; Customize's
   palettes and accents read as radio buttons with checked state. Ctrl H then
   Tab through history rows (read by title, then address and age) and Clear
   browsing data (checkboxes and time-range radio buttons).
10. **Page content.** With focus inside a web page, NVDA's browse mode reads
    the page. Then, from the page, use NVDA's object navigation
    (NVDA+Shift+Up) to see whether you can reach Syrinx's sidebar without F6;
    record whether the two trees are connected.
11. **No crash with NVDA attached.** Use Syrinx for a few minutes with NVDA
    running, opening the palette, peek and Customize. The `local` build keeps
    debug assertions, so a malformed accessibility tree would stop Syrinx;
    report any such stop with its message.
12. **Ring visibility.** In each of the five palettes, the focus ring is
    visible on the toolbar, a sidebar row, an accent-filled button (Save in a
    tile form) and the selected time-range chip.

## Owner questions

1. **Focus ring after mouse clicks (DW-62).** The ring shows on any focus,
   so a clicked chrome button keeps a ring until focus moves. Hiding it after
   clicks needs a GPUI change, because GPUI cannot see the F6 that leaves a
   page. Recommendation: accept the current behaviour for v1; it errs toward
   keyboard users.
2. **Sidebar keyboard model (DW-61).** Every row is a Tab stop today.
   Recommendation: move to one Tab stop with arrow keys before the week of
   use (P1-40), because a long imported folder makes Tab slow.
3. **Palette announcements (DW-60).** Recommendation: change the palette so
   its list container owns focus and the query field (no GPUI patch), as a
   follow-up slice; the alternative is asking for a vendored GPUI change.
4. **Installing NVDA.** The checks above need NVDA on your machine; agents
   do not install software. Narrator, which ships with Windows, is an
   acceptable first pass if you prefer not to install NVDA yet.
5. **F6 belongs to Syrinx, before the page.** WebView2's accelerator handler
   takes F6 and Shift F6 before a page sees them, so a web app that uses F6
   itself (rare; some spreadsheet and IDE pages do) never gets it. Edge and
   Chrome make the same choice. Recommendation: keep it, because few pages use
   F6 and the alternative leaves keyboard users stuck in a page.

## Files

- `crates/syrinx-ui/src/focus.rs` (new): pane order, open panes, ring tokens and
  their tests; `crates/syrinx-ui/src/lib.rs`.
- `crates/syrinx-platform/src/browser.rs`, `crates/syrinx-platform/src/browser/page.rs`:
  F6 and Tab-out from a page, page focus arrival, tests.
- `crates/syrinx-app/src/controls.rs` (new), `crates/syrinx-app/src/ui/panes.rs`
  (new) and the chrome files that now use them (`ui.rs`, `ui/*.rs`,
  `palette.rs`, `palette/render.rs`).
- `docs/plans/Syrinx v1 — Plan.md` (DW-60 to DW-62),
  `docs/implementation-status.md`, `docs/trapstoknow.md`.
