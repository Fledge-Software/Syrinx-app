# Local development on Windows and Linux

This is how to build, run and check Syrinx on this machine. Work one
approved slice at a time, each on its own branch, usually in its own git
worktree (D41). Since 2026-10-13 the repository is on GitHub at
`Fledge-Software/Syrinx-app`: a branch reaches `main` through a reviewed
pull request, and the Gate workflow runs the same gate on GitHub (see
"Run and check"). Product commits still require the owner's request. The
planning baseline is `2c80273`. The sections after "What the architecture
check protects" also keep each early slice's dated gate record; those
records describe the code as it was on their date. A `DW-n` name is an
item of Plan 1's old deferred-work register, read at `af3de53`; open
deferred work now has a GitHub issue (`docs/dev/deferred-work.md`).

## What exists

- `Cargo.toml` defines the virtual workspace, resolver 3, shared package
  settings/lints and the `local` executable profile.
- `crates/syrinx-app` is the browser: its window, sidebar, page card, Home,
  palette and Settings, and the session worker that saves them. Its native
  page host lives in `syrinx-platform`. It began as D49's integrated preview
  ([`docs/integrated-browser-preview.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/integrated-browser-preview.md), at af3de53).
  The wiki's Architecture page maps every crate in the workspace.
- `crates/xtask` runs the gate and dependency-boundary check. Add product
  crates when their approved slices begin; there are no placeholder Plugin
  APIs or copied spike implementations.
- `crates/syrinx-core` owns live tab identities, selection and ordering, a
  20-entry closed-web-tab history, and saved-page identities that survive
  closing their live page. Its one third-party dependency is `url` (owner
  approval 2026-10-12, DW-1), for the shared web-address rule. The sidebar and
  palette read its shared row projection.

Use the installed Windows MSVC Rust toolchain. This scaffold was developed
with Rust/Cargo 1.97.1 and the `x86_64-pc-windows-msvc` target. Node 24.15.0
and pnpm 10.34.4 are available, but the build does not use them. The build
adds no toolchain downloader, system installation or registry setting.
The product is licensed under MPL-2.0 (`LICENSE`); Cargo publishing stays disabled.

### The Windows SDK resource compiler

`crates/syrinx-app/build.rs` compiles the hawk icon
(`design/brand/icon/syrinx.ico`) and the version details into `syrinx.exe`
with the Windows SDK's resource compiler, `rc.exe` (owner decision,
2026-10-13). The build fails with an explanation if it cannot find one,
rather than producing an executable without an icon. It looks, in order, at:

1. the `RC` environment variable, which must hold `rc.exe`'s full path;
2. `%WindowsSdkVerBinPath%\x64\rc.exe`, set by a Developer Command Prompt;
3. `rc.exe` on `PATH`;
4. the newest `C:\Program Files (x86)\Windows Kits\10\bin\<version>\x64\rc.exe`.

The Windows SDK comes with Visual Studio's "Desktop development with C++"
workload, which the MSVC Rust toolchain already needs for its linker. On this
machine the fourth route finds SDK `10.0.26100.0`. The resource script uses
no SDK headers, so no include path is needed. It is written as UTF-16 with
a byte-order mark: rc.exe reads a plain UTF-8 script in the ANSI code page,
and a checkout path with characters outside it then fails with RC2135. A
probe on 2026-10-13 compiled the icon from a folder named with accented and
CJK characters as UTF-16 and failed as UTF-8. If the SDK folder exists but
cannot be read, the build reports that error rather than treating it as a
missing SDK. The icon and brand rasters
are committed; building never runs Edge or Python (`design/brand/README.md`).

## Run and check

Run these from the repository in PowerShell (on Linux, any shell; see "On Linux" below):

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
cargo clippy --package syrinx-update --features release-tool --all-targets --profile local --offline --locked -- -D warnings
cargo test --package syrinx-update --features release-tool --profile local --offline --locked
cargo run --package syrinx-app --profile local --offline --locked -- --smoke
```

The first three lines show the standalone equivalents of the checks that
`xtask` runs internally (`crates/xtask/src/main.rs`, `check_workspace`).
The test command retains Cargo's default target selection,
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
Since P1-39c the workspace tests include `syrinx-update-net`'s untrusted
certificate check, which starts `python.exe` from the path with P1-14's
standard-library TLS fixture on 127.0.0.1 (a fresh self-signed certificate,
never installed); building still runs no Python. No gate step contacts the
update host or any machine other than this one: the update tests use an
in-process fake host and loopback servers, and the development build's
"Check for updates" refuses before any request.
Normal `cargo syrinx` uses
`%LOCALAPPDATA%\SyrinxProductPreview`, separate from the Phase 0 profiles;
its State and WebView2 subfolders persist between launches. Before the
product was named Syrinx (2026-10-12) this folder was
`%LOCALAPPDATA%\AtlasProductPreview`; Syrinx never reads, moves or deletes
it, or the installed `%LOCALAPPDATA%\Atlas\data`, and starts fresh. The old
folders may hold signed-in WebView2 profiles: the owner deletes them by hand
when they are no longer needed (see `docs/installing.md`, "Where
things live", for old registrations). Compiling alone
does not establish D38. Fix formatting with
`cargo fmt --all`. A passing gate is required before review and merge, and
it does not replace inspecting a window once a slice introduces UI.

On GitHub, `.github/workflows/gate.yml` runs the same `cargo xtask check` on
every pull request and every push to `main`, on a `windows-latest` runner
with the pinned toolchain (Rust 1.97.1) and the locked dependencies fetched
first, through `.github/scripts/run-gate.sh`, which keeps the log and, when
the gate fails, puts its end in the run summary. A separate Linux job checks
only formatting, so a formatting slip is reported within a minute or two.
The weekly release runs the gate the same way before it builds anything
(`docs/releasing.md`).

## On Linux (owner, 2026-10-31)

The same commands run from a Linux checkout:

```sh
cargo syrinx
cargo xtask check
```

You need Rust 1.97 (`rustup toolchain install 1.97.1`), WebKitGTK 4.1
(2.38 or newer), GTK 3, libsoup 3, bubblewrap (WebKit's page sandbox) and an
X11 display. On Arch-based systems such as Omarchy that is `webkit2gtk-4.1`,
`gtk3`, `libsoup3` and `bubblewrap`. Under a Wayland compositor (Hyprland,
GNOME, KDE) Syrinx runs through XWayland: GTK can embed a page only inside an
X11 window, so GPUI is built with its X11 backend alone and GDK is limited to
X11 (`syrinx-platform/src/linux/gtk_thread.rs`).

What differs from Windows:

- **Pages** render in WebKitGTK, driven directly through `webkit2gtk`
  (`syrinx-platform/src/browser_gtk/`), each embedded as an X11 child of
  Syrinx's window. The commands and events are the same protocol as the
  WebView2 host (`browser/protocol.rs`). WebKit's bubblewrap sandbox is on,
  and the engine refuses to start if `WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS`
  or an inspector-server variable is set.
- **NVIDIA's proprietary driver** draws WebKit's DMA-BUF renderer black, so
  Syrinx sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` at start-up when
  `/proc/driver/nvidia/version` exists and the owner has not set it
  (`syrinx_platform::prepare_process`).
- **Data** lives in `$XDG_DATA_HOME/SyrinxProductPreview` (normally
  `~/.local/share`), or `Syrinx/data` for the installed feature. The storage
  guard refuses network and FUSE filesystems and the folders of common sync
  clients (Dropbox, OneDrive, Nextcloud and others) in place of Windows'
  registered sync roots.
- **One running Syrinx per profile** is kept with an `flock`ed lock file and
  a mode-0600 Unix socket in `$XDG_RUNTIME_DIR`, which checks the peer's user
  id as the Windows pipe checks its server's.
- **Windows-only for now**: the setup program and packaging (`cargo xtask
  package`), release signing, update downloads, page-text capture,
  hidden-page suspension, pausing a download, the palette acceptance run, and
  the WebView2 example fixtures (they print that they need Windows). Tests
  that assert Windows file-sharing rules are `#[cfg(windows)]`; on Unix the
  guard revalidates identities instead (`docs/dev/trapstoknow.md`, "Linux").
- **Known limits**: no Widevine in WebKitGTK, so Disney+ and Hulu do not
  play; Google may treat WebKitGTK as an unfamiliar browser at sign-in, and
  the user agent is still never changed (D22). The toolbar icons use Windows'
  Segoe MDL2 Assets font, which Linux does not have, so they show as wrong
  glyphs until Syrinx ships its own icons.

The Linux gate skips the release tool's two steps (it signs through Windows
key storage) and parses no PowerShell; the Windows gate still runs both.

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
`target/local/syrinx-app.exe`, and launching that exact file printed
`Atlas 0.1.0` (its name then) and exited successfully. This proved that the
console scaffold of that date started unsigned, not that the later browser
window was implemented.

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

`cargo xtask check-deps` reads Cargo metadata with format version 1,
`--all-features --offline --locked`, including the resolved dependency graph.
It checks the actual package name, so a renamed import cannot hide GPUI or
WebView2. Normal, optional, target-specific, development and build dependency
declarations all count, and so do entries in a crate's own `[features]` table.

- Only `syrinx-app` and `syrinx-ui` may directly depend on GPUI crates.
- Only `syrinx-platform` may directly depend on wry, WebView2 bindings or
  the checked `windows`/`windows-sys` Win32 bindings.
- `syrinx-core`, `syrinx-store`, `syrinx-search`, `syrinx-plugin-api` and
  `plugin-*` must not reach shell crates through other workspace crates.
- A plugin's only Syrinx dependency is `syrinx-plugin-api`, and that API
  must not depend on other Syrinx crates. Fledge services stay behind Host.
- No Syrinx crate may declare a network client or ask `windows` or
  `windows-sys` for a networking feature (D9), except
  `Win32_Networking_WinHttp` in `syrinx-update-net`, which only
  `syrinx-app` may use (P1-39c). A Syrinx crate asking `winapi` for a
  network module is refused by the resolved-feature check below, since no
  approved entry names it. The headless crates, the update
  download included, must not reach a network client even through a
  third-party library in the resolved graph.
- Every resolved `windows`, `windows-sys` and `winapi` build may carry no
  networking feature except WinHTTP on the build `syrinx-update-net` uses
  and the third-party entries the owner approved on 2026-10-14 (DW-68,
  `THIRD_PARTY_NETWORK_FEATURES` in
  `crates/xtask/src/dependency_policy/resolved_windows.rs`), each pinned to
  the crates whose manifest activates it: a declaration whose version
  requirement matches that build, or a `[features]` entry whose own feature
  is switched on. A new requester, version, feature or namespace fails, and
  so does an approved entry that no longer matches. Networking here means
  the Win32 networking, network-management, web, URL-moniker, RPC and WinRM
  namespaces, WinRT's `Networking`, `Web` and web-authentication broker,
  and winapi's socket, HTTP, IP-helper, DNS, name-service, LAN Manager
  share and workstation, URL-moniker and RPC modules.
  WinHTTP is approved per build: a third party asking for it on the
  download's build passes, since Cargo switches it on there anyway. Cargo's
  metadata unifies features across every target platform, so this can
  report a Linux-only crate's request (zbus) as well as Windows ones.
- Syrinx's source may not name a socket binding: `crates/xtask/src/network_sources.rs`
  refuses `WinSock`, `NetworkManagement`, any `WSA`-prefixed name (as a
  token, so `AWSAccess` passes) and the `ws2_32`, `wsock32`, `mswsock` and
  `iphlpapi` libraries (any letter case, with or without `.dll`, so
  `#[link(name = "ws2_32")]` and `LoadLibraryW(w!("ws2_32"))` count) in
  every crate except xtask, and WinHTTP in every crate but
  `syrinx-update-net`. The approved third-party features compile in the
  shell crates that share those builds, so this scan, not the feature
  check, is what keeps Syrinx from calling them. A feature check cannot see
  a raw `#[link]` or `windows-link` extern at all; this scan sees one only
  when it names one of those libraries or bindings.

This is a manifest architecture check, not a runtime sandbox or complete
third-party dependency audit. Internal routes are matched by normalized
workspace package names; third-party packages are leaves. It permits
`syrinx-app -> syrinx-platform -> wry`, and does not mistake GPUI's own Windows
dependencies for a Syrinx boundary violation. Before introducing ambiguous
same-name internal/external packages, extend the matcher to resolve their
paths. Dependency audit details are in `docs/dev/dependency-audit.md`.

The architecture tests use synthetic Cargo metadata to cover allowed shell
composition, aliased and inactive forbidden imports, internal facade/cycle
handling, plugin/API isolation, resolved `windows` features with their
approved and stale entries, and malformed metadata. Core and search tests
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

P1-03 added `syrinx-store` and the filesystem part of `syrinx-platform`,
while the application was still a console scaffold. The session worker now
obtains a `WindowsDirectoryLease` from the platform at start-up and passes
it to `Store::open` (`crates/syrinx-app/src/session.rs`); the store never
imports Windows APIs.
[`docs/storage-path-preflight.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/storage-path-preflight.md) explains the location checks and their limits.

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
P1-03 is committed and merged into `claude/plan-atlas-v1` as `a30ef72`.

## P1-04 SQLite prerequisite

The owner approved a pinned SQLite 3.53.4 maintenance patch before backup
recovery handles damaged files. `Cargo.toml` selects the local
`vendor/libsqlite3-sys-0.38.2` package; Rust dependency versions and features
remain unchanged. Keep the vendor files byte-identical to their recorded
hashes. Its provenance and omitted optional sources are documented in
`vendor/libsqlite3-sys-0.38.2.provenance.md` and the rationale is in
`docs/dev/vendor-reviews/p1-04-sqlite-maintenance-review.md`.

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
clearing and replay participants; see [`docs/p1-27-clear-data.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-27-clear-data.md).
Since #121 the journal has a second category, `SiteZoom` (category byte 2),
for the zoom levels that Clear browsing data's "Cookies and site data"
removes: `Store::apply_deletions` applies every category of a request batch
in one transaction and advances the one applied generation, and the staged
replay covers the `site_zoom` table on copies of schema 7 or later. The
record layout is unchanged. A build that does not know a category refuses
the journal, as it always has; this build names such a record as written by
a newer Syrinx.

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
was then still the console scaffold; no production database or profile was
opened. The owner explicitly approved P1-04's commit
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
pin and `safe_read` feature; [`docs/p1-05-search.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-05-search.md) records the protocol and
verification status. The full local gate remains required separately.

P1-05's first validation was partial: formatting and the full workspace Clippy
command passed. Across the workspace attempt and focused xtask run, 132 tests
passed: 30 core, 40 native unit, 4 backup integration, 18 storage integration,
26 search and 14 xtask. Windows blocked the store unit-test executable before
its 38 tests ran; the subsequent engine-identity test and workspace
documentation-test stage were not reached. The console launch passed unsigned.
[`docs/p1-05-search.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-05-search.md) records both failures and the separate CPU diagnostic.
The complete gate has since passed: on 2026-10-13, after P1-05's D48 work
merged to `main`, `cargo xtask check` passed 1,050 tests and the smoke run
(Plan 1's ledger, [at af3de53](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/implementation-status.md)).
D48's performance gate is still unverified: it waits on the owner's three
quiet-machine `cargo xtask bench` runs (`docs/owner-checklist.md`, item 2).

## Palette acceptance (P1-33)

`cargo xtask palette-acceptance` is on demand and not part of the gate. It
builds the `local` app first, serves two synthetic pages on an ephemeral
`127.0.0.1` port from the xtask process, and launches Syrinx visibly with
`--test-palette-acceptance <report.json> http://127.0.0.1:<port>` on a fresh
synthetic profile. Syrinx then times palette opening and per-keystroke ranking
at 1,000, 10,000 and 100,000 in-memory synthetic rows, checks overlay focus and
stacking, clicks a sidebar peek row in-process (#10), checks that a page focus
event leaves the find field focused only while the chrome holds the keys (#57),
checks that a click in the main window closes an open palette (#172),
and closes itself. Reports go to `target\p1-33\` as JSON and a
Markdown summary. Run it in the foreground on a quiet machine and leave the
keyboard and mouse alone: focus checks need Syrinx to be the active app. Method
and results: [`docs/p1-33-palette-acceptance.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-33-palette-acceptance.md).

## Packaging (P1-37)

```powershell
cargo xtask package
```

builds `syrinx-app` with the `local` profile and its `installed` feature
(data in `%LOCALAPPDATA%\Syrinx\data`) into the separate
`target\package-build` directory, so `target\local\syrinx.exe` stays the
development build using `SyrinxProductPreview`. It writes a versioned folder
and zip under `target\package`, with a manifest and the owner-run scripts
from `packaging/`, and packs that folder into the setup program,
`<package>-setup.exe`, beside the zip (the download since 2026-10-14). It
installs and registers nothing. The gate's
`cargo xtask check-packaging` step refuses a `packaging/syrinx-registration.ps1`
that differs from the Rust registry table (regenerate it with
`cargo xtask write-registration`) and parses every script without running
it. Every icon the registration and Start menu shortcut name is
`syrinx.exe,0`: index 0 is the executable's first icon, which is the hawk
compiled in as resource 1 (its only icon).
Agents never run the packaging scripts; installation, registration and
choosing the default browser are the owner's. See
`docs/installing.md`.

Since P1-39a the package folder also holds P1-38's unsigned update
`manifest`, written by the update crate's release tool
(`cargo run --package syrinx-update --bin syrinx-update-tool --features
release-tool`), because `check-deps` still forbids xtask from linking the
verifier. The zip is built before any signing, so it contains the unsigned
`manifest` but no `manifest.sig`, `latest` or `latest.sig`. Since P1-39b the
gate's workspace Clippy and test steps are featureless, so Syrinx is checked
as it ships, and two separate `--package syrinx-update --features
release-tool` steps check the release tool; `check-deps` refuses any
dependency that enables that feature.

Since P1-39b the package also holds `syrinx-apply.exe`, the update helper,
built from `syrinx-update` without the release tool. "Install update from
file…" (Settings' Updates section and the palette) works only in an `installed` build
running from a folder named `Syrinx`; a development build says so and
touches nothing. With `PRODUCTION_KEYS` still empty (issue #55), an installed
build refuses every package as signed by an unknown key. The staging and
recovery tests write only under `target\syrinx-update-tests`. See
`docs/update-format.md`, "P1-39b".

## Signing an update (P1-39a, owner only)

```powershell
cargo xtask sign-update --package <package folder> --storage <software|smart-card> --key <key name> --serial <n> --days <1-60>
```

signs the folder's `manifest` and a fresh latest-version statement with the
owner's release key, named in Windows' software or smart-card key storage,
and writes `manifest.sig`, `latest` and `latest.sig` into the folder.
Windows may ask for the key's PIN or password; the command never reads the
private key. `--serial` must exceed the serial of any `latest` already in
the folder, and `--days` is the statement's lifetime, capped at 60. That
serial check sees only the same package folder, so the owner must track the
highest serial they have issued outside the repository; installed copies
refuse a reused serial whose statement differs, and a lower one. The
command verifies what it signed against `PRODUCTION_KEYS` before writing,
so until the owner's keys are pinned (issue #55) it refuses every key. Only the
owner runs it; agents never create, open or name a release key. Its tests
use in-memory keys only. Format and checks:
`docs/update-format.md`, "P1-39a: the latest-version statement".

## Restricted-token reproduction (#59)

`crates/syrinx-platform/examples/restricted_token/repro.ps1` reproduces
#59's `msedgewebview2.exe` breakpoint: it runs `syrinx.exe --smoke` under a
restricted copy of your token and reads the Crashpad dumps in that run's own
synthetic profile with `dumpinfo.py` beside it, so it needs `python` on
`PATH`. It is outside `cargo xtask check`, because the failure belongs to the
WebView2 runtime under a token Syrinx never gets in daily use. Run it as
`powershell -File repro.ps1 -Exe <syrinx.exe> [-Mode normal|lua|restricted]
[-DisableGpu]`. Exit 0 is clean (the profile is deleted), 1 a breakpoint
dump, 2 cannot analyse (no python, no single new profile, or an unreadable
dump) and 3 an inconclusive run (a timeout, a non-zero exit or another dump).
Every non-zero exit keeps the profile; the script deletes only the profile
its own run created. See the trap in `docs/dev/trapstoknow.md`.

## Data and artifacts

A development run opens only `%LOCALAPPDATA%\SyrinxProductPreview` or a
`--test-data-dir` folder, never the installed `%LOCALAPPDATA%\Syrinx\data`.
Syrinx makes no network call of its own: the one call is "Check for
updates", which only the owner starts, and a development build refuses it
before any request. The usage report (D39) counts locally, and only while
the owner has switched it on; never collect or export real usage while
developing. Tests use synthetic dependency metadata and disposable tab/store
data. Cargo output stays under the ignored `target/` directory; `Cargo.lock`
is intentionally versioned.

Product data belongs in local app data, outside synced folders (D13).
Page text and its index expire after seven days and stay outside daily
durable-state backups (D40).
