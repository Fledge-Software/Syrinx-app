# GPUI Windows idle-paint patch review

Date: 2026-10-10. Status: source investigation only. No patch has been
implemented, vendored, built or measured. D37's idle-performance gate remains
open; this document does not complete P1-06 or change its ordering.

Continuous native paint requests waste work even when GPUI's view tree is
clean. The smallest candidate uses the frame-demand hooks already present
in the pinned core, changing only the Windows backend. Keep all package
versions and features unchanged, and add no dependency.

## Pinned source evidence

The inspected Cargo cache packages are `gpui-pre-windows 0.3.8` and
`gpui-pre 0.3.8`, under `registry/src/<registry>/` in Cargo's local cache.
Their metadata identifies Zed snapshot
`279fe070bb389b79652e52065b2f001edcc0b11b`. Paths below are relative to that
upstream repository; the published Windows package contains the same
`src/` paths beneath its package directory.

| Upstream path and function | Observed behavior and proposed change |
|---|---|
| `crates/gpui_windows/src/platform.rs:429`, `WindowsPlatform::begin_vsync_thread` | Every display tick calls `RedrawWindow` for every tracked window at line 487. Add a shared per-window demand latch to `TrackedWindow`; invalidate only demanded, visible, non-minimized windows. |
| `crates/gpui_windows/src/window.rs:970`, `PlatformWindow` implementation | Windows does not implement the existing `frame_waker()` or `schedule_frame()` hooks. Implement both using the demand latch. Capture its `Arc`, avoiding a strong window-reference cycle. |
| `crates/gpui_windows/src/events.rs:1329`, `draw_window` | Drawing ends with `ValidateRect`. Consume demand only after acquiring `DrawCoordinator` and taking the callback. Preserve or rearm demand when a nested draw is deferred. |
| `crates/gpui_windows/src/direct_manipulation.rs:97,108,207,261` | Touchpad contact, manual updates and viewport/content callbacks currently depend on paint delivery. Wake on contact/status/content changes and sustain demand during active manipulation and inertia, including the final ended event. |

The core contract already exists in `crates/gpui/src/platform.rs`
(`PlatformWindow::frame_waker`, `schedule_frame`) and
`crates/gpui/src/window.rs`: lines 179–246 wake on initial attachment and
clean-to-dirty transitions; lines 1789 and 1857–1874 rearm throttled frames,
remaining invalidations and animation callbacks; `on_next_frame` at line
2650 wakes without requiring a dirty view. Existing core tests at lines
8131 and 8189 cover demand wakeups and pending callbacks. These are source
observations, not evidence that the proposed Windows patch passes them.

Keep `crates/gpui_windows/src/vsync.rs::wait_for_vsync`, the foreground
dispatcher's message wakeups and timers, and device-loss polling unchanged
for the first candidate. This preserves pacing and recovery while removing
continuous UI paint requests. Whether the remaining VSync thread meets the
CPU bar must be measured; parking it would require a further reviewed
change with explicit shutdown and device-recovery wakeups.

## Wakeups that must not be lost

- **Do not consume demand on the VSync thread.** It can invalidate a window
  while a frame is drawing, only for the final `ValidateRect` to erase that
  invalidation. Consume at successful UI-frame entry; demand raised during
  that frame must survive for a subsequent tick.
- **Keep reentrant and forced frames pending.** The present
  `DrawCoordinator` fallback explicitly relies on unconditional VSync
  repainting. Preserve `force_render_pending` and replace that reliance with
  explicit demand. Do not consume demand when the callback is unavailable.
- **Preserve hidden-window demand.** Minimizing removes the frame callback;
  restoration must deliver pending work. Native exposure, resizing, modal
  move/menu timers and GPU recovery must continue to paint correctly.
- **Do not strand touchpad inertia.** GPUI selects `MANUALUPDATE` and calls
  `IDirectManipulationUpdateManager::Update` in the paint path. Microsoft
  documents that this mode does not update content automatically during
  input. Contact must start frame demand, and active gestures must continue
  it until completion. See [viewport options](https://learn.microsoft.com/en-us/windows/win32/api/directmanipulation/ne-directmanipulation-directmanipulation_viewport_options)
  and [Update](https://learn.microsoft.com/en-us/windows/win32/api/directmanipulation/nf-directmanipulation-idirectmanipulationupdatemanager-update).

## Verification still required

Add focused regression coverage for initial paint, idle-to-notify,
callback-only demand, demand raised during drawing, nested/forced draws,
animation completion and return to idle. Exercise two windows,
minimize/restore, native resizing/menu loops, touchpad scroll/pinch inertia
and device-recovery scheduling. Run the product gate and the patched
backend's focused tests explicitly: workspace tests do not automatically
execute dependency tests. Inspect the actual window as required by
`AGENTS.md`; source review cannot establish responsive input or animation.

Compare unpatched and patched builds on this machine, with the same build
profile and unchanged Windows/security/power settings. D37 requires idle
CPU below 0.5% of **one core** over five minutes with eight tabs suspended;
do not divide the process CPU-time delta by the machine's 24 logical
processors. Record process and child-process accounting and use fresh
synthetic profiles. Verify actual suspension: hiding the preview's page
views is not suspension and cannot establish this exit criterion. Repeat
the equivalent start-to-first-page measurement over five launches for both
throwaway candidates, as required by P1-06. Retain the product-shell check
for P1-18. Failure to reach the bar returns the engine/UI tradeoff to the
owner under D37, without silently relaxing the threshold.

## Licence and approval boundaries

The published Windows package declares Apache-2.0 and includes
`LICENSE-APACHE`; the proposed whole source files fall under that licence.
No Zed editor or language crate is needed. Preserve the licence and source
provenance if the package is vendored, and verify the copied archive and
unchanged dependency graph before building it.

D37 already authorizes a small pinned GPUI patch under D12's maintenance
rules. It does not authorize public submission: prepare the upstream diff
for review, and obtain the owner's approval before publishing it. Remove
the local patch when its upstream replacement is deliberately adopted;
do not move the current pins as part of this change.

A whole-package vendor copy would retain two upstream deferred-work
comments in `crates/gpui_windows/src/shaders.hlsl:490` and `:704`. The current
`AGENTS.md` exception for `TODO`/`FIXME` comments applies only to
`vendor/libsqlite3-sys-0.38.2`. A similarly narrow exception for these
unchanged GPUI comments is needed before that copy; do not delete or rewrite
upstream comments to evade the rule.

P1-06 still follows P1-05 in the approved map. P1-05's performance gate is
open. D49 brings the integrated browser preview forward; this read-only
preparation does not extend that exception to beginning P1-06. Check the
owner's current sequencing authorization before implementation. Commits and
merges retain their separate owner approvals.
