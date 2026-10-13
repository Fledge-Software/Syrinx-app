# Local development on Windows 11

Syrinx's first scaffold supplies the build and architecture checks (P1-01);
P1-02 adds the tab and saved-page model without a window. Work on this machine,
in the local repository, one approved slice at a time. The planning baseline
is `d79fc29`; P1-01 was merged into `claude/plan-atlas-v1` as `ed19d6d`, and
P1-02 as `892d3bc` from `codex/p1-02-tab-model`. D41 permits local branches and
independent review, with the owner's approval before each merge. Product
commits still require the owner's request. There is no remote or push.

## What exists

- `Cargo.toml` defines the virtual workspace, resolver 3, shared package
  settings/lints and the `local` executable profile.
- `crates/syrinx-app` now contains D49's integrated preview window, session
  worker and Ctrl K palette. The owner approved bringing this integration
  forward; it does not mark the remaining slices complete. Its native page
  host lives in `syrinx-platform`. See `docs/integrated-browser-preview.md`
  for the implementation and validation status.
- `crates/xtask` runs the gate and dependency-boundary check. Add product
  crates when their approved slices begin; there are no placeholder Plugin
  APIs or copied spike implementations.
- `crates/syrinx-core` owns live tab identities, selection and ordering, a
  20-entry closed-web-tab history, and saved-page identities that survive
  closing their live page. Its one third-party dependency is `url` (owner
  approval 2026-10-12, DW-1), for the shared web-address rule. The sidebar and
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
cargo syrinx
cargo xtask check
```

The aliases in `.cargo/config.toml` use the `local` profile, the committed
lockfile and offline dependency resolution. Existing cached packages suffice
on this machine. If a required package or toolchain is missing, report it;
do not silently download a toolchain or change a Windows security setting.

`cargo xtask check` stops at the first failure and runs:

```powershell
cargo xtask check-deps
cargo xtask check-deferred
cargo xtask check-packaging
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --profile local --offline --locked -- -D warnings
cargo test --workspace --profile local --offline --locked
cargo run --package syrinx-app --profile local --offline --locked -- --smoke
```

The first line shows the standalone equivalent of the check that `xtask`
runs internally. The test command retains Cargo's default target selection,
including documentation tests when library crates arrive; `--all-targets`
would omit those tests. The final application launch verifies that the executable
actually runs: `--smoke` opens a GPUI window and a WebView2 `about:blank`
page in a fresh temporary profile, then closes the native workers and store.
It returns failure on startup errors or timeout. The generated directory is
reserved with exclusive creation before startup; an existing path is refused.
A separate watchdog bounds this synthetic test at 30 seconds even if GPUI
stalls or orderly shutdown never finishes. Normal launches have no watchdog.
The watchdog exits with code 124 on its deadline or 125 if its control channel
disconnects. It performs no output before exiting, so a locked stderr stream
cannot prevent termination. A subprocess regression verifies both exits while
holding stderr locked; other smoke failures retain their existing diagnostics.
Normal `cargo syrinx` uses
`%LOCALAPPDATA%\SyrinxProductPreview`, separate from the Phase 0 profiles;
its State and WebView2 subfolders persist between launches. Before the
product was named Syrinx (2026-10-12) this folder was
`%LOCALAPPDATA%\AtlasProductPreview`; Syrinx never reads, moves or deletes
it, or the installed `%LOCALAPPDATA%\Atlas\data`, and starts fresh. The old
folders may hold signed-in WebView2 profiles: the owner deletes them by hand
when they are no longer needed (see `docs/p1-37-installer-design.md`, "Where
things live", for old registrations). Compiling alone
does not establish D38. Fix formatting with
`cargo fmt --all`. A passing gate is required before review and merge, and
it does not replace inspecting a window once a slice introduces UI.

## Why the profile is explicit

Smart App Control blocked unsigned debug builds during Phase 0. D38 keeps
the Windows policy unchanged and uses an optimized profile with line-table
debug information. The workspace's `local` profile inherits `release` but
explicitly keeps debug assertions and overflow checks enabled for the gate.
Its build override also sets optimization level 3 for build scripts and
their dependencies: Cargo otherwise leaves those helpers unoptimized even
under a release profile. Assertions and overflow checks still inherit `local`.
Use it for application runs, tests and Clippy. Plain `cargo test` chooses a
different profile and is not the gate on this machine.

This is a locally tested route, not a guarantee that Windows will accept
every future executable. If execution is blocked, preserve the error and
bring it to the owner; never change Smart App Control, SmartScreen or signing
settings to get past it. Child Cargo commands also set `RUSTUP_AUTO_INSTALL=0`
for that process, without changing the user's persistent environment.

The owner requires local Syrinx to run unsigned and has no signing certificate.
A certificate is not an application requirement. `xtask.exe` is a separate
development tool that runs the quality checks listed above; "optimized" means
compiled with the release-style `local` profile. It is not the browser.
On 2026-10-10, signature inspection reported `NotSigned` for
`target/local/atlas-app.exe`, and launching that exact file printed
`Atlas 0.1.0` (its name then) and exited successfully. This proves the current console
scaffold starts unsigned, not that the later browser window is implemented.

During P1-05, Windows blocked `target/local/xtask.exe` and the store unit-test
executable even on unchanged retries. Syrinx-specific Code Integrity event
3077 identifies policy `VerifiedAndReputableDesktop` and status `0xc0e90002`.
Microsoft explains that Smart App Control can accept unsigned code it judges
safe and block unsigned code it cannot confidently classify; optimization
does not grant an exemption. See the
[Microsoft FAQ](https://support.microsoft.com/en-us/windows/security/threat-malware-protection/smart-app-control-frequently-asked-questions).
Preserve those failures, keep development unsigned and verify each changed
app build. Do not claim a complete gate while tests remain blocked.

On 2026-10-10, P1-01's full `cargo xtask check` passed: dependency boundaries,
formatting, Clippy, all seven architecture tests and the application launch
(`Syrinx 0.1.0`). Immediately after an earlier rebuild, Windows had blocked
`xtask.exe` with error 4551. Retrying the unchanged executable passed without
rebuilding it or changing settings. The cause of that temporary block remains
unproved; the successful run does not establish a policy exemption.

## What the architecture check protects

`cargo xtask check-deps` reads Cargo metadata with format version 1 and
`--no-deps --all-features`. It checks the actual package name, so a renamed
import cannot hide GPUI or WebView2. Normal, optional, target-specific,
development and build dependency declarations all count.

- Only `syrinx-app` and `syrinx-ui` may directly depend on GPUI crates.
- Only `syrinx-platform` may directly depend on wry, WebView2 bindings or
  the checked `windows`/`windows-sys` Win32 bindings.
- `syrinx-core`, `syrinx-store`, `syrinx-search`, `syrinx-plugin-api` and
  `plugin-*` must not reach shell crates through other workspace crates.
- A plugin's only Syrinx dependency is `syrinx-plugin-api`, and that API
  must not depend on other Syrinx crates. Fledge services stay behind Host.

This is a manifest architecture check, not a runtime sandbox or complete
third-party dependency audit. Internal routes are matched by normalized
workspace package names; third-party packages are leaves. It permits
`syrinx-app -> syrinx-platform -> wry`, and does not mistake GPUI's own Windows
dependencies for a Syrinx boundary violation. Before introducing ambiguous
same-name internal/external packages, extend the matcher to resolve their
paths. Dependency audit details are in `docs/dependency-audit.md`.

The architecture tests use synthetic Cargo metadata to cover allowed shell
composition, aliased and inactive forbidden imports, internal facade/cycle
handling, plugin/API isolation and malformed metadata. Core and search tests
use made-up titles and reserved domains.

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
cargo test --package syrinx-core --profile local --offline --locked
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

P1-03 adds `syrinx-store` and the filesystem part of `syrinx-platform`. The
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
cargo test --package syrinx-store --package syrinx-platform --profile local --offline --locked
```

Native integration tests prepare fresh disposable directories. They query
registered sync-root metadata without printing provider names or paths;
they never discover or open the owner's Syrinx database. Synthetic junctions
and hardlinks stay inside test-owned directories, and their target bytes are
checked before cleanup. No Windows settings are changed.

On 2026-10-10, P1-03 passed `cargo xtask check`: dependency boundaries,
formatting, Clippy, all 77 tests (30 core, 18 native guard, 9 native/SQLite
integration, 13 store and 7 architecture), documentation-test selection and
the console application launch. Independent reviews passed after correcting
repeated Windows discovery on ordinary reads/saves, startup validation order
and long-path compatibility. The latter tests include both native and SQLite
buffer limits and the journal-suffix boundary. No GUI or production data was
opened. The owner then requested the commit and approved the local merge;
P1-03 is committed and merged into `claude/plan-atlas-v1` as `c4a6631`.

## P1-04 SQLite prerequisite

The owner approved a pinned SQLite 3.53.4 maintenance patch before backup
recovery handles damaged files. `Cargo.toml` selects the local
`vendor/libsqlite3-sys-0.38.2` package; Rust dependency versions and features
remain unchanged. Keep the vendor files byte-identical to their recorded
hashes. Its provenance and omitted optional sources are documented in
`vendor/libsqlite3-sys-0.38.2.provenance.md` and the rationale is in
`docs/p1-04-sqlite-maintenance-review.md`.

The full `cargo xtask check` passed with this patch on 2026-10-10: 78 tests,
Clippy, formatting, dependency boundaries and the console launch. The added
engine-identity test checks the version and source ID of the actual linked
SQLite library. This is a verified prerequisite, not completion of P1-04.
The 78-test result above applies to the maintenance prerequisite alone; the
complete 138-test result is recorded below. No production database has been opened.

## P1-04 storage lifecycle and maintenance

`WindowsDirectoryLease` now holds an exclusive `syrinx.lifecycle` control
file. A second process or a second store using the same lease cannot start
an overlapping session. The store flushes running status before opening
SQLite. Explicit shutdown marks clean only after both readers close and the
writer finishes; fallback destruction leaves unclean evidence. Missing or
malformed evidence causes conservative checking of an existing database.
An `syrinx.recovery-intent` entry prevents normal startup from creating a
missing primary database after an interrupted restoration.

A lifecycle write or flush failure is returned to the caller and poisons
that control against reuse. A failed final flush can leave readable cached
bytes; it provides no guarantee about what survives a later process restart.
The clean record is attempted only after the database connections have
already closed successfully. No Windows durability setting is changed.

`syrinx-store::deletion` defines D47's content-free journal format and replay
contract. Records contain a category, half-open Unix-second range and
generation, with damage detection and paired request/completion records.
Pending clears, missing required generations and malformed records refuse
replay. Diagnostic output redacts the ranges. This module performs no file
I/O or history collection. P1-27 persists it as `syrinx.deletions` beside the
database, with schema 5's applied generation as the barrier, and connects the
clearing and replay participants; see `docs/p1-27-clear-data.md`.

`Store::backup_if_due(utc_seconds, idle)` is a blocking host-worker call on
the existing writer queue. It takes a daily `VACUUM INTO` snapshot only when
idle and due. Complete read-only validation precedes publication; the exact
new candidate must validate again before seven-copy rotation. Failed output
and unknown entries remain preserved. A failed attempt remains due in the
current session; restart discovers valid completion evidence again. Routine
not-due calls use a cached timestamp and perform no archive enumeration.

`syrinx_platform::offer_recovery(directory, deletions)` returns a dated offer
for the newest verified backup when the primary is missing or demonstrably
damaged. The UI must obtain the owner's confirmation before calling
`RecoveryOffer::confirm`. An offer retains exclusive storage ownership; drop
it to cancel. Restoration preserves the old primary and sidecars locally,
stages and validates a copy, and never replaces an unexpected destination.
Known interrupted moves yield a renewed offer for the same source. Damaged
intent or changed identities remain refused rather than guessed away.
Since P1-27, recovery replays the deletion journal onto the staged copy before
installing it, and refuses when the journal cannot cover a backup's clears.

On 2026-10-10, P1-04 passed the complete `cargo xtask check`: dependency
boundaries, formatting, Clippy, **138 tests**, documentation-test selection
and console launch. The tests comprise 30 core, 40 native unit, 4 native
backup integration, 18 native storage integration, 38 store and one linked
engine identity test, plus seven architecture tests. No tests were ignored
or filtered. Independent source reviews passed for the SQLite patch, store
ordering, native backup/recovery and deletion contract. Final documentation
review also passed, including agreement between both 40-row issue maps. The application
remains the console scaffold until its approved UI slices; no production
database or profile was opened. The owner explicitly approved P1-04's commit
and local merge on 2026-10-10; Plan 1's ledger records their completion.

## P1-05 local catalogue search

`syrinx-search` gives the later palette a derived in-memory catalogue. Rust
callers provide typed tab, saved-page, history and command records; the crate
collects no history and performs no I/O. Queries return ranked, highlighted
results and uncapped scope counts. Saved destinations and live pages remain
distinct, preserving D43 when the running page navigates elsewhere. See
`crates/syrinx-search/README.md` for the input policy and matching limits.

For focused development and the separate performance gate, use:

```powershell
cargo test --package syrinx-search --profile local --offline --locked
cargo xtask bench > target/p1-05-search-run.json
```

Choose a fresh output name for every benchmark run so failures remain
available. `bench` prints a JSON report containing every synthetic sample,
per-case median, 95th percentile and maximum, and the overall maximum. It
returns failure if any case's 95th percentile reaches 5 ms (D48). Its 100,000
records exercise the complete matching, ranking, counting and highlighting
path. Do not run it alongside a build. Keep the audited `frizbee =0.13.0`
pin and `safe_read` feature; `docs/p1-05-search.md` records the protocol and
verification status. The full local gate remains required separately.

Current P1-05 validation is partial: formatting and the full workspace Clippy
command passed. Across the workspace attempt and focused xtask run, 132 tests
passed: 30 core, 40 native unit, 4 backup integration, 18 storage integration,
26 search and 14 xtask. Windows blocked the store unit-test executable before
its 38 tests ran; the subsequent engine-identity test and workspace
documentation-test stage were not reached. The console launch passed unsigned.
The complete quality gate and D48 performance gate remain unresolved;
`docs/p1-05-search.md` records both failures and the separate CPU diagnostic.

## Palette acceptance (P1-33)

`cargo xtask palette-acceptance` is on demand and not part of the gate. It
builds the `local` app first, serves two synthetic pages on an ephemeral
`127.0.0.1` port from the xtask process, and launches Syrinx visibly with
`--test-palette-acceptance <report.json> http://127.0.0.1:<port>` on a fresh
synthetic profile. Syrinx then times palette opening and per-keystroke ranking
at 1,000, 10,000 and 100,000 in-memory synthetic rows, checks overlay focus and
stacking, and closes itself. Reports go to `target\p1-33\` as JSON and a
Markdown summary. Run it in the foreground on a quiet machine and leave the
keyboard and mouse alone: focus checks need Syrinx to be the active app. Method
and results: `docs/p1-33-palette-acceptance.md`.

## Packaging (P1-37)

```powershell
cargo xtask package
```

builds `syrinx-app` with the `local` profile and its `installed` feature
(data in `%LOCALAPPDATA%\Syrinx\data`) into the separate
`target\package-build` directory, so `target\local\syrinx.exe` stays the
development build using `SyrinxProductPreview`. It writes a versioned folder
and zip under `target\package`, with a manifest and the owner-run scripts
from `packaging/`. It installs and registers nothing. The gate's
`cargo xtask check-packaging` step refuses a `packaging/syrinx-registration.ps1`
that differs from the Rust registry table (regenerate it with
`cargo xtask write-registration`) and parses every script without running
it. Agents never run the packaging scripts; installation, registration and
choosing the default browser are the owner's. See
`docs/p1-37-installer-design.md`.

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
