# Local development on Windows 11

Atlas's first scaffold supplies the build and architecture checks (P1-01);
P1-02 adds the tab and saved-page model without a window. Work on this machine,
in the local repository, one approved slice at a time. The planning baseline
is `d79fc29`; P1-01 was merged into `claude/plan-atlas-v1` as `ed19d6d`, and
P1-02 as `892d3bc` from `codex/p1-02-tab-model`. D41 permits local branches and
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
- `crates/atlas-core` owns live tab identities, selection and ordering, a
  20-entry closed-web-tab history, and saved-page identities that survive
  closing their live page. It has no third-party dependency. The sidebar and
  palette will consume its shared row projection; this slice adds no UI,
  database, folder manager or importer.

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

The architecture tests use synthetic Cargo metadata to cover allowed shell
composition, aliased and inactive forbidden imports, internal facade/cycle
handling, plugin/API isolation and malformed metadata. Core model tests use
made-up titles and reserved domains; search tests arrive with P1-05.

## The tab and saved-page model

A saved destination and a running page have different lifetimes (D43).
`SavedPageId` identifies a saved row; `TabId` identifies one live instance.
Closing a saved page removes that instance while leaving its saved row.
Opening it again uses a fresh live identity, so delayed events from the
closed page cannot change the replacement. Opening an already-open saved
page focuses its existing instance instead of duplicating it.

Saving captures the destination's title/address. Navigation changes only
live metadata. Unpinning removes the saved record and keeps an open page as
an ordinary tab. The model's sidebar projection presents saved rows, including
unopened ones, followed by ordinary live tabs. Folders and HTML import will
extend this same model in P1-21/P1-22; they do not create a second bookmark list.

The 20-entry reopen history is in memory only. An ordinary tab restores its
closed metadata; a still-saved page uses its saved destination and existing
row. Reopening history must never recreate a saved record the owner removed.
Content-bearing debug output is redacted, and errors carry structural values
rather than titles, addresses or page contents.

For focused model development, use:

```powershell
cargo test --package atlas-core --profile local --offline --locked
```

Run the full gate before calling the slice done. The library contains no
plugin lifecycle, window or web-view code; its complete tab-kind enum leaves
room for those later approved slices.

On 2026-10-10, P1-02 passed `cargo xtask check`: dependency boundaries,
formatting, Clippy, four library tests, twenty integration tests, the seven
architecture tests and the console application launch. The model tests cover
saved/live lifetimes, ordering, history, stale identities, atomic failures,
redacted diagnostics and a 600-operation invariant sequence. No GUI behavior
or persistence is claimed by this result.

## Durable saved pages and session records

P1-03 adds `atlas-store` and the filesystem part of `atlas-platform`. The
console application still does not open a production database. The later
application composition will obtain a `WindowsDirectoryLease` from the
platform and pass it to `Store::open`; the store never imports Windows APIs.
`docs/storage-path-preflight.md` explains the location checks and their limits.

`Store` is a blocking interface for host worker threads, not the future UI
event thread. One writer thread owns all changes; two separately locked
readers observe committed snapshots. `load` returns a snapshot and revision.
`save` accepts that expected revision and rejects stale writes, so a delayed
caller cannot replace newer state. Call `shutdown` to observe close errors.

The initial migration stores flat saved rows, live tabs, their associations,
selection and allocation watermarks together. Saved and live metadata stay
separate, preserving D43's fixed destination while its open page navigates.
Identities and counters use eight-byte values to retain their full unsigned
range. The closed-tab history remains in memory. Daily backups and recovery
are P1-04; folders, history and browser restoration integration remain in
their approved later slices.

For focused storage development, use:

```powershell
cargo test --package atlas-store --package atlas-platform --profile local --offline --locked
```

Native integration tests prepare fresh disposable directories. They query
registered sync-root metadata without printing provider names or paths;
they never discover or open the owner's Atlas database. Synthetic junctions
and hardlinks stay inside test-owned directories, and their target bytes are
checked before cleanup. No Windows settings are changed.

On 2026-10-10, P1-03 passed `cargo xtask check`: dependency boundaries,
formatting, Clippy, all 77 tests (30 core, 18 native guard, 9 native/SQLite
integration, 13 store and 7 architecture), documentation-test selection and
the console application launch. Independent reviews passed after correcting
repeated Windows discovery on ordinary reads/saves, startup validation order
and long-path compatibility. The latter tests include both native and SQLite
buffer limits and the journal-suffix boundary. No GUI or production data was
opened; P1-03 still awaits the owner's commit and merge approval.

## Data and artifacts

The scaffold opens no database, browser profile, account, note vault or
project folder. It collects no analytics and makes no application network
calls. Tests use synthetic dependency metadata and disposable tab/store data.
Cargo output stays under the ignored `target/` directory; `Cargo.lock` is
intentionally versioned.

Later product data belongs in local app data, outside synced folders (D13).
Page text and its index expire after seven days and stay outside daily
durable-state backups (D40). The local analytics proposal is a separate
later slice; it does not authorize collection while building the scaffold.
