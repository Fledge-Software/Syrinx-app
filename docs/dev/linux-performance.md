# Linux CPU, memory and startup measurements

This supplies a repeatable method for D6/D37 without presenting sandbox
software-rendering figures as the development machine's performance. The
coordinator must collect the real figures outside the agent sandbox.
[Issue #218](https://github.com/Fledge-Software/Syrinx-app/issues/218) owns
that run and any subsequent patch proposal.

## Status, 2026-11-02

The script's Syrinx path passed a native software-rendered validation run:
fresh profiles, mapped-window and document-ready timing, ordinary window
closure, one uncontrolled CPU sample, and ten loaded synthetic tabs. Eight
accounting, state and cleanup tests passed. **Focused, unfocused, minimized, reference-browser
and hardware acceptance figures have not been collected.** A one-second
software sample is a script check, not an idle-CPU result for this machine.

The reference Chromium launch selected native Wayland and failed to connect
with its scratch runtime directory. Its display backend and sandbox were
left unchanged. The coordinator's choice of reference display setup is
pending; no reference comparison is claimed from that failed trial.

| Hardware result | Recorded value |
| --- | --- |
| Focused idle CPU | Pending coordinator run. |
| Unfocused idle CPU | Pending coordinator run. |
| Minimized idle CPU | Pending coordinator run, or an explicit unsupported-compositor result. |
| Ten-tab Syrinx and reference memory | Pending coordinator run with the reference display setup resolved. |
| Startup | Pending coordinator run. |

## Run it

Build with the approved toolchain/profile, or point at the verified portable
binary. Stop builds and other artificial load before the actual measurement.
The default interval is 30 seconds, settling time five seconds and startup
sample count three. Keep the inherited display, session bus, input method
and scale settings. Do not run against an existing profile.

```sh
cargo build --offline --locked --profile local -p syrinx-app
python3 scripts/linux/measure.py --binary target/local/syrinx --reference none
```

The script creates fresh data/cache/config locations, fresh browser profiles
and a short mode-0700 `/tmp/sx-runtime-*` directory. The short runtime prefix
is required by Unix socket-path limits, independently of the sandbox's
read-only `/run/user/1000` restriction. Results and synthetic-only logs stay
under a fresh `target/linux-measurements/run-*` directory. No owner browser
profile, account, cookie, history or page text is used.

For each idle state, press Enter, then use the five-second countdown to set
the scratch Syrinx window focused, unfocused or minimized. Keep it that way
through the sample. Choosing `skip` records an unsupported state explicitly;
it never substitutes moving the window to a hidden workspace for minimizing.
The helper checks X11 state; on Hyprland it also checks the active process,
because X11 focus alone can remain stale while a Wayland window is active.
Only the resulting focus boolean is retained, not another window's title.

The reference option accepts `chromium`, `firefox` or `none`. It uses its own
fresh profile and XDG locations, loads the same ten authored documents,
waits for all ten to report readiness and measures the whole process tree.
Chromium's first-run, sync, default-app and background/component-update
features are disabled in this synthetic reference invocation; its sandbox
is never disabled. Firefox uses a separate non-remote profile. The reference
session is terminated after measurement. Reference-display failure remains
a reported failure until the coordinator selects its setup; do not make a
silent backend or socket substitution.

Cleanup records process start-time identities, attempts ordinary Syrinx
window closure and verifies descendant exit. A failed probe or timeout still
runs bounded termination and kill fallbacks for those owned identities while
retaining the original failure. Reference cleanup also verifies descendants,
including one that ignores termination in the regression fixture.
The runner temporarily becomes a Linux child subreaper: orphaned descendants
are adopted by the runner even if their root exits first or they start another
session. It reaps those children and restores its prior subreaper flag.
This changes the runner's own process attribute, not Syrinx or a system
setting. See [PR_SET_CHILD_SUBREAPER(2)](https://man7.org/linux/man-pages/man2/PR_SET_CHILD_SUBREAPER.2const.html).

## Definitions and limits

- CPU is the change in process-tree user plus system clock ticks divided by
  elapsed monotonic time. One fully used core is 100%, so multiple busy cores
  can exceed it. Children of every thread are included. Changed process sets,
  reused process identities or backwards counters invalidate the sample
  rather than produce a deceptively low number. Very short-lived processes
  between endpoints can be missed; use a quiet, steady process tree.
- Memory is reported in KiB. Summed RSS (resident set size) counts each process's resident pages
  and can double-count shared libraries. Summed PSS (proportional set size) assigns each mapping its
  proportional share and is preferable for the comparison. If even one
  process's PSS cannot be read, the total is unavailable, with a missing count;
  it is never replaced by zero or presented as a complete partial total.
- Each browser loads the same ten static synthetic documents, each with 256
  identical paragraphs and one authored readiness fetch. There is no polling,
  account, permission request, external asset or host-injected script.
  Forwarded Syrinx launches use its ordinary single-instance link path.
- Startup uses fresh profiles and a warm file cache: computing the binary's
  SHA-256 intentionally warms the executable. It reports launch-to-observed
  mapped X11 main window and launch-to-authored document readiness separately.
  The polling helper adds process-launch overhead and up to roughly 50 ms
  between observations. Neither endpoint claims first compositor presentation
  or cold-boot startup. Keep all individual samples rather than only a best one.
- The desktop, default window geometry and inherited scale can affect the
  comparison. Record them with the coordinator's report. Inspect the renderer
  outside the sandbox before describing a run as hardware-rendered.
- `completed: true` means the requested script phases completed; a run with
  `--reference none` still supplies no reference comparison. A failure retains
  a partial report with `completed: false` and exits unsuccessfully.

The kernel field definitions are in [proc_pid_stat(5)](https://man7.org/linux/man-pages/man5/proc_pid_stat.5.html)
and the proportional-memory definition in [proc_pid_smaps(5)](https://man7.org/linux/man-pages/man5/proc_pid_smaps.5.html).
The runtime-path limit is described by [unix(7)](https://man7.org/linux/man-pages/man7/unix.7.html).

## Sandbox validation only

```sh
python3 -m unittest discover -s scripts/linux -p 'test_measure_*.py'
python3 scripts/linux/measure.py --binary /path/to/verified/syrinx \
  --software-validation --seconds 1 --settle 1 --startup-runs 1 --reference none
```

The successful trial used packaged source `b01bf7f`, without `/dev/dri`.
Its JSON explicitly said **“software-rendered script validation; not machine
acceptance”**. The uncontrolled one-second sample reported 0.984% CPU, ten-tab
PSS 1,137,323 KiB and RSS 2,481,044 KiB; the mapped-window endpoint was
0.392 s and document-ready endpoint 1.441 s. **These values validate plumbing
only and do not represent this machine.** No reference value was obtained.

## Does the X11 backend need a D37 vendor patch?

No new vendor patch is applied or proposed from these numbers. Source
inspection of pinned `gpui-pre-linux 0.3.8`,
`src/linux/x11/client.rs::start_refresh_loop`, shows periodic frame requests
with `force_render: false` and `require_presentation: false`. In pinned
`gpui-pre 0.3.8`, `src/window.rs`, drawing is guarded by dirty state or a
forced render. A timer wake is therefore not itself proof of idle repainting.
This is code-read evidence, not an observed hardware frame count.

If the coordinator's steady-state run demonstrates unnecessary painting,
issue #218 requires a separate proposal with the exact diff, reason,
animation/expose/recovery risks and validation before any owner-approved
vendor edit. D37's Windows-only approval cannot authorize a Linux patch.

GDK `_gdk_frame_clock_freeze`/`thaw` critical messages occurred in both the
untouched ordinary app and smoke run. They persisted in the corrected gate
but were not seen in the later ordinary-window logs. The complete record
is [linux-port-acceptance.md](linux-port-acceptance.md), with diagnosis tracked
in [#225](https://github.com/Fledge-Software/Syrinx-app/issues/225); the messages
were not suppressed or attributed solely to smoke mode.
