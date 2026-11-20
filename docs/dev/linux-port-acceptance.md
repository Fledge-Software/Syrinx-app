# Linux landing acceptance, 2026-11-02

This records the Linux side of the 2026-10-31 platform decision so the landing
PR can be assessed alongside Windows CI. Tracking belongs to [Linux platform
#217](https://github.com/Fledge-Software/Syrinx-app/issues/217); the review fixes
are [#224](https://github.com/Fledge-Software/Syrinx-app/issues/224).

## Environment and gate

Omarchy/Arch, Hyprland through XWayland, Rust 1.97.1, GTK 3.24.52,
WebKitGTK 2.54.1, libsoup 3.8.0 and bubblewrap 0.13.1 were already installed.
`dev-tools status` confirmed them; no system packages were changed. The app
kept the inherited `GDK_SCALE=2` and `XMODIFIERS=@im=fcitx`.

The untouched `0190d0c` baseline passed `cargo xtask check`: 1,696 tests,
zero failures or ignored tests. After the reviewed fixes, the full gate
passed with **1,704 tests**, zero failures, ignored, measured or filtered
tests across 32 suite summaries. No gate or assertion was weakened.

After `main` was merged in (#230's plugin host, #216, #235) and the
2026-11-03 review fixes, the Linux gate passed again on `1c71ca7`:
**1,755 tests** across 35 suite summaries, zero failures or ignored tests,
with the licence check passing for both targets and the `--smoke` run
included. That run was local, on Ubuntu 24.04 under Xvfb with software
rendering, a short fresh mode-0700 `XDG_RUNTIME_DIR` and scratch XDG data,
cache and config folders. Linux has no CI job yet (#222).

The sandbox makes `/run/user/1000` read-only. Both successful full runs used
fresh mode-0700 `XDG_RUNTIME_DIR`, plus scratch `XDG_DATA_HOME`,
`XDG_CACHE_HOME` and `XDG_CONFIG_HOME`. `DISPLAY`, `DBUS_SESSION_BUS_ADDRESS`
and the input-method settings were preserved. Ten initial instance-test
failures in the real runtime directory were environmental; the coordinator
also ran all 14 instance tests successfully outside the sandbox. Product
code and tests were not changed to accommodate that restriction.

The post-fix gate's summary lines were:

```text
Dependency boundaries passed.
Deferred-work markers are all registered.
Packaging scripts match the registration table; the Windows gate parses them.
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.36s
test result: ok. 639 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 19.49s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 158 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 29 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 151 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.91s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 221 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 31.48s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.18s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.78s
test result: ok. 108 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.49s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 122 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s
test result: ok. 107 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.67s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.40s
test result: ok. 96 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.31s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
The release tool signs through Windows key storage; the Windows gate checks it.
Local gate passed.
```

## Real-window smoke

All app runs used synthetic scratch profiles and authored loopback pages.
No real account, owner profile, browsing data or permission grant was used.
Screenshots below show only those windows.

| Check | Result and evidence |
| --- | --- |
| Open, close and reopen tabs | Pass. Ctrl T, Ctrl W and Ctrl Shift T restored a synthetic tab; a subsequent process restart restored both persisted tab rows. [Reopen](linux-port-evidence/fixed-tab-restored.png), [restart](linux-port-evidence/fixed-session-restored.png). |
| Ctrl T/W/L/K with page focus | Pass. New/close, location entry and palette worked after clicking the native page. |
| Palette and popup placement | Pass on the available single 1920×1080 monitor. After moving the main window to (700,180), size 1100×760, the palette opened at (773,258), size 953×607. Download popups remained beneath their trigger. A second monitor was unavailable. [Palette](linux-port-evidence/fixed-palette-after-move.png). |
| Page form versus Find typing | Pass with fcitx unchanged and pointer over the page. The form retained `syrinxsynthetic`; Find received `amber` and reported two matches. [Form](linux-port-evidence/fixed-form-input.png), [Find](linux-port-evidence/fixed-find-input.png). |
| Download and Cancel | Pass. A slow 16 MiB synthetic download appeared in the flyout. Cancelling a subsequent transfer produced a server-side disconnect and removed its partial file. [Progress](linux-port-evidence/fixed-download-flyout.png), [cancelled](linux-port-evidence/fixed-download-cancelled-flyout.png). |
| Move, resize and inherited GTK scale | Pass. The X11 page container and its child both measured 796×688 after the correction. Before it, GTK's child was twice its container's physical size. [Scale 2](linux-port-evidence/fixed-page-scale2.png). |
| Delegated permission wording | Pass. A localhost frame delegated by 127.0.0.1 produced the explicit “or a site it embeds” prompt; Deny refused it. No Remember or Allow was selected. [Prompt](linux-port-evidence/fixed-delegated-permission-prompt.png), [denied](linux-port-evidence/fixed-delegated-permission-denied.png). Camera and microphone delegation remain unverified because the positive controls had no usable device; owner follow-up is #234. The four-kind observations are in [linux-permission-delegation.md](linux-permission-delegation.md). |
| Print and command-pump reentry | Pass. The palette's Print command opened GTK's dialog; resizing the main window while it was open did not panic. Escape cancelled the dialog. A second print followed by closing the main window exited with status 0; the rebuilt app was checked again after download teardown changed. [Print](linux-port-evidence/fixed-print-dialog-resize.png). No print was submitted. |
| NVIDIA black-page workaround | Not applicable: no proprietary NVIDIA driver was present. The sandbox also exposes no `/dev/dri`, so these are software-rendered correctness checks, not GPU acceptance. |
| Closing the app | The scratch main process exited with status 0, including with Print open, and its windows disappeared. The native teardown regressions additionally prove retained context/registry release and active-download cancellation. |

## Package and scratch install

`cargo xtask package` succeeded for source commit
`b01bf7f539e40bbf49b4f432143980fa1bbb7eaa`, producing
`Syrinx-0.1.0-b01bf7f539e4-linux-x64.tar.gz`. The separate package build took
20 minutes 13 seconds. Its archive SHA-256 was
`2ef02e6e736e0c06255d75a8ea644a763ccba639fbac8affefa4c4f98a5bc484`.

The archive was extracted into a fresh scratch directory. Every
`SHA256SUMS` entry passed. With HOME and all four XDG locations pointed into
scratch directories, `install.sh` succeeded; the installed program and icon
matched the archive bytes, the program mode was 0755, and the desktop entry
named the scratch program's full path. No owner's desktop entry or default
browser setting was touched.

`uninstall.sh` removed the program, entry and icon while preserving a
synthetic data sentinel. A second uninstall was harmless. Reinstall also
succeeded and `ldd` reported no missing library. The installed program then
rendered a loopback page under the unchanged display/session bus/input-method
environment. [Packaged app](linux-port-evidence/packaged-loopback-page.png).
After closing it, the main process exited with status 0 and all seven
tracked descendant process identities disappeared. Tracking read children
from every thread, including GTK's, rather than only the process leader.
The final scratch uninstall again retained the synthetic data. Nothing was
installed into the owner's real HOME.

## Review and remaining limits

A fresh independent reviewer audited the whole Linux delta, independently ran
eight focused regressions, inspected the initial ten screenshots and approved
after the additional download-context ownership finding was fixed. That pass
did not independently repeat the full gate or four-kind manual matrix and
cannot establish Windows compatibility; the PR's Windows gate must do that.
The restart screenshot was captured afterward.

Mesa's DRM errors reflect the sandbox's missing GPU. GDK frame-clock
freeze/thaw critical messages occurred in both the untouched normal app and
smoke run. They persisted in the post-fix gate; they were not observed in the
later normal-window logs. [#225](https://github.com/Fledge-Software/Syrinx-app/issues/225)
tracks diagnosis; they were not suppressed. Native GTK child-window
user-time warnings also remain visible in the logs.

No CPU, memory, startup or frame number from this environment represents the
machine. [#218](https://github.com/Fledge-Software/Syrinx-app/issues/218) owns
the repeatable measurement script and the coordinator's hardware run.
Google sign-in remains an owner-only account test. Browser registration and
choosing Syrinx as the Linux default remain owner decisions.
