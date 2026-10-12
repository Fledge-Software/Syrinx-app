# P1-13: reachable browser essentials

Everyday browser controls must work while a page owns keyboard focus. This
slice routes Ctrl F/J/P, Ctrl +/−/0 and the owner-requested Ctrl H through the
existing accelerator queue. Its synchronous callback still only marks the
key handled and posts a message. No page script, identity override, dependency
or vendor change is involved.

## Implementation

`crates/atlas-platform/src/browser/essentials.rs` calls the pinned WebView2
interfaces through the controller. Find uses native matching and highlighting;
downloads use the engine's minimal progress/cancel list; print opens preview,
without submitting a job. Default context menus are explicitly enabled. Zoom
steps are bounded and the toolbar displays the controller's actual percentage.
The download event override is necessary: wry's default callback marks downloads
handled and suppresses the engine's list even without an Atlas callback.

`crates/atlas-app/src/ui/find.rs` owns the find bar. Opening it transfers native
keyboard focus to Atlas and focuses its input. Enter/Next and Previous move
between matches; Escape/Close stops finding and returns focus to the page.
Switching, navigating or closing the tab dismisses it. Monotonic request IDs
reject obsolete results, including closing and reopening on the same tab.
Native query text stays in memory and is never logged or persisted.

The main alternative was the engine's own find bar. In three diagnostic runs
(`target/p1-13-native-1.log` through `-3.log`), empty `Find.Start` did not complete
on this runtime; nonempty text returned the expected two matches. The Atlas bar
therefore opens without a search call. Clearing invokes `Stop` and acknowledges
an empty result; this is explicit application behavior, not a claim that native
empty `Start` succeeded. Existing lifecycle assertions remain unchanged.
Microsoft documents the intended native [find contract](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2find?view=webview2-1.0.3537.50)
and [download dialog controls](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_9?view=webview2-1.0.4022.49).

Ctrl H is forwarded and covered by the accelerator mapping test. This base has
no history-list implementation: chrome reports that it awaits P1-20 integration.
No later history feature was built in this slice.

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
Unit tests verify focused-page key mapping, command targeting, zoom boundaries
and stale find results. These do not claim physical-key, visual layout, actual
file-download or completed-print acceptance.

The original intermittent lifecycle navigation timeout remains known and
undiagnosed, as recorded in `docs/fixture-transport-timing-review.md`. A passing
run does not repair it; the owner expressly permits continuation.

## Owner visual checks

Desktop inspection is unavailable in this session. The gate launches the
application, but visual acceptance remains with the owner:

1. Under `target/`, create an HTML file containing two occurrences of
   `syntheticneedle`, an input and an ordinary link. Serve that directory on
   loopback with `python -m http.server 8765 --bind 127.0.0.1 --directory <directory>`.
   Run `cargo atlas -- --test-data-dir <new-absolute-directory-under-target>` and
   open that local page with Ctrl L.
2. Click the page input, press Ctrl F, and immediately type `syntheticneedle`
   without another click. The Atlas bar should show two matches and native
   highlights. Verify Enter/Next, Previous, clear, Escape and typing back in the
   page. Close/reopen Find rapidly, switch tabs and navigate while searching;
   no stale count or old bar should remain. Check a narrow window and DPI scaling.
3. With the page focused, press Ctrl J. Verify the engine's download list opens.
   For an actual synthetic download, use a local text attachment and choose a
   destination under this worktree's `target/`; verify progress/completion and
   cancel. Do not use a personal download or profile for this acceptance check.
4. Press Ctrl P and verify preview appears; cancel without printing. Right-click
   page text, its link and its input to verify the default context menus.
5. Press Ctrl +, Ctrl − and Ctrl 0. Verify page scaling and toolbar percentage,
   then repeat from chrome. Ctrl H should reach the explanatory history message
   on this branch; the integrated history list must receive it after integration.

## Integration handoff

The shared `crates/atlas-app/src/ui.rs` wires the modules, actions, events and
page bounds. `Cargo.toml`, `Cargo.lock` and platform `lib.rs` are unchanged.
Update only P1-13's ledger row on this branch. There are no new deferred-work
markers. P1-12's schema-2 permission migration still needs renumbering when
combined with parallel schema work.
