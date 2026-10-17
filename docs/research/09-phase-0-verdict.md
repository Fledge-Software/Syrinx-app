# Phase 0 verdict: Syrinx on Windows 11

Written 2026-10-10 from two throwaway spikes built on 2026-10-09 and the
owner's own tests. The spikes' code was never merged; their full results are
on the local branches `spike/gpui` (`spike-gpui/RESULTS.md`, at `bfac1c5`)
and `spike/tauri` (`spike-tauri/RESULTS.md`, at `2579d86`). This verdict is
for Windows 11 only (D2); macOS and Linux were not tested.

## Verdict

**Go, with GPUI, on two conditions.** The engine (WebView2 driven from Rust
through wry, D3) works for the owner's hardest needs, and the GPUI chrome
beats the Tauri + Svelte control by far more than D4's rule requires. The
conditions are:

1. **Idle CPU must be fixed.** GPUI's Windows backend repaints every window
   on every display refresh even when nothing changed, so the spike idles at
   3.75% of one core against the 0.5% bar (D6). The owner allowed a small,
   pinned GPUI patch offered upstream (D37). If the patch can't bring idle
   CPU under 0.5%, GPUI against Tauri goes back to the owner.
2. **The untested musts are tested before Syrinx becomes the daily browser.**
   Hulu, the 13 other daily sites, Google sign-in with a password and phone
   prompt, and staying signed in after a restart were not tried in Phase 0.
   They move into plan 1's Phase 2 exit criteria.

## What worked

| Check (D6, D7) | Result | Evidence |
|---|---|---|
| Google sign-in, passkey | Pass | Owner, 2026-10-10 |
| Google sign-in, Workspace account | Pass (so no device-check block) | Owner, 2026-10-10 |
| YouTube | Pass | Owner, 2026-10-10 |
| Disney+ | Pass | Owner, 2026-10-10 |
| Live palette and sidebar peek over a page and over the editor, Esc returns focus | Pass in both spikes | GPUI 50/50 and Tauri 25/25 automated checks |
| Ctrl T, W, L, K, Ctrl Tab, Ctrl Shift T from a page and from the editor | Pass in both | Same suites |
| Tabs, navigation, back/forward, address sync, 10 tabs | Pass in both | Same suites |
| Fullscreen video, crashed tab with reload, suspended tabs, popups as tabs keeping `window.opener` | Pass in both | Same suites |
| Page web views with no IPC handler, no injected script, no custom user agent; crash dumps local; SmartScreen on | Pass in both | Same suites; the Tauri control built page tabs straight from wry |

## What didn't, or wasn't tested

- **Idle CPU:** GPUI 3.75% of a core (fail); the Tauri control 0.54% (just
  over the bar). Condition 1.
- **Acrylic blur:** every call succeeds but no blur of the page is visible.
  Blur is optional (D7); the palette stays near-opaque.
- **Not tested by the owner:** Hulu; Google sign-in with a password and
  phone prompt; still signed in after a restart; the 13 other daily sites.
  The owner expects them to work because the engine is the same; D6 counted
  them, so they are tested in plan 1's Phase 2. Condition 2.
- **Start to first page** was measured at different moments in the two
  spikes (page title shown for GPUI, page starting to load for Tauri), so it
  is re-measured like for like with the idle-CPU patch.
- **Smart App Control** blocks unsigned debug builds on this machine; release
  builds run. Development uses a release-style profile (D38).

## D4's rule: GPUI against Tauri + Svelte

D4: switch to Tauri if GPUI saves less than about 50 MB and 150 ms, or if the
overlays work only in Tauri.

| Measure (medians) | GPUI | Tauri + Svelte | GPUI's lead |
|---|---|---|---|
| Chrome on screen | 0.52 s | 1.44 s | about 0.9 s |
| Memory, no tabs | 27 MB (chrome alone) | 175 MB | about 148 MB |
| Memory, one tab | 273 MB | 335 MB | about 62 MB |
| Palette opens | 37 ms | 84–118 ms | about 50–80 ms |
| Idle CPU, 8 tabs suspended | 3.75% (fail) | 0.54% | Tauri leads |
| Overlays | Work | Work | Neither |

GPUI clears both thresholds, and the overlays work in both, so D4 keeps GPUI.
Edge, measured by the GPUI spike on a fresh profile, opened its window in
0.95 s and used 980 MB with one tab and 2,049 MB with ten; a fresh Edge
profile is a noisy baseline, so these are indications, not the D6 test,
which is repeated on the real app in plan 1's Phase 5.

## Corrections to earlier notes

- **GPUI draws above the WebView2 child window,** not below it, on this
  machine (GPUI spike). `docs/trapstoknow.md` said the opposite; it is
  corrected. Overlays still use separate popup windows, which work.
- Note 08's code reading of the popup re-owning (`GWLP_HWNDPARENT`) and the
  focus hand-off (`WebView::focus()`) is confirmed by running it.

## Recommendation

Start plan 1 with GPUI. Put the GPUI idle-loop patch and a like-for-like
re-measure into its foundations (Phase 1), and the owner's untested sites
into Phase 2's exit criteria. Break plan 1 into issues once the owner has
created a repository (D16).
