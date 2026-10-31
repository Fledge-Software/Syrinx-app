# Dependency audit

Dependencies enter Syrinx only after their maintenance, security record and
licence have been checked (`AGENTS.md`). Record the exact package graph and
features: a permissive top-level crate does not establish its dependencies'
licences, and a lockfile can contain packages that this configuration never
compiles. Repeat the audit when versions or enabled features change.

## P1-01: reading Cargo metadata, 2026-10-10

The architecture check needs Cargo's structured metadata so dependency
aliases and target/build/development declarations cannot hide a forbidden
dependency. Use `serde_json::Value` in `xtask` to read that JSON, rather than
writing a JSON parser or adding a larger Cargo-specific library. This is a
development-tool dependency; `syrinx-app` has no third-party dependency yet.

The selected direct pin is `serde_json =1.0.151`, default features disabled,
with only `std` enabled. A read-only audit inspected the cached manifests,
licence files, index entries and build scripts before this dependency was
added. The selected versions were not marked yanked. The package's
[releases](https://github.com/serde-rs/json/releases) and the upstream
manifests below show current maintenance. The offline lockfile resolution
and Windows `cargo tree` output matched the five active packages below.

| Active package | Version | Enabled features | Allowed licence selected | Upstream manifest |
|---|---|---|---|---|
| serde_json | 1.0.151 | std | MIT | [serde_json](https://raw.githubusercontent.com/serde-rs/json/master/Cargo.toml) |
| serde_core | 1.0.229 | std | MIT | [serde_core](https://raw.githubusercontent.com/serde-rs/serde/master/serde_core/Cargo.toml) |
| itoa | 1.0.18 | None | MIT | [itoa](https://raw.githubusercontent.com/dtolnay/itoa/master/Cargo.toml) |
| memchr | 2.8.3 | std, alloc | MIT | [memchr](https://raw.githubusercontent.com/BurntSushi/memchr/master/Cargo.toml) |
| zmij | 1.0.23 | None | MIT | [zmij](https://raw.githubusercontent.com/dtolnay/zmij/master/Cargo.toml) |

The [RustSec package index](https://rustsec.org/packages/) had no published
entry for these five packages on the review date. This is a scoped advisory
check, not a guarantee that no vulnerability exists. The inspected active
build scripts check compiler/target properties and, for `serde_core`, write
generated content under Cargo's output directory; no network operation was
found. The scaffold resolves and builds offline from the existing cache.

Serde uses `cfg(any())` dependencies to keep otherwise inactive serde/derive
versions aligned. Empty `any()` is false ([Rust conditional compilation](https://doc.rust-lang.org/reference/conditional-compilation.html#predicate)),
so these entries may appear in `Cargo.lock` without entering this build.
Unicode-3.0 appears in that inactive derive closure; this scaffold does not
enable, compile or vendor it. Its presence does not approve future use.
Activating derive or another route to that code requires a fresh licence
review. Future Cargo commands can fetch inactive packages; the lockfile is
not a claim that every listed crate was used or audited as active code.

Verify with:

```powershell
cargo tree --workspace --target x86_64-pc-windows-msvc --edges normal,build,features --offline --locked
```

GPUI, wry, WebView2 bindings and other future crates are not added by this
slice. Their existing spike pins stay unchanged and need their own audit
before entering the product workspace.

## P1-03: persistence and Windows paths, 2026-10-10

Storage must reject registered sync roots and unsuitable filesystem paths
before opening SQLite (D13). The Windows discovery code belongs in
`syrinx-platform`, composed with the store by the application; it must not
make `syrinx-store` depend on a shell crate. The design and its limits are
in [`docs/storage-path-preflight.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/storage-path-preflight.md). This record establishes the dependency
graph, not successful compilation or runtime verification of that guard.

The owner explicitly approved the additional Unicode-3.0 terms for
`unicode-ident 1.0.26`, which compiles through the already-pinned
`windows =0.62.2` bindings' procedural-macro dependencies. Its licence is
`(MIT OR Apache-2.0) AND Unicode-3.0`; it is not merely dual-licensed MIT or
Apache. This narrow allowance and the completed audit below permit this
exact closure. P1-01's account of its then-inactive dependencies remains
historical. The allowance does not choose Syrinx's product licence or permit
unrelated licences.

**Zlib is approved for `foldhash 0.2.0`.** The owner explicitly approved
this additional licence for the statement-cache dependency chain on
2026-10-10. The allowance is limited to that package and version; it does
not choose Syrinx's product licence or permit unrelated licences. SQLite's
bundled C source is [public domain](https://www.sqlite.org/copyright.html)
under the already-approved bundled choice, not covered by the Rust binding's
MIT licence.

A download-only audit manifest under ignored `target/p1-03-audit-20261010/`
resolved these exact direct pins and fetched their published sources:

```toml
rusqlite = { version = "=0.40.2", default-features = false, features = ["bundled", "cache"] }
rusqlite_migration = { version = "=2.6.0", default-features = false }
windows = { version = "=0.62.2", default-features = false, features = ["Storage_Provider", "Win32_Foundation", "Win32_Security", "Win32_Storage_FileSystem", "Win32_System_Com", "Win32_System_WinRT", "Win32_UI_Shell", "Win32_System_WindowsProgramming"] }
```

The audit project is outside the product workspace and must not be built.
Product integration also checks the resulting product lockfile and active
graph. The original isolated audit made no claim that existing dependency
versions have remained unchanged.

The `windows` default `std` feature is disabled. `Storage_Provider` also
enables `Storage` and `Foundation`; Win32 namespaces enable their parent
namespaces. No GPUI, wry, WebView2 or CloudFilters code enters this graph.
The procedural macros still compile with these narrow features, so the
Unicode licence allowance is necessary.

The following 31 registry packages are the actual resolved graph, including
host build dependencies. All published archives' SHA-256 values matched
both their registry-index checksums and the audit lockfile. Every exact
index record read after fetching reported `yanked = false`. Published
manifests, licence files or source licence notices, and build scripts were
inspected. The newer `cc`, `smallvec` and `hashlink` versions below supersede
the initial cached candidates.

| Package | Resolved version | Published licence | Role or enabled feature |
|---|---|---|---|
| rusqlite | 0.40.2 | MIT | `bundled`, `cache`, `modern_sqlite`, `hashlink`. |
| rusqlite_migration | 2.6.0 | Apache-2.0 | No default features; uses rusqlite and log. |
| libsqlite3-sys | 0.38.2 | MIT; bundled SQLite is public domain | Bundled C source and bindings; default minimum-version feature enables pkg-config and vcpkg helpers. |
| bitflags | 2.13.2 | MIT OR Apache-2.0 | Rusqlite flags; no optional dependencies. |
| smallvec | 1.16.3 | MIT OR Apache-2.0 | Rusqlite collections; no optional dependencies. |
| fallible-iterator | 0.3.0 | MIT OR Apache-2.0 | Default `alloc` feature. |
| fallible-streaming-iterator | 0.1.9 | MIT OR Apache-2.0 | Rusqlite iteration; no additional dependencies. |
| hashlink | 0.12.2 | MIT OR Apache-2.0 | Statement cache; hashbrown with defaults disabled and `default-hasher` enabled. |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | `default-hasher` enables foldhash. |
| foldhash | 0.2.0 | Zlib — approved for this package and version | No default features; cache hashing. |
| log | 0.4.34 | MIT OR Apache-2.0 | Migration logging facade; no optional dependencies or logger backend. |
| cc | 1.7.0 | MIT OR Apache-2.0 | Native compiler helper; `parallel` disabled. |
| find-msvc-tools | 0.1.14 | MIT OR Apache-2.0 | Finds the installed Microsoft compiler. |
| shlex | 2.0.1 | MIT OR Apache-2.0 | Compiler argument parsing with `std`. |
| pkg-config | 0.3.34 | MIT OR Apache-2.0 | SQLite build helper. |
| vcpkg | 0.2.15 | MIT OR Apache-2.0 | SQLite build helper. |
| windows | 0.62.2 | MIT OR Apache-2.0 | Namespace features listed above; defaults disabled. |
| windows-collections | 0.3.2 | MIT OR Apache-2.0 | WinRT collection types. |
| windows-core | 0.62.2 | MIT OR Apache-2.0 | Windows interface support and procedural macros. |
| windows-future | 0.3.2 | MIT OR Apache-2.0 | Windows asynchronous-interface support. |
| windows-numerics | 0.3.1 | MIT OR Apache-2.0 | Dependency of the Windows binding. |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | Windows result types. |
| windows-strings | 0.5.1 | MIT OR Apache-2.0 | Windows string types. |
| windows-threading | 0.2.1 | MIT OR Apache-2.0 | Dependency of windows-future. |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | Windows import declarations. |
| windows-implement | 0.60.2 | MIT OR Apache-2.0 | Procedural macro. |
| windows-interface | 0.59.3 | MIT OR Apache-2.0 | Procedural macro. |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | Procedural macro tokens; `proc-macro` enabled through syn. |
| quote | 1.0.47 | MIT OR Apache-2.0 | Procedural macro output; `proc-macro` enabled through syn. |
| syn | 2.0.119 | MIT OR Apache-2.0 | `clone-impls`, `full`, `parsing`, `printing`, `proc-macro`. |
| unicode-ident | 1.0.26 | (MIT OR Apache-2.0) AND Unicode-3.0 — approved | Identifier tables used by the Windows macro dependencies. |

The graph contains no bindgen, SQLCipher, OpenSSL, WASM binding, migration
directory loader, hashbrown allocator-api2/equivalent/serde/rayon, or cc
parallel dependencies. These would need a new audit if enabled later.
The selected features and graph were verified without compilation:

```powershell
cargo tree --manifest-path target/p1-03-audit-20261010/Cargo.toml --target x86_64-pc-windows-msvc --edges normal,build,features --offline --locked
```

All required sources are now cached and this command succeeds offline.
The product workspace was then resolved offline and its active graph checked
with the same target and feature-edge options for `syrinx-store` and
`syrinx-platform`. Its lockfile adds 28 registry packages and two workspace
crates; every pre-existing package/version pair from `eb32545` remains.
`cargo xtask check-deps` passes. These are dependency-resolution and boundary
checks, not a passing implementation gate.

### Maintenance, advisories and build scripts

The [rusqlite 0.40.2 release](https://github.com/rusqlite/rusqlite/releases/tag/v0.40.2)
and [migration releases](https://github.com/cljoly/rusqlite_migration/releases)
show current maintenance. Migration 2.6.0 requires Rust 1.95 and accepts
rusqlite 0.40; the installed Rust 1.97.1 satisfies that requirement.
[Smallvec 1.16.3](https://github.com/servo/rust-smallvec/releases/tag/v1.16.3)
and [hashlink 0.12.2](https://github.com/djc/hashlink/releases/tag/v0.12.2)
are recent maintained releases. The [CC release history](https://github.com/rust-lang/cc-rs/releases)
also shows continuing maintenance; its exact 1.7.0 web release page was
unavailable, so the published package and registry record supplied the
version-specific evidence. The Windows bindings remain on the existing
0.62.2 pin; [upstream releases](https://github.com/microsoft/windows-rs/releases)
show the project is maintained.

The reviewed [RustSec package index](https://rustsec.org/packages/) contained
advisories for six packages in this graph. Their selected versions are
outside the published affected ranges:

- Rusqlite's [2020-0014](https://rustsec.org/advisories/RUSTSEC-2020-0014.html)
  and [2021-0128](https://rustsec.org/advisories/RUSTSEC-2021-0128.html) fixes
  are included in 0.40.2.
- Libsqlite3-sys 0.38.2 is beyond the >=0.25.1 fix for
  [2022-0090](https://rustsec.org/advisories/RUSTSEC-2022-0090.html).
- Hashbrown 0.17.1 is beyond the >=0.15.1 fix for
  [2024-0402](https://rustsec.org/advisories/RUSTSEC-2024-0402.html).
- Smallvec 1.16.3 is beyond all five fix ranges on its
  [advisory page](https://rustsec.org/packages/smallvec.html).
- Shlex 2.0.1 is beyond the >=1.3.0 fix for
  [2024-0006](https://rustsec.org/advisories/RUSTSEC-2024-0006.html).
- Windows 0.62.2 is beyond the >=0.32.0 fix for
  [2022-0008](https://rustsec.org/advisories/RUSTSEC-2022-0008.html).

The remaining packages had no entry in that index on the review date.
This is a scoped advisory check, not a guarantee that no defect exists.

Four packages in this graph have active build scripts:

- `libsqlite3-sys` copies the included generated bindings and compiles the
  bundled `sqlite3.c` through `cc`. It needs the installed C compiler, not
  bindgen or a downloaded SQLite installation. Its pkg-config and vcpkg
  helpers are compiled, but ordinary bundled execution does not locate an
  external SQLite through them.
- `rusqlite_migration` reads its packaged README and writes a Markdown file
  for rustdoc below Cargo's `OUT_DIR`. It invokes no external command.
- `proc-macro2` probes the local compiler and creates then removes probe
  output below `OUT_DIR`.
- `quote` queries the local compiler version.

No network operation was found in these scripts. The three older manifests
without an explicit `build` field (the fallible iterator crates and vcpkg)
also contain no implicit `build.rs`. Before compiling, check that
`LIBSQLITE3_SYS_USE_PKG_CONFIG` is absent or `0`; another value overrides the
bundled route. Unexpected `LIBSQLITE3_FLAGS` overrides also need inspection.
The pre-build environment check found no unexpected overrides for those
variables or `SQLITE3_LIB_DIR`, `SQLITE3_INCLUDE_DIR` and `SQLITE3_STATIC`.
See the [SQLite binding build script](https://raw.githubusercontent.com/rusqlite/rusqlite/v0.40.2/libsqlite3-sys/build.rs)
and [migration build script](https://raw.githubusercontent.com/cljoly/rusqlite_migration/v2.6.0/rusqlite_migration/build.rs).

### Bundled SQLite boundary

The inspected published header identifies SQLite **3.53.2**, not the newest
SQLite release. Later 3.53 patches improve handling of damaged database,
WAL and journal inputs. A June 23 change also corrects classification of
extended DOS-device paths as UNC paths when choosing shared-memory locks;
Mozilla's linked report describes the Windows performance regression.
These are bounded upstream findings, not evidence that Syrinx has exercised
or fixed every affected case.
[SQLite patch timeline](https://sqlite.org/src/timeline?from=version-3.53.0&to=version-3.53.4&to2=branch-3.53&y=ci),
[Windows locking performance report](https://bugzilla.mozilla.org/show_bug.cgi?id=2016438).

The P1-03 guard retains the normal absolute DOS spelling when the database
and its longest sidecar fit the Windows legacy limit. Runtime testing found
that longer normal paths fail with SQLite code 14 on this machine. The guard
therefore supplies an extended DOS spelling when needed, and all store
connections select SQLite's existing locking `win32-longpath` file-access
implementation. This also raises SQLite's own pathname buffer limit;
selecting it alone does not remove the native Windows limit. No custom
filesystem, unlocked VFS, feature override or Windows setting is involved.
[SQLite's standard Windows VFS implementations](https://www.sqlite.org/vfs.html#standard_windows_vfses).

The necessary extended spelling retains the pinned version's known locking
performance caveat on long paths. Neither spelling provides the later
damaged-input fixes. Revisit these upstream limits before P1-04 recovery.
Keep those maintenance limits visible, use only disposable synthetic
databases for implementation tests, and do not silently substitute a
different SQLite build. Product lockfile review passed as described above.
The final P1-03 `cargo xtask check` also passed on 2026-10-10: 77 tests,
Clippy, formatting, dependency boundaries and the console launch. Independent
implementation review passed. These runtime results include long-path and
sidecar-boundary persistence, not a benchmark of the known locking slowdown
or coverage of every later SQLite damaged-input fix.

### P1-04 approved SQLite maintenance patch

Recovery deliberately handles damaged input, so the owner approved a narrow
SQLite 3.53.4 source patch on 2026-10-10 before that work proceeds. The
earlier 3.53.2 discussion above records P1-03's historical build. Syrinx now
uses `vendor/libsqlite3-sys-0.38.2` through Cargo's path patch: the same
audited Rust wrapper and build script, with four full files from exact
upstream revision `901f9946efdaaa289e6b1c5bd56dc67f4b651e51`. It includes
the reviewed journal, damaged-page and Windows path-classification fixes.

`docs/dev/vendor-reviews/p1-04-sqlite-maintenance-review.md` records the defects, alternatives,
approval scope and verification. `vendor/libsqlite3-sys-0.38.2.provenance.md`
records the 17-file inventory, omissions, licences and all hashes. Thirteen
files match the published archive byte for byte; four replacements identify
3.53.4, and the C source matches SQLite's independently published SHA3-256.
The dormant SQLCipher source is omitted. No new feature or licence category
is enabled, and the owner-approved upstream-comment exception applies only
inside this dependency directory.

All package versions, active Windows features and dependency edges are
unchanged. Cargo.lock only removes libsqlite3-sys's registry source/checksum
when selecting the local path. The vendor package is excluded from workspace
membership; the complete product gate remains unchanged. No SQLite build
environment override was present.

The 2026-10-10 `cargo xtask check` passed all 78 tests, including a new
runtime version/source-ID assertion and the existing native long-path cases,
plus formatting, Clippy, dependency checks and console launch. This proves
the patched engine builds and preserves existing behavior. It does not claim
full P1-04 recovery tests or a new locking-performance measurement. Replace
the path patch only through a reviewed, owner-approved maintenance change
to an appropriate published dependency.

## P1-05: the in-memory matcher, 2026-10-10

The palette must search locally on each keystroke (D9), without rebuilding
its catalogue or sending the query elsewhere. Frizbee supplies the fuzzy
matching score; `syrinx-search` remains responsible for catalogue updates,
frecency, the open-tab boost, scope counts and the final result limit.
Upstream's matcher benchmarks do not establish Syrinx's complete 5 ms budget
at 100,000 rows.

The selected pin makes the memory-read policy explicit:

```toml
frizbee = { version = "=0.13.0", default-features = false, features = ["std", "safe_read"] }
```

No frizbee archive, source or index entry was found in the existing Cargo
cache. The published archive was downloaded only for inspection under
ignored `target/dependency-audit/frizbee-0.13.0/`; that inspection did not
build it or change the product lockfile. Its SHA-256 is
`19435f276a44d90570cb403d22fbe4b20c80eabc134f718d6ec0ae888eb47edc`,
matching the [crates.io version record](https://crates.io/api/v1/crates/frizbee/0.13.0).
That record identifies 0.13.0 as non-yanked, published on 2026-08-13, with
Rust 1.89.0 as its minimum. The installed Rust 1.97.1 satisfies it. The
archive's `.cargo_vcs_info.json` identifies upstream commit
`d608923f526c57c7ac90b79c0cb1b8619b449179`.

| Active package | Version | Enabled features | Allowed licence | Dependency closure |
|---|---|---|---|---|
| frizbee | 0.13.0 | `std`, `safe_read` | MIT | No active normal or build dependencies. |

The published [manifest](https://docs.rs/crate/frizbee/0.13.0/source/Cargo.toml)
declares no build script. Its only normal dependency is optional serde,
which stays disabled; `std` forwards to serde only when serde is already
enabled. The package's dev dependencies are for its own tests and
benchmarks and do not enter Syrinx by depending on the library. The archive's
MIT licence grants use with its copyright and licence notice retained. No
new licence exception is required for the selected configuration.

**Product resolution is verified.** The product fetch and lockfile review
add `syrinx-search` and frizbee 0.13.0 with the audited checksum; every
pre-existing package version remains unchanged. The active Windows graph
contains only `syrinx-core` and frizbee with `std` and `safe_read` beneath
`syrinx-search`. This was checked offline after fetching:

```powershell
cargo tree -p syrinx-search --target x86_64-pc-windows-msvc --edges normal,build,features --offline --locked
```

Cargo.lock also records frizbee's optional serde edge and serde's derive
edge, using packages already present in the lockfile. Neither enters the
active search graph; their appearance does not enable serde or derive for
frizbee. These are resolution and feature checks, not a passing compilation
or implementation gate. Enabling serde, bindings or other features requires
a fresh review of their active closure.

### Maintenance and security evidence

The [upstream repository](https://github.com/saghen/frizbee) is not archived
and its latest push was 2026-09-18. The
[release history](https://github.com/saghen/frizbee/blob/main/CHANGELOG.md)
records recent matching, Unicode and arithmetic fixes. The newest commit
titled ["fix windows build"](https://github.com/saghen/frizbee/commit/5fa0fa4cb05ad64178af8f21038e8ae135419438)
changes the C binding's packaging script, not the Rust library used here.
Do not replace the published pin with unreleased main to pick up that title.

On the review date, the complete, nontruncated
[RustSec database tree](https://api.github.com/repos/RustSec/advisory-db/git/trees/main?recursive=1)
contained no `crates/frizbee/` entries, and the
[upstream published security-advisory list](https://github.com/saghen/frizbee/security/advisories)
was empty. These are scoped checks for published advisories, not a guarantee
that no vulnerability or correctness defect exists.

The [upstream safety documentation](https://github.com/saghen/frizbee#safety)
describes intentional reads beyond a haystack's allocation when a SIMD
load stays within a memory page. `safe_read` disables that optimization;
the published `src/prefilter/algo/load.rs` returns false from
`can_overread` with this feature. Other unsafe SIMD code remains. Upstream
reports a performance cost without AVX-512, so Syrinx's measurements must
include this feature rather than quoting the faster default configuration.

### API behavior the implementation must preserve

The published [matcher source](https://docs.rs/crate/frizbee/0.13.0/source/src/matcher/mod.rs)
accepts borrowed lists and returns every match; it is not a persistent
catalogue and has no Rust top-K parameter. `SortStrategy::IndexAsc` avoids
sorting by the unmodified fuzzy score before Syrinx applies its own ranking.
Count all matching records before limiting results, then calculate match
positions only for the finalists. Query changes rebuild the compiled
matcher when needed; incremental record updates belong to Syrinx. Scores
are `u16`, and input-row indexes are `u32`; excessively large input lists
panic rather than returning a structured error.

The [Unicode API tests](https://docs.rs/crate/frizbee/0.13.0/source/tests/api_properties.rs)
confirm that match positions are reverse-ordered UTF-8 byte indexes,
including continuation bytes. For example, `éx` against `é😀x` returns
`[6, 1, 0]`. Syrinx must produce valid byte ranges in the original displayed
field rather than treating those values as character or UTF-16 indexes.
They do not identify complete grapheme clusters.

`Matcher::new` treats query text literally; `from_query` additionally parses
operators and whitespace-separated patterns. That distinction is visible
in the [pattern source](https://docs.rs/crate/frizbee/0.13.0/source/src/pattern.rs).
The default is smart-case matching. The default Unicode mode selects its
Unicode path only for non-ASCII queries. Frizbee provides no normalization,
accent folding or complete Unicode case folding; case changes with a
different byte length or multiple code points are excluded. Record and
test the chosen behavior rather than promising language-independent matches.
[Unicode limitations](https://github.com/saghen/frizbee#limitations).

The [scoring implementation](https://docs.rs/crate/frizbee/0.13.0/source/src/smith_waterman/algo/mod.rs)
uses a greedy fallback when the haystack window remaining after prefiltering
exceeds 1,024 bytes. It does not guarantee optimal alignment for every long
title. Very long queries can also saturate scores and increase matrix
allocation. Preserve valid zero-score matches, test long inputs and
Unicode highlights, and measure complete ranking, counts and finalist
highlights with synthetic data before calling P1-05 complete.

## Approved wry close-event patch — 2026-10-10

Page-requested closure must save Syrinx's logical tab state before releasing
the native window. The owner approved the two-file wry 0.57.0 correction in
`docs/dev/vendor-reviews/page-close-event-review.md` and the scoped exception for unchanged
upstream comments and lint annotations. `vendor/wry-0.57.0.provenance.md`
records the checksum-verified published archive and all 84 baseline/final
file hashes; only `src/lib.rs` and `src/webview2/mod.rs` differ. The existing
Apache-2.0/MIT licence files remain byte-for-byte unchanged.

The workspace excludes that directory and selects it through a path patch.
Comparison with the immediately preceding lockfile confirms that only wry's
registry source and checksum lines were removed. No version, feature or
dependency edge changed. The exact dependency pin remains `=0.57.0`.
`vendor/wry-0.57.0.window-close-requested.patch` is the prepared local
upstream diff; it has not been sent or published. The separate frizbee and
COM findings remain outside this approval. Runtime verification is recorded
in the close-event report, not inferred from source provenance.

## Approved frizbee padding correction — 2026-10-10

The owner approved the exact three-file frizbee 0.13.0 correction and a narrow
exception for unchanged upstream comments and lint annotations. Zero-filled
SIMD padding must not become a match or an out-of-range highlight for a NUL
query. The workspace now selects `vendor/frizbee-0.13.0` through a path patch,
with the exact `=0.13.0` pin, MIT licence, `std` and `safe_read` unchanged.
No new dependency or feature is required by this correction.

Independent archive/provenance review verifies all 76 packaged files: exactly
`src/smith_waterman/algo/ascii.rs`, `mod.rs` and `unicode.rs` differ, and the
other 73 files remain byte-identical. The complete inventory and final hashes
are in `vendor/frizbee-0.13.0.provenance.md`; the adjacent `.padding.patch`
reproduces the exact approved diff. This is a package-specific authorization,
not permission to patch another dependency or publish an upstream message.

Root's edit record identifies the lockfile change as removal of only
frizbee's registry source/checksum lines; every subsequent Cargo invocation
uses `--locked`, and the gate's dependency check passes. No immediate pre-patch
lockfile snapshot was retained, so this is not an independent whole-lock
before/after comparison. The large preceding uncommitted dependency graph
is preserved; do not mistake its diff from HEAD for changes from this patch.

Five public regressions and all eight available scalar/SIMD backend variants
pass. The private backend harness uses a separately hashed ignored overlay
and adds no test dependency; it does not modify vendor or disable Syrinx tests.
All 1,040 unchanged benchmark samples preserve their exact checksums and
counts, but only 9/26 timing cases pass D48. The initial full gate exposed one
fixture that expected the corrected bug; its test-only replacement preserves
explicit invalid-range rejection, and all workspace tests now pass. Restricted-context smoke fails; the
unchanged normal-desktop full gate subsequently passes. Full logs,
hashes, source scope and limitations are recorded in
`docs/dev/vendor-reviews/p1-05-frizbee-maintenance-review.md`.

### Full test pass and restricted-context smoke failure

`target/continuation-frizbee-final-gate.log` (SHA-256
`d897444e3aafd474f20251e7739f27e7912f437cac9e6fe19896b6fd6a6a72ce`)
passes dependency checks,
formatting, workspace Clippy and all 341 workspace tests, including all 147
search tests and five public matcher regressions. No tests are ignored or
filtered, and all documentation-test stages pass. The reviewed fixture
correction preserves strict invalid-range validation; both original NUL
regressions now pass.

The gate rebuilds `target/local/syrinx-app.exe` in 18.05 seconds, SHA-256
`6edfe4113ccc7b0d5995faccaf7cb23e66541af147712443eb1b17bd9adddf1c`, then fails
its restricted-context smoke run at the 30-second watchdog. A subsequent
`Chrome_WidgetWin_0` unregister error reports 1411. This is a failed gate,
not a passing application check. The unchanged complete gate subsequently
passes on the normal desktop in `target/continuation-frizbee-desktop-gate.log`,
ending with `Local gate passed.` Its 341 tests, including 147 search and five
public matcher regressions, have zero failed, ignored or filtered cases;
documentation-test stages and application smoke also pass. The log SHA-256 is
`043845edb8374f2141a6d69935494a473bc51bd216c21969523dee5db6891c5f`.
The binary hash is unchanged. No security setting or test was relaxed, and no
continuation job remains active.
The original WebView2 crash and remaining interactive acceptance stay open.

## P1-38: update verification through Windows CNG, 2026-10-12

Updates must be checked against a pinned key before anything is installed
(`docs/update-format.md`). The verifier needs SHA-256 and ECDSA P-256
signature verification. It uses Windows CNG (`bcrypt.dll`, part of Windows)
through the `windows =0.62.2` bindings that P1-03 already audited, instead of
adding a Rust cryptography library. The rejected alternative, RustCrypto's
`p256`/`sha2` or `ring`, would add a new package closure (and, for `ring`,
assembly and a build script) for functions the operating system already
provides and Microsoft maintains.

The new crate `crates/syrinx-update` declares:

```toml
windows = { workspace = true, features = ["Win32_Foundation", "Win32_Security_Cryptography"] }
```

`Win32_Security_Cryptography` enables `Win32_Security`, `Win32` and
`Win32_Foundation`; `syrinx-platform` already enables the last three. Cargo
features are not recorded in `Cargo.lock`, and these namespace features gate
only generated bindings inside the `windows` package; they activate no
optional dependency. The lockfile diff is exactly the new workspace member's
own seven-line entry (`syrinx-update 0.1.0`, depending on `windows 0.62.2`).
No package was added, removed or moved. Verify with:

```powershell
cargo tree -p syrinx-update --target x86_64-pc-windows-msvc --edges normal,build,features --offline --locked
```

Its output lists only packages already in the P1-03 table above, at the same
versions: `windows`, `windows-collections`, `windows-core`, `windows-future`,
`windows-numerics`, `windows-result`, `windows-strings`, `windows-threading`,
`windows-link`, `windows-implement`, `windows-interface`, `proc-macro2`,
`quote`, `syn` and `unicode-ident 1.0.26` (under its approved Unicode-3.0
exception). The licences, maintenance and advisory findings recorded there
are unchanged by enabling a namespace feature. Because Cargo unifies
features, `syrinx-platform` now also compiles the cryptography bindings; it
does not call them.

The dependency check (`crates/xtask/src/dependency_policy.rs`) still allows
only `syrinx-platform` to use WebView2, wry and `windows-sys`; `syrinx-update`
is the single other crate allowed `windows`, and every workspace crate was
refused a dependency on `syrinx-update` until P1-39 was approved (since
P1-39b, `syrinx-app` alone may depend on it).
`syrinx-update` may request only the `windows` features `Win32_Foundation`
and `Win32_Security_Cryptography`, with default features disabled; any other
feature, including a networking namespace, fails the check. The
staging-directory payload uses only `std::fs` with Windows open flags, so it
needs no further feature.

This limits what `syrinx-update` declares, not what it can call. Cargo
merges the features every workspace crate requests for the same package,
so `syrinx-update` is compiled against a `windows` build that also has
`syrinx-platform`'s namespaces enabled, and code in it could call those APIs
without the check noticing. The check stops the verifier from asking for a
networking or other namespace itself; review of its source is what keeps it
to CNG.

## P1-39b: `syrinx-app` uses the update verifier, 2026-10-14

No package was added and no feature changed. `syrinx-app` now declares the
workspace's own `syrinx-update` (P1-39's approved `check-deps` change 1), so
the only lockfile change is one line in `syrinx-app`'s dependency list.
`syrinx-update` gained a second executable, `syrinx-apply`, and an install
module; both use only `std` and the existing CNG bindings, so its `windows`
features stay `Win32_Foundation` and `Win32_Security_Cryptography`.
`syrinx-platform`'s new free-space query (`GetDiskFreeSpaceExW`) and folder
picker (`FOS_PICKFOLDERS`) are in namespaces it already enables
(`Win32_Storage_FileSystem`, `Win32_UI_Shell`). `check-deps` refuses every
crate other than `syrinx-app` depending on `syrinx-update`, and any
dependency on it that enables `release-tool`.

## P1-39c: WinHTTP in `syrinx-update-net`, 2026-10-14

The owner approved one network exception to D9 on 2026-10-13: the
owner-pressed update check, through WinHTTP (`winhttp.dll`, part of
Windows) and the `windows =0.62.2` bindings P1-03 audited. `reqwest` with
`rustls` was rejected so no TLS stack or `tokio` becomes Syrinx code
(`docs/update-delivery.md`). The new crate
`crates/syrinx-update-net` declares:

```toml
syrinx-update = { path = "../syrinx-update" }
windows = { workspace = true, features = ["Win32_Networking_WinHttp"] }
```

`Win32_Networking_WinHttp` enables only `Win32_Networking` and `Win32`
inside the `windows` package and activates no optional dependency. The
lockfile diff is the new member's entry (`syrinx-update-net 0.1.0`,
depending on `syrinx-update` and `windows 0.62.2`) and one line in
`syrinx-app`'s dependency list. No package was added, removed or moved, so
the licences, maintenance and advisory findings above are unchanged.

Cargo unifies features, so every crate that uses `windows` now compiles
WinHTTP's bindings too. `check-deps` limits who declares the feature
(`syrinx-update-net` alone, with exactly that feature list, no
`[features]` table of its own, and only `syrinx-app` depending on it), and
`crates/xtask/src/network_sources.rs` reads every other crate's source for
`Networking::WinHttp`, `WinHttpOpen` and `winhttp.dll`, so another crate
cannot call it unnoticed. The `xtask` tooling, which never ships, is
excluded because its checks name those strings.

## P1-25: `serde_json` in `syrinx-platform`, 2026-10-12

P1-25 parses the JSON that WebView2 returns for the one approved DevTools
call, `DOMSnapshot.captureSnapshot`. Writing a JSON parser for an
attacker-influenced, multi-megabyte response would be riskier than reusing
the parser the workspace already audited, so `syrinx-platform` now declares
`serde_json.workspace = true`: the P1-01 pin `=1.0.151`, default features
off, `std` only. The approval proposal names this route
(`docs/page-text-indexing.md`, "Recommendation").

No package enters the build. `serde_json 1.0.151` was already compiled into
`syrinx-app` through GPUI (`schemars` requests its `alloc` feature), as
`cargo tree -p syrinx-app -i serde_json` showed before the change, and its
five-package closure is the one audited in the P1-01 section above. The only
`Cargo.lock` change is the new `serde_json` line in `syrinx-platform`'s own
dependency list. `serde` derive is not enabled; the parser reads
`serde_json::Value`, so the inactive derive closure and its Unicode-3.0
licence remain out of the build.

## Branding: one `windows` namespace feature, 2026-10-13

`syrinx-platform` enables `Win32_System_LibraryLoader` on the existing
`windows =0.62.2` pin, for `GetModuleHandleW` in `src/app_icon.rs` (loading
the executable's own icon). GPUI already enables that feature on the same
`windows` version, so feature unification compiled it before; no package,
version, licence or `Cargo.lock` line changes. The icon itself is compiled by
the Windows SDK's `rc.exe` from `crates/syrinx-app/build.rs`, which adds no
Rust dependency (owner decision 2026-10-13; `embed-resource` was rejected).

## #27: the wasmtime 48.0.7 pre-build audit, 2026-10-24

The plugin host that #28 builds adds `wasmtime =48.0.7`, and as #27 lists
it `wasmtime-wasi =48.0.7`, to `crates/syrinx-plugin-host` only. Because that
graph adds about fifty crates and a licence the owner approved by name
(`Apache-2.0 WITH LLVM-exception`, decision 2 of 2026-10-21), its audit has
its own file, `docs/dev/wasmtime-audit.md`, in the shape of
`docs/dev/fulltext-audit.md`: the exact pin and resolved features, every
transitive crate with its licence read from the published archive, the
advisory check against the RustSec database, the maintenance record of each
new crate, and which crates are new to this lockfile. It was done under #27
in a throwaway project outside the repository, so nothing in this workspace
changes until #28 commits the pin; #28 reruns its `cargo tree` commands
against the real lockfile and updates that file where they disagree.

With `wasmtime-wasi` the pin fails this workspace's D9 network gate
(`io-extras 0.19.0` asks `windows-sys 0.60.2` for WinSock), so whether
`wasmtime-wasi` stays is an open owner decision; the audit's "Windows
networking (D9, DW-68)" section and
`docs/dev/vendor-reviews/wasmtime-wit-prebuild-check.md`, "Owner decides",
have the evidence and the options.

## Linux platform layer, 2026-10-31

The owner made Syrinx one app for Windows, Linux and macOS on 2026-10-31
(`docs/decisions.md`). Building `syrinx-platform` on Linux adds the GTK 3 and
WebKitGTK bindings and GPUI's X11 backend, all as Linux-only target
dependencies: nothing below is compiled for Windows. `Cargo.lock` gains 60
packages and loses none.

**Direct dependencies of `syrinx-platform` on Linux**, each pinned exactly:
`gtk =0.18.2`, `gdkx11 =0.18.2`, `webkit2gtk =2.0.2` (feature `v2_38`),
`x11-dl =2.21.0`, and `libc =0.2.190` on every Unix. All are MIT. The
gtk-rs 0.18 family is the release wry 0.57 itself builds against, so the
versions match what the pinned wry would have chosen. wry is now a
Windows-only dependency: its GTK backend injects a `window.ipc` script into
every page, which the page rules forbid (`docs/dev/trapstoknow.md`, "Linux").

**`syrinx-update` on Unix** verifies signatures with `ring =0.17.14`
(Apache-2.0 AND ISC), already in the lockfile before this change, in place
of Windows CNG.

**Licences of the 60 added packages.** MIT: atk, atk-sys, cairo-rs,
cairo-sys-rs, filedescriptor, gdk, gdk-pixbuf, gdk-pixbuf-sys, gdk-sys,
gdkx11, gdkx11-sys, gio, gio-sys, glib, glib-macros, glib-sys, gobject-sys,
gtk, gtk-sys, gtk3-macros, is-docker, is-wsl, javascriptcore-rs,
javascriptcore-rs-sys, open, pango, pango-sys, soup3, soup3-sys,
version-compare, webkit2gtk, webkit2gtk-sys, winnow 0.5.40, x11-dl, xcursor,
xim-ctext, xim-parser, xkbcommon, zed-xim. MIT OR Apache-2.0: ahash 0.8.12,
as-raw-xcb-connection, cfg-expr, const-random, const-random-macro,
field-offset, hashbrown 0.14.5, proc-macro-crate 1.3.1 and 2.0.0,
proc-macro-error, proc-macro-error-attr, syn 1.0.109, system-deps,
toml_edit 0.19.15 and 0.20.7, x11rb, x11rb-protocol. Apache-2.0: gethostname.
MIT OR Apache-2.0 OR Zlib: xkeysym (used under MIT).

**Approved narrow licence exception (owner, 2026-10-31).** Two packages are
outside the MIT/Apache-2.0/BSD list, and both run only while compiling;
neither is linked into Syrinx:

- `target-lexicon 0.12.16`, Apache-2.0 WITH LLVM-exception (the exception
  only adds permissions), reached through `system-deps` and `cfg-expr` in
  the build scripts of every GTK `-sys` crate.
- `tiny-keccak 2.0.2`, CC0-1.0 (a public-domain dedication), reached through
  `const-random-macro`, a proc-macro behind `ahash`'s `compile-time-rng`
  feature, which GPUI's X11 input-method crate `zed-xim` enables.

The approval covers exactly these versions; a different version, or the
same licence on another package, needs its own approval.

**D9 network features (owner, 2026-10-31).** `cargo metadata` unifies
features across every target, so two Linux-only crates show up asking for
winapi's socket modules for their own Windows builds, which Syrinx never
compiles: `gio-sys 0.18.1` (`winsock2`, `ws2def`) and `filedescriptor
0.8.3` (`winsock2`). Both are added to `THIRD_PARTY_NETWORK_FEATURES` in
`crates/xtask/src/dependency_policy/resolved_windows.rs`, pinned to those
versions, on the same reasoning as `zbus` (DW-68).

