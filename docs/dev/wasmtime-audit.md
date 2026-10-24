# wasmtime 48.0.x dependency audit (#27, for #28)

Syrinx's plugin runtime embeds wasmtime (owner decision 1 of 2026-10-21,
`docs/decisions.md`), and `AGENTS.md` lets a dependency in only after its
maintenance, security record and licence have been checked, for every
transitive crate, not just the top one. This audit is that check for the
pin #28 will add to `crates/syrinx-plugin-host`. It was done on 2026-10-24
under #27, which the owner folded the pre-build checks into (decision 2 of
2026-10-24), in a throwaway Cargo project outside the repository; nothing was
added to Syrinx's `Cargo.toml` or `Cargo.lock`. #28 commits the real pin and
reruns the two `cargo tree` commands below against its lockfile; where they
disagree with this file, the lockfile is the truth and this file is updated.

The companion report, `docs/dev/vendor-reviews/wasmtime-wit-prebuild-check.md`,
records the WIT parse-and-load check and the end-to-end run done the same day.

## The pin

```toml
wasmtime = { version = "=48.0.7", default-features = false, features = ["runtime", "cranelift", "component-model", "std", "parallel-compilation", "anyhow"] }
wasmtime-wasi = { version = "=48.0.7", default-features = false, features = ["p2"] }
```

**48.0.7 is the latest 48.x patch on 2026-10-24.** The crates.io index lists
48.0.0 through 48.0.7 for both crates (48.0.7 published 2026-10-19), with
49.0.4 and 50.0.0 as the current non-LTS lines. 48.x is wasmtime's
long-term-support line, supported for 24 months from 2026-08-20 (research note
10, section 1). Seven patch releases in two months is the cadence to expect:
each is a security or correctness backport, and under decision 1 of 2026-10-21
a move within 48.0.x is a routine pin bump that passes the gate, while a move
to 49 or later needs the owner.

**Features as resolved** (`cargo tree -e features -i wasmtime`, Windows):
`anyhow`, `async`, `component-model`, `cranelift`, `parallel-compilation`,
`runtime`, `std`, and the internal `once_cell`,
`wasmtime-jit-icache-coherence` and `wasmtime-unwinder`. `async` is not
asked for: `wasmtime-wasi 48.0.7` depends on `wasmtime-wasi-io 48.0.7`,
whose `std` feature turns wasmtime's `async` on unconditionally, so it is
compiled in and Syrinx simply never enables `Config::async_support` (note 10,
"Reconciled: no async execution in 0.1.0"). `wasmtime-wasi` resolves with only
`p2`; `wasmtime-wasi-io` with only `std`. Nothing in the lockfile names Winch,
the pooling allocator, `component-model-async`, WASI 0.3 (`p3`) or
`wasi-http`: those stay off, which is where most of 2026's advisories sit
(below).

## Method

- **Two resolutions, both for `x86_64-pc-windows-msvc`.** A *fresh* one
  (`cargo generate-lockfile` in the throwaway crate, which picks the newest
  compatible version of everything) and a *seeded* one, where Syrinx's own
  `Cargo.lock` was copied in first and Cargo resolved only what was missing.
  The seeded resolution is what #28 will get when it adds the dependency,
  because Cargo keeps every version already locked; it is the approval
  inventory below. The fresh one shows what a careless `cargo update` would
  move (see "Fresh against seeded").
- **Graph.** `cargo tree -e normal,build --target x86_64-pc-windows-msvc`,
  the feature-resolved graph including build scripts and procedural macros, as
  `docs/dev/fulltext-audit.md` did. The lockfile also holds crates that only
  other platforms compile; they are listed and marked, since a future
  `cargo` command can fetch them.
- **Licences were read from the published archives**, not from crates.io
  metadata: every `.crate` file of the seeded lockfile was extracted from the
  local registry cache and its `LICENSE*`, `LICENCE*`, `COPYING*` and
  `UNLICENSE*` files were classified by their text, then compared with the
  `license` field of the same archive's `Cargo.toml`. The crates whose archive
  carries no licence file are named below and were checked against their
  repositories instead.
- **Advisories.** `cargo audit` is not installed and nothing was installed for
  this. The RustSec advisory database was cloned at commit
  `21d909e4c8bd4e14c7aea58bf980c881a0053ab1` (2026-10-24), every package name in
  both lockfiles was matched against its `crates/` folders, and each matching
  advisory's `patched` and `unaffected` ranges were evaluated against the locked
  version with a small semver evaluator. This is a manual scoped review, not a
  `cargo audit` pass and not a claim that no undisclosed flaw exists.
- **Maintenance.** crates.io's API was asked, one request a second, for the
  publish date of each locked version, whether it is yanked, the latest stable
  version and the repository, for every crate that is new to Syrinx's lockfile
  and for the two direct crates. Crates already in Syrinx's lockfile at the same
  version keep their earlier audits.

## The graph in numbers

| | Lockfile (all platforms) | Compiled on Windows |
|---|---|---|
| Third-party crates in the seeded resolution | 197 | 170 |
| New crate names against Syrinx's `Cargo.lock` | 54 | 48 |
| New versions of crates Syrinx already has | 4 (`addr2line 0.26.1`, `gimli 0.33.0`, `mach2 0.6.0`, `object 0.39.1`) | 2 (`gimli 0.33.0`, `object 0.39.1`) |
| Crates under `Apache-2.0 WITH LLVM-exception` alone | 32 | 31 |

The 27 lockfile-only crates that Windows never compiles are the usual
platform shards (`windows_*` for other targets, `linux-raw-sys`,
`rustix-linux-procfs`, `memfd`, `mach2`, `r-efi`, `wasi 0.11`) plus
`wasmtime-internal-jit-debug`, `addr2line`, `cpp_demangle`, `rustc-demangle`,
`embedded-io` (two versions) and `slab`, which wasmtime's Linux and macOS
debugging paths pull. Research note 10 counted 59 new name-and-version pairs at
49.0.4; at 48.0.7 against today's lockfile it is 58 (50 compiled on Windows),
the same shape.

## Licences

Every crate in both graphs can be taken under MIT, Apache-2.0, BSD-3-Clause or
one of the owner's existing narrow approvals. No crate is GPL-only, and no
licence outside the list in `AGENTS.md` needs a new ruling. The table of
declared strings, with the election made for each, is generated from the
archives below; the findings that matter:

1. **`Apache-2.0 WITH LLVM-exception`, 32 crates, all inside the family the
   owner approved on 2026-10-21** (decision 2: the `cranelift-*`, `pulley-*`,
   `wasmtime*`, `wasmtime-internal-*` crates, `regalloc2`, `target-lexicon` and
   `winx`), at exactly the pinned versions: thirteen `cranelift-*` at 0.135.7,
   `pulley-interpreter` and `pulley-macros` at 48.0.7, `wasmtime`,
   `wasmtime-environ`, `wasmtime-wasi`, `wasmtime-wasi-io` and ten
   `wasmtime-internal-*` at 48.0.7, `regalloc2 0.15.2`, `target-lexicon 0.13.5`
   and `winx 0.36.4`. Decision 2 said "about 31"; the 32nd is
   `wasmtime-internal-jit-debug`, which the lockfile holds for Linux and macOS
   and Windows never compiles. No crate outside the family carries the
   exception alone.
2. **Fourteen crates of that family ship no licence file in their archive**:
   `cranelift-assembler-x64`, `cranelift-assembler-x64-meta`, `cranelift-bitset`,
   `cranelift-isle`, `cranelift-srcgen`, `pulley-interpreter`, `pulley-macros`,
   `wasmtime-internal-component-macro`, `-component-util`, `-core`,
   `-jit-debug`, `-jit-icache-coherence`, `-versioned-export-macros`,
   `-wit-bindgen` and `wasmtime-wasi-io`. Their `Cargo.toml` declares
   `Apache-2.0 WITH LLVM-exception`, and all of them live in the
   `bytecodealliance/wasmtime` repository, whose `LICENSE` at tag `v48.0.7` is
   the Apache License 2.0 with the "LLVM Exceptions to the Apache 2.0 License"
   appended (fetched through the GitHub API on 2026-10-24). The other eighteen
   carry the same text in their archive. `regalloc2`, `target-lexicon` and
   `winx` carry it in their repositories' `LICENSE` and in their archives.
3. **Sixteen crates offer `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR
   MIT`; MIT is elected** for each (`rustix`, `cap-primitives`,
   `fs-set-times`, `io-extras`, `io-lifetimes` 2.0.4 and 3.0.1,
   `ambient-authority`, the `wasm-tools` crates `wasmparser`, `wasm-encoder`,
   `wasm-metadata`, `wasmprinter`, `wit-parser` and `wit-component` at 0.254.2,
   and the Linux-only `linux-raw-sys` and `rustix-linux-procfs`). Their
   archives carry all three licence files.
4. **The existing narrow approvals hold unchanged in the seeded resolution.**
   `unicode-ident` stays at **1.0.26**; `foldhash` resolves to **0.2.0** only
   (Zlib, through `hashbrown 0.17.1`); the eighteen Unicode-3.0 crates
   (`icu_*` 2.3.0 and `icu_provider 2.3.1`, `litemap 0.8.3`, `potential_utf
   0.1.6`, `tinystr 0.8.4`, `writeable 0.6.4`, `yoke 0.8.3`, `yoke-derive
   0.8.4`, `zerofrom 0.1.8`, `zerofrom-derive 0.1.8`, `zerotrie 0.2.5`,
   `zerovec 0.11.8`, `zerovec-derive 0.11.6`) and `encoding_rs 0.8.42`
   (`(Apache-2.0 OR MIT) AND BSD-3-Clause`) are at the versions already in
   Syrinx's lockfile. `r-efi 6.0.0` (`MIT OR Apache-2.0 OR
   LGPL-2.1-or-later`, MIT elected, no licence file in its archive) is already
   in Syrinx's lockfile and is compiled only for UEFI targets, never on Windows.
5. **`mach2 0.6.0`** (`BSD-2-Clause OR MIT OR Apache-2.0`, MIT elected) is a
   new version of a macOS-only crate Syrinx's lockfile already lists at 0.4.3;
   Windows never compiles it.

### Fresh against seeded: what `cargo update` would move

A fresh resolution of the same declaration differs from the seeded one in 21
crates. Three of them touch approvals or advisories, which is why #28 must add
the dependency without a blanket `cargo update`, and why `xtask`'s dependency
policy should keep refusing the moves:

| Crate | Seeded (Syrinx today) | Fresh | Why it matters |
|---|---|---|---|
| `unicode-ident` | 1.0.26 | 1.0.27 | the Unicode-3.0 approval names 1.0.26 only (`docs/dev/dependency-audit.md`, P1-03) |
| `icu_*`, `tinystr`, `yoke`, `zerovec`, `zerovec-derive` | 2.3.0/2.3.1, 0.8.4, 0.8.3, 0.11.8, 0.11.6 | 2.4.0, 0.8.5, 0.8.4, 0.11.9, 0.11.7 | the Unicode-3.0 approval is per exact version (`docs/dev/integrated-preview-dependencies.md`) |
| `tokio` | 1.53.2 | 1.54.1 | 1.53.2 is in RUSTSEC-2026-0386's range (below); 1.54.1 is patched |

The other moves are ordinary: `cc` 1.7.0 to 1.10.0, `find-msvc-tools` 0.1.14
to 0.1.15, `crc32fast`, `equivalent`, `libc`, `serde_json`, `smallvec`, `syn`
and `generic-array` one patch each.

## Advisories

116 advisories match a package name in the seeded lockfile. **One applies at
the locked versions, and it is Syrinx's own `tokio 1.53.2`, not wasmtime's:**

- [RUSTSEC-2026-0386](https://rustsec.org/advisories/RUSTSEC-2026-0386.html)
  (`tokio`, informational "unsound", 2026-10-13): the safe constructors of
  `tokio::io::unix::AsyncFd` accept a raw file descriptor without proof of
  ownership, which can end in a use-after-free in the I/O driver. Patched in
  `>= 1.51.6, < 1.52.0` and `>= 1.53.3`. The affected functions live in
  `tokio::io::unix`, which does not exist on Windows. Today `tokio 1.53.2` sits
  in Syrinx's lockfile without being compiled for Windows
  (`cargo tree -i tokio --target x86_64-pc-windows-msvc` prints nothing);
  `wasmtime-wasi` would compile it for the first time, with the features
  `rt`, `rt-multi-thread`, `net`, `io-util`, `io-std`, `sync` and `time`.
  **Recommendation for #28:** move `tokio` to a patched version (a fresh
  resolution picks 1.54.1) in the same change, so the first build that compiles
  tokio compiles a clean one; that is a one-crate lockfile move within the
  approved shape, not a new dependency. RUSTSEC-2026-0384, also on `tokio`,
  concerns 1.54.0 only and leaves 1.53.2 unaffected.

**wasmtime's own record at 48.0.7.** 64 advisories name `wasmtime` and 9 name
`wasmtime-wasi`; 48.0.7 is inside the patched or unaffected range of every
one, including the three batches of October 2026: RUSTSEC-2026-0325 to -0327
(fixed in 48.0.4), -0353 to -0364 (fixed in 48.0.6; -0360 affects 49.x only)
and -0378 and -0379 (fixed in 48.0.7 on 2026-10-19). RUSTSEC-2026-0380, the
critical Cranelift bit-packing overflow note 10 cites, was published the same
day with the patched range `>= 43.0.0` beside `>= 36.0.19`, so every 48.x
release is outside it. The advisories that
hit the chosen configuration in note 10's list (x86_64 code generation,
component-model string transcoding, WASI filesystem escapes) are all patched at
48.0.7. The ones the configuration avoids by keeping Winch, the pooling
allocator, `component-model-async`, WASI 0.3 and `wasi-http` off are avoided
here too, because none of those features is in the resolved graph.

One point to carry into #28: `wasmtime-wasi`'s `p2` feature compiles its
filesystem implementation (`cap-primitives`, `fs-set-times`, `io-extras`,
`winx`) even though Syrinx gives plugins no preopened directory. The
filesystem-escape advisories are therefore patched code that is compiled but
unreachable; note 10's rule that plugins reach files only through Syrinx's own
host functions is what keeps it unreachable, and the test host's refusal of
any component importing `wasi:filesystem` is the check that it stays so.

The remaining 114 matches are older advisories whose patched range contains the
locked version (`smallvec`, `once_cell`, `hashbrown`, `rand_core`, `mio`,
`futures-*`, `bumpalo`, `crossbeam-*`, `zerovec*`, `idna`, `cap-primitives`,
`shlex`, `sha2`, `chacha20`, `generic-array`, `socket2`, `slab`, `bytes`,
`rand`, `anyhow`, `tracing` and six more on `tokio`). They are listed in the
appendix in the shape `docs/dev/fulltext-audit.md` used.

## Maintenance

The direct crates are maintained on a published schedule: wasmtime releases a
major on the 20th of every month and patches its two supported lines, and on
2026-10-19 alone it published 36.0.19, 48.0.7 and 49.0.4 together (note 10,
section 1). Every crate new to Syrinx's lockfile is listed below with the date
its locked version was published, the latest stable version crates.io knows
and its repository; none is yanked. Four are worth a sentence:

- The `wasm-tools` crates (`wasmparser`, `wasm-encoder`, `wasm-metadata`,
  `wasmprinter`, `wit-parser`, `wit-component`) resolve at 0.254.2 because
  wasmtime 48 pins that line; 0.262.0 is current. They move only when wasmtime
  moves, which is the LTS line's point.
- `maybe-owned 0.3.4` (2020), `leb128fmt 0.1.0` (2023) and
  `ambient-authority 0.0.2` (2023) are small, finished crates with no newer
  release; `cobs 0.3.0` and `io-lifetimes 2.0.4` are older lines kept beside
  newer ones by their dependants (`postcard` and `winx`). None has an advisory.
- `cc 1.7.0` compiles wasmtime's small C helper on Windows with the MSVC tools
  the GPUI shader patch already requires; the seeded resolution keeps Syrinx's
  `cc` rather than moving to 1.10.0.

## Guest side: `wit-bindgen 0.63.0`

The guest crate lives in plugin repositories and the template, never in
`syrinx.exe`, but #27 asked for it to be audited the same way. `wit-bindgen
0.63.0` (published 2026-10-14; the only 0.63.x on crates.io) with
`default-features = false, features = ["macros"]` resolves to 32 third-party
crates for a `wasm32-wasip2` build: `wit-bindgen`, `wit-bindgen-core`,
`wit-bindgen-rust` and `wit-bindgen-rust-macro` at 0.63.0 and the `wasm-tools`
crates at 0.262.0, all `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`
with **MIT elected** (their archives carry all three files); `foldhash 0.2.0`
(Zlib, approved); `unicode-ident`, which a fresh resolution puts at **1.0.27**
(`(MIT OR Apache-2.0) AND Unicode-3.0`), while the template and the Notes
repository lock it at the approved 1.0.26 under their own `licences.toml`
exception; and 22 crates under MIT or Apache-2.0. No advisory applies: the two
matches (`anyhow 1.0.104`, `hashbrown 0.17.1`) are in patched ranges. Only the
`wit-bindgen` runtime crate is compiled into a plugin's `.wasm`; the rest is
build-time code on the plugin developer's machine. `wasm-tools` the command-line
program was not used: it is not installed, and the WIT parse check used the
`wit-parser 0.262.0` library instead.

## Reproducing this audit

In a throwaway crate outside the repository, declare the pin above and
nothing else, copy Syrinx's `Cargo.lock` beside it, then:

```powershell
cargo tree -e normal,build --target x86_64-pc-windows-msvc --prefix none --format "{p}|{l}"
cargo tree -e features -i wasmtime --target x86_64-pc-windows-msvc --locked --offline
```

The seeded lockfile's SHA-256 was
`76ef8d8484fae3af37efbbd1026f0eda1403489e66e9d9757acf4eaa5d5b25b2`, the fresh
one's `563ac12a39f64dfae397c36b2475ac0baa5aefa14518f5330fa490e581305bc4` and
the guest workspace's `e1ba0b081f01823620c8bca26a6857b8180cdd83139c429f0312bc28e1963916`
(LF line endings). The archive hashes in the table are the lockfile's
`checksum` values, which Cargo verifies against each downloaded `.crate`.

## Licence strings in the Windows-compiled graph

Declared licence strings as the archives' `Cargo.toml` state them, with the election made. Legacy slash-separated spellings are kept as published.

| Declared licence | Crates | Elected | Crates |
|---|---|---|---|
| MIT OR Apache-2.0 | 80 | MIT | `allocator-api2 0.2.21`, `anyhow 1.0.104`, `arbitrary 1.5.0`, `async-trait 0.1.92`, `bitflags 2.13.2`, `block-buffer 0.10.4`, `bumpalo 3.20.3`, `cc 1.7.0`, `cfg-if 1.0.5`, `chacha20 0.10.2`, `cobs 0.3.0`, `cpufeatures 0.2.17`, `cpufeatures 0.3.1`, `crc32fast 1.5.2`, `crossbeam-deque 0.8.8`, `crossbeam-epoch 0.9.21`, `crossbeam-utils 0.8.23`, `crypto-common 0.1.7`, `digest 0.10.7`, `displaydoc 0.2.7`, `either 1.19.0`, `errno 0.3.14`, `find-msvc-tools 0.1.14`, `form_urlencoded 1.2.2`, `futures 0.3.34`, `futures-channel 0.3.34`, `futures-core 0.3.34`, `futures-io 0.3.34`, `futures-sink 0.3.34`, `futures-task 0.3.34`, `futures-util 0.3.34`, `getrandom 0.4.3`, `gimli 0.33.0`, `hashbrown 0.16.1`, `hashbrown 0.17.1`, `heck 0.5.0`, `idna 1.1.0`, `ipnet 2.12.2`, `itertools 0.14.0`, `itoa 1.0.18`, `leb128fmt 0.1.0`, `libc 0.2.190`, `log 0.4.34`, `maybe-owned 0.3.4`, `once_cell 1.21.4`, `percent-encoding 2.3.2`, `postcard 1.1.3`, `proc-macro2 1.0.107`, `quote 1.0.47`, `rand 0.10.3`, `rand_core 0.10.1`, `rayon 1.12.0`, `rayon-core 1.13.0`, `rustversion 1.0.23`, `scopeguard 1.2.0`, `semver 1.0.28`, `serde 1.0.229`, `serde_core 1.0.229`, `serde_derive 1.0.229`, `serde_json 1.0.151`, `sha2 0.10.9`, `shlex 2.0.1`, `simdutf8 0.1.5`, `smallvec 1.16.3`, `socket2 0.6.5`, `stable_deref_trait 1.2.1`, `syn 2.0.119`, `syn 3.0.6`, `thiserror 2.0.21`, `thiserror-impl 2.0.21`, `typenum 1.20.1`, `url 2.5.8`, `windows-link 0.2.1`, `windows-sys 0.59.0`, `windows-sys 0.60.2`, `windows-sys 0.61.2`, `windows-targets 0.52.6`, `windows-targets 0.53.5`, `windows_x86_64_msvc 0.52.6`, `windows_x86_64_msvc 0.53.1` |
| Apache-2.0 WITH LLVM-exception | 31 | Apache-2.0 WITH LLVM-exception (approved, exact version) | `cranelift-assembler-x64 0.135.7`, `cranelift-assembler-x64-meta 0.135.7`, `cranelift-bforest 0.135.7`, `cranelift-bitset 0.135.7`, `cranelift-codegen 0.135.7`, `cranelift-codegen-meta 0.135.7`, `cranelift-codegen-shared 0.135.7`, `cranelift-control 0.135.7`, `cranelift-entity 0.135.7`, `cranelift-frontend 0.135.7`, `cranelift-isle 0.135.7`, `cranelift-native 0.135.7`, `cranelift-srcgen 0.135.7`, `pulley-interpreter 48.0.7`, `pulley-macros 48.0.7`, `regalloc2 0.15.2`, `target-lexicon 0.13.5`, `wasmtime 48.0.7`, `wasmtime-environ 48.0.7`, `wasmtime-internal-component-macro 48.0.7`, `wasmtime-internal-component-util 48.0.7`, `wasmtime-internal-core 48.0.7`, `wasmtime-internal-cranelift 48.0.7`, `wasmtime-internal-fiber 48.0.7`, `wasmtime-internal-jit-icache-coherence 48.0.7`, `wasmtime-internal-unwinder 48.0.7`, `wasmtime-internal-versioned-export-macros 48.0.7`, `wasmtime-internal-wit-bindgen 48.0.7`, `wasmtime-wasi 48.0.7`, `wasmtime-wasi-io 48.0.7`, `winx 0.36.4` |
| Unicode-3.0 | 18 | Unicode-3.0 (approved, exact version) | `icu_collections 2.3.0`, `icu_locale_core 2.3.0`, `icu_normalizer 2.3.0`, `icu_normalizer_data 2.3.0`, `icu_properties 2.3.0`, `icu_properties_data 2.3.0`, `icu_provider 2.3.1`, `litemap 0.8.3`, `potential_utf 0.1.6`, `tinystr 0.8.4`, `writeable 0.6.4`, `yoke 0.8.3`, `yoke-derive 0.8.4`, `zerofrom 0.1.8`, `zerofrom-derive 0.1.8`, `zerotrie 0.2.5`, `zerovec 0.11.8`, `zerovec-derive 0.11.6` |
| Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 13 | MIT | `ambient-authority 0.0.2`, `cap-primitives 4.0.3`, `fs-set-times 0.20.3`, `io-extras 0.19.0`, `io-lifetimes 2.0.4`, `io-lifetimes 3.0.1`, `rustix 1.1.5`, `wasm-encoder 0.254.2`, `wasm-metadata 0.254.2`, `wasmparser 0.254.2`, `wasmprinter 0.254.2`, `wit-component 0.254.2`, `wit-parser 0.254.2` |
| MIT | 10 | MIT | `bytes 1.12.1`, `generic-array 0.14.7`, `libm 0.2.16`, `mio 1.2.4`, `synstructure 0.14.0`, `tokio 1.53.2`, `tracing 0.1.44`, `tracing-attributes 0.1.31`, `tracing-core 0.1.36`, `zmij 1.0.23` |
| Apache-2.0 OR MIT | 8 | MIT | `equivalent 1.0.2`, `idna_adapter 1.2.2`, `indexmap 2.14.2`, `multiversion_no_op 1.0.0`, `object 0.39.1`, `pin-project-lite 0.2.17`, `rustc-hash 2.1.3`, `utf8_iter 1.0.4` |
| MIT/Apache-2.0 | 3 | MIT | `core_detect 1.0.0`, `id-arena 2.3.0`, `version_check 0.9.5` |
| Unlicense OR MIT | 3 | MIT | `memchr 2.8.3`, `termcolor 1.4.1`, `winapi-util 0.1.11` |
| (Apache-2.0 OR MIT) AND BSD-3-Clause | 1 | MIT AND BSD-3-Clause | `encoding_rs 0.8.42` |
| Apache-2.0 / MIT | 1 | MIT | `fnv 1.0.7` |
| Zlib | 1 | Zlib (approved, exact version) | `foldhash 0.2.0` |
| (MIT OR Apache-2.0) AND Unicode-3.0 | 1 | MIT AND Unicode-3.0 (approved, exact version) | `unicode-ident 1.0.26` |

## Maintenance of the crates new to Syrinx's lockfile

From crates.io on 2026-10-24. None of the locked versions is yanked. Crates Windows never compiles are marked; they are in the lockfile for other platforms.

| Crate | Locked | Published | Latest stable | Compiled on Windows | Repository |
|---|---|---|---|---|---|
| `ambient-authority` | 0.0.2 | 2023-04-06 | 0.0.2 | yes | sunfishcode/ambient-authority |
| `cap-primitives` | 4.0.3 | 2026-08-20 | 4.0.3 | yes | bytecodealliance/cap-std |
| `cobs` | 0.3.0 | 2025-01-31 | 0.5.1 | yes | jamesmunns/cobs.rs |
| `cpp_demangle` | 0.5.1 | 2025-11-05 | 0.5.1 | no | gimli-rs/cpp_demangle |
| `cranelift-assembler-x64` | 0.135.7 | 2026-10-19 | 0.137.0 | yes |  |
| `cranelift-assembler-x64-meta` | 0.135.7 | 2026-10-19 | 0.137.0 | yes |  |
| `cranelift-bforest` | 0.135.7 | 2026-10-19 | 0.137.0 | yes | bytecodealliance/wasmtime |
| `cranelift-bitset` | 0.135.7 | 2026-10-19 | 0.137.0 | yes | bytecodealliance/wasmtime |
| `cranelift-codegen` | 0.135.7 | 2026-10-19 | 0.137.0 | yes | bytecodealliance/wasmtime |
| `cranelift-codegen-meta` | 0.135.7 | 2026-10-19 | 0.137.0 | yes | bytecodealliance/wasmtime |
| `cranelift-codegen-shared` | 0.135.7 | 2026-10-19 | 0.137.0 | yes | bytecodealliance/wasmtime |
| `cranelift-control` | 0.135.7 | 2026-10-19 | 0.137.0 | yes | bytecodealliance/wasmtime |
| `cranelift-entity` | 0.135.7 | 2026-10-19 | 0.137.0 | yes | bytecodealliance/wasmtime |
| `cranelift-frontend` | 0.135.7 | 2026-10-19 | 0.137.0 | yes | bytecodealliance/wasmtime |
| `cranelift-isle` | 0.135.7 | 2026-10-19 | 0.137.0 | yes | bytecodealliance/wasmtime/tree/main/cranelift/isle |
| `cranelift-native` | 0.135.7 | 2026-10-19 | 0.137.0 | yes | bytecodealliance/wasmtime |
| `cranelift-srcgen` | 0.135.7 | 2026-10-19 | 0.137.0 | yes | bytecodealliance/wasmtime |
| `embedded-io` | 0.4.0 | 2022-11-25 | 0.7.1 | no | rust-embedded/embedded-hal |
| `embedded-io` | 0.6.1 | 2023-10-22 | 0.7.1 | no | rust-embedded/embedded-hal |
| `fs-set-times` | 0.20.3 | 2025-03-10 | 0.20.3 | yes | bytecodealliance/fs-set-times |
| `id-arena` | 2.3.0 | 2026-01-14 | 2.3.0 | yes | fitzgen/id-arena |
| `io-extras` | 0.19.0 | 2025-08-06 | 0.19.0 | yes | sunfishcode/io-extras |
| `io-lifetimes` | 2.0.4 | 2024-12-04 | 3.0.1 | yes | sunfishcode/io-lifetimes |
| `io-lifetimes` | 3.0.1 | 2025-08-06 | 3.0.1 | yes | sunfishcode/io-lifetimes |
| `leb128fmt` | 0.1.0 | 2023-10-02 | 0.1.0 | yes | bluk/leb128fmt |
| `maybe-owned` | 0.3.4 | 2020-05-23 | 0.3.4 | yes | rustonaut/maybe-owned |
| `memfd` | 0.6.6 | 2026-09-01 | 0.6.6 | no | lucab/memfd-rs |
| `postcard` | 1.1.3 | 2025-07-24 | 1.1.3 | yes | jamesmunns/postcard |
| `pulley-interpreter` | 48.0.7 | 2026-10-19 | 50.0.0 | yes | bytecodealliance/wasmtime/tree/main/pulley |
| `pulley-macros` | 48.0.7 | 2026-10-19 | 50.0.0 | yes | bytecodealliance/wasmtime/tree/main/pulley/macros |
| `regalloc2` | 0.15.2 | 2026-07-20 | 0.15.2 | yes | bytecodealliance/regalloc2 |
| `rustix-linux-procfs` | 0.1.1 | 2025-04-21 | 0.1.1 | no | sunfishcode/rustix-linux-procfs |
| `target-lexicon` | 0.13.5 | 2026-02-15 | 0.13.5 | yes | bytecodealliance/target-lexicon |
| `wasm-encoder` | 0.254.2 | 2026-10-02 | 0.262.0 | yes | bytecodealliance/wasm-tools/tree/main/crates/wasm-encoder |
| `wasm-metadata` | 0.254.2 | 2026-10-02 | 0.262.0 | yes | bytecodealliance/wasm-tools/tree/main/crates/wasm-metadata |
| `wasmparser` | 0.254.2 | 2026-10-02 | 0.262.0 | yes | bytecodealliance/wasm-tools/tree/main/crates/wasmparser |
| `wasmprinter` | 0.254.2 | 2026-10-02 | 0.262.0 | yes | bytecodealliance/wasm-tools/tree/main/crates/wasmprinter |
| `wasmtime` | 48.0.7 | 2026-10-19 | 50.0.0 | yes | bytecodealliance/wasmtime |
| `wasmtime-environ` | 48.0.7 | 2026-10-19 | 50.0.0 | yes | bytecodealliance/wasmtime |
| `wasmtime-internal-component-macro` | 48.0.7 | 2026-10-19 | 50.0.0 | yes | bytecodealliance/wasmtime |
| `wasmtime-internal-component-util` | 48.0.7 | 2026-10-19 | 50.0.0 | yes | bytecodealliance/wasmtime |
| `wasmtime-internal-core` | 48.0.7 | 2026-10-19 | 50.0.0 | yes |  |
| `wasmtime-internal-cranelift` | 48.0.7 | 2026-10-19 | 50.0.0 | yes | bytecodealliance/wasmtime |
| `wasmtime-internal-fiber` | 48.0.7 | 2026-10-19 | 50.0.0 | yes | bytecodealliance/wasmtime |
| `wasmtime-internal-jit-debug` | 48.0.7 | 2026-10-19 | 50.0.0 | no | bytecodealliance/wasmtime |
| `wasmtime-internal-jit-icache-coherence` | 48.0.7 | 2026-10-19 | 50.0.0 | yes | bytecodealliance/wasmtime |
| `wasmtime-internal-unwinder` | 48.0.7 | 2026-10-19 | 50.0.0 | yes | bytecodealliance/wasmtime |
| `wasmtime-internal-versioned-export-macros` | 48.0.7 | 2026-10-19 | 50.0.0 | yes | bytecodealliance/wasmtime |
| `wasmtime-internal-wit-bindgen` | 48.0.7 | 2026-10-19 | 50.0.0 | yes | bytecodealliance/wasmtime |
| `wasmtime-wasi` | 48.0.7 | 2026-10-19 | 50.0.0 | yes | bytecodealliance/wasmtime |
| `wasmtime-wasi-io` | 48.0.7 | 2026-10-19 | 50.0.0 | yes | bytecodealliance/wasmtime |
| `winx` | 0.36.4 | 2024-12-04 | 0.36.4 | yes | sunfishcode/winx |
| `wit-component` | 0.254.2 | 2026-10-02 | 0.262.0 | yes | bytecodealliance/wasm-tools/tree/main/crates/wit-component |
| `wit-parser` | 0.254.2 | 2026-10-02 | 0.262.0 | yes | bytecodealliance/wasm-tools/tree/main/crates/wit-parser |

## Every third-party crate in the seeded resolution

197 crates. "Licence files read" names each licence file found in the published archive and what its text is; "none in the archive" is explained under Licences, finding 2. Dates come from crates.io for the crates new to Syrinx; the others keep their earlier audits. The hash is the lockfile's SHA-256 of the `.crate` archive.

| Package | Version | Compiled on Windows | Declared licence | Licence files read | Elected | New to Syrinx | Published | Archive SHA-256 |
|---|---|---|---|---|---|---|---|---|
| `addr2line` | 0.26.1 | no | Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | new version |  | `59317f77929f0e679d39364702289274de2f0f0b22cbf50b2b8cff2169a0b27a` |
| `allocator-api2` | 0.2.21 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `683d7910e743518b0e34f1186f92494becacb047c7b6bf616c96772180fef923` |
| `ambient-authority` | 0.0.2 | yes | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-Apache-2.0_WITH_LLVM-exception`: Apache-2.0 WITH LLVM-exception; `LICENSE-MIT`: MIT | MIT | new crate | 2023-04-06 | `e9d4ee0d472d1cd2e28c97dfa124b3d8d992e10eb0a035f33f5d12e3a177ba3b` |
| `anyhow` | 1.0.104 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `330a5ed07fa54e4702c9d6c4174f74427fc0ef6e214bbd677ae50a5099946470` |
| `arbitrary` | 1.5.0 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `3bc62ac97cc33321f50863d514c3bc38a453947a8f9e781137e47c7401020aed` |
| `async-trait` | 0.1.92 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `82f6aeea286b8eb4dd3431a1be1b59d290ace00f5bfd8e2a159bc2a05e2c1667` |
| `bitflags` | 2.13.2 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `3ded4057c258ba199e2d26386d3af3780957ecaee6c4ef4041c6b4b8b97c0b06` |
| `block-buffer` | 0.10.4 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `3078c7629b62d3f0439517fa394996acacc5cbc91c5a20d8c658e77abd503a71` |
| `bumpalo` | 3.20.3 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `72f5acc6cb2ba439de613abc23857ec3d78374d8ed5ac84e9d11336e87da8649` |
| `bytes` | 1.12.1 | yes | MIT | `LICENSE`: MIT | MIT | no |  | `fc652a48c352aef3ea3aed32080501cf3ef6ed5da78602a020c991775b0aff04` |
| `cap-primitives` | 4.0.3 | yes | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-Apache-2.0_WITH_LLVM-exception`: Apache-2.0 WITH LLVM-exception; `LICENSE-MIT`: MIT | MIT | new crate | 2026-08-20 | `8b5f74729fd2f44701d1a8eb47e906cdb3ccd9ec0f02baad85a744b791940b18` |
| `cc` | 1.7.0 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `50a649af8a827553c29fb0cb4bd4a6f1a0dd695bd3232b9bc98bd9c8a3ffbb8b` |
| `cfg-if` | 1.0.5 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `4e7648175b45a9a48536d676f68d918270699102aa8dab5496df06904c914600` |
| `chacha20` | 0.10.2 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `65c35e4b699c7e15ccbe7ee35c005e4fc0a278d22238a2857e6ce2dadeda1b06` |
| `cobs` | 0.3.0 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | new crate | 2025-01-31 | `0fa961b519f0b462e3a3b4a34b64d119eeaca1d59af726fe450bbba07a9fc0a1` |
| `core_detect` | 1.0.0 | yes | MIT/Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `7f8f80099a98041a3d1622845c271458a2d73e688351bf3cb999266764b81d48` |
| `cpp_demangle` | 0.5.1 | no | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | new crate | 2025-11-05 | `0667304c32ea56cb4cd6d2d7c0cfe9a2f8041229db8c033af7f8d69492429def` |
| `cpufeatures` | 0.2.17 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `59ed5838eebb26a2bb2e58f6d5b5316989ae9d08bab10e0e6d103e656d1b0280` |
| `cpufeatures` | 0.3.1 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `5ca28b0ae3115b884660db4118d803791fd6756b6e88f39c0f3f7859060d7566` |
| `cranelift-assembler-x64` | 0.135.7 | yes | Apache-2.0 WITH LLVM-exception | none in the archive | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `e6071a4f1df4837aa03d1e4d53067784acafe270ffc7dad66a7f39832204da3f` |
| `cranelift-assembler-x64-meta` | 0.135.7 | yes | Apache-2.0 WITH LLVM-exception | none in the archive | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `5e3b92a9cc90388fe70e63640af30b734387449445aef0176ab2028e9892d70e` |
| `cranelift-bforest` | 0.135.7 | yes | Apache-2.0 WITH LLVM-exception | `LICENSE`: Apache-2.0 WITH LLVM-exception | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `f45c85646c4425cc85b868effaf20c302b437a3fb79bb27a64513dd522f07411` |
| `cranelift-bitset` | 0.135.7 | yes | Apache-2.0 WITH LLVM-exception | none in the archive | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `94f56b5c7d564cfa03aa269a6b98c916cab4d545196a391d31403a23ef39333a` |
| `cranelift-codegen` | 0.135.7 | yes | Apache-2.0 WITH LLVM-exception | `LICENSE`: Apache-2.0 WITH LLVM-exception | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `2c073641ce6e382814a8220331cc1ea2c782a9ea5d5da6d0f7f7c0541192693e` |
| `cranelift-codegen-meta` | 0.135.7 | yes | Apache-2.0 WITH LLVM-exception | `LICENSE`: Apache-2.0 WITH LLVM-exception | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `db226fafea5194921de10dccf35ac2863500313990f770cbe8b26c51eebec4d4` |
| `cranelift-codegen-shared` | 0.135.7 | yes | Apache-2.0 WITH LLVM-exception | `LICENSE`: Apache-2.0 WITH LLVM-exception | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `cf8ac46b2c8e328ca979810426ea635ef90ecc8c54fd3296bde31ab470a48a1d` |
| `cranelift-control` | 0.135.7 | yes | Apache-2.0 WITH LLVM-exception | `LICENSE`: Apache-2.0 WITH LLVM-exception | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `6754e0ab97915209ea6ce6549f6443b018e1338142ada4c856cec213ee82e4b8` |
| `cranelift-entity` | 0.135.7 | yes | Apache-2.0 WITH LLVM-exception | `LICENSE`: Apache-2.0 WITH LLVM-exception | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `774d25e36df18696f63a284e9c4ea43455e2ebcca6308e0e02301f18c8e777e2` |
| `cranelift-frontend` | 0.135.7 | yes | Apache-2.0 WITH LLVM-exception | `LICENSE`: Apache-2.0 WITH LLVM-exception | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `69cfcb31405e2de2a48e5a6de8e07a3d8b77f35caa3e944d627780734b36c138` |
| `cranelift-isle` | 0.135.7 | yes | Apache-2.0 WITH LLVM-exception | none in the archive | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `2f5a26df22db546ab92be6feaf21835ae91082af0a32bbb398b277df3765412f` |
| `cranelift-native` | 0.135.7 | yes | Apache-2.0 WITH LLVM-exception | `LICENSE`: Apache-2.0 WITH LLVM-exception | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `19f258c08e590f90dcaf9904d25ba142ee2980c3aaeac4bb45f1452b1a407c2a` |
| `cranelift-srcgen` | 0.135.7 | yes | Apache-2.0 WITH LLVM-exception | none in the archive | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `67055b4f315e8f6af3e8eccec74ad38f7407f0fd854adbd49b4b3b27bdb2e3be` |
| `crc32fast` | 1.5.2 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `01a7799fd6b852db0e61728dde9a204c423b44d689dbd432522543614b490e78` |
| `crossbeam-deque` | 0.8.8 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `622f3fc73690be383c7214310406f28a90e6edeadc3cea882f9d71e495b9711a` |
| `crossbeam-epoch` | 0.9.21 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `dc74980687109a3b14c72fd458107bf0baa1da1a1a805e178d15501ba9b86d9d` |
| `crossbeam-utils` | 0.8.23 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `a31eee39dddec8330830986fcd7625edb5a24ec90ea038215273bbc3adb08ac6` |
| `crypto-common` | 0.1.7 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `78c8292055d1c1df0cce5d180393dc8cce0abec0a7102adb6c7b1eef6016d60a` |
| `digest` | 0.10.7 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `9ed9a281f7bc9b7576e61468ba615a66a5c8cfdff42420a70aa82701a3b1e292` |
| `displaydoc` | 0.2.7 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `c6232dd377dcc64799954cbd3a9bb882e9cdc1308ccd87b1c098f1fb2eaf82a8` |
| `either` | 1.19.0 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `0e9c71c2167ca323c882b99918929403426e2373ea17242ff5653e0d5e1058be` |
| `embedded-io` | 0.4.0 | no | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | new crate | 2022-11-25 | `ef1a6892d9eef45c8fa6b9e0086428a2cca8491aca8f787c534a3d6d0bcb3ced` |
| `embedded-io` | 0.6.1 | no | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | new crate | 2023-10-22 | `edd0f118536f44f5ccd48bcb8b111bdc3de888b58c74639dfb034a357d0f206d` |
| `encoding_rs` | 0.8.42 | yes | (Apache-2.0 OR MIT) AND BSD-3-Clause | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT; `LICENSE-WHATWG`: BSD-3-Clause | MIT AND BSD-3-Clause | no |  | `8e985e0451871ad22fb8d2b6b076e2028a502a0d3950998c2c5c0a4f9b5d9679` |
| `equivalent` | 1.0.2 | yes | Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `877a4ace8713b0bcf2a4e7eec82529c029f1d0619886d18145fea96c3ffe5c0f` |
| `errno` | 0.3.14 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `39cab71617ae0d63f51a36d69f866391735b51691dbda63cf6f96d042b63efeb` |
| `find-msvc-tools` | 0.1.14 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `aedcfb3409746eddb02b9e19ebda1c3394f759a152e48ee875a0844d1b955484` |
| `fnv` | 1.0.7 | yes | Apache-2.0 / MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `3f9eec918d3f24069decb9af1554cad7c880e2da24a9afd88aca000531ab82c1` |
| `foldhash` | 0.2.0 | yes | Zlib | `LICENSE`: Zlib | Zlib (approved, exact version) | no |  | `77ce24cb58228fbb8aa041425bb1050850ac19177686ea6e0f41a70416f56fdb` |
| `form_urlencoded` | 1.2.2 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `cb4cb245038516f5f85277875cdaa4f7d2c9a0fa0468de06ed190163b1581fcf` |
| `fs-set-times` | 0.20.3 | yes | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-Apache-2.0_WITH_LLVM-exception`: Apache-2.0 WITH LLVM-exception; `LICENSE-MIT`: MIT | MIT | new crate | 2025-03-10 | `94e7099f6313ecacbe1256e8ff9d617b75d1bcb16a6fddef94866d225a01a14a` |
| `futures` | 0.3.34 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `9a31d2a3fbaaeb2af2368bbdd904aa8e812d3c04a1ee10d3171f52d556e5d0a3` |
| `futures-channel` | 0.3.34 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `b1f9e3d69d39e4862ffed03ed071a76f9a13ba1d9109d355b0f0aa6b15e393c4` |
| `futures-core` | 0.3.34 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `92d699e522242e69e3003b94ecc1f960f3a5e015aa7c5d7486e65ad01dd94f5e` |
| `futures-io` | 0.3.34 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `53c0fa8157de1303bfffdaa1cc2a673bfffb60102f76b0ef4441659124373fed` |
| `futures-sink` | 0.3.34 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `1944426bf7d03f1d14f708785e4b33efd750b36d48a157b836b3efc15ede8e1d` |
| `futures-task` | 0.3.34 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `cd417de3d1d015fc3bfd2b1ea46dfc7bab72ef86f1cc7cc9c78e728b34a6d1fd` |
| `futures-util` | 0.3.34 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `0d50a92467f8ba5dd6e3ee5d4bd04d73ab2e4e1c44474a0674821dfce14b79bc` |
| `generic-array` | 0.14.7 | yes | MIT | `LICENSE`: MIT | MIT | no |  | `85649ca51fd72272d7821adaf274ad91c288277713d9c18820d8499a7ff69e9a` |
| `getrandom` | 0.4.3 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `300e883d756b2e4ec94e02791f39b04b522276138852cfc41d9fb7e904106099` |
| `gimli` | 0.33.0 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | new version |  | `0bf7f043f89559805f8c7cacc432749b2fa0d0a0a9ee46ce47164ed5ba7f126c` |
| `hashbrown` | 0.16.1 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `841d1cc9bed7f9236f321df977030373f4a4163ae1a7dbfe1a51a2c1a51d9100` |
| `hashbrown` | 0.17.1 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `ed5909b6e89a2db4456e54cd5f673791d7eca6732202bbf2a9cc504fe2f9b84a` |
| `heck` | 0.5.0 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `2304e00983f87ffb38b55b444b5e3b60a884b5d30c0fca7d82fe33449bbe55ea` |
| `icu_collections` | 2.3.0 | yes | Unicode-3.0 | `LICENSE`: MIT/Unicode-3.0 | Unicode-3.0 (approved, exact version) | no |  | `fa68d21081c4a05d5a901a1c62add574c77048b6a1c67be3b50ce0b60d4ca513` |
| `icu_locale_core` | 2.3.0 | yes | Unicode-3.0 | `LICENSE`: MIT/Unicode-3.0 | Unicode-3.0 (approved, exact version) | no |  | `d56e28588da92eee5c3201a6eff33fabdd49b62269c8938d4ff050ce4d900deb` |
| `icu_normalizer` | 2.3.0 | yes | Unicode-3.0 | `LICENSE`: MIT/Unicode-3.0 | Unicode-3.0 (approved, exact version) | no |  | `12f9cf5f235641ed274641dd81c3f28d870e276763d0797aeeab72317b1c646f` |
| `icu_normalizer_data` | 2.3.0 | yes | Unicode-3.0 | `LICENSE`: MIT/Unicode-3.0 | Unicode-3.0 (approved, exact version) | no |  | `1563da1ed3e0b3bf3d74c9b85917ac9c56464d2f57242270c09c9e752f8021a0` |
| `icu_properties` | 2.3.0 | yes | Unicode-3.0 | `LICENSE`: MIT/Unicode-3.0 | Unicode-3.0 (approved, exact version) | no |  | `7e7ca276ad3145661a65914e6daf131ca5120cd3dcee8f8f3214b8875184a148` |
| `icu_properties_data` | 2.3.0 | yes | Unicode-3.0 | `LICENSE`: MIT/Unicode-3.0 | Unicode-3.0 (approved, exact version) | no |  | `e590f038c1464a96894fd6d10127e90a8be4509f56ff7ecef851b15cee0b7caa` |
| `icu_provider` | 2.3.1 | yes | Unicode-3.0 | `LICENSE`: MIT/Unicode-3.0 | Unicode-3.0 (approved, exact version) | no |  | `d27bbb9d3abbefac45d55f647c9de1d44aafcd1186eb91879afef17c396c3e73` |
| `id-arena` | 2.3.0 | yes | MIT/Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | new crate | 2026-01-14 | `3d3067d79b975e8844ca9eb072e16b31c3c1c36928edf9c6789548c524d0d954` |
| `idna` | 1.1.0 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `3b0875f23caa03898994f6ddc501886a45c7d3d62d04d2d90788d47be1b1e4de` |
| `idna_adapter` | 1.2.2 | yes | Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `cb68373c0d6620ef8105e855e7745e18b0d00d3bdb07fb532e434244cdb9a714` |
| `indexmap` | 2.14.2 | yes | Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `cc4e190f5d26ca7051642629da2c52fc03bde85a03197c99408dcd291734c855` |
| `io-extras` | 0.19.0 | yes | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-Apache-2.0_WITH_LLVM-exception`: Apache-2.0 WITH LLVM-exception; `LICENSE-MIT`: MIT | MIT | new crate | 2025-08-06 | `20fd6de4ccfcc187e38bc21cfa543cb5a302cb86a8b114eb7f0bf0dc9f8ac00f` |
| `io-lifetimes` | 2.0.4 | yes | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-Apache-2.0_WITH_LLVM-exception`: Apache-2.0 WITH LLVM-exception; `LICENSE-MIT`: MIT | MIT | new crate | 2024-12-04 | `06432fb54d3be7964ecd3649233cddf80db2832f47fec34c01f65b3d9d774983` |
| `io-lifetimes` | 3.0.1 | yes | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-Apache-2.0_WITH_LLVM-exception`: Apache-2.0 WITH LLVM-exception; `LICENSE-MIT`: MIT | MIT | new crate | 2025-08-06 | `2f0fb0570afe1fed943c5c3d4102d5358592d8625fda6a0007fdbe65a92fba96` |
| `ipnet` | 2.12.2 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `791930b43c0d5973160d90a8f3894509f2b273430f5c5c73b668636d0287c5c0` |
| `itertools` | 0.14.0 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `2b192c782037fadd9cfa75548310488aabdbf3d2da73885b31bd0abd03351285` |
| `itoa` | 1.0.18 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `8f42a60cbdf9a97f5d2305f08a87dc4e09308d1276d28c869c684d7777685682` |
| `leb128fmt` | 0.1.0 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | new crate | 2023-10-02 | `09edd9e8b54e49e587e4f6295a7d29c3ea94d469cb40ab8ca70b288248a81db2` |
| `libc` | 0.2.190 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `ce5d3ddc6d3fa000eb1536d85e147bfe31aacaba692ed6a876f95cb7c855be78` |
| `libm` | 0.2.16 | yes | MIT | `LICENSE.txt`: Apache-2.0/MIT | MIT | no |  | `b6d2cec3eae94f9f509c767b45932f1ada8350c4bdb85af2fcab4a3c14807981` |
| `linux-raw-sys` | 0.12.1 | no | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-Apache-2.0_WITH_LLVM-exception`: Apache-2.0 WITH LLVM-exception; `LICENSE-MIT`: MIT | MIT | no |  | `32a66949e030da00e8c7d4434b251670a91556f4144941d37452769c25d58a53` |
| `litemap` | 0.8.3 | yes | Unicode-3.0 | `LICENSE`: MIT/Unicode-3.0 | Unicode-3.0 (approved, exact version) | no |  | `47d9d19d1d6efa0109d2f65ff4c85cddd50bd572e5a00127ab10987290bcefae` |
| `log` | 0.4.34 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `f9f8bd3e56ce4dfc153cf470fffbfa98c7620958b312ca5c3a4b8d5181fd13c6` |
| `mach2` | 0.6.0 | no | BSD-2-Clause OR MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-BSD`: BSD-2-Clause; `LICENSE-MIT`: MIT | MIT | new version |  | `dae608c151f68243f2b000364e1f7b186d9c29845f7d2d85bd31b9ad77ad552b` |
| `maybe-owned` | 0.3.4 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | new crate | 2020-05-23 | `4facc753ae494aeb6e3c22f839b158aebd4f9270f55cd3c79906c45476c47ab4` |
| `memchr` | 2.8.3 | yes | Unlicense OR MIT | `LICENSE-MIT`: MIT; `UNLICENSE`: Unlicense | MIT | no |  | `cf8baf1c55e62ffcace7a9f06f4bd9cd3f0c4beb022d3b367256b91b87513d98` |
| `memfd` | 0.6.6 | no | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | new crate | 2026-09-01 | `57804b2c9b69967f1536a56f86297e367a33b19e98852ed624b84551cdbc0d90` |
| `mio` | 1.2.4 | yes | MIT | `LICENSE`: MIT | MIT | no |  | `1788edb87fdc09c7e26304471e2f5be8cdefb1b6930d6e3985fc02ff53bf86ee` |
| `multiversion_no_op` | 1.0.0 | yes | Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `743fb55ba31b18fb1ecef6bdc9aa2743314978ac084044301a7eee33fb99a20d` |
| `object` | 0.39.1 | yes | Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | new version |  | `2e5a6c098c7a3b6547378093f5cc30bc54fd361ce711e05293a5cc589562739b` |
| `once_cell` | 1.21.4 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `9f7c3e4beb33f85d45ae3e3a1792185706c8e16d043238c593331cc7cd313b50` |
| `percent-encoding` | 2.3.2 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `9b4f627cb1b25917193a259e49bdad08f671f8d9708acfd5fe0a8c1455d87220` |
| `pin-project-lite` | 0.2.17 | yes | Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `a89322df9ebe1c1578d689c92318e070967d1042b512afbe49518723f4e6d5cd` |
| `postcard` | 1.1.3 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | new crate | 2025-07-24 | `6764c3b5dd454e283a30e6dfe78e9b31096d9e32036b5d1eaac7a6119ccb9a24` |
| `potential_utf` | 0.1.6 | yes | Unicode-3.0 | `LICENSE`: MIT/Unicode-3.0 | Unicode-3.0 (approved, exact version) | no |  | `d83eb9bc6d8e5cf568e7a1101d60ee05e81ed50ea106026f3d18deeb046d7661` |
| `proc-macro2` | 1.0.107 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `985e7ec9bb745e6ce6535b544d84d6cd6f7ad8bd711c398938ae983b91a766d9` |
| `pulley-interpreter` | 48.0.7 | yes | Apache-2.0 WITH LLVM-exception | none in the archive | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `2b8187c8e7db87d085865fc106502e7b34d5c25f8ce4c272945b21503371f6a0` |
| `pulley-macros` | 48.0.7 | yes | Apache-2.0 WITH LLVM-exception | none in the archive | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `7f98ce0e381d9e45e63ea1d20bba3cf131797086077f43af9203f1fb7de0ed6b` |
| `quote` | 1.0.47 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `1fbf4db142a473a8d80c26bbf18454ed458bf8d26c8219c331daecfdbd079001` |
| `r-efi` | 6.0.0 | no | MIT OR Apache-2.0 OR LGPL-2.1-or-later | none in the archive | MIT | no |  | `f8dcc9c7d52a811697d2151c701e0d08956f92b0e24136cf4cf27b57a6a0d9bf` |
| `rand` | 0.10.3 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `65c9fb96cbc91e3478eaae79a69fcd3f1ae4ad052e471fe6732fff548984b4af` |
| `rand_core` | 0.10.1 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `63b8176103e19a2643978565ca18b50549f6101881c443590420e4dc998a3c69` |
| `rayon` | 1.12.0 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `fb39b166781f92d482534ef4b4b1b2568f42613b53e5b6c160e24cfbfa30926d` |
| `rayon-core` | 1.13.0 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `22e18b0f0062d30d4230b2e85ff77fdfe4326feb054b9783a3460d8435c8ab91` |
| `regalloc2` | 0.15.2 | yes | Apache-2.0 WITH LLVM-exception | `LICENSE`: Apache-2.0 WITH LLVM-exception | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-07-20 | `757712e8e61590d6d4f5d563483755538b5aa13467837a3b41cd9832509a7f85` |
| `rustc-demangle` | 0.1.28 | no | MIT/Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `b74b56ffa8bb2830709a538c2cbcae9aa062db0d2a42563bfb09bdaae44020eb` |
| `rustc-hash` | 2.1.3 | yes | Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `6b1e7f9a428571be2dc5bc0505c13fb6bf936822b894ec87abf8a08a4e51742d` |
| `rustix` | 1.1.5 | yes | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-Apache-2.0_WITH_LLVM-exception`: Apache-2.0 WITH LLVM-exception; `LICENSE-MIT`: MIT | MIT | no |  | `891efababe418670775f199f0d233d84843c227a0949a883ce15b37c78d6629d` |
| `rustix-linux-procfs` | 0.1.1 | no | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-Apache-2.0_WITH_LLVM-exception`: Apache-2.0 WITH LLVM-exception; `LICENSE-MIT`: MIT | MIT | new crate | 2025-04-21 | `2fc84bf7e9aa16c4f2c758f27412dc9841341e16aa682d9c7ac308fe3ee12056` |
| `rustversion` | 1.0.23 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `cf54715a573b99ac80df0bc206da022bcd442c974952c7b9720069370852e21f` |
| `scopeguard` | 1.2.0 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `94143f37725109f92c262ed2cf5e59bce7498c01bcc1502d7b9afe439a4e9f49` |
| `semver` | 1.0.28 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `8a7852d02fc848982e0c167ef163aaff9cd91dc640ba85e263cb1ce46fae51cd` |
| `serde` | 1.0.229 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba` |
| `serde_core` | 1.0.229 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `67dca2c9c51e58a4791a4b1ed58308b39c64224d349a935ab5039aa360942a48` |
| `serde_derive` | 1.0.229 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `e7a5d71263a5a7d47b41f6b3f06ba276f10cc18b0931f1799f710578e2309348` |
| `serde_json` | 1.0.151 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14` |
| `sha2` | 0.10.9 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `a7507d819769d01a365ab707794a4084392c824f54a7a6a7862f8c3d0892b283` |
| `shlex` | 2.0.1 | yes | MIT OR Apache-2.0 | `LICENSE-MIT`: MIT | MIT | no |  | `f8fadd59c855ef2080decdef8ff161eb6661b86933c9d82e5ba29dc602a55aba` |
| `simdutf8` | 0.1.5 | yes | MIT OR Apache-2.0 | `LICENSE-Apache`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `e3a9fe34e3e7a50316060351f37187a3f546bce95496156754b601a5fa71b76e` |
| `slab` | 0.4.12 | no | MIT | `LICENSE`: MIT | MIT | no |  | `0c790de23124f9ab44544d7ac05d60440adc586479ce501c1d6d7da3cd8c9cf5` |
| `smallvec` | 1.16.3 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `5b3dc8af474f516a851ff4bd12db780f948b9250ad37211e4eec0bccea54e01b` |
| `socket2` | 0.6.5 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `c3d1e2c7f27f8d4cb10542a02c49005dbd6e93095799d6f3be745fae9f8fedd4` |
| `stable_deref_trait` | 1.2.1 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `6ce2be8dc25455e1f91df71bfa12ad37d7af1092ae736f3a6cd0e37bc7810596` |
| `syn` | 2.0.119 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `872831b642d1a07999a962a351ed35b955ea2cfc8f3862091e2a240a84f17297` |
| `syn` | 3.0.6 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `8593e8e72159ed2257d083c7a454a85cbf854f37a0966d8d483aff8c8a3ebcee` |
| `synstructure` | 0.14.0 | yes | MIT | `LICENSE`: MIT | MIT | no |  | `901704edd0dfe137f1987838ee4f259e4e063c31371bdb423f7ae38ec6f77f02` |
| `target-lexicon` | 0.13.5 | yes | Apache-2.0 WITH LLVM-exception | `LICENSE`: Apache-2.0 WITH LLVM-exception | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-02-15 | `adb6935a6f5c20170eeceb1a3835a49e12e19d792f6dd344ccc76a985ca5a6ca` |
| `termcolor` | 1.4.1 | yes | Unlicense OR MIT | `LICENSE-MIT`: MIT; `UNLICENSE`: Unlicense | MIT | no |  | `06794f8f6c5c898b3275aebefa6b8a1cb24cd2c6c79397ab15774837a0bc5755` |
| `thiserror` | 2.0.21 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `09e52cb86a36cede5cb101bf8908837b3e4c6e5e59fe7fd85c23fb56200d189e` |
| `thiserror-impl` | 2.0.21 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `fe5197923287db20a58125f0bc85c062f7f2c892de97b18c356f9efb14b28524` |
| `tinystr` | 0.8.4 | yes | Unicode-3.0 | `LICENSE`: MIT/Unicode-3.0 | Unicode-3.0 (approved, exact version) | no |  | `b1e27c91459209c2986af3dcf603a5a74a4368754ce37414f59acc971167f643` |
| `tokio` | 1.53.2 | yes | MIT | `LICENSE`: MIT | MIT | no |  | `e95f91fcc7a621e8b030f6aa23c71fe9838ae2fb4d8118b75602a328f5144044` |
| `tracing` | 0.1.44 | yes | MIT | `LICENSE`: MIT | MIT | no |  | `63e71662fa4b2a2c3a26f570f037eb95bb1f85397f3cd8076caed2f026a6d100` |
| `tracing-attributes` | 0.1.31 | yes | MIT | `LICENSE`: MIT | MIT | no |  | `7490cfa5ec963746568740651ac6781f701c9c5ea257c58e057f3ba8cf69e8da` |
| `tracing-core` | 0.1.36 | yes | MIT | `LICENSE`: MIT | MIT | no |  | `db97caf9d906fbde555dd62fa95ddba9eecfd14cb388e4f491a66d74cd5fb79a` |
| `typenum` | 1.20.1 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `b6f5e870be6c3b371b77fe0ee0bafb859fa4964b4404c27de1d380043c4dda20` |
| `unicode-ident` | 1.0.26 | yes | (MIT OR Apache-2.0) AND Unicode-3.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT; `LICENSE-UNICODE`: MIT/Unicode-3.0 | MIT AND Unicode-3.0 (approved, exact version) | no |  | `d245f478577f809a851594d02313b640fb437e0bb33866753cff937863096954` |
| `url` | 2.5.8 | yes | MIT OR Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `ff67a8a4397373c3ef660812acab3268222035010ab8680ec4215f38ba3d0eed` |
| `utf8_iter` | 1.0.4 | yes | Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `b6c140620e7ffbb22c2dee59cafe6084a59b5ffc27a8859a5f0d494b5d52b6be` |
| `version_check` | 0.9.5 | yes | MIT/Apache-2.0 | `LICENSE-APACHE`: Apache-2.0; `LICENSE-MIT`: MIT | MIT | no |  | `0b928f33d975fc6ad9f86c8f283853ad26bdd5b10b7f1542aa2fa15e2289105a` |
| `wasi` | 0.11.1+wasi-snapshot-preview1 | no | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-Apache-2.0_WITH_LLVM-exception`: Apache-2.0 WITH LLVM-exception; `LICENSE-MIT`: MIT | MIT | no |  | `ccf3ec651a847eb01de73ccad15eb7d99f80485de043efb2f370cd654f4ea44b` |
| `wasm-encoder` | 0.254.2 | yes | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-Apache-2.0_WITH_LLVM-exception`: Apache-2.0 WITH LLVM-exception; `LICENSE-MIT`: MIT | MIT | new crate | 2026-10-02 | `ebaca0b540bdb15d195ff34fb23074255948708b8916a11a339ba4513dc61906` |
| `wasm-metadata` | 0.254.2 | yes | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-Apache-2.0_WITH_LLVM-exception`: Apache-2.0 WITH LLVM-exception; `LICENSE-MIT`: MIT | MIT | new crate | 2026-10-02 | `5d1880dd9a848aa68f387966256c34ad49abb4607e3cfa35720d2723d0591cd3` |
| `wasmparser` | 0.254.2 | yes | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-Apache-2.0_WITH_LLVM-exception`: Apache-2.0 WITH LLVM-exception; `LICENSE-MIT`: MIT | MIT | new crate | 2026-10-02 | `c5ff8685b587722894b6f25958fc608d315b93b8d23647bfb567664e7006d757` |
| `wasmprinter` | 0.254.2 | yes | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-Apache-2.0_WITH_LLVM-exception`: Apache-2.0 WITH LLVM-exception; `LICENSE-MIT`: MIT | MIT | new crate | 2026-10-02 | `8b03c8f270f3f384f70f42869b5ff7f6253fe4f0aa8590f7edd332c1943e5f51` |
| `wasmtime` | 48.0.7 | yes | Apache-2.0 WITH LLVM-exception | `LICENSE`: Apache-2.0 WITH LLVM-exception | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `12300d040e8bea7e7ccde75b9919d27ebba47a217a64b09ba058ae8d3b4290b6` |
| `wasmtime-environ` | 48.0.7 | yes | Apache-2.0 WITH LLVM-exception | `LICENSE`: Apache-2.0 WITH LLVM-exception | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `805d6554eb6dd6e62891061a31383a89d69fd891f785e5ae68f0835a554b7a2a` |
| `wasmtime-internal-component-macro` | 48.0.7 | yes | Apache-2.0 WITH LLVM-exception | none in the archive | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `b832b80853365d9d3a741681044554be291a4f977aae074b318b55c7a4835f83` |
| `wasmtime-internal-component-util` | 48.0.7 | yes | Apache-2.0 WITH LLVM-exception | none in the archive | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `4c8a11aaf68d8945991429d45950f32490a30ddcdcdb56629816cb41a255d8fe` |
| `wasmtime-internal-core` | 48.0.7 | yes | Apache-2.0 WITH LLVM-exception | none in the archive | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `ea266d6cfb31c324c075c8432386654bcbb1559aa8d29be9d47eacf7dcc41012` |
| `wasmtime-internal-cranelift` | 48.0.7 | yes | Apache-2.0 WITH LLVM-exception | `LICENSE`: Apache-2.0 WITH LLVM-exception | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `d427759553b07c296c8ed7bff8ac930a94b9ba99e6a79243ea79bcc51aa58d96` |
| `wasmtime-internal-fiber` | 48.0.7 | yes | Apache-2.0 WITH LLVM-exception | `LICENSE`: Apache-2.0 WITH LLVM-exception | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `b54c8c5b02ea985149a90f917e1761af99734c6c27e6d7fdd5b6f69fc1db3950` |
| `wasmtime-internal-jit-debug` | 48.0.7 | no | Apache-2.0 WITH LLVM-exception | none in the archive | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `72bb10c94724f072b6eca515085033e61da9efae65d054545126df37a15530c3` |
| `wasmtime-internal-jit-icache-coherence` | 48.0.7 | yes | Apache-2.0 WITH LLVM-exception | none in the archive | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `1b781a27e8958fd07fd3135c0be216f77301efecd15cbf4f14ff210cdad5f169` |
| `wasmtime-internal-unwinder` | 48.0.7 | yes | Apache-2.0 WITH LLVM-exception | `LICENSE`: Apache-2.0 WITH LLVM-exception | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `121188de1b2741b8e35530a9b0c8adf525b3aa5913a2ca3d4522509cfbc0494a` |
| `wasmtime-internal-versioned-export-macros` | 48.0.7 | yes | Apache-2.0 WITH LLVM-exception | none in the archive | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `933f1990e4e5177547a4abd51dcfa5cb7616fe42b9fb6f0f2b7423627758223c` |
| `wasmtime-internal-wit-bindgen` | 48.0.7 | yes | Apache-2.0 WITH LLVM-exception | none in the archive | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `417ebd61d2581d86d65413517f3a41294a9ad105da26f0dc7039912a0d97e1ed` |
| `wasmtime-wasi` | 48.0.7 | yes | Apache-2.0 WITH LLVM-exception | `LICENSE`: Apache-2.0 WITH LLVM-exception | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `3ef0d3ec88f9406bc5e498848489c49ff515577500acc9f03fa93d0883dce435` |
| `wasmtime-wasi-io` | 48.0.7 | yes | Apache-2.0 WITH LLVM-exception | none in the archive | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2026-10-19 | `fe7dfc924e222e15455bd6d445ff7065a4595fffc7698c7bbc01acc0ed6c0421` |
| `winapi-util` | 0.1.11 | yes | Unlicense OR MIT | `LICENSE-MIT`: MIT; `UNLICENSE`: Unlicense | MIT | no |  | `c2a7b1c03c876122aa43f3020e6c3c3ee5c05081c9a00739faf7503aeba10d22` |
| `windows-link` | 0.2.1 | yes | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `f0805222e57f7521d6a62e36fa9163bc891acd422f971defe97d64e70d0a4fe5` |
| `windows-sys` | 0.59.0 | yes | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `1e38bc4d79ed67fd075bcc251a1c39b32a1776bbe92e5bef1f0bf1f8c531853b` |
| `windows-sys` | 0.60.2 | yes | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `f2f500e4d28234f72040990ec9d39e3a6b950f9f22d3dba18416c35882612bcb` |
| `windows-sys` | 0.61.2 | yes | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `ae137229bcbd6cdf0f7b80a31df61766145077ddf49416a728b02cb3921ff3fc` |
| `windows-targets` | 0.52.6 | yes | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `9b724f72796e036ab90c1021d4780d4d3d648aca59e491e6b98e725b84e99973` |
| `windows-targets` | 0.53.5 | yes | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `4945f9f551b88e0d65f3db0bc25c33b8acea4d9e41163edf90dcd0b19f9069f3` |
| `windows_aarch64_gnullvm` | 0.52.6 | no | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `32a4622180e7a0ec044bb555404c800bc9fd9ec262ec147edd5989ccd0c02cd3` |
| `windows_aarch64_gnullvm` | 0.53.1 | no | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `a9d8416fa8b42f5c947f8482c43e7d89e73a173cead56d044f6a56104a6d1b53` |
| `windows_aarch64_msvc` | 0.52.6 | no | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `09ec2a7bb152e2252b53fa7803150007879548bc709c039df7627cabbd05d469` |
| `windows_aarch64_msvc` | 0.53.1 | no | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `b9d782e804c2f632e395708e99a94275910eb9100b2114651e04744e9b125006` |
| `windows_i686_gnu` | 0.52.6 | no | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `8e9b5ad5ab802e97eb8e295ac6720e509ee4c243f69d781394014ebfe8bbfa0b` |
| `windows_i686_gnu` | 0.53.1 | no | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `960e6da069d81e09becb0ca57a65220ddff016ff2d6af6a223cf372a506593a3` |
| `windows_i686_gnullvm` | 0.52.6 | no | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `0eee52d38c090b3caa76c563b86c3a4bd71ef1a819287c19d586d7334ae8ed66` |
| `windows_i686_gnullvm` | 0.53.1 | no | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `fa7359d10048f68ab8b09fa71c3daccfb0e9b559aed648a8f95469c27057180c` |
| `windows_i686_msvc` | 0.52.6 | no | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `240948bc05c5e7c6dabba28bf89d89ffce3e303022809e73deaefe4f6ec56c66` |
| `windows_i686_msvc` | 0.53.1 | no | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `1e7ac75179f18232fe9c285163565a57ef8d3c89254a30685b57d83a38d326c2` |
| `windows_x86_64_gnu` | 0.52.6 | no | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `147a5c80aabfbf0c7d901cb5895d1de30ef2907eb21fbbab29ca94c5b08b1a78` |
| `windows_x86_64_gnu` | 0.53.1 | no | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `9c3842cdd74a865a8066ab39c8a7a473c0778a3f29370b5fd6b4b9aa7df4a499` |
| `windows_x86_64_gnullvm` | 0.52.6 | no | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `24d5b23dc417412679681396f2b49f3de8c1473deb516bd34410872eff51ed0d` |
| `windows_x86_64_gnullvm` | 0.53.1 | no | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `0ffa179e2d07eee8ad8f57493436566c7cc30ac536a3379fdf008f47f6bb7ae1` |
| `windows_x86_64_msvc` | 0.52.6 | yes | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `589f6da84c646204747d1270a2a5661ea66ed1cced2631d546fdfb155959f9ec` |
| `windows_x86_64_msvc` | 0.53.1 | yes | MIT OR Apache-2.0 | `license-apache-2.0`: Apache-2.0; `license-mit`: MIT | MIT | no |  | `d6bbff5f0aada427a1e5a6da5f1f98158182f26556f345ac9e04d36d0ebed650` |
| `winx` | 0.36.4 | yes | Apache-2.0 WITH LLVM-exception | `LICENSE`: Apache-2.0 WITH LLVM-exception | Apache-2.0 WITH LLVM-exception (approved, exact version) | new crate | 2024-12-04 | `3f3fd376f71958b862e7afb20cfe5a22830e1963462f3a17f49d82a6c1d1f42d` |
| `wit-component` | 0.254.2 | yes | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-Apache-2.0_WITH_LLVM-exception`: Apache-2.0 WITH LLVM-exception; `LICENSE-MIT`: MIT | MIT | new crate | 2026-10-02 | `1c96b3cea6838dbea55173baa56991fcb087896c321b0a770db201e5a0bd3506` |
| `wit-parser` | 0.254.2 | yes | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | `LICENSE-APACHE`: Apache-2.0; `LICENSE-Apache-2.0_WITH_LLVM-exception`: Apache-2.0 WITH LLVM-exception; `LICENSE-MIT`: MIT | MIT | new crate | 2026-10-02 | `22672d61bfb9df46a16b46ce38522293aee1cc4749fc84a3464ce4083b3d5fd6` |
| `writeable` | 0.6.4 | yes | Unicode-3.0 | `LICENSE`: MIT/Unicode-3.0 | Unicode-3.0 (approved, exact version) | no |  | `3ad82d2a33cdc9674dc7465672f271e096168fcdbe0f799d9e6db8c5892679dc` |
| `yoke` | 0.8.3 | yes | Unicode-3.0 | `LICENSE`: MIT/Unicode-3.0 | Unicode-3.0 (approved, exact version) | no |  | `709fe23a0424b6a435d82152b1bd3fdfb0833487d5fa90d05d42762a9891fef5` |
| `yoke-derive` | 0.8.4 | yes | Unicode-3.0 | `LICENSE`: MIT/Unicode-3.0 | Unicode-3.0 (approved, exact version) | no |  | `ec8ebde2db3681e8c9980cc27822030e68752690ddfa9473e739aeb4dbde6d71` |
| `zerofrom` | 0.1.8 | yes | Unicode-3.0 | `LICENSE`: MIT/Unicode-3.0 | Unicode-3.0 (approved, exact version) | no |  | `0ec05a11813ea801ff6d75110ad09cd0824ddba17dfe17128ea0d5f68e6c5272` |
| `zerofrom-derive` | 0.1.8 | yes | Unicode-3.0 | `LICENSE`: MIT/Unicode-3.0 | Unicode-3.0 (approved, exact version) | no |  | `f75b4683f6c7f45248d4d64056a24298c6281e0993356d7d1b4a1a962ef10d4a` |
| `zerotrie` | 0.2.5 | yes | Unicode-3.0 | `LICENSE`: MIT/Unicode-3.0 | Unicode-3.0 (approved, exact version) | no |  | `4ea269c3bd32f0a32c321907a2ae912ba6f4649bb0fc764a15627e99a7095a3f` |
| `zerovec` | 0.11.8 | yes | Unicode-3.0 | `LICENSE`: MIT/Unicode-3.0 | Unicode-3.0 (approved, exact version) | no |  | `bb0464e17806c1d976d5cba29399c7f08e516e279e2ba493f63123b5fca67dd8` |
| `zerovec-derive` | 0.11.6 | yes | Unicode-3.0 | `LICENSE`: MIT/Unicode-3.0 | Unicode-3.0 (approved, exact version) | no |  | `34df6fc39dbd26ddc9c10e6a2984476e13acce22e64e4487636ef494369225da` |
| `zmij` | 1.0.23 | yes | MIT | `LICENSE-MIT`: MIT | MIT | no |  | `29666d0abbfad1e3dc4dcf6144730dd3a3ab225bbbdac83319345b1b44ccfc1b` |

## Advisory range evidence

Every RustSec advisory whose package name is in the seeded lockfile, with its ranges as the database states them and the locked version beside the package. The first list is what applies; the second is matched by name but patched or unaffected at the locked version.

### Applicable at the locked versions (1)

- id = "RUSTSEC-2026-0386"; package = "tokio" 1.53.2; informational = "unsound"; patched = ['>= 1.51.6, < 1.52.0', '>= 1.53.3']; unaffected = ['< 0.3.2']; date = 2026-10-13; title = ''

### Matched by name, locked version in a patched or unaffected range (115)

- id = "RUSTSEC-2026-0190"; package = "anyhow" 1.0.104; informational = "unsound"; patched = ['>= 1.0.103']; date = 2026-06-25; title = ''
- id = "RUSTSEC-2020-0006"; package = "bumpalo" 3.20.3; patched = ['>= 3.2.1']; unaffected = ['< 3.0.0']; date = 2020-03-24; title = ''
- id = "RUSTSEC-2022-0078"; package = "bumpalo" 3.20.3; informational = "unsound"; patched = ['>= 3.11.1']; unaffected = ['< 1.1.0']; date = 2022-01-14; title = ''
- id = "RUSTSEC-2026-0007"; package = "bytes" 1.12.1; patched = ['>= 1.11.1']; unaffected = ['< 1.2.1']; date = 2026-02-03; title = ''
- id = "RUSTSEC-2024-0445"; package = "cap-primitives" 4.0.3; patched = ['>= 3.4.1']; date = 2024-11-05; title = ''
- id = "RUSTSEC-2019-0029"; package = "chacha20" 0.10.2; patched = ['>= 0.2.3']; date = 2019-10-22; title = ''
- id = "RUSTSEC-2021-0067"; package = "cranelift-codegen" 0.135.7; patched = ['>= 0.73.1']; date = 2021-05-21; title = ''
- id = "RUSTSEC-2021-0093"; package = "crossbeam-deque" 0.8.8; patched = ['>= 0.7.4, < 0.8.0', '>= 0.8.1']; date = 2021-07-30; title = ''
- id = "RUSTSEC-2026-0204"; package = "crossbeam-epoch" 0.9.21; patched = ['>= 0.9.20']; unaffected = ['< 0.9.0']; date = 2026-07-06; title = ''
- id = "RUSTSEC-2022-0041"; package = "crossbeam-utils" 0.8.23; informational = "unsound"; patched = ['>= 0.8.7']; date = 2022-02-05; title = ''
- id = "RUSTSEC-2020-0060"; package = "futures-task" 0.3.34; patched = ['>= 0.3.6']; unaffected = ['<= 0.2.1']; date = 2020-09-04; title = ''
- id = "RUSTSEC-2020-0061"; package = "futures-task" 0.3.34; patched = ['>= 0.3.5']; date = 2020-05-03; title = ''
- id = "RUSTSEC-2020-0059"; package = "futures-util" 0.3.34; patched = ['>= 0.3.7']; unaffected = ['< 0.3.2']; date = 2020-10-22; title = ''
- id = "RUSTSEC-2020-0062"; package = "futures-util" 0.3.34; patched = ['>= 0.3.2']; unaffected = ['< 0.3.0']; date = 2020-01-24; title = ''
- id = "RUSTSEC-2020-0146"; package = "generic-array" 0.14.7; patched = ['>= 0.8.4, < 0.9.0', '>= 0.9.1, < 0.10.0', '>= 0.10.1, < 0.11.0', '>= 0.11.2, < 0.12.0', '>= 0.12.4, < 0.13.0', '>= 0.13.3']; unaffected = ['< 0.8.0']; date = 2020-04-09; title = ''
- id = "RUSTSEC-2024-0402"; package = "hashbrown" 0.16.1; patched = ['>= 0.15.1']; unaffected = ['< 0.15.0']; date = 2024-10-11; title = ''
- id = "RUSTSEC-2024-0402"; package = "hashbrown" 0.17.1; patched = ['>= 0.15.1']; unaffected = ['< 0.15.0']; date = 2024-10-11; title = ''
- id = "RUSTSEC-2024-0421"; package = "idna" 1.1.0; patched = ['>= 1.0.0']; date = 2024-12-09; title = ''
- id = "RUSTSEC-2020-0081"; package = "mio" 1.2.4; informational = "unsound"; patched = ['>= 0.7.6']; unaffected = ['< 0.7.0']; date = 2020-11-02; title = ''
- id = "RUSTSEC-2024-0019"; package = "mio" 1.2.4; patched = ['>= 0.8.11']; unaffected = ['< 0.7.2']; date = 2024-03-04; title = ''
- id = "RUSTSEC-2019-0017"; package = "once_cell" 1.21.4; patched = ['>= 1.0.1']; unaffected = ['< 0.2.5']; date = 2019-09-01; title = ''
- id = "RUSTSEC-2026-0097"; package = "rand" 0.10.3; informational = "unsound"; patched = ['>= 0.10.1', '< 0.10.0, >= 0.9.3', '< 0.9.0, >= 0.8.6']; unaffected = ['< 0.7.0']; date = 2026-04-09; title = ''
- id = "RUSTSEC-2019-0035"; package = "rand_core" 0.10.1; informational = "unsound"; patched = ['^ 0.3.1', '>= 0.4.2']; date = 2019-04-19; title = ''
- id = "RUSTSEC-2021-0023"; package = "rand_core" 0.10.1; patched = ['>= 0.6.2']; unaffected = ['< 0.6.0']; date = 2021-02-12; title = ''
- id = "RUSTSEC-2021-0100"; package = "sha2" 0.10.9; patched = ['>= 0.9.8']; unaffected = ['< 0.9.7']; date = 2021-09-08; title = ''
- id = "RUSTSEC-2024-0006"; package = "shlex" 2.0.1; patched = ['>= 1.3.0']; date = 2024-01-21; title = ''
- id = "RUSTSEC-2025-0047"; package = "slab" 0.4.12; patched = ['>= 0.4.11']; unaffected = ['< 0.4.10']; date = 2025-08-12; title = ''
- id = "RUSTSEC-2018-0003"; package = "smallvec" 1.16.3; patched = ['>= 0.6.3', '^0.3.4', '^0.4.5', '^0.5.1']; unaffected = ['< 0.3.2']; date = 2018-07-19; title = ''
- id = "RUSTSEC-2018-0018"; package = "smallvec" 1.16.3; informational = "unsound"; patched = ['>= 0.6.13']; date = 2018-09-25; title = ''
- id = "RUSTSEC-2019-0009"; package = "smallvec" 1.16.3; patched = ['>= 0.6.10']; unaffected = ['< 0.6.5']; date = 2019-06-06; title = ''
- id = "RUSTSEC-2019-0012"; package = "smallvec" 1.16.3; patched = ['>= 0.6.10']; unaffected = ['< 0.6.3']; date = 2019-07-19; title = ''
- id = "RUSTSEC-2021-0003"; package = "smallvec" 1.16.3; patched = ['>= 0.6.14, < 1.0.0', '>= 1.6.1']; unaffected = ['< 0.6.3']; date = 2021-01-08; title = ''
- id = "RUSTSEC-2020-0079"; package = "socket2" 0.6.5; informational = "unsound"; patched = ['>= 0.3.16']; date = 2020-11-06; title = ''
- id = "RUSTSEC-2021-0072"; package = "tokio" 1.53.2; patched = ['>= 1.5.1, < 1.6.0', '>= 1.6.3, < 1.7.0', '>= 1.7.2, < 1.8.0', '>= 1.8.1']; unaffected = ['< 0.3.0']; date = 2021-07-07; title = ''
- id = "RUSTSEC-2021-0124"; package = "tokio" 1.53.2; patched = ['>= 1.8.4, < 1.9.0', '>= 1.13.1']; unaffected = ['< 0.1.14']; date = 2021-11-16; title = ''
- id = "RUSTSEC-2023-0001"; package = "tokio" 1.53.2; patched = ['>= 1.18.4, < 1.19.0', '>= 1.20.3, < 1.21.0', '>= 1.23.1']; unaffected = ['< 1.7.0']; date = 2023-01-04; title = ''
- id = "RUSTSEC-2023-0005"; package = "tokio" 1.53.2; informational = "unsound"; patched = ['>= 1.18.5, < 1.19.0', '>= 1.20.4, < 1.21.0', '>= 1.24.2']; unaffected = ['< 0.2.0']; date = 2023-01-11; title = ''
- id = "RUSTSEC-2025-0023"; package = "tokio" 1.53.2; informational = "unsound"; patched = ['>= 1.38.2, < 1.39.0', '>= 1.42.1, < 1.43.0', '>= 1.43.1, < 1.44.0', '>= 1.44.2']; unaffected = ['< 0.2.5']; date = 2025-04-07; title = ''
- id = "RUSTSEC-2026-0384"; package = "tokio" 1.53.2; patched = ['>= 1.54.1']; unaffected = ['< 1.54.0']; date = 2026-10-21; title = ''
- id = "RUSTSEC-2023-0078"; package = "tracing" 0.1.44; informational = "unsound"; patched = ['>= 0.1.40']; unaffected = ['<= 0.1.37']; date = 2023-10-19; title = ''
- id = "RUSTSEC-2021-0110"; package = "wasmtime" 48.0.7; patched = ['>= 0.30.0']; date = 2021-09-17; title = ''
- id = "RUSTSEC-2022-0016"; package = "wasmtime" 48.0.7; patched = ['>= 0.35.2', '>= 0.34.2, < 0.35.0']; unaffected = ['< 0.34.0']; date = 2022-03-31; title = ''
- id = "RUSTSEC-2022-0075"; package = "wasmtime" 48.0.7; patched = ['>= 1.0.2, < 2.0.0', '>= 2.0.2']; date = 2022-11-10; title = ''
- id = "RUSTSEC-2022-0076"; package = "wasmtime" 48.0.7; patched = ['>= 1.0.2, < 2.0.0', '>= 2.0.2']; date = 2022-11-10; title = ''
- id = "RUSTSEC-2022-0095"; package = "wasmtime" 48.0.7; patched = ['>= 0.38.1']; date = 2022-06-27; title = ''
- id = "RUSTSEC-2022-0096"; package = "wasmtime" 48.0.7; patched = ['>= 0.33.1, < 0.34.0', '>= 0.34.1']; date = 2022-02-17; title = ''
- id = "RUSTSEC-2022-0097"; package = "wasmtime" 48.0.7; patched = ['>= 1.0.2, < 2.0.0', '>= 2.0.2']; date = 2022-11-07; title = ''
- id = "RUSTSEC-2022-0098"; package = "wasmtime" 48.0.7; patched = ['>= 1.0.2, < 2.0.0', '>= 2.0.2']; date = 2022-11-05; title = ''
- id = "RUSTSEC-2022-0099"; package = "wasmtime" 48.0.7; patched = ['>= 0.35.2', '>= 0.34.2, < 0.35.0']; date = 2022-03-28; title = ''
- id = "RUSTSEC-2022-0100"; package = "wasmtime" 48.0.7; patched = ['>= 0.38.2']; date = 2022-07-12; title = ''
- id = "RUSTSEC-2022-0101"; package = "wasmtime" 48.0.7; patched = ['>= 0.38.2']; date = 2022-07-05; title = ''
- id = "RUSTSEC-2022-0102"; package = "wasmtime" 48.0.7; patched = ['>= 1.0.2, < 2.0.0', '>= 2.0.2']; date = 2022-11-05; title = ''
- id = "RUSTSEC-2023-0090"; package = "wasmtime" 48.0.7; patched = ['>= 4.0.1, < 5.0.0', '>= 5.0.1, < 6.0.0', '>= 6.0.1']; date = 2023-03-02; title = ''
- id = "RUSTSEC-2023-0091"; package = "wasmtime" 48.0.7; patched = ['>= 10.0.2, < 11.0.0', '>= 11.0.2, < 12.0.0', '>= 12.0.2']; date = 2023-09-05; title = ''
- id = "RUSTSEC-2023-0092"; package = "wasmtime" 48.0.7; patched = ['>= 6.0.2, < 7.0.0', '>= 7.0.1, < 8.0.0', '>= 8.0.1']; date = 2023-04-21; title = ''
- id = "RUSTSEC-2023-0093"; package = "wasmtime" 48.0.7; patched = ['>= 4.0.1, < 5.0.0', '>= 5.0.1, < 6.0.0', '>= 6.0.1']; date = 2023-03-03; title = ''
- id = "RUSTSEC-2024-0438"; package = "wasmtime" 48.0.7; patched = ['>= 24.0.2, < 25.0.0', '>= 25.0.3, < 26.0.0', '>= 26.0.1']; date = 2024-11-02; title = ''
- id = "RUSTSEC-2024-0439"; package = "wasmtime" 48.0.7; patched = ['>= 21.0.2, < 22.0.0', '>= 22.0.1, < 23.0.0', '>= 23.0.3, < 24.0.0', '>= 24.0.1, < 25.0.0', '>= 25.0.2']; unaffected = ['< 19.0.0']; date = 2024-10-03; title = ''
- id = "RUSTSEC-2024-0440"; package = "wasmtime" 48.0.7; patched = ['>= 21.0.2, < 22.0.0', '>= 22.0.1, < 23.0.0', '>= 23.0.3, < 24.0.0', '>= 24.0.1, < 25.0.0', '>= 25.0.2']; unaffected = ['< 21.0.0']; date = 2024-10-02; title = ''
- id = "RUSTSEC-2024-0441"; package = "wasmtime" 48.0.7; patched = ['>= 19.0.1']; unaffected = ['< 19.0.0']; date = 2024-04-02; title = ''
- id = "RUSTSEC-2025-0046"; package = "wasmtime" 48.0.7; patched = ['>= 34.0.2', '>= 33.0.2, < 34.0.0', '>= 24.0.4, < 25.0.0']; unaffected = ['< 10.0.0']; date = 2025-07-18; title = ''
- id = "RUSTSEC-2025-0112"; package = "wasmtime" 48.0.7; patched = ['>= 38.0.3']; unaffected = ['< 38.0.0']; date = 2025-07-18; title = ''
- id = "RUSTSEC-2025-0118"; package = "wasmtime" 48.0.7; patched = ['>= 38.0.4', '>= 37.0.3, < 38.0.0', '>= 36.0.3, < 37.0.0', '>= 24.0.5, < 25.0.0']; date = 2025-11-11; title = ''
- id = "RUSTSEC-2026-0006"; package = "wasmtime" 48.0.7; patched = ['>= 41.0.1', '>= 40.0.3, < 41.0.0', '>= 36.0.5, < 37.0.0', '< 29.0.0']; date = 2026-01-26; title = ''
- id = "RUSTSEC-2026-0020"; package = "wasmtime" 48.0.7; patched = ['>= 24.0.6, < 25.0.0', '>= 36.0.6, < 37.0.0', '>= 40.0.4, < 41.0.0', '>= 41.0.4']; date = 2026-02-24; title = ''
- id = "RUSTSEC-2026-0021"; package = "wasmtime" 48.0.7; patched = ['>= 24.0.6, < 25.0.0', '>= 36.0.6, < 37.0.0', '>= 40.0.4, < 41.0.0', '>= 41.0.4']; date = 2026-02-24; title = ''
- id = "RUSTSEC-2026-0022"; package = "wasmtime" 48.0.7; patched = ['>= 40.0.4, < 41.0.0', '>= 41.0.4']; unaffected = ['< 39.0.0']; date = 2026-02-24; title = ''
- id = "RUSTSEC-2026-0085"; package = "wasmtime" 48.0.7; patched = ['>= 24.0.7, < 25.0.0', '>= 36.0.7, < 37.0.0', '>= 42.0.2, < 43.0.0', '>= 43.0.1']; date = 2026-04-09; title = ''
- id = "RUSTSEC-2026-0086"; package = "wasmtime" 48.0.7; patched = ['>= 36.0.7, < 37.0.0', '>= 42.0.2, < 43.0.0', '>= 43.0.1']; date = 2026-04-09; title = ''
- id = "RUSTSEC-2026-0087"; package = "wasmtime" 48.0.7; patched = ['>= 24.0.7, < 25.0.0', '>= 36.0.7, < 37.0.0', '>= 42.0.2, < 43.0.0', '>= 43.0.1']; date = 2026-04-09; title = ''
- id = "RUSTSEC-2026-0088"; package = "wasmtime" 48.0.7; patched = ['>= 36.0.7, < 37.0.0', '>= 42.0.2, < 43.0.0', '>= 43.0.1']; date = 2026-04-09; title = ''
- id = "RUSTSEC-2026-0089"; package = "wasmtime" 48.0.7; patched = ['>= 36.0.7, < 37.0.0', '>= 42.0.2, < 43.0.0', '>= 43.0.1']; date = 2026-04-09; title = ''
- id = "RUSTSEC-2026-0090"; package = "wasmtime" 48.0.7; patched = ['>= 43.0.1']; unaffected = ['< 43.0.0']; date = 2026-04-09; title = ''
- id = "RUSTSEC-2026-0091"; package = "wasmtime" 48.0.7; patched = ['>= 24.0.7, < 25.0.0', '>= 36.0.7, < 37.0.0', '>= 42.0.2, < 43.0.0', '>= 43.0.1']; date = 2026-04-09; title = ''
- id = "RUSTSEC-2026-0092"; package = "wasmtime" 48.0.7; patched = ['>= 24.0.7, < 25.0.0', '>= 36.0.7, < 37.0.0', '>= 42.0.2, < 43.0.0', '>= 43.0.1']; date = 2026-04-09; title = ''
- id = "RUSTSEC-2026-0093"; package = "wasmtime" 48.0.7; patched = ['>= 24.0.7, < 25.0.0', '>= 36.0.7, < 37.0.0', '>= 42.0.2, < 43.0.0', '>= 43.0.1']; date = 2026-04-09; title = ''
- id = "RUSTSEC-2026-0094"; package = "wasmtime" 48.0.7; patched = ['>= 36.0.7, < 37.0.0', '>= 42.0.2, < 43.0.0', '>= 43.0.1']; date = 2026-04-09; title = ''
- id = "RUSTSEC-2026-0095"; package = "wasmtime" 48.0.7; patched = ['>= 36.0.7, < 37.0.0', '>= 42.0.2, < 43.0.0', '>= 43.0.1']; date = 2026-04-09; title = ''
- id = "RUSTSEC-2026-0096"; package = "wasmtime" 48.0.7; patched = ['>= 36.0.7, < 37.0.0', '>= 42.0.2, < 43.0.0', '>= 43.0.1']; date = 2026-04-09; title = ''
- id = "RUSTSEC-2026-0114"; package = "wasmtime" 48.0.7; patched = ['>= 36.0.8, < 37.0.0', '>= 43.0.2, < 44.0.0', '>= 44.0.1']; unaffected = ['< 30.0.0']; date = 2026-04-30; title = ''
- id = "RUSTSEC-2026-0222"; package = "wasmtime" 48.0.7; patched = ['>= 24.0.12, < 25.0.0', '>= 36.0.13, < 37.0.0', '>= 46.0.2, < 47.0.0', '>= 47.0.3']; date = 2026-07-31; title = ''
- id = "RUSTSEC-2026-0223"; package = "wasmtime" 48.0.7; patched = ['>= 46.0.2, < 47.0.0', '>= 47.0.3']; unaffected = ['< 46.0.0']; date = 2026-07-31; title = ''
- id = "RUSTSEC-2026-0268"; package = "wasmtime" 48.0.7; patched = ['>= 46.0.3, < 47.0.0', '>= 47.0.4']; unaffected = ['< 46.0.0']; date = 2026-08-20; title = ''
- id = "RUSTSEC-2026-0269"; package = "wasmtime" 48.0.7; patched = ['>= 24.0.13, < 25.0.0', '>= 36.0.14, < 37.0.0', '>= 46.0.3, < 47.0.0', '>= 47.0.4']; date = 2026-08-20; title = ''
- id = "RUSTSEC-2026-0315"; package = "wasmtime" 48.0.7; patched = ['>= 48.0.3, < 49.0.0', '>= 49.0.1']; unaffected = ['< 47.0.0']; date = 2026-09-24; title = ''
- id = "RUSTSEC-2026-0316"; package = "wasmtime" 48.0.7; patched = ['>= 36.0.16, < 37.0.0', '>= 48.0.3, < 49.0.0', '>= 49.0.1']; date = 2026-09-24; title = ''
- id = "RUSTSEC-2026-0325"; package = "wasmtime" 48.0.7; patched = ['>= 48.0.4, < 49.0.0', '>= 49.0.2']; unaffected = ['< 47.0.0']; date = 2026-10-02; title = ''
- id = "RUSTSEC-2026-0326"; package = "wasmtime" 48.0.7; patched = ['>= 48.0.4, < 49.0.0', '>= 49.0.2']; unaffected = ['< 47.0.0']; date = 2026-10-02; title = ''
- id = "RUSTSEC-2026-0327"; package = "wasmtime" 48.0.7; patched = ['>= 48.0.4, < 49.0.0', '>= 49.0.2']; unaffected = ['< 39.0.0']; date = 2026-10-02; title = ''
- id = "RUSTSEC-2026-0353"; package = "wasmtime" 48.0.7; patched = ['>= 48.0.6, < 49.0.0', '>= 49.0.3']; unaffected = ['< 46.0.0']; date = 2026-10-14; title = ''
- id = "RUSTSEC-2026-0354"; package = "wasmtime" 48.0.7; patched = ['>= 48.0.6, < 49.0.0', '>= 49.0.3']; unaffected = ['< 46.0.0']; date = 2026-10-14; title = ''
- id = "RUSTSEC-2026-0355"; package = "wasmtime" 48.0.7; patched = ['>= 48.0.6, < 49.0.0', '>= 49.0.3']; unaffected = ['< 46.0.0']; date = 2026-10-14; title = ''
- id = "RUSTSEC-2026-0356"; package = "wasmtime" 48.0.7; patched = ['>= 48.0.6, < 49.0.0', '>= 49.0.3']; unaffected = ['< 46.0.0']; date = 2026-10-14; title = ''
- id = "RUSTSEC-2026-0357"; package = "wasmtime" 48.0.7; patched = ['>= 48.0.6, < 49.0.0', '>= 49.0.3']; unaffected = ['< 46.0.0']; date = 2026-10-14; title = ''
- id = "RUSTSEC-2026-0358"; package = "wasmtime" 48.0.7; patched = ['>= 48.0.6, < 49.0.0', '>= 49.0.3']; unaffected = ['< 46.0.0']; date = 2026-10-14; title = ''
- id = "RUSTSEC-2026-0359"; package = "wasmtime" 48.0.7; patched = ['>= 48.0.6, < 49.0.0', '>= 49.0.3']; unaffected = ['< 42.0.0']; date = 2026-10-14; title = ''
- id = "RUSTSEC-2026-0360"; package = "wasmtime" 48.0.7; patched = ['>= 49.0.3']; unaffected = ['< 49.0.0']; date = 2026-10-14; title = ''
- id = "RUSTSEC-2026-0361"; package = "wasmtime" 48.0.7; patched = ['>= 48.0.6, < 49.0.0', '>= 49.0.3']; unaffected = ['< 46.0.0']; date = 2026-10-14; title = ''
- id = "RUSTSEC-2026-0362"; package = "wasmtime" 48.0.7; patched = ['>= 48.0.6, < 49.0.0', '>= 49.0.3']; unaffected = ['< 42.0.0']; date = 2026-10-14; title = ''
- id = "RUSTSEC-2026-0363"; package = "wasmtime" 48.0.7; patched = ['>= 36.0.18, < 37.0.0', '>= 48.0.6, < 49.0.0', '>= 49.0.3']; date = 2026-10-14; title = ''
- id = "RUSTSEC-2026-0364"; package = "wasmtime" 48.0.7; patched = ['>= 48.0.6, < 49.0.0', '>= 49.0.3']; unaffected = ['< 47.0.0']; date = 2026-10-14; title = ''
- id = "RUSTSEC-2026-0378"; package = "wasmtime" 48.0.7; patched = ['>= 48.0.7, < 49.0.0', '>= 49.0.4']; unaffected = ['< 46.0.0']; date = 2026-10-19; title = ''
- id = "RUSTSEC-2026-0379"; package = "wasmtime" 48.0.7; patched = ['>= 36.0.19, < 37.0.0', '>= 48.0.7, < 49.0.0', '>= 49.0.4']; date = 2026-10-19; title = ''
- id = "RUSTSEC-2026-0380"; package = "wasmtime" 48.0.7; patched = ['>= 36.0.19, < 37.0.0', '>= 43.0.0']; date = 2026-10-19; title = ''
- id = "RUSTSEC-2026-0149"; package = "wasmtime-wasi" 48.0.7; patched = ['>= 24.0.9, < 25.0.0', '>= 36.0.10, < 37.0.0', '>= 44.0.2, < 45.0.0', '>= 45.0.0']; date = 2026-05-21; title = ''
- id = "RUSTSEC-2026-0182"; package = "wasmtime-wasi" 48.0.7; patched = ['>= 45.0.2', '>= 44.0.3, < 45.0.0', '>= 36.0.11, < 37.0.0', '>= 24.0.10, < 25.0.0']; date = 2026-06-15; title = ''
- id = "RUSTSEC-2026-0188"; package = "wasmtime-wasi" 48.0.7; patched = ['>= 46.0.1', '>= 45.0.3, < 46.0.0', '>= 36.0.12, < 37.0.0', '>= 24.0.11, < 25.0.0']; date = 2026-06-24; title = ''
- id = "RUSTSEC-2026-0314"; package = "wasmtime-wasi" 48.0.7; patched = ['>= 36.0.16, < 37.0.0', '>= 48.0.3, < 49.0.0', '>= 49.0.1']; date = 2026-09-24; title = ''
- id = "RUSTSEC-2026-0321"; package = "wasmtime-wasi" 48.0.7; patched = ['>= 36.0.17, < 37.0.0', '>= 48.0.4, < 49.0.0', '>= 49.0.2']; date = 2026-10-02; title = ''
- id = "RUSTSEC-2026-0322"; package = "wasmtime-wasi" 48.0.7; patched = ['>= 36.0.17, < 37.0.0', '>= 48.0.4, < 49.0.0', '>= 49.0.2']; date = 2026-10-02; title = ''
- id = "RUSTSEC-2026-0323"; package = "wasmtime-wasi" 48.0.7; patched = ['>= 36.0.17, < 37.0.0', '>= 48.0.4, < 49.0.0', '>= 49.0.2']; date = 2026-10-02; title = ''
- id = "RUSTSEC-2026-0324"; package = "wasmtime-wasi" 48.0.7; patched = ['>= 48.0.4, < 49.0.0', '>= 49.0.2']; unaffected = ['< 46.0.0']; date = 2026-10-02; title = ''
- id = "RUSTSEC-2026-0352"; package = "wasmtime-wasi" 48.0.7; patched = ['>= 36.0.18, < 37.0.0', '>= 48.0.6, < 49.0.0', '>= 49.0.3']; date = 2026-10-14; title = ''
- id = "RUSTSEC-2024-0347"; package = "zerovec" 0.11.8; patched = ['>= 0.10.4', '>= 0.9.7, <0.10.0']; date = 2024-07-01; title = ''
- id = "RUSTSEC-2024-0346"; package = "zerovec-derive" 0.11.6; patched = ['>= 0.10.3', '>= 0.9.7, <0.10.0']; date = 2024-07-01; title = ''

