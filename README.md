# Syrinx

Syrinx is a browser-first workspace for Windows 11: a web browser with a
vertical-tab sidebar and one command palette, where code (Forge), notes (Notes)
and business tools (Fledge) will open as tabs beside your pages. Pages render
in Microsoft Edge WebView2, driven from Rust; Syrinx's own interface is drawn
in Rust with GPUI.

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

Syrinx builds only on Windows 11 with the MSVC Rust toolchain (1.97 or
newer) and the Windows SDK. From a checkout:

```powershell
cargo syrinx              # run Syrinx
cargo xtask check         # the gate: dependencies, format, Clippy, tests, smoke run
cargo xtask package       # the setup program and portable zip under target\package
```

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
