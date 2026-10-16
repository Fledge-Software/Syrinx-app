# P1-13: reachable browser essentials

Everyday browser controls must remain reachable without taking keys away from
web applications. The owner's 2026-10-12 decision gives focused pages Ctrl F,
Ctrl H, Ctrl P and Ctrl +/−/0 first, including numeric-keypad variants. Syrinx
leaves them unhandled and keeps WebView2 browser accelerators enabled for the
engine fallback. Ctrl J and the existing Syrinx tab/navigation shortcuts retain
their queue-only handling. No page script, identity override, dependency or
vendor change is involved.

## Implementation

`crates/syrinx-platform/src/browser/essentials.rs` calls the pinned WebView2
interfaces through the controller. Find uses native matching and highlighting;
downloads use the engine's minimal progress/cancel list; print opens preview,
without submitting a job. Default context menus are explicitly enabled. Zoom
steps are bounded and the toolbar displays the controller's actual percentage.
Syrinx supplies no wry download callbacks, so wry installs no download-start
override and the engine already owns its list. The redundant `SetHandled(false)`
handler was removed; Syrinx observes download-dialog state for focus handling.

`crates/syrinx-app/src/ui/find.rs` owns the find bar. Opening it transfers native
keyboard focus to Syrinx and focuses its input. Enter/Next and Previous move
between matches; Escape/Close stops finding and returns focus to the page.
Switching, navigating or closing the tab dismisses it. Monotonic request IDs
reject obsolete results, including closing and reopening on the same tab.
Native query text stays in memory and is never logged or persisted.

The main alternative was the engine's own find bar. In three diagnostic runs
(`target/p1-13-native-1.log` through `-3.log`), empty `Find.Start` did not complete
on this runtime; nonempty text returned the expected two matches. The Syrinx bar
therefore opens without a search call. Clearing invokes `Stop` and acknowledges
an empty result; this is explicit application behavior, not a claim that native
empty `Start` succeeded. Existing lifecycle assertions remain unchanged.
Microsoft documents the intended native [find contract](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2find?view=webview2-1.0.3537.50)
and [download dialog controls](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_9?view=webview2-1.0.4022.49).

Find, Downloads, Print, History and zoom controls are available in the toolbar.
They also appear by name in the existing local search palette, using its command
targets. `crates/syrinx-app/src/commands.rs` supplies the entries and action routes;
the session seeds them on startup, and palette activation invokes the same
actions as chrome. This base has no general command registry or history list.
History reports that it awaits P1-20 integration; integration must connect both
entry points to that list. No later history feature was built in this slice.

## Evidence and limits

`target/p1-13-gate-final.log` is green: dependency boundaries, formatting,
Clippy, all 448 tests and the application smoke run passed. No error 4551
was observed. The unchanged upstream wry dead-code warning remains.

`target/p1-13-native-final.log` passes all four suites, including the native
chrome-focus assertion. `target/p1-13-native-4.log` is the earlier successful
run before adding that assertion. Independent review caught the reopen race
and missing explicit native focus handoff; both were fixed before the final run.
`target/p1-13-gate-1.log` retains an earlier linker failure: the gate tried to
replace the example executable while our native fixture was running. Subsequent
runs are serialized. No test or security setting was weakened.

Native checks use fresh synthetic profiles under this worktree's `target/`.
They exercise commands after page focus, native focus return, two matches and
clearing, download-dialog state, zoom readback and print-preview acceptance.
Unit tests verify that the page-owned keys are not intercepted, Syrinx's retained
key mappings, palette command search after restart, selection and action routes,
zoom boundaries and stale find results. These do not claim physical-key, actual
file-download or completed-print acceptance.

The original intermittent lifecycle navigation timeout remains known and
undiagnosed, as recorded in `docs/fixture-transport-timing-review.md`. A passing
run does not repair it; the owner expressly permits continuation.

## Owner visual checks

The 2026-10-12 review continuation passed `cargo xtask check` in
`target/restart-p1-13-gate-final.log`. The initial run in
`target/restart-p1-13-gate.log` caught two status-label test expectations that
needed the newly displayed command count; the exact assertions were updated.
`target/restart-p1-13-native.log` passes all native suites, including downloads,
find, zoom, print preview and the original lifecycle checks, without weakening
their assertions or deadlines.

Synthetic-profile desktop inspection confirmed the toolbar, searchable palette
commands and History's integration message. The separate palette capture is
`target/restart-p1-13-palette-history-command.png`. Opening Find from the palette
and typing immediately after activation produced two native highlighted matches
in `target/restart-p1-13-find-ready-find-result.png`. The earlier scripted run
typed part of the command into the page before the palette was foreground;
the final harness explicitly waited for the palette before typing its command.
No daily profile or real account was used. Additional owner checks remain:

1. Under `target/`, create an HTML file containing two occurrences of
   `syntheticneedle`, an input and an ordinary link. Serve that directory on
   loopback with `python -m http.server 8765 --bind 127.0.0.1 --directory <directory>`.
   Run `cargo syrinx -- --test-data-dir <new-absolute-directory-under-target>` and
   open that local page with Ctrl L.
2. Use the toolbar's Find control or select Find in page from Ctrl K, then type `syntheticneedle`
   without another click. The Syrinx bar should show two matches and native
   highlights. Verify Enter/Next, Previous, clear, Escape and typing back in the
   page. Close/reopen Find rapidly, switch tabs and navigate while searching;
   no stale count or old bar should remain. Check a narrow window and DPI scaling.
3. With the page focused, press Ctrl J. Verify the engine's download list opens.
   For an actual synthetic download, use a local text attachment and choose a
   destination under this worktree's `target/`; verify progress/completion and
   cancel. Do not use a personal download or profile for this acceptance check.
4. Use Print from the toolbar and palette and verify preview appears; cancel without printing. Right-click
   page text, its link and its input to verify the default context menus.
5. Use the toolbar and palette zoom actions. Verify page scaling and toolbar
   percentage. History in either surface should reach the explanatory message
   on this branch; both must open the history list after P1-20 integration.
6. With the page focused, verify Ctrl F/H/P and Ctrl +/−/0 reach the page first;
   any key the page does not handle uses WebView2's own behavior. Confirm Ctrl
   T/W/L/K/Tab/Shift T still reach Syrinx.

## Integration handoff

The shared `crates/syrinx-app/src/ui.rs` wires the modules, actions, events and
page bounds. `Cargo.toml`, `Cargo.lock` and platform `lib.rs` are unchanged.
Update only P1-13's ledger row on this branch. There are no new deferred-work
markers. P1-12's schema-2 permission migration still needs renumbering when
combined with parallel schema work.

## Integration, 2026-10-12

On `claude/integrate-p1-11-16` the branch's own palette command list
(`crates/syrinx-app/src/commands.rs`) was removed and its commands folded into
the one command registry in `crates/syrinx-core/src/commands.rs`: Find in page
(Ctrl F), Downloads (Ctrl J), Print (Ctrl P), Zoom in, Zoom out and Reset zoom.
The branch's separate "History" command became the registry's existing "Open
history" (Ctrl H), which opens P1-20's history list, and the toolbar's History
control now opens that list instead of the placeholder message. Find, Downloads
and Print take their chrome key bindings from the registry; the zoom keys stay
bound beside it because the registry's shortcut syntax cannot spell `=` or `-`.
The palette runs these commands through `PaletteEvent::Run`, and Find keeps
focus in its bar rather than returning it to the page. The page accelerator
handler still captures only Ctrl J among these keys; Ctrl F/H/P and the zoom
keys reach a focused page first, per the owner's 2026-10-12 decision. The find
bar's height now feeds `page_card_bounds` in `crates/syrinx-app/src/ui/page_card.rs`,
and P1-12's permissions became schema version 6 (see `docs/p1-12-permissions.md`).

## DW-71: keep find input focus after native results

Native matching must not make the next character disappear from Syrinx's find
bar. This was reproduced on 2026-10-16 from `main` at `af3de53`, on Windows
build 26300, using only P1-13's synthetic HTML and a fresh profile. The work is
on `codex/fix-dw-71`, awaiting review; it is not merged.

### Reproduction and cause

Serve the `PAGE` constant from
`crates/syrinx-platform/examples/page_close/essentials.rs` on loopback. It
contains two occurrences of `syntheticneedle` and an empty input. Launch
`cargo syrinx --test-data-dir <new-absolute-directory-under-target> -- <loopback-url>`.
Move focus from the page to chrome with F6, then open Syrinx Find with Ctrl F.
Click the find input, type `syn`, wait for two matches, then append
`theticneedle` without clicking again. After the full matches appeared, UI
Automation reported the window root as focused. Pressing `x` left the query
at `syntheticneedle` and the count at two. This used Computer Use's SendInput
path, not characters posted directly to Syrinx's window. Exact observations
are in `target/repro-dw-71.txt` and launch output in `target/repro-run-dw-71.log`.

The native regression establishes the cause: WebView2 delivered `PageFocused`
for a search while the native chrome input still held keyboard focus. Syrinx
queued that notification without checking whether its focus state was still
current. `page_took_focus` then cleared GPUI's find focus to the window root,
so later characters no longer reached the input.

### Change and regression

The page's focus callback now puts its native window identity in the existing
callback ticket (`browser/page.rs`, `browser.rs` and `browser/lifecycle.rs` in
`syrinx-platform`). When the UI unwraps the event, the ticket checks that the
page still has focus, either on its window or a descendant. An obsolete focus
event is discarded, just as a callback from a retired page already was. The
check reuses `acceptance::keyboard_focus`, which reads the window's own input
queue with `GetGUIThreadInfo`, even when the receiver is the GPUI thread. A
query failure is reported explicitly instead of accepting an unverified
focus transfer. The find code and `page_took_focus` stay unchanged. No page contents, identity,
dependency, vendor file or public event shape changes. Refocusing after every
result was rejected because it can override an intentional page click.

`find_updates_keep_chrome_focus` extends the P1-13 native fixture with a native
edit control. It types `syntheticneedlex` one character at a time into the
currently focused fixture control, waits for each native result, checks the
input's exact text and focus, and refuses a spurious delivered `PageFocused`.
Each prefix through `syntheticneedle` has two matches; the final `x` has none.
The deliberate initial page focus must still deliver its event. The original
find, clearing, downloads, zoom, print and other native suites remain intact.

`target/regression-dw-71-before-2.log` fails on unchanged production code with
`find emitted PageFocused for request 100; native chrome focus retained: true`.
`target/regression-dw-71-after-final.log` passes that regression and every native
suite. The first attempt, `target/regression-dw-71-before.log`, failed in setup
before a search: its page-focus command had not finished. The fixture now uses
the existing command barrier before focusing its input; that first log is not
claimed as a DW-71 regression failure.

The unchanged baseline gate passed in `target/gate-fix-dw-71-before.log`:
1,359 workspace tests plus 127 release-tool test executions, all passing with
zero ignored, and a passing smoke run. The final `cargo xtask check` passed
in full with exit code 0 in `target/gate-fix-dw-71-final.log`, including the
application smoke run: 1,486 test executions before and 1,486 after, zero
failed and zero ignored. The new regression is an additional case in the
separately run native fixture, not a counted Rust test. The earlier
`target/gate-fix-dw-71.log` passed before the cross-thread focus-query
correction and is retained as intermediate evidence.

### Visible acceptance and observations outside this fix

The final source was launched with `cargo syrinx` using
`target/dw-71-verified-final-profile` and the same loopback fixture; its output
is in `target/verified-run-dw-71-final.log`. Computer Use's SendInput path
repeated the reproduction: `syn` showed two matches; appending `theticneedle`
kept the complete query; pressing `x` without refocusing changed it to
`syntheticneedlex` with zero matches. Backspace restored `syntheticneedle`
and two matches. Screenshots showed the caret remaining in the field through
those updates. Clicking the page input then typing `synthetic page input`
changed only that input, not the find query. These checks used fresh test
profiles, with no owner data or sign-in.

Two additional observations were not fixed. A Page menu Find click did not
open the bar during the initial Computer Use attempt; F6 then Ctrl F did.
Also, after deliberately clicking the page input, the find field could keep
its ring/caret even while typing reached the page. A later click back into
the find field could show its caret while typing still reached the page.
Their product causes are unconfirmed; the latter was observed both before
and after replacing the receiver-thread focus query with the page-thread
query. Temporary structural diagnostics in `target/diagnostic-focus-dw-71.log`
were removed from source. They are not claimed as resolved by DW-71.

If the owner checks these separate observations, use the synthetic fixture:
open Find from the Page menu; then search for `syntheticneedle`, click the page
input and type made-up text, click back into Find and type another character.
Check both the actual recipient and the focus ring. The original DW-71
typing-after-results check is complete; this does not close P1-13's other
operator checks or DW-70's NVDA check.
