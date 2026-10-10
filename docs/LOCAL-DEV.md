# Local development on Windows 11

Atlas's first scaffold makes the build and architecture checks runnable
before the tab model or browser shell exists (P1-01). Work on this machine,
in the local repository, one approved slice at a time. The baseline is
`d79fc29`; P1-01 uses `codex/p1-01-scaffold`. D41 permits local branches and
independent review, with the owner's approval before each merge. Product
commits still require the owner's request. There is no remote or push.

## What exists

- `Cargo.toml` defines the virtual workspace, resolver 3, shared package
  settings/lints and the `local` executable profile.
- `crates/atlas-app` is the runnable entry point. It currently prints its
  version and exits; the first GUI sample belongs to P1-07, and the browser
  shell begins in P1-08.
- `crates/xtask` runs the gate and dependency-boundary check. Add product
  crates when their approved slices begin; there are no placeholder Plugin
  APIs or copied spike implementations.

Use the installed Windows MSVC Rust toolchain. This scaffold was developed
with Rust/Cargo 1.97.1 and the `x86_64-pc-windows-msvc` target. Node 24.15.0
and pnpm 10.34.4 are available, but this slice does not use them. It adds no
toolchain downloader, system installation, registry setting or CI service.
The product licence is still undecided; Cargo publishing is disabled.

## Run and check

Run these from the repository in PowerShell:

```powershell
cargo atlas
cargo xtask check
```

The aliases in `.cargo/config.toml` use the `local` profile, the committed
lockfile and offline dependency resolution. Existing cached packages suffice
on this machine. If a required package or toolchain is missing, report it;
do not silently download a toolchain or change a Windows security setting.

`cargo xtask check` stops at the first failure and runs:

```powershell
cargo xtask check-deps
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --profile local --offline --locked -- -D warnings
cargo test --workspace --profile local --offline --locked
cargo run --package atlas-app --profile local --offline --locked
```

The first line shows the standalone equivalent of the check that `xtask`
runs internally. The test command retains Cargo's default target selection,
including documentation tests when library crates arrive; `--all-targets`
would omit those tests. The final application launch verifies that the executable
actually runs; compiling alone does not establish D38. Fix formatting with
`cargo fmt --all`. A passing gate is required before review and merge, and
it does not replace inspecting a window once a slice introduces UI.

## Why the profile is explicit

Smart App Control blocked unsigned debug builds during Phase 0. D38 keeps
the Windows policy unchanged and uses an optimized profile with line-table
debug information. The workspace's `local` profile inherits `release` but
explicitly keeps debug assertions and overflow checks enabled for the gate.
Use it for application runs, tests and Clippy. Plain `cargo test` chooses a
different profile and is not the gate on this machine.

This is a locally tested route, not a guarantee that Windows will accept
every future executable. If execution is blocked, preserve the error and
bring it to the owner; never change Smart App Control, SmartScreen or signing
settings to get past it. Child Cargo commands also set `RUSTUP_AUTO_INSTALL=0`
for that process, without changing the user's persistent environment.

On 2026-10-10, P1-01's full `cargo xtask check` passed: dependency boundaries,
formatting, Clippy, all seven architecture tests and the application launch
(`Atlas 0.1.0`). Immediately after an earlier rebuild, Windows had blocked
`xtask.exe` with error 4551. Retrying the unchanged executable passed without
rebuilding it or changing settings. The cause of that temporary block remains
unproved; the successful run does not establish a policy exemption.

## What the architecture check protects

`cargo xtask check-deps` reads Cargo metadata with format version 1 and
`--no-deps --all-features`. It checks the actual package name, so a renamed
import cannot hide GPUI or WebView2. Normal, optional, target-specific,
development and build dependency declarations all count.

- Only `atlas-app` and `atlas-ui` may directly depend on GPUI crates.
- Only `atlas-platform` may directly depend on wry, WebView2 bindings or
  the checked `windows`/`windows-sys` Win32 bindings.
- `atlas-core`, `atlas-store`, `atlas-search`, `atlas-plugin-api` and
  `plugin-*` must not reach shell crates through other workspace crates.
- A plugin's only Atlas dependency is `atlas-plugin-api`, and that API
  must not depend on other Atlas crates. Fledge services stay behind Host.

This is a manifest architecture check, not a runtime sandbox or complete
third-party dependency audit. Internal routes are matched by normalized
workspace package names; third-party packages are leaves. It permits
`atlas-app -> atlas-platform -> wry`, and does not mistake GPUI's own Windows
dependencies for an Atlas boundary violation. Before introducing ambiguous
same-name internal/external packages, extend the matcher to resolve their
paths. Dependency audit details are in `docs/dependency-audit.md`.

The synthetic tests cover allowed shell composition, aliased and inactive
forbidden imports, internal facade/cycle handling, plugin/API isolation and
malformed metadata. Core model and search tests arrive with P1-02 and P1-05;
this scaffold does not fake those implementations to produce a test count.

## Data and artifacts

The scaffold opens no database, browser profile, account, note vault or
project folder. It collects no analytics and makes no application network
calls. Tests use synthetic dependency metadata. Cargo output stays under
the ignored `target/` directory; `Cargo.lock` is intentionally versioned.

Later product data belongs in local app data, outside synced folders (D13).
Page text and its index expire after seven days and stay outside daily
durable-state backups (D40). The local analytics proposal is a separate
later slice; it does not authorize collection while building the scaffold.
