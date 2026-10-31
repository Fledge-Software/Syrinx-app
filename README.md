# Syrinx

Syrinx is a browser-first workspace for Windows, Linux and macOS: one web
browser with a vertical-tab sidebar and one command palette, where code
(Forge), notes (Notes) and business tools (Fledge) will open as tabs beside
your pages. Pages render in the system's web engine, driven from Rust:
Microsoft Edge WebView2 on Windows and WebKitGTK on Linux. Syrinx's own
interface is drawn in Rust with GPUI. The macOS build (WKWebView) is the next
platform to be written.

Website: [syrinxapp.dev](https://syrinxapp.dev) · Documentation:
[docs.syrinxapp.dev](https://docs.syrinxapp.dev)

## Status

Syrinx is early. The browser core (tabs, saved pages, history, session
restore, the palette and local search) is being built and used now. Forge,
Notes and Fledge are designed but not yet built. A preview build is published
every Thursday on the [releases page](https://github.com/Fledge-Software/Syrinx-app/releases);
previews are unsigned, so Windows SmartScreen warns before the first run.
`docs/releasing.md` explains how releases are made.

## Building

One workspace builds on every platform, with Rust 1.97 or newer. From a
checkout:

```sh
cargo syrinx              # run Syrinx
cargo xtask check         # the gate: dependencies, format, Clippy, tests, smoke run
```

- **Windows 11**: the MSVC toolchain and the Windows SDK. `cargo xtask
  package` builds the setup program and portable zip under `target\package`.
- **Linux**: WebKitGTK 4.1 (2.38 or newer), GTK 3, libsoup 3 and
  bubblewrap, and an X11 display; under Wayland (Hyprland, GNOME, KDE)
  Syrinx runs through XWayland. Protected video (Disney+, Hulu) does not
  play, because WebKitGTK has no Widevine.
- **macOS**: not yet; its platform layer is the next one to be written.

`docs/dev/LOCAL-DEV.md` has the details and the traps.

## Where things are

- `crates/`: the Rust workspace (app, core model, storage, search, UI, platform).
- `docs/architecture.md`: a newcomer's map of the Cargo workspace, its crates and the gate.
- `docs/spec.md`, `docs/decisions.md` and `docs/architecture-decisions.md`:
  what Syrinx is, the phased plan and the decisions made. Work is tracked in
  GitHub issues.
- `docs/dev/`: contributor material, such as how to run the gate, the traps
  and the dependency audits.
- `design/`: the design target and brand assets.
- `AGENTS.md` and `docs/dev/trapstoknow.md`: rules and known traps for anyone,
  person or agent, changing the code.

## Licence

Syrinx is licensed under the [Mozilla Public License 2.0](LICENSE). Vendored
dependencies under `vendor/` keep their own licences.
