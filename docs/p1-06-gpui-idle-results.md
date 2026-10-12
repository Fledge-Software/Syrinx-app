# P1-06: GPUI idle repaint patch, results

Date: 2026-10-12. Branch `claude/p1-06-gpui-idle-v2`. Status: patch vendored,
measured and gated; the eight-suspended-tab criterion and the first-page
startup comparison remain open (see "What remains").

## Why this work exists

D37 approved GPUI for Atlas's interface on condition that an idle Atlas costs
less than 0.5% of **one** CPU core. Phase 0 measured about 3.75% of a core
(`docs/research/09-phase-0-verdict.md`) and traced it to GPUI's Windows
backend: a background "VSync" thread calls `RedrawWindow` on every window on
every display refresh, so each window receives a `WM_PAINT` message 60 times a
second (on this 60 Hz display) even when nothing in it changed. GPUI then
decides there is nothing to draw, but waking the UI thread, running the
message handler and polling input on each tick still costs CPU. Without a fix,
Atlas would fail D37's bar and the engine/UI choice would go back to the owner.

The design, and the list of wakeups that must not be lost, is in
`docs/gpui-idle-patch-review.md`. This document records what was built, how it
was measured and what it showed.

## What the patch does

The vendored package is `vendor/gpui-pre-windows-0.3.8`, selected by a
`[patch.crates-io]` entry in `Cargo.toml`; its provenance, exact diff and file
hashes are in `vendor/gpui-pre-windows-0.3.8.provenance.md` and
`vendor/gpui-pre-windows-0.3.8.idle.patch`. Pins, features and the licence are
unchanged, and `Cargo.lock` changed only in that package's source and checksum
lines.

GPUI's core already tells a platform when a window needs a frame, through
`PlatformWindow::frame_waker` and `schedule_frame`; other backends use them,
but the Windows backend ignored both and repainted unconditionally. The patch
implements them with a per-window "frame demand" latch (`src/frame_demand.rs`):

- GPUI raises demand when a window becomes dirty, schedules a next-frame
  callback (animations), or is throttled and needs a retry.
- The VSync thread keeps its pacing but invalidates a window only while its
  demand is pending and the window is visible and not minimized. It reads the
  latch and never clears it.
- The UI thread clears demand in `draw_window` only after it has the draw
  guard and the frame callback, so demand raised while that frame is drawing
  survives to the next tick.
- A deferred re-entrant draw, a draw while minimized (no callback), device-loss
  recovery and touchpad gestures (Direct Manipulation contact, status changes
  and running inertia) all raise demand explicitly, replacing their old
  reliance on unconditional repainting.

The rejected alternative was to stop or park the VSync thread when idle. It
would save the remaining wakeups but needs new shutdown and device-recovery
signalling; the review deferred it unless measurement required it, and the
measurements below do not yet require it.

## Method

All builds used D38's `local` profile with unchanged Windows, security and
power settings. The machine has 24 logical processors and a 60 Hz display.
Other agents were compiling on the same machine during the runs; per-process
CPU time is largely insulated from that, but treat small differences between
runs as noise.

- **Builds compared.** The unpatched build uses the registry package; the
  patched build uses the vendored package. Each candidate was built twice:
  a minimal throwaway GPUI window ("harness") and the real `atlas-app`. The
  unpatched `atlas-app` was built by temporarily removing the patch line and
  restored afterwards; the two executables differ only in that package and
  its dependants.
- **Harness.** A throwaway program (source kept in the ignored
  `target/p1-06-idle-evidence/harness/`, not committed) opens static GPUI
  windows. A thread-local `WH_GETMESSAGE` hook counts `WM_PAINT` messages
  removed from the queue for each window, and each view counts its GPUI
  renders. It does not modify GPUI, so it runs identically on both builds.
- **Idle CPU.** `target/p1-06-idle-evidence/measure.ps1` launches the
  program with a normal visible window, waits 30 s, reads
  `Get-Process ... TotalProcessorTime` for the process and every descendant
  process, waits 300 s, reads again, and divides the CPU-seconds used by the
  wall time. The result is a percentage of **one** core; it is not divided by
  24. Each build ran twice, interleaved unpatched/patched
  (`series.ps1`). `atlas-app` ran with `--test-data-dir` pointing at a fresh,
  newly named temporary directory, so it opened a synthetic empty profile; it
  showed the Home view and started no WebView2 process, so its process tree
  was atlas-app alone. It was closed through its window afterwards.
- **Behaviour scenarios.** The harness's `scenarios --expect-idle` mode runs
  ten checks over two windows: initial paint, idle paint rate, notify after
  idle (only the notified window renders), a next-frame callback with no
  dirty view, demand raised during a frame, a 60-frame animation that then
  returns to idle, notifying the second window, work requested while
  minimized delivered on restore, a native resize, and final idle.
- **Unit tests.** `src/frame_demand.rs` carries eight tests for the latch and
  the invalidation rule.

## Results

### Paint rate

| Build | Idle `WM_PAINT` per second (one window) | GPUI renders while idle |
| --- | ---: | ---: |
| Unpatched, 335 s idle runs | 59.04, 57.99 | 1, 1 |
| Patched, 335 s idle runs | 0.00, 0.00 | 0, 0 |

The unpatched build repaints at the display refresh rate while drawing
nothing; the patched build does not repaint at all while idle. This is the
check the earlier harness lacked: its correctness scenarios passed on both
builds, so they could not show the patch was needed. With `--expect-idle`,
the unpatched build now fails three checks (idle paints 55/s, 42/s after an
animation and 18/s at the end) and the patched build passes all of them.

### Idle CPU, percentage of one core over 300 s

| Program | Unpatched run 1 | Unpatched run 2 | Patched run 1 | Patched run 2 |
| --- | ---: | ---: | ---: | ---: |
| Harness (one window) | 1.859% | 1.656% | 0.460% | 0.244% |
| `atlas-app`, fresh profile | 1.996% | 1.755% | 0.347% | 0.229% |

All four patched runs are under the 0.5% bar; all four unpatched runs are
over it by more than three times. The patch removes roughly 80 to 85% of idle
CPU. The margin is thin in one run (0.460%). The remaining cost is most likely
the VSync thread still waking once per display refresh, as the design review
predicted; this was inferred, not profiled per thread.

### Behaviour

The scenario runs are in `target/p1-06-idle-evidence/scenarios-*.log`. All
behavioural checks pass on both builds: initial paint of two windows, notify
after idle in about 15 ms with no render of the other window, a next-frame
callback in 16 to 32 ms, mid-frame demand served, a 60-frame animation in
about 2.65 s (both builds) followed by zero renders, no renders while
minimized and pending work delivered on restore, and a native resize render in
about 22 ms. The eight `frame_demand` unit tests pass
(`target/p1-06-idle-evidence/frame-demand-tests.log`).

### Gate

`cargo xtask check` passed on 2026-10-12 with the patch in place: dependency
boundaries, deferred-work scan, formatting, Clippy with warnings denied,
525 tests passed with none failed or ignored, documentation tests and the
synthetic `--smoke` launch. No Application Control block occurred. The only
warning is the existing unused-function warning inside `vendor/wry-0.57.0`.
The log is `target/p1-06-idle-evidence/gate.log`, SHA-256
`7c58bb5ebb99d6a4de27b85bc2c927f4d482e44f8aea24dcec3a0c627f00d097`.
The workspace gate does not run the vendored crate's tests (see below).

## Failures and limits

- **The crate's own upstream tests cannot run offline here.** The package's
  `#[cfg(test)]` code needs its `test-support` feature (without it, the test
  build fails with E0407 on `render_to_image`), and that feature pulls
  `proptest` and other development packages that are not in this machine's
  Cargo cache. Fetching them would be a new download, which was not done. The
  earlier agent's E0463 ("can't find crate gpui") belongs to the same class of
  packaging problem. Instead, `src/frame_demand.rs`, which depends only on the
  standard library, was compiled and run on its own with
  `rustc --edition 2024 --test -D warnings`. The workspace gate does not run
  dependency tests.
- **The earlier agent's scratch work was not found** anywhere on disk; the
  patch, harness and unit tests were redone from the verified archive.
- **Not exercised:** real touchpad scroll/pinch inertia, GPU device loss,
  modal move/size loops beyond a programmatic resize, DPI changes and
  multiple monitors. The code paths raise demand for each, but only source
  review supports them.
- **Window inspection.** The scenarios check renders and paint counts, not
  pixels. The agent could not take screenshots of the Atlas window; the owner
  should look at a patched window, resize it, minimize and restore it, and
  scroll with a touchpad.
- **Eight suspended tabs.** D37's bar is stated with eight tabs suspended.
  Suspension does not exist until P1-15, and hiding page views is not
  suspension, so this criterion stays open. Suspended WebView2 processes will
  add their own cost on top of the 0.23 to 0.35% measured here.
- **First-page startup.** P1-06 also asks for the equivalent start-to-first-
  page comparison over five launches; it was not run in this slice.

## Upstream-ready description

*Not submitted. Publishing requires the owner's approval.*

> **gpui_windows: repaint only windows with frame demand**
>
> The Windows backend's VSync thread calls `RedrawWindow` on every tracked
> window on every display refresh, so an idle window still receives a
> `WM_PAINT` per refresh (60/s at 60 Hz). GPUI then finds nothing dirty, but
> the wakeups cost about 1.7 to 2.0% of a core per idle app on our machine.
>
> This change implements the existing `PlatformWindow::frame_waker` and
> `schedule_frame` hooks on Windows with a per-window atomic demand latch. The
> VSync thread keeps its pacing but invalidates only visible, non-minimized
> windows with pending demand, and never clears it. `draw_window` clears the
> demand after it acquires the draw guard and the frame callback, so demand
> raised during a frame survives it. Deferred re-entrant draws, draws while
> minimized, device-loss recovery and Direct Manipulation contact, status
> changes and active inertia raise demand explicitly instead of relying on
> unconditional repaints.
>
> Measured over 300 s idle: `WM_PAINT` drops from about 59/s to 0/s, and
> process CPU from 1.66 to 2.00% of one core to 0.23 to 0.46%. Animation,
> next-frame callbacks, notify-after-idle, minimize/restore and resize behave
> as before in a two-window harness. Unit tests cover the latch.

## What remains

- Remeasure with eight suspended tabs once P1-15 implements suspension; if
  the result exceeds 0.5% of one core, the next step is the reviewed VSync
  parking change, and failure returns the tradeoff to the owner under D37.
- Run the five-launch start-to-first-page comparison for both candidates.
- Owner inspection of a patched window, including touchpad scrolling.
- Owner approval before offering the patch upstream; when an upstream
  replacement is adopted, remove this vendor copy instead of moving pins.
