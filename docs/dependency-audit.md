# Dependency audit

Dependencies enter Atlas only after their maintenance, security record and
licence have been checked (`AGENTS.md`). Record the exact package graph and
features: a permissive top-level crate does not establish its dependencies'
licences, and a lockfile can contain packages that this configuration never
compiles. Repeat the audit when versions or enabled features change.

## P1-01: reading Cargo metadata, 2026-10-10

The architecture check needs Cargo's structured metadata so dependency
aliases and target/build/development declarations cannot hide a forbidden
dependency. Use `serde_json::Value` in `xtask` to read that JSON, rather than
writing a JSON parser or adding a larger Cargo-specific library. This is a
development-tool dependency; `atlas-app` has no third-party dependency yet.

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
`atlas-platform`, composed with the store by the application; it must not
make `atlas-store` depend on a shell crate. The design and its limits are
in `docs/storage-path-preflight.md`. This record establishes the dependency
graph, not successful compilation or runtime verification of that guard.

The owner explicitly approved the additional Unicode-3.0 terms for
`unicode-ident 1.0.26`, which compiles through the already-pinned
`windows =0.62.2` bindings' procedural-macro dependencies. Its licence is
`(MIT OR Apache-2.0) AND Unicode-3.0`; it is not merely dual-licensed MIT or
Apache. This narrow allowance and the completed audit below permit this
exact closure. P1-01's account of its then-inactive dependencies remains
historical. The allowance does not choose Atlas's product licence or permit
unrelated licences.

**Zlib is approved for `foldhash 0.2.0`.** The owner explicitly approved
this additional licence for the statement-cache dependency chain on
2026-10-10. The allowance is limited to that package and version; it does
not choose Atlas's product licence or permit unrelated licences. SQLite's
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
with the same target and feature-edge options for `atlas-store` and
`atlas-platform`. Its lockfile adds 28 registry packages and two workspace
crates; every pre-existing package/version pair from `9a6ae35` remains.
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
These are bounded upstream findings, not evidence that Atlas has exercised
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
