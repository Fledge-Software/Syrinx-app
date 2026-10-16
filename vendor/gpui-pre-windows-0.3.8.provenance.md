# gpui-pre-windows 0.3.8 with demand-driven repainting

GPUI's Windows backend asks Windows to repaint every window on every display
refresh, even when nothing in the window changed. On this machine that is 60
`WM_PAINT` messages a second per idle window, and D37 sets an idle budget of
0.5% of one core. The owner approved vendoring the pinned package with the
idle-repaint patch described in `docs/dev/vendor-reviews/gpui-idle-patch-review.md` on
2026-10-11 (`AGENTS.md`, "Approved narrow source patch, 2026-10-11"). This
file records the source import; the measurements and behavioural checks are
in `docs/dev/vendor-reviews/p1-06-gpui-idle-results.md`.

## Source and scope

The baseline is the complete published `gpui-pre-windows 0.3.8` registry
archive, read from Cargo's existing local cache
(`registry/cache/index.crates.io-1949cf8c6b5b557f/gpui-pre-windows-0.3.8.crate`)
without downloading anything. Its SHA-256 is
`371f9bb13f97e3bea2f8b8e5e81232bb74d735153c244ae67f7400e743925b1c`, which
matches the checksum Atlas's lockfile recorded for the registry package before
the path override. The archive has no `.cargo_vcs_info.json`; its manifest
metadata (`[package.metadata.gpui-pre]`) names Zed crate `gpui_windows` at
revision `279fe070bb389b79652e52065b2f001edcc0b11b`. The published archive,
not an assumed checkout of that revision, is the byte baseline.

The archive holds 28 files and 655,835 bytes. The vendored package holds 29
files and 662,641 bytes: five original files changed and one private module
was added. The other 23 files, including both manifests, the package's own
`Cargo.lock`, the build script, `LICENSE-APACHE` and every shader, are
byte-for-byte identical to the archive.

- `src/frame_demand.rs` (new) holds `FrameDemand`, a per-window latch shared
  by the window, GPUI's frame waker and the VSync thread, and the pure
  `should_invalidate` rule, with eight unit tests.
- `src/gpui_windows.rs` declares and re-exports that private module.
- `src/window.rs` gives each window a latch, passes it to Direct
  Manipulation, and implements GPUI's existing `PlatformWindow::frame_waker`
  and `schedule_frame` hooks with it. The waker captures only the latch, so
  it creates no reference cycle with the window.
- `src/platform.rs` stores the latch in each tracked window. The VSync thread
  now invalidates only windows whose demand is pending, visible and not
  minimized; it reads demand but never consumes it. It still drops a hidden
  window's profiler frame signal as before.
- `src/events.rs` consumes demand in `draw_window` only after the draw guard
  is acquired and the frame callback is taken. A deferred re-entrant draw
  and a draw without a callback (a minimized window) keep the demand and
  any forced render pending. Device-loss recovery raises demand, and an
  active Direct Manipulation gesture raises it after each poll.
- `src/direct_manipulation.rs` raises demand on touchpad contact and on every
  viewport status change, and records whether a gesture or its inertia is
  running so the paint path keeps polling until the final `Ended` event.

The VSync thread, its pacing, the foreground dispatcher, timers and the
device-loss polling are unchanged. The patch adds no dependency, feature,
public API, lint annotation or deferred-work comment, and changes no
manifest. It does not touch page web views or any Atlas privacy setting.

## Licence and approval boundary

The unchanged manifest declares `Apache-2.0`, and `LICENSE-APACHE` is retained
unchanged; the new module is contributed under the same licence. No Zed editor
or language crate is involved. Unchanged upstream deferred-work comments
remain in `src/shaders.hlsl` (lines 490 and 704), `src/direct_write.rs`,
`src/platform.rs` and `src/window.rs`, together with the upstream lint
annotations; the approval permits them only inside this directory. It does
not permit another dependency patch, commits, merges, pushes, publication or
an upstream message.

## Reproducible local patch

`vendor/gpui-pre-windows-0.3.8.idle.patch` is the complete six-file unified
diff with package-relative paths (`a/src/...`, `b/src/...`). Its SHA-256 is
`17766aa3e7a904f57a0ed2e270a7dbb4c502e9ea1527cacb6379abbf81b92e6b`
(14,470 bytes). Applying it with `patch -p1` to a fresh extraction of the
verified archive reproduced this directory exactly (`diff -r` reported no
difference). The patched files pass `rustfmt --edition 2024 --check`. The
patch is prepared locally under D12 and has not been sent upstream.

## Workspace integration and lockfile

`Cargo.toml` excludes this directory from the workspace and selects it with
a `[patch.crates-io]` path override, as for wry, frizbee and
libsqlite3-sys. The exact `=0.3.8` requirements on `gpui-pre` and
`gpui-pre-platform` and their features are unchanged; `gpui-pre-windows` is
reached only through `gpui-pre-platform`. Resolving the override removed only
the `source` and `checksum` lines of the `gpui-pre-windows` entry in
`Cargo.lock`; no package version, dependency edge or other entry changed.
`.gitattributes` disables text conversion for this directory and the patch
file, preserving the hashes below across checkouts on this machine.

## Embedded shaders patch (2026-10-14)

The first preview release, `v0.1.0`, closed at start-up on every machine but
the GitHub runner that built it: a GPUI panic, "Recreating GPU state for
DirectWrite: The system cannot find the path specified". Upstream GPUI embeds
precompiled shaders only when debug assertions are off. With them on, as in
D38's `local` profile, `build.rs` skips `fxc.exe` and the renderer compiles
`shaders.hlsl` and `color_text_raster.hlsl` at run time from
`CARGO_MANIFEST_DIR`, the build machine's own source folder
(`D:\a\Syrinx-app\...` on the runner). The owner approved this second patch
on 2026-10-14 (`AGENTS.md`).

- `build.rs` compiles the shaders with `fxc.exe` in every profile.
- `src/directx_renderer.rs` always takes the embedded bytes; the run-time
  `D3DCompileFromFile` path and its helpers are removed.

Every executable is now independent of where it was built, and the gate tests
the same shader bytes that ship. Building needs the Windows SDK's `fxc.exe`
(found through `GPUI_FXC_PATH`, `PATH` or the newest installed SDK), which the
GitHub Windows runner has. `vendor/gpui-pre-windows-0.3.8.embedded-shaders.patch`
is the two-file unified diff against the idle-patched files, with
package-relative paths. Its SHA-256 is
`5c8384bcec08e66ff6b500568e0ea00f55093235faaad83bbf915b856ec46a13`
(6,095 bytes). It adds no dependency or feature and changes no manifest. The
package now holds 29 files and 658,607 bytes; the table below shows the
final hashes after both patches.

## Complete baseline and final inventory

Each SHA-256 covers the exact file bytes (all LF). Equal columns prove the
retained files; the seven changed rows and one added row are the two patches.

| File | Bytes, baseline / final | Baseline SHA-256 | Final SHA-256 |
| --- | ---: | --- | --- |
| `build.rs` | 7683 / 7704 | `3192734d0448f3ec44525e3b55c025d30d9926edce6b24e5575ac27a1ab3ad06` | `e0e0f8d7a8523961cf03c61efa82b06a51d24ec3ef07d7229ac75e43138cc69b` |
| `Cargo.lock` | 122892 / 122892 | `45b818c80b163e8837dafacbaa833ce6ff15b63c8f29083762f9d2b66a3cc7f2` | `45b818c80b163e8837dafacbaa833ce6ff15b63c8f29083762f9d2b66a3cc7f2` |
| `Cargo.toml` | 5930 / 5930 | `7ea59094436bcb0d65f09922efcadff5f0d878486ee515ace03cf570b651971c` | `7ea59094436bcb0d65f09922efcadff5f0d878486ee515ace03cf570b651971c` |
| `Cargo.toml.orig` | 1630 / 1630 | `71ad31ff7e524ed399091bb963d3bcfbc562dd3e59cae0b3a279e2733aab367f` | `71ad31ff7e524ed399091bb963d3bcfbc562dd3e59cae0b3a279e2733aab367f` |
| `LICENSE-APACHE` | 10768 / 10768 | `752daf2fb234ca4a1fa372c073fe127f44b7b90fd2529ae44273a64f9d53da7a` | `752daf2fb234ca4a1fa372c073fe127f44b7b90fd2529ae44273a64f9d53da7a` |
| `src/alpha_correction.hlsl` | 2010 / 2010 | `d37b106f1266b40a0e17478455dfaac1205214f597e23fbd0a26455c49806bf4` | `d37b106f1266b40a0e17478455dfaac1205214f597e23fbd0a26455c49806bf4` |
| `src/clipboard.rs` | 13208 / 13208 | `eb68368598663c85916328d03582435d923e2b6eeae87a7a365a386848b87bf9` | `eb68368598663c85916328d03582435d923e2b6eeae87a7a365a386848b87bf9` |
| `src/color_text_raster.hlsl` | 1207 / 1207 | `53bb7f737dcf5b7c7a2c474cf6e7f649cc4e7d49b5526c2ac5f75f75086ce2b8` | `53bb7f737dcf5b7c7a2c474cf6e7f649cc4e7d49b5526c2ac5f75f75086ce2b8` |
| `src/destination_list.rs` | 7044 / 7044 | `2f9bfdb5e7fdfdb2f72e4e150683677538b38dc4087390e916df765fab46a6e4` | `2f9bfdb5e7fdfdb2f72e4e150683677538b38dc4087390e916df765fab46a6e4` |
| `src/dialog.rs` | 11442 / 11442 | `94f35e514b4f42ef96aa0a66934193a33534bd1536cc526e2be3547c745adb41` | `94f35e514b4f42ef96aa0a66934193a33534bd1536cc526e2be3547c745adb41` |
| `src/direct_manipulation.rs` | 12095 / 13248 | `da83877b3e6c2d3f7d391eb151cc4cc56c70dbc95b261f5bda2e3e861d8aca1d` | `a340335f2c5567b3e6a1fae795e8eab1e4b58f34a6a2eef4e55193e6562f93ed` |
| `src/direct_write.rs` | 76544 / 76544 | `27ff8aa902705794c5b9c0405bb7c3063e4b63e0d3ac44d92148d9ed18ee866c` | `27ff8aa902705794c5b9c0405bb7c3063e4b63e0d3ac44d92148d9ed18ee866c` |
| `src/directx_atlas.rs` | 14420 / 14420 | `eff897707e3b8b4e549cd247725a6cee9c14b6a8188e32d32ad6360d693ce455` | `eff897707e3b8b4e549cd247725a6cee9c14b6a8188e32d32ad6360d693ce455` |
| `src/directx_devices.rs` | 6143 / 6143 | `83372170f6225db45033473d118fc43708942ec9687467de09e2b5215c835cf1` | `83372170f6225db45033473d118fc43708942ec9687467de09e2b5215c835cf1` |
| `src/directx_renderer.rs` | 72814 / 68759 | `4f4e51ce4ff30b1884d01179873a17483440e205c420d981bb2a5fa2155fa7b0` | `4435b7658cf177ed89202e9dd1d2f3c0c97f22ce9c8c81415044488edab67f17` |
| `src/dispatcher.rs` | 7052 / 7052 | `a24daa0105ceadc2a4d50f0d1afab5a4af441a6a1254412d9811a04095f8cdd4` | `a24daa0105ceadc2a4d50f0d1afab5a4af441a6a1254412d9811a04095f8cdd4` |
| `src/display.rs` | 6446 / 6446 | `763040fe7a9c29d78c46e42dd8c4ffaa8eb703d7cdba0748b709a8e76787f614` | `763040fe7a9c29d78c46e42dd8c4ffaa8eb703d7cdba0748b709a8e76787f614` |
| `src/events.rs` | 66120 / 66854 | `dd12944e3a5ec5ed25faba1114d59ddd4438eca214b4841a5a53b538db0767af` | `e524bcec4d34f6d57c1bcf8a26175bf7d2e1b70e66a6f712b72376e3728da5cc` |
| `src/frame_demand.rs` | absent / 3854 | absent | `12ba6669df35652083f73b616cb6ce7f07567013b45e6607b4ed92f43063da41` |
| `src/gpui_windows.rs` | 952 / 1002 | `75343c98433a942fa1f4a700592e11361edb9a236a5523fd9a6327523b0c9b6a` | `baf028be7582d67a6f12009be3e80ec666eeceb81146ad2177f5dda7294f0ce7` |
| `src/keyboard.rs` | 11243 / 11243 | `4e373ce1a3f23b6527b1abb3051b59c96d5cc2b0f777d4d4097dd27dd1673516` | `4e373ce1a3f23b6527b1abb3051b59c96d5cc2b0f777d4d4097dd27dd1673516` |
| `src/platform.rs` | 68827 / 69248 | `ce5cce9a6086e081dc6a1cb659c7945d9f517d3ecbfdd10a9adfff6623b9887d` | `ce2a42de519598365f8d2754ada0975d173ce4be714917fbc1f811394dc89914` |
| `src/shaders.hlsl` | 46996 / 46996 | `16a7d6ba1e2095bf1aa53c4d91a3ba54282851c4c097dbaf3d688d6f6983c4fc` | `16a7d6ba1e2095bf1aa53c4d91a3ba54282851c4c097dbaf3d688d6f6983c4fc` |
| `src/system_notifications.rs` | 7614 / 7614 | `d0159dbf6fd10622a5b106806ea2f57b3fe983eb6e2fc8635ab38bdf6529879d` | `d0159dbf6fd10622a5b106806ea2f57b3fe983eb6e2fc8635ab38bdf6529879d` |
| `src/system_settings.rs` | 2551 / 2551 | `a9d22571c2405f610d44b85d86fbcf0356f7cc6aeeffad269a534b42eeffbdd9` | `a9d22571c2405f610d44b85d86fbcf0356f7cc6aeeffad269a534b42eeffbdd9` |
| `src/util.rs` | 5638 / 5638 | `e9bdd79c80149f7d60d2a35f4e86398343762a6c2549fd67de1de4de9ab65147` | `e9bdd79c80149f7d60d2a35f4e86398343762a6c2549fd67de1de4de9ab65147` |
| `src/vsync.rs` | 4038 / 4038 | `01dd8a23fdf57877b34ca674e7d17b2a98bf2e578d2250b9c04efd23f3163cde` | `01dd8a23fdf57877b34ca674e7d17b2a98bf2e578d2250b9c04efd23f3163cde` |
| `src/window.rs` | 61588 / 62182 | `79eecdc48f88fb53dca9a619c7fdf7d46214238a6f453f683597a70f880e2247` | `f6d18e50ebe27219be329a6b691ac15eebb34c6ac88091804197b576b9dc4e69` |
| `src/wrapper.rs` | 940 / 940 | `a17bfb94697904d650a0db4007880262256d06bf96cb00af387e73fde7536c8b` | `a17bfb94697904d650a0db4007880262256d06bf96cb00af387e73fde7536c8b` |
