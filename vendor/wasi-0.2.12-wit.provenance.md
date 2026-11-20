# WASI 0.2.12 WIT, as copied from wasmtime-wasi 48.0.7

Syrinx's plugin host provides the WASI 0.2 interfaces that Rust's standard
library imports for `wasm32-wasip2` itself, instead of depending on
`wasmtime-wasi` (owner decision G1 of 2026-10-26 in `docs/decisions.md`).
wasmtime's `bindgen!` needs those interfaces as WIT text to generate the host
traits, so the owner approved committing the four files here under
`Apache-2.0 WITH LLVM-exception` (decision G2 of 2026-10-26). They are
interface definitions, not code; the reasoning and the alternatives that were
rejected are in `docs/dev/vendor-reviews/wasmtime-wit-prebuild-check.md`,
"(b) Drop `wasmtime-wasi`".

## Source

The published `wasmtime-wasi 48.0.7` archive from crates.io
(`https://static.crates.io/crates/wasmtime-wasi/wasmtime-wasi-48.0.7.crate`,
SHA-256 `3ef0d3ec88f9406bc5e498848489c49ff515577500acc9f03fa93d0883dce435`),
folder `src/p2/wit/deps/`. wasmtime's `ci/vendor-wit.sh` at `v48.0.7` fetched
those files from the WASI 0.2.12 packages (`wasi:io`, `wasi:clocks`,
`wasi:random` and `wasi:cli` at `0.2.12`), which WASI publishes as
`Apache-2.0 WITH LLVM-exception`. The WASI specification itself is published
by the W3C WebAssembly Community Group (`WebAssembly/WASI`, tag `v0.2.12`);
this note names that specification and version, as the W3C Community
Contributor License Agreement asks of derivative works.

| File | Lines | SHA-256 as committed | Change |
|---|---|---|---|
| `io.wit` | 299 | `96e206d00076fa0480df32c5bcf255a3fa4862805ac2f6b8537a781cce54f433` | none |
| `clocks.wit` | 162 | `6ed8aa65bb8cbe224a0b2cbac9fc1b3bd25bdb17eda5ae0d23c983ed31c447cc` | none |
| `random.wit` | 92 | `48578c40213d5cab6650980905fa146a0be8c39c4433ee5e7e00637b87dbe08f` | none |
| `cli.wit` | 144 | `5bb6c55aeb5662dd7697dd5774a623d06003e95ed6e99c751bbb2aeec6ac8200` | cut, with a notice |
| `LICENSE` | — | `268872b9816f90fd8e85db5a28d33f8150ebb8dd016653fb39ef1f94f2686bc5` | none (the crate's) |

`cli.wit` is the original's lines 1 to 139 (original SHA-256
`fb6ac5d23fcaf3d231142a3c2f9bb1e9bc1fe5af6f1f329f6b1a5555ce0d3873`; the cut
alone, before the notice, is
`b3434fc7c04a09a94521d789999b686e4c947ba596a8b976fc9d394ca42a9ca3`, the
figure the pre-build check recorded). The two worlds after line 139,
`imports` and `command`, were cut because they import the filesystem, sockets
and the full `wasi:random` interfaces, which Syrinx never links for a plugin.
A five-line change notice is prepended, as section 4(b) of the Apache License
asks of a modified file. The other three files and `LICENSE` are byte for byte
as published, and `.gitattributes` keeps them so.

`filesystem.wit` and `sockets.wit` from the same folder are deliberately not
copied: a plugin that imports them is refused at load.

## Updating

Moving to a later WASI 0.2 patch means copying the same files from the
`wasmtime-wasi` release that matches Syrinx's wasmtime pin, cutting `cli.wit`'s
worlds again, renaming this folder and updating this note and the hashes.
Guests built for an older 0.2 patch keep loading, because wasmtime's linker
serves `@0.2.9` imports from `@0.2.12` definitions by semver matching.
