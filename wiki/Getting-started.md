# Getting started

Syrinx is a Windows desktop application with no server and no deployment, so
everything is built, run and tested on a Windows 11 machine. v1 targets
Windows 11 only (decision D2); there are no macOS or Linux code paths yet.

## What you need

- **Windows 11.**
- **The Rust toolchain for MSVC**, version 1.97 or newer. MSVC is Microsoft's
  C and C++ compiler and linker; Rust's `x86_64-pc-windows-msvc` target uses
  it.
- **The Windows SDK.** Building needs its resource compiler (`rc.exe`, which
  embeds the app icon) and its shader compiler (`fxc.exe`, because Syrinx's
  patched GPUI embeds precompiled shaders in every build).
- **The Evergreen WebView2 runtime**, which ships with Windows 11. Syrinx
  never downloads or installs it itself.

## The three commands

From a checkout of the repository:

```powershell
cargo syrinx              # run Syrinx
cargo xtask check         # the gate: dependencies, format, Clippy, tests, smoke run
cargo xtask package       # the setup program and portable zip under target\package
```

`cargo xtask check` is **the gate**: the single command that must pass before
any change is called done. It checks the dependency rules between crates,
formatting, Clippy with warnings denied, every test, and a short smoke run of
the application under the optimized `local` build profile (D38). A passing
gate does not prove that something you can see looks right, so after
changing anything the owner sees, run the app and look at the window.

## Where the details are

- [`docs/dev/LOCAL-DEV.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/dev/LOCAL-DEV.md)
  is the authority on how to build, run and test on this machine: the exact
  commands, profiles, test data folders and what the gate runs. It is not
  repeated here, so it cannot go out of date in two places.
- [`docs/dev/trapstoknow.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/dev/trapstoknow.md)
  lists the mistakes that look fine, often pass the gate, and fail somewhere
  else: WebView2 and GPUI quirks, Windows Application Control blocking
  locally built programs, line endings, and more. Read it before you debug a
  build, a test or a window.
- [`AGENTS.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/AGENTS.md)
  holds the rules for changing the code, including the guardrails that keep
  Google sign-in working and the owner's data private.
- [How we track work](How-we-track-work) explains how to pick up an issue and
  get a change merged.

Never use real personal data while developing: tests and fixtures use
generated values on reserved domains such as `example.com` and `.test`.
