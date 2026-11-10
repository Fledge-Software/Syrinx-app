# Notes' parser candidates for `wasm32-wasip2`

Notes parses Markdown and frontmatter in its own Rust, compiled to
WebAssembly for the `wasm32-wasip2` target, inside the plugin (#36, P3-02;
research note 10). #36 names `pulldown-cmark` and `serde-saphyr` as the
plan's candidates, not pins, and asks for an exact-version audit of each:
features, maintenance, security record and every transitive licence under
Syrinx's rule (permissive licences or whole MPL-2.0 files, no GPL). #98 lists
that audit as paper work that can start now.

Neither crate is added to any `Cargo.toml` in this repository: they compile
into the Notes plugin, which lives in `Fledge-Software/Syrinx-Notes-Plugin` and
keeps its own audit (`docs/dependency-audit.md`, "Notes 1 additions", written
2026-10-24) and `THIRD-PARTY-LICENSES`. This file is the host side's
independent check at the versions current on 2026-11-10, so #36's review of
the plugin has something to compare against. It found one problem in the
plugin's current pin; see "pulldown-cmark".

## Method

Done on 2026-11-10 on the development Linux machine with Rust 1.97.1, the
version `rust-toolchain.toml` pins, and its installed `wasm32-wasip2` target.

1. Versions, licences, yanks and publication dates came from the crates.io
   API.
2. For each crate a throwaway program outside the repository depended on it
   alone at an exact version, with the features below. `cargo tree --target
   wasm32-wasip2 -e normal,build` and `cargo metadata --filter-platform
   wasm32-wasip2` gave the closure and every licence.
3. Each program was built with `cargo build --release --target wasm32-wasip2`
   (`opt-level = "s"`, link-time optimisation, one codegen unit, stripped,
   `panic = "abort"`) and its size compared with a program that only prints
   its argument's length (66,590 bytes). The pulldown-cmark program enables
   wikilinks and YAML metadata blocks and walks `into_offset_iter`; the
   serde-saphyr program deserialises a string map.
4. The RustSec advisory database was read at commit
   `d9cb059f3217db87aeccd190b81eaffe8cc29cb9` (2026-11-09) for every crate in
   both closures. `cargo-audit` is not installed on this machine, and
   installing software is the owner's.
5. Each crate's `unsafe` policy and build script were read in its published
   source.

## pulldown-cmark

**Recommended pin: `pulldown-cmark = { version = "=0.13.6", default-features = false }`.**

| Crate | Version | Licence | Note |
|---|---|---|---|
| `pulldown-cmark` | 0.13.6 | MIT | crates.io checksum `3db2be46835f013dd3f4fa7f5f7402c537787b54ebb6fe2b94bbdd7e2669bb7b`, published 2026-10-30 |
| `bitflags` | 2.13.2 | MIT OR Apache-2.0 | `Options` |
| `memchr` | 2.8.3 | Unlicense OR MIT | byte search; Syrinx elects MIT |
| `unicase` | 2.10.0 | MIT OR Apache-2.0 | case folding for link references |

- **Features.** The defaults are `getopts` (a command-line helper) and `html`
  (an HTML renderer and `pulldown-cmark-escape`); Notes needs neither, so
  `default-features = false` keeps both out. `simd` stays off.
- **Unsafe code.** `src/lib.rs` has `#![cfg_attr(not(feature = "simd"),
  forbid(unsafe_code))]`, so the crate as configured has none.
- **Build script.** `build.rs` generates spec tests only under the
  `gen-tests` feature; otherwise it compiles to nothing.
- **Builds for `wasm32-wasip2`:** yes. It adds about 200 KB (266,218 bytes
  against the 66,590-byte baseline).
- **Maintenance.** The 0.13 line has had seven releases since 2025-02,
  0.13.5 on 2026-10-21 and 0.13.6 on 2026-10-30, published by a maintainer of
  the `pulldown-cmark/pulldown-cmark` organisation. The crate has about 200
  million downloads.
- **Security.** RustSec holds two advisories, both denial of service when
  untrusted Markdown is parsed with `Options::ENABLE_WIKILINKS`, which Notes
  turns on:
  - RUSTSEC-2026-0414 (2026-10-18): exponential event growth from a piped
    wikilink with empty text (`[[a|]]`); patched in 0.13.5.
  - RUSTSEC-2026-0433 (2026-10-28): quadratic parsing of nested image
    wikilinks (`![[![[…]]]]`); patched in **0.13.6**.

  ⚠️ **The Notes plugin pins `=0.13.5`** (its `Cargo.toml`, audited
  2026-10-24, before RUSTSEC-2026-0433 was published), so it is affected by
  the second advisory. A vault note is untrusted input to the plugin, and a
  quadratic parse would spend the per-call budget #135 sets. Moving the
  plugin's pin to 0.13.6 is a change in the Notes repository, not here; it is
  recorded for #36's review.

## serde-saphyr

**Recommended pin if it is used: `serde-saphyr = { version = "=2.1.1", default-features = false, features = ["deserialize"] }`.**
The Notes plugin chose not to use it (below).

| Crate | Version | Licence | Note |
|---|---|---|---|
| `serde-saphyr` | 2.1.1 | MIT OR Apache-2.0 | crates.io checksum `d43fb82d550b07e6f929a58d7d0935182602f48ddead875f5ee313c41c8f07fe`, published 2026-11-05 |
| `granit-parser` | 2.1.0 | MIT OR Apache-2.0 | the author's own YAML parser, a fork of `saphyr-parser` |
| `annotate-snippets` | 0.12.17 | MIT OR Apache-2.0 | error snippets |
| `anstyle` | 1.0.14 | MIT OR Apache-2.0 | |
| `unicode-width` | 0.2.3 | MIT OR Apache-2.0 | |
| `encoding_rs_io` | 0.1.8 | MIT OR Apache-2.0 | |
| `encoding_rs` | 0.8.44 | (Apache-2.0 OR MIT) AND BSD-3-Clause | both sides allowed; build script |
| `cfg-if` | 1.0.5 | MIT OR Apache-2.0 | |
| `scopeguard` | 1.2.0 | MIT OR Apache-2.0 | new in encoding_rs 0.8.44 |
| `multiversion_no_op` | 1.0.0 | Apache-2.0 OR MIT | procedural macro, build machine only |
| `num-traits` | 0.2.19 | MIT OR Apache-2.0 | build script |
| `serde_core` | 1.0.229 | MIT OR Apache-2.0 | build script |
| `smallvec` | 1.16.5 | MIT OR Apache-2.0 | |
| `arraydeque` | 0.5.1 | MIT/Apache-2.0 | read as a choice |
| `rustversion` | 1.0.23 | MIT OR Apache-2.0 | procedural macro, build machine only |
| `autocfg` | 1.5.1 | Apache-2.0 OR MIT | build dependency of `num-traits` |

- **Features.** The defaults add `serialize` (with `base64`, `zmij` and
  `nohash-hasher`) and `comments`; reading frontmatter needs only
  `deserialize`.
- **Unsafe code.** `serde-saphyr` and `granit-parser` both declare
  `#![forbid(unsafe_code)]`.
- **Builds for `wasm32-wasip2`:** yes. It adds about 564 KB (630,517 bytes
  against the baseline), most of it `encoding_rs`'s tables.
- **Maintenance.** Fast-moving: 1.0.0 on 2026-07-31, 2.0.0 on 2026-10-13,
  2.1.0 on 2026-11-03 (yanked without a message) and 2.1.1 on 2026-11-05,
  each major version within weeks of the last. The exact pin absorbs that,
  but every move is a re-audit. `granit-parser` is maintained by the same
  author and has a small following.
- **Security.** RustSec holds no advisory for `serde-saphyr`, `granit-parser`
  or any crate above except `smallvec`, whose five advisories (2018 to 2021)
  were all patched by 1.6.1; 1.16.5 is unaffected.
- **Fit.** #36's "Watch" notes that it does not capture freestanding comments
  and gives byte offsets only for string input, so Notes must keep the
  original text. serde deserialises into a typed schema, while P3-02 indexes
  property values as written.

**The Notes plugin chose `saphyr-parser` instead.** Its audit rejected
`serde-saphyr 2.0.0` for the fit above and its size, and pinned
`saphyr-parser =0.2.2` (MIT OR Apache-2.0; with `arraydeque` and `thiserror`),
an event parser whose spans are byte offsets. This review agrees: nothing in
2.1.1 changes the reason. RustSec holds no advisory for `saphyr-parser`,
`arraydeque` or `thiserror`. `saphyr-parser` has since moved to 0.3.x (0.3.3 on
2026-10-27); whether to follow is the plugin repository's reviewed change.

Syrinx's own `Cargo.lock` already holds `serde-saphyr 1.3.0`, through
`rust-i18n-support`, the translation loader under the interface component
library. That is a host dependency for the Windows and Linux targets, judged
by `cargo xtask check-licences`; this file neither covers nor changes it.

## Still open

- ⚠️ **Parse speed inside WebAssembly is unmeasured** (#36; research note 10).
  The Notes repository measures natively (`docs/workloads.md` there) and will
  measure in WebAssembly once the test host can feed a component a vault (the
  `documents` import). The synthetic workloads are chosen in
  `docs/dev/notes-vault-workloads.md`.
- `comrak` and `markdown-rs`, the alternatives in research note 04, section 7,
  were not audited: pulldown-cmark builds and gives the byte offsets P3-02
  needs.
