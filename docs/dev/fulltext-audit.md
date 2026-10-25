# P1-23 full-text candidate audit

Page recall needs measured costs for deletion and seven-day expiry before Syrinx chooses a store. This audit was prepared on 2026-10-11 and continued on 2026-10-12 under the owner's explicit Tantivy approval. It covers benchmark-only use. The owner chose Tantivy on 2026-10-24 (`docs/decisions.md`); P1-24's production adapter (#53) still has to satisfy the production preconditions below.

## SQLite FTS5: existing dependency, no patch change

The workspace pins `rusqlite =0.40.2`, with defaults disabled and `bundled,cache`, and patches `libsqlite3-sys 0.38.2` to the approved local source. `vendor/libsqlite3-sys-0.38.2/sqlite3/sqlite3.h:149` identifies SQLite 3.53.4. The unchanged build script at `build.rs:159` already passes `-DSQLITE_ENABLE_FTS5`. No new feature, build flag, dependency or vendor modification is needed. The existing SQLite maintenance audit remains `docs/dev/dependency-audit.md`, P1-04; this is not an approval to alter that patch.

Use a separate disposable cache database, never Syrinx's durable store or snapshots. FTS5 prefix indexes (`prefix='2 3'`), unicode61 tokenization and `ORDER BY rank LIMIT 20` need SQL schema configuration, not compile flags. Runtime construction and version checks belong in the benchmark. D40 requires active-storage deletion and restart/replay protection, not merely a query-time filter; it does not promise forensic erasure. The decisions file has the D40 prose but no literal Q2 entry; the plan's Storage section supplies the corresponding cache/backup separation.

## Tantivy: exact candidate and approval boundary

Current pin, since 2026-10-24: `tantivy = { version = "=0.27.0", default-features = false, features = ["mmap", "lz4-compression"] }`. The move from 0.26.2 and its audit are in "Tantivy 0.27.0 move, 2026-10-24" below; the rest of this section and the inventory after it record the original 0.26.2 audit.

Originally approved benchmark pin: `tantivy = { version = "=0.26.2", default-features = false, features = ["mmap", "lz4-compression"] }`. MIT is declared in its packaged manifest and licence. Memory-mapped directories permit a disk/recovery comparison; LZ4 supplies document compression. Stemming, stopwords and Zstandard are omitted to avoid changing query semantics or adding unnecessary native compression dependencies. The main alternative is Tantivy's broader default feature set; it should not silently replace this audited configuration.

The [packaged changelog](https://docs.rs/crate/tantivy/0.26.2/source/CHANGELOG.md) records recent scorer and aggregation fixes, while [upstream releases](https://github.com/quickwit-oss/tantivy/releases) show ongoing maintenance. Its manifest requires Rust 1.86; the installed 1.97.1 meets this. Release activity is evidence of maintenance, not proof of correctness or security.

**Approved for the benchmark, with the advisory retained.** On 2026-10-12 the owner explicitly instructed this session to measure the audited candidate despite its `lru 0.16.4` advisory if no compatible fixed version was cached. The local crates.io sparse-index cache ends at non-yanked Tantivy 0.26.2; its `lru ^0.16.3` requirement excludes the fixed `>=0.18.2` range. No network refresh, ineffective `cargo update`, patch or vendoring of lru was performed.

[RUSTSEC-2026-0253](https://rustsec.org/advisories/RUSTSEC-2026-0253.html) describes use-after-free/double-free when a key destructor panics during `pop` and execution continues after catching the panic. Reinspection of cached `tantivy-0.26.2/src/store/reader.rs` found `LruCache<usize, Block>` at line 74 and get/put operations at lines 84/95. Integer keys have no panicking destructor, so the advisory's stated trigger is not reachable through this inspected store-reader path, including Syrinx's intended stored-document retrieval. This does not establish that every future use of Tantivy/lru is safe or replace a package-level advisory review. The benchmark exercises only synthetic data and is not a product dependency.

**Resolved by Tantivy 0.27.0 (2026-10-24).** Tantivy 0.27.0 requires `lru ^0.18.2`, and the lockfile resolves `lru 0.18.5`, inside the advisory's patched range `>= 0.18.2`. The owner approved the move on 2026-10-24 (`docs/decisions.md`); the details are in "Tantivy 0.27.0 move, 2026-10-24" below. The paragraph that follows is kept as the original precondition.

**P1-24 precondition if Tantivy is selected:** resolve the advisory with an audited upstream candidate or compatible fixed dependency before production integration. This session does not authorize a lru patch, suppress the finding, or approve shipping the affected version. The owner's benchmark exception supersedes the previous recommendation to withhold measurement.

`cargo audit` is not installed. Instead, the public RustSec advisory archive was downloaded and package names in the entire resolved normal/build graph were matched against its crate directories. All matching advisory version ranges were read; the lru finding above remains applicable. This is a manual scoped review, not a cargo-audit pass or a claim that no undisclosed flaw exists. Other matched advisories have patched ranges containing the resolved versions. The appendix below records the matched advisory IDs and ranges.

## Audit evidence and licences

The final approval inventory comes from the disposable workspace copy described below, not from the preliminary standalone resolution. It adds 36 registry packages without upgrading/removing existing package versions. The approved patch is now applied to the real branch lockfile. The only direct Tantivy edge is from `fulltext-bench`; no product crate depends on the benchmark or Tantivy.

The RustSec archive (`https://github.com/RustSec/advisory-db/archive/refs/heads/main.zip`) SHA-256 is `09A09A05E691D44FD3B496E569B3A36A42C28D10C2F43201166B3EBAF975BB2F`; it remains under `target/p1-23-audit`. Package names and each matching advisory's version ranges were checked for the final closure. The graph's lru version remains affected; all other matching entries have patched ranges containing their resolved versions. No automated cargo-audit pass is claimed.

MIT, Apache-2.0 or BSD-3-Clause can be selected wherever offered. `foldhash 0.2.0` uses the existing exact-version Zlib exception; `unicode-ident 1.0.26` uses the existing Unicode-3.0 exception. The LGPL alternative on `r-efi` is not selected. Legacy slash-separated expressions below are preserved as Cargo reports them. No new licence category is requested.

## Advisory range evidence

- id = "RUSTSEC-2020-0091"; package = "arc-swap"; patched = [">= 0.4.8, < 1.0.0-0", ">= 1.1.0"]; unaffected = ["< 0.4.2"]
- id = "RUSTSEC-2017-0004"; package = "base64"; patched = [">= 0.5.2"]
- id = "RUSTSEC-2020-0052"; package = "crossbeam-channel"; patched = [">= 0.4.4"]; unaffected = ["< 0.4.3"]
- id = "RUSTSEC-2022-0019"; package = "crossbeam-channel"; informational = "unsound"; patched = [">= 0.4.3"]
- id = "RUSTSEC-2025-0024"; package = "crossbeam-channel"; patched = [">= 0.5.15"]; unaffected = ["<= 0.5.11"]
- id = "RUSTSEC-2021-0093"; package = "crossbeam-deque"; patched = [">= 0.7.4, < 0.8.0", ">= 0.8.1"]
- id = "RUSTSEC-2026-0204"; package = "crossbeam-epoch"; patched = [">= 0.9.20"]; unaffected = ["< 0.9.0"]
- id = "RUSTSEC-2022-0041"; package = "crossbeam-utils"; informational = "unsound"; patched = [">= 0.8.7"]
- id = "RUSTSEC-2020-0060"; package = "futures-task"; patched = [">= 0.3.6"]; unaffected = ["<= 0.2.1"]
- id = "RUSTSEC-2020-0061"; package = "futures-task"; patched = [">= 0.3.5"]
- id = "RUSTSEC-2020-0059"; package = "futures-util"; patched = [">= 0.3.7"]; unaffected = ["< 0.3.2"]
- id = "RUSTSEC-2020-0062"; package = "futures-util"; patched = [">= 0.3.2"]; unaffected = ["< 0.3.0"]
- id = "RUSTSEC-2024-0402"; package = "hashbrown"; patched = [">= 0.15.1"]; unaffected = ["< 0.15.0"]
- id = "RUSTSEC-2023-0057"; package = "inventory"; informational = "unsound"; patched = [">= 0.2.0"]
- id = "RUSTSEC-2023-0058"; package = "inventory"; informational = "unsound"; patched = [">= 0.2.0"]
- id = "RUSTSEC-2021-0130"; package = "lru"; patched = [">= 0.7.1"]
- id = "RUSTSEC-2026-0002"; package = "lru"; informational = "unsound"; patched = [">= 0.16.3"]; unaffected = ["< 0.9.0"]
- id = "RUSTSEC-2026-0253"; package = "lru"; informational = "unsound"; patched = [">= 0.18.2"]; unaffected = []
- id = "RUSTSEC-2026-0041"; package = "lz4_flex"; patched = [">= 0.11.6, < 0.12.0", ">= 0.12.1"]
- id = "RUSTSEC-2026-0186"; package = "memmap2"; informational = "unsound"; patched = [">= 0.9.11"]
- id = "RUSTSEC-2019-0017"; package = "once_cell"; patched = [">= 1.0.1"]; unaffected = ["< 0.2.5"]
- id = "RUSTSEC-2026-0005"; package = "oneshot"; informational = "unsound"; patched = [">= 0.1.12"]; unaffected = ["<= 0.1.1"]
- id = "RUSTSEC-2020-0082"; package = "ordered-float"; patched = ["^1.1.1", ">= 2.0.1"]; unaffected = ["< 0.2.2"]
- id = "RUSTSEC-2022-0013"; package = "regex"; patched = [">= 1.5.5"]
- id = "RUSTSEC-2025-0047"; package = "slab"; patched = [">= 0.4.11"]; unaffected = ["< 0.4.10"]
- id = "RUSTSEC-2018-0003"; package = "smallvec"; patched = [">= 0.6.3", "^0.3.4", "^0.4.5", "^0.5.1"]; unaffected = ["< 0.3.2"]
- id = "RUSTSEC-2018-0018"; package = "smallvec"; informational = "unsound"; patched = [">= 0.6.13"]
- id = "RUSTSEC-2019-0009"; package = "smallvec"; patched = [">= 0.6.10"]; unaffected = ["< 0.6.5"]
- id = "RUSTSEC-2019-0012"; package = "smallvec"; patched = [">= 0.6.10"]; unaffected = ["< 0.6.3"]
- id = "RUSTSEC-2021-0003"; package = "smallvec"; patched = [">= 0.6.14, < 1.0.0", ">= 1.6.1"]; unaffected = ["< 0.6.3"]
- id = "RUSTSEC-2020-0071"; package = "time"; patched = [">= 0.2.23"]; unaffected = ["=0.2.0", "=0.2.1", "=0.2.2", "=0.2.3", "=0.2.4", "=0.2.5", "=0.2.6"]
- id = "RUSTSEC-2026-0009"; package = "time"; patched = [">= 0.3.47"]; unaffected = ["< 0.3.6"]


The expanded closure also includes bumpalo 3.20.3, which is in the patched ranges for RUSTSEC-2020-0006 (>=3.2.1) and RUSTSEC-2022-0078 (>=3.11.1).

## Workspace-copy resolution: approval inventory

A preliminary standalone resolution was followed by this definitive workspace-copy check. A `git archive` of base `b0e84b3` was expanded under `target/p1-23-workspace-audit`, and a benchmark-only package was added there with the real workspace package settings, existing rusqlite dependency and the proposed Tantivy pin/features. Only that disposable copy's lockfile was resolved. No Tantivy code was built or executed during that initial audit; the approved continuation builds it only for the benchmark.

This resolution adds 36 registry packages and upgrades/removes no existing package versions, as checked against the real branch lockfile. Shared features nevertheless expand Tantivy's visible dependency closure; `serde_json 1.0.151` and `syn 3.0.6` replace the standalone audit's newer selections. The complete workspace-copy closure is the approval inventory below. Its packages' advisory ranges were checked again: lru 0.16.4 remains the applicable finding; no additional applicable entry was found among the expanded closure. Existing workspace packages outside Tantivy's closure retain their prior audits, not a new audit claim here.

The candidate lockfile SHA-256 is `994CC838E35FE188C4C0B549F2CAFC60F0B25A75C429A8FE03A3F6ABF26A139E`. The compact, inert `crates/fulltext-bench/audit/workspace-tantivy.patch` records its difference from this branch's real lockfile. Applying it with an explicit disposable `--directory` target and `--unidiff-zero` and normalizing Git's CRLF conversion back to LF reproduced that hash. The owner subsequently approved this exact dependency configuration; this session applied the retained patch to the branch lockfile. To reconstruct the candidate in a disposable copy of this branch, add the proposed Tantivy declaration to its benchmark manifest, apply this zero-context patch with `git apply --unidiff-zero` to its copied `Cargo.lock`, then run the locked commands below. Product manifests and vendor files were unchanged in the scratch experiment.

```powershell
cargo tree --manifest-path target/p1-23-workspace-audit/Cargo.toml -p tantivy --offline --target all --edges normal,build --prefix none --format '{p} | {l}'
cargo tree --manifest-path target/p1-23-workspace-audit/Cargo.toml -p tantivy --locked --offline --target x86_64-pc-windows-msvc --edges normal,build --prefix none --format '{p} | {l}'
```

For reconstruction from the retained patch, add `--locked` to the first command too. Non-Windows entries below describe graph resolution, not non-Windows testing or product support. Select MIT, Apache-2.0 or BSD-3-Clause where offered, retaining the same two exact-package licence exceptions already noted.

| Package/version | Declared licence |
| --- | --- |
| aho-corasick v1.1.5 | Unlicense OR MIT |
| allocator-api2 v0.2.21 | MIT OR Apache-2.0 |
| arc-swap v1.9.2 | MIT OR Apache-2.0 |
| async-trait v0.1.92 | MIT OR Apache-2.0 |
| autocfg v1.5.1 | Apache-2.0 OR MIT |
| base64 v0.22.1 | MIT OR Apache-2.0 |
| bitflags v2.13.2 | MIT OR Apache-2.0 |
| bitpacking v0.9.3 | MIT |
| bon v3.10.2 | MIT OR Apache-2.0 |
| bon-macros v3.10.2 | MIT OR Apache-2.0 |
| bumpalo v3.20.3 | MIT OR Apache-2.0 |
| byteorder v1.5.0 | Unlicense OR MIT |
| census v0.4.2 | MIT |
| cfg-if v1.0.5 | MIT OR Apache-2.0 |
| crc32fast v1.5.2 | MIT OR Apache-2.0 |
| crossbeam-channel v0.5.17 | MIT OR Apache-2.0 |
| crossbeam-deque v0.8.8 | MIT OR Apache-2.0 |
| crossbeam-epoch v0.9.21 | MIT OR Apache-2.0 |
| crossbeam-utils v0.8.23 | MIT OR Apache-2.0 |
| crunchy v0.2.4 | MIT |
| darling v0.24.1 | MIT |
| darling_core v0.24.1 | MIT |
| darling_macro v0.24.1 | MIT |
| datasketches v0.2.0 | Apache-2.0 |
| deranged v0.5.8 | MIT OR Apache-2.0 |
| downcast-rs v2.0.2 | MIT OR Apache-2.0 |
| either v1.19.0 | MIT OR Apache-2.0 |
| equivalent v1.0.2 | Apache-2.0 OR MIT |
| erased-serde v0.4.10 | MIT OR Apache-2.0 |
| errno v0.3.14 | MIT OR Apache-2.0 |
| fastdivide v0.4.2 | zlib-acknowledgement OR MIT |
| fastrand v2.5.0 | Apache-2.0 OR MIT |
| fnv v1.0.7 | Apache-2.0 / MIT |
| foldhash v0.2.0 | Zlib |
| fs4 v0.13.1 | MIT OR Apache-2.0 |
| futures-channel v0.3.34 | MIT OR Apache-2.0 |
| futures-core v0.3.34 | MIT OR Apache-2.0 |
| futures-io v0.3.34 | MIT OR Apache-2.0 |
| futures-macro v0.3.34 | MIT OR Apache-2.0 |
| futures-sink v0.3.34 | MIT OR Apache-2.0 |
| futures-task v0.3.34 | MIT OR Apache-2.0 |
| futures-util v0.3.34 | MIT OR Apache-2.0 |
| getrandom v0.4.3 | MIT OR Apache-2.0 |
| hashbrown v0.16.1 | MIT OR Apache-2.0 |
| hashbrown v0.17.1 | MIT OR Apache-2.0 |
| htmlescape v0.3.1 | Apache-2.0 / MIT / MPL-2.0 |
| ident_case v1.0.1 | MIT/Apache-2.0 |
| indexmap v2.14.2 | Apache-2.0 OR MIT |
| inventory v0.3.25 | MIT OR Apache-2.0 |
| itertools v0.14.0 | MIT OR Apache-2.0 |
| itoa v1.0.18 | MIT OR Apache-2.0 |
| js-sys v0.3.106 | MIT OR Apache-2.0 |
| levenshtein_automata v0.2.1 | MIT |
| libc v0.2.190 | MIT OR Apache-2.0 |
| libm v0.2.16 | MIT |
| linux-raw-sys v0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| log v0.4.34 | MIT OR Apache-2.0 |
| lru v0.16.4 | MIT |
| lz4_flex v0.13.1 | MIT |
| measure_time v0.9.0 | MIT |
| memchr v2.8.3 | Unlicense OR MIT |
| memmap2 v0.9.11 | MIT OR Apache-2.0 |
| minimal-lexical v0.2.1 | MIT/Apache-2.0 |
| murmurhash32 v0.3.1 | MIT |
| nom v7.1.3 | MIT |
| num-conv v0.2.2 | MIT OR Apache-2.0 |
| num-traits v0.2.19 | MIT OR Apache-2.0 |
| once_cell v1.21.4 | MIT OR Apache-2.0 |
| oneshot v0.1.13 | MIT OR Apache-2.0 |
| ordered-float v5.5.0 | MIT |
| ownedbytes v0.9.0 | MIT |
| pin-project-lite v0.2.17 | Apache-2.0 OR MIT |
| powerfmt v0.2.1 | MIT OR Apache-2.0 |
| prettyplease v0.3.0 | MIT OR Apache-2.0 |
| proc-macro2 v1.0.107 | MIT OR Apache-2.0 |
| quote v1.0.47 | MIT OR Apache-2.0 |
| r-efi v6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| rayon v1.12.0 | MIT OR Apache-2.0 |
| rayon-core v1.13.0 | MIT OR Apache-2.0 |
| regex v1.13.1 | MIT OR Apache-2.0 |
| regex-automata v0.4.18 | MIT OR Apache-2.0 |
| regex-syntax v0.8.11 | MIT OR Apache-2.0 |
| rustc-hash v2.1.3 | Apache-2.0 OR MIT |
| rustix v1.1.5 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| rustversion v1.0.23 | MIT OR Apache-2.0 |
| serde v1.0.229 | MIT OR Apache-2.0 |
| serde_core v1.0.229 | MIT OR Apache-2.0 |
| serde_derive v1.0.229 | MIT OR Apache-2.0 |
| serde_fmt v1.1.0 | Apache-2.0 OR MIT |
| serde_json v1.0.151 | MIT OR Apache-2.0 |
| sha1_smol v1.0.1 | BSD-3-Clause |
| sketches-ddsketch v0.4.1 | Apache-2.0 |
| slab v0.4.12 | MIT |
| smallvec v1.16.3 | MIT OR Apache-2.0 |
| stable_deref_trait v1.2.1 | MIT OR Apache-2.0 |
| strsim v0.11.1 | MIT |
| syn v3.0.6 | MIT OR Apache-2.0 |
| tantivy v0.26.2 | MIT |
| tantivy-bitpacker v0.10.0 | MIT |
| tantivy-columnar v0.7.0 | MIT |
| tantivy-common v0.11.0 | MIT |
| tantivy-fst v0.5.0 | Unlicense/MIT |
| tantivy-query-grammar v0.26.0 | MIT |
| tantivy-sstable v0.7.0 | MIT |
| tantivy-stacker v0.7.0 | MIT |
| tantivy-tokenizer-api v0.7.0 | MIT |
| tempfile v3.27.0 | MIT OR Apache-2.0 |
| thiserror v2.0.21 | MIT OR Apache-2.0 |
| thiserror-impl v2.0.21 | MIT OR Apache-2.0 |
| time v0.3.55 | MIT OR Apache-2.0 |
| time-core v0.1.9 | MIT OR Apache-2.0 |
| time-macros v0.2.32 | MIT OR Apache-2.0 |
| typeid v1.0.3 | MIT OR Apache-2.0 |
| typetag v0.2.23 | MIT OR Apache-2.0 |
| typetag-impl v0.2.23 | MIT OR Apache-2.0 |
| unicode-ident v1.0.26 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| utf8-ranges v1.0.5 | Unlicense/MIT |
| uuid v1.28.0 | Apache-2.0 OR MIT |
| value-bag v1.14.1 | Apache-2.0 OR MIT |
| value-bag-serde1 v1.14.1 | Apache-2.0 OR MIT |
| wasm-bindgen v0.2.129 | MIT OR Apache-2.0 |
| wasm-bindgen-macro v0.2.129 | MIT OR Apache-2.0 |
| wasm-bindgen-macro-support v0.2.129 | MIT OR Apache-2.0 |
| wasm-bindgen-shared v0.2.129 | MIT OR Apache-2.0 |
| winapi v0.3.9 | MIT/Apache-2.0 |
| winapi-i686-pc-windows-gnu v0.4.0 | MIT/Apache-2.0 |
| winapi-x86_64-pc-windows-gnu v0.4.0 | MIT/Apache-2.0 |
| windows_aarch64_gnullvm v0.52.6 | MIT OR Apache-2.0 |
| windows_aarch64_msvc v0.52.6 | MIT OR Apache-2.0 |
| windows_i686_gnu v0.52.6 | MIT OR Apache-2.0 |
| windows_i686_gnullvm v0.52.6 | MIT OR Apache-2.0 |
| windows_i686_msvc v0.52.6 | MIT OR Apache-2.0 |
| windows_x86_64_gnu v0.52.6 | MIT OR Apache-2.0 |
| windows_x86_64_gnullvm v0.52.6 | MIT OR Apache-2.0 |
| windows_x86_64_msvc v0.52.6 | MIT OR Apache-2.0 |
| windows-link v0.2.1 | MIT OR Apache-2.0 |
| windows-sys v0.59.0 | MIT OR Apache-2.0 |
| windows-sys v0.61.2 | MIT OR Apache-2.0 |
| windows-targets v0.52.6 | MIT OR Apache-2.0 |
| zmij v1.0.23 | MIT |

The workspace-copy closure has 140 all-target packages and 119 Windows packages, including Tantivy itself.

## Approved continuation verification, 2026-10-12

`cargo tree -p tantivy --locked --offline --target all --edges normal,build --prefix none --format '{p} | {l}'` reproduces all 140 entries in the inventory above (Cargo's proc-macro annotation is omitted from the table). A parsed comparison against the committed baseline lockfile shows exactly 36 added registry name/version pairs and zero removed or upgraded versions. The existing SQLite source and build flags, all workspace pins and all vendor files remain unchanged. The two existing exact-package licence exceptions remain sufficient; no new licence exception is needed.

The raw tree is `target/p1-23-approved-tree.txt`. Maintenance/security conclusions remain those of the retained 2026-10-11 source/advisory snapshot; no fresh advisory-database download or automated cargo-audit pass is claimed on 2026-10-12. The latest-version statement is deliberately limited to the local registry cache, as requested by the owner.

## Tantivy 0.27.0 move, 2026-10-24

**Why.** The bake-off chose Tantivy (#52, `docs/decisions.md`, 2026-10-24), and the tested 0.26.2 requires `lru ^0.16.3`, which RUSTSEC-2026-0253 covers. Tantivy 0.27.0 requires `lru ^0.18.2`, the advisory's first fixed version. The owner approved moving the exact pin and the lockfile growth it brings on 2026-10-24. The benchmark crate is still the only Tantivy user; `crates/xtask/src/dependency_policy.rs` (`check_fulltext_quarantine`) keeps it that way until #53 brings Tantivy into Syrinx.

**What changed.** `crates/fulltext-bench/Cargo.toml` pins `=0.27.0` with the same `default-features = false` and the same `mmap` and `lz4-compression` features, both of which still exist in 0.27.0's manifest. `Cargo.lock` was changed only with `cargo update -p tantivy --precise 0.27.0`; no blanket update was run. The benchmark source needed no change: it touches none of the breaking changes in 0.27.0's CHANGELOG, such as `set_fast` taking a string, the new `FieldType::TieBreaker` variant and the removed `BooleanWeight::new` and `RamDirectory::deep_clone`, and it compiled and passed its tests in `cargo xtask check` unchanged; its capture-time field uses the `FAST` flag, not `set_fast`. 0.27.0 requires Rust 1.88; the installed 1.97.1 meets this. Tantivy 0.27.0 was published on 2026-10-17 and is the latest stable release; it is young, so the comparison was rerun on it (below).

**RUSTSEC-2026-0253 resolved.** The lockfile resolves `lru 0.18.5` (published 2026-09-23, MIT), inside the patched range `>= 0.18.2`; 0.18.5 is also patched for RUSTSEC-2021-0130 and RUSTSEC-2026-0002. Tantivy 0.27.0 still uses `lru` only for the doc-store block cache (`src/store/reader.rs`, `LruCache<usize, Block>`).

**Lockfile changes.** Two packages are new to `Cargo.lock` and eleven change version; nothing is removed. All are MIT or Apache-2.0, so no licence exception is needed.

| Package | Before | After | Declared licence | Published (crates.io) |
| --- | --- | --- | --- | --- |
| tantivy | 0.26.2 | 0.27.0 | MIT | 2026-10-17 |
| tantivy-bitpacker | 0.10.0 | 0.27.0 | MIT | 2026-10-16 |
| tantivy-columnar | 0.7.0 | 0.27.0 | MIT | 2026-10-17 |
| tantivy-common | 0.11.0 | 0.27.0 | MIT | 2026-10-16 |
| tantivy-query-grammar | 0.26.0 | 0.27.0 | MIT | 2026-10-16 |
| tantivy-sstable | 0.7.0 | 0.27.0 | MIT | 2026-10-16 |
| tantivy-stacker | 0.7.0 | 0.27.0 | MIT | 2026-10-17 |
| lru | 0.16.4 | 0.18.5 | MIT | 2026-09-23 |
| lz4_flex | 0.13.1 | 0.14.0 | MIT | 2026-07-14 |
| murmurhash32 | 0.3.1 | 0.4.0 | MIT | 2026-04-09 |
| datasketches | 0.2.0 | 0.5.0 | Apache-2.0 | 2026-09-04 |
| serde-tuple-vec-map | new | 1.0.1 | MIT | 2022-04-04 |
| unwrap-infallible | new | 1.0.0 | MIT OR Apache-2.0 | 2025-12-06 |

Maintenance: the seven Tantivy crates, lz4_flex and murmurhash32 come from the Tantivy/Quickwit maintainers; datasketches is the Apache DataSketches project's Rust port; lru 0.19.0 exists upstream (2026-10-18), so the crate is maintained, and Tantivy's `^0.18.2` requirement keeps it on 0.18. serde-tuple-vec-map is a small, finished serde helper with no release since 2022, and unwrap-infallible is a one-function helper; neither has a RustSec entry. None of these versions is yanked.

**Packages that join Tantivy's closure without changing the lockfile.** These were already in `Cargo.lock` for other crates and keep their earlier audits; they now also sit under Tantivy: base64 0.23.1, rand 0.9.5, rand_chacha 0.9.0, rand_core 0.9.5, ppv-lite86 0.2.21, getrandom 0.3.4, r-efi 5.3.0, wasip2 1.0.4, wit-bindgen 0.57.1, zerocopy 0.8.62, zerocopy-derive 0.8.62 and syn 2.0.119. All offer MIT or Apache-2.0 (zerocopy also BSD-2-Clause; r-efi's LGPL alternative is not selected, as before). allocator-api2 0.2.21, base64 0.22.1 and hashbrown 0.16.1 leave Tantivy's closure but stay in the lockfile for other crates. The closure is now 151 all-target packages and 127 Windows packages, from `cargo tree -p tantivy --locked --offline --target all --edges normal,build --prefix none --format '{p} | {l}'` (and `--target x86_64-pc-windows-msvc`).

**Advisories.** `cargo audit` is still not installed. Each new or changed package, and each package newly in the closure, was matched against the RustSec advisory database's `crates/` directories at commit `21d909e4c8bd4e14c7aea58bf980c881a0053ab1` (2026-10-24), read through the GitHub API. Matches and their patched ranges, all of which contain the resolved versions:

- id = "RUSTSEC-2021-0130"; package = "lru"; patched = [">= 0.7.1"]
- id = "RUSTSEC-2026-0002"; package = "lru"; informational = "unsound"; patched = [">= 0.16.3"]
- id = "RUSTSEC-2026-0253"; package = "lru"; informational = "unsound"; patched = [">= 0.18.2"]
- id = "RUSTSEC-2026-0041"; package = "lz4_flex"; patched = [">= 0.11.6, < 0.12.0", ">= 0.12.1"]
- id = "RUSTSEC-2017-0004"; package = "base64"; patched = [">= 0.5.2"]
- id = "RUSTSEC-2026-0097"; package = "rand"; informational = "unsound"; patched = [">= 0.10.1", "< 0.10.0, >= 0.9.3", "< 0.9.0, >= 0.8.6"]
- id = "RUSTSEC-2019-0035"; package = "rand_core"; informational = "unsound"; patched = ["^ 0.3.1", ">= 0.4.2"]
- id = "RUSTSEC-2021-0023"; package = "rand_core"; patched = [">= 0.6.2"]
- id = "RUSTSEC-2023-0074"; package = "zerocopy"; patched = [..., ">= 0.7.31"]

No other package listed above has a RustSec entry. As before, this is a manual scoped review, not a cargo-audit pass or a claim that no undisclosed flaw exists.

**The prefix-snippet trap still holds.** `src/query/phrase_prefix_query/phrase_prefix_query.rs` and `SnippetGenerator::create` are byte-for-byte the same in 0.26.2 and 0.27.0: `query_terms` still visits only the phrase terms, never the prefix, so a one-word prefix query still gives `SnippetGenerator::create` no terms. The benchmark keeps its own prefix expansion and its empty-snippet rejection (`docs/dev/trapstoknow.md`, "Local search"). This is a source comparison, not a new measurement.

**Confirming rerun on 0.27.0, 2026-10-24.** An agent ran `run-comparison.ps1 -Label p1-23-quiet-0270-2026-10-24 -QuietWaitMinutes 20` with the same twelve runs as #52's 0.26.2 comparison: three per engine on each synthetic corpus. The runner saw no Cargo or rustc process on any check. All twelve exited 0 with empty error logs, passed the crash-recovery and stale-replay checks, and both engines matched the same number of pages for every query. The middle of three runs on the broad corpus:

| What was measured | FTS5 | Tantivy 0.27.0 | Tantivy 0.26.2 (#52) |
| --- | ---: | ---: | ---: |
| Slowest query, 95th percentile (`lexeme000*`) | 26.8 ms | 2.7 ms | 2.3 ms |
| Expiring 40,000 pages | 317 s | 2.4 s | 2.2 s |
| Reopening after the crash | 30 ms | 11 ms | 11 ms |
| Disk after expiry | 408 MB | 55 MB | 55 MB |
| Peak working set | 383 MB | 562 MB | 562 MB |

Tantivy 0.27.0 performs as 0.26.2 did, within run-to-run noise, so #52's choice stands on the version Syrinx will ship. The raw files are `target/p1-23-quiet-0270-2026-10-24-*` on the owner's machine, outside the repository.
