> **Research note dated 2026-10-20.** Read for reasoning, not current fact; later decisions in [`docs/decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/decisions.md) and [`docs/architecture-decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/architecture-decisions.md) win.
>
> Written for [#26](https://github.com/Fledge-Software/Syrinx-app/issues/26) (epic [#25](https://github.com/Fledge-Software/Syrinx-app/issues/25)). It proposes no dependency pin: adding wasmtime or any other crate is a separate audited step, and nothing here is built until the owner has reviewed this note. Anything marked ⚠️ is unverified and is checked before anything is built on it.

# The WASM plugin runtime: runtime, interface, capabilities, interface views and distribution

**Date:** 2026-10-20

**Status:** Research note 10. It answers the questions #26 sets before #27 re-cuts the slices of Plan 2 to 4: which WebAssembly runtime Syrinx embeds, what crosses the plugin boundary and how it is versioned, which powers the host offers and how the owner grants them, how a plugin's interface is drawn and talks to its module, and how plugins are packaged, installed, updated and reviewed. It was assembled from five research reports written the same day. Sources were read on 2026-10-20. Measurements were taken on a 4-core **Linux** x86_64 virtual machine, not on the owner's Windows 11 machine; every such number is labelled and is re-measured on Windows in #28. Nothing under `crates/` was changed and nothing was built in the Syrinx repository.

## Terms

- **WebAssembly (WASM)** is a portable bytecode. A **runtime** is the library inside `syrinx.exe` that loads a plugin's `.wasm` file, checks it, compiles it to machine code (or interprets it) and runs it inside a **sandbox**: the module can touch nothing outside its own memory except through functions the host hands it.
- The **host** is Syrinx itself, the Rust program that loads and runs plugins. The **guest** is the plugin's WASM module running inside it.
- A **WASM component** is a WASM binary in the **component model** format, the WebAssembly standard for modules that exchange typed values (strings, records, lists, variants and resources) rather than raw bytes.
- **WIT** (WebAssembly Interface Types) is the interface language of the component model. A WIT **world** lists what a component **imports** (here, what Syrinx offers the plugin) and **exports** (what the plugin offers Syrinx). The host side (wasmtime's `bindgen!`) and the guest side (`wit-bindgen`) are both generated from the same `.wit` files, so the two cannot disagree about a type without the build or the load failing.
- **WASI 0.2** (also called Preview 2) is the WebAssembly System Interface built on the component model: standard WIT interfaces for clocks, random numbers, standard input and output, files and sockets. It shipped in January 2024. **WASI 0.3** adds native async and is the current preview.
- A **capability** is a named, scoped power the guest can use only if the host hands it over, for example "read and write this one folder". In capability-based security a program holds no ambient authority. A **preopen** is WASI's form of this: a directory the host opens before the guest starts and passes in as a handle, the only way a WASI guest gets filesystem access.
- **Epoch interruption** is wasmtime's cheap way to stop running guest code: compiled code checks a global counter (the epoch) against a per-store deadline, the host advances the counter, and when the deadline passes the guest traps (stops with an unrecoverable error). **Fuel metering** is the deterministic, more expensive alternative that counts instructions.
- A **web view** is an embedded browser surface. Syrinx has **page web views**, which show the web and carry the rules that keep Google working, and will now have **plugin views**, Syrinx's own local web views that show a plugin's HTML. **IPC** (inter-process communication) here means a message channel from script in a web view into Rust. Page web views never get one ([`AGENTS.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/AGENTS.md), "Page web views and Google").
- **Full trust** describes a capability whose power equals running arbitrary programs as the owner, such as starting a shell. Once it is granted, no sandbox stands between the plugin and the owner's files.

## Why this research exists

On 2026-10-15 the owner decided that every Syrinx plugin, Forge, Notes and Fledge included, is Rust compiled to WebAssembly, built from a template repository with each plugin in its own repository, and run by Syrinx in a sandbox from the start ([`docs/decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/decisions.md), 2026-10-15). A plugin "reaches files, git, processes and the network only through host functions its manifest asks for and the owner grants (the Chrome extension model), and draws its interface in HTML and JavaScript inside Syrinx's own local web view, as Notes and Forge already planned for CodeMirror (D28)". The decision changed two settled decisions: D17's sandbox is no longer deferred until someone else writes a plugin, and D34's "`Plugin` trait" becomes a WIT interface and host. The same entry requires a research note "before Plan 2's slice breakdown", and the owner chose all this after being told the cost: "Forge needs the host to provide real capabilities (terminals, git, language servers, tree-sitter), as Zed's host does."

Three things make getting this right before building matter more than usual.

- **The built-ins use the third-party API.** `AGENTS.md` already says built-in plugins use the same API a third party will. With WASM that becomes literal: Notes and Forge are loaded from their own repositories' packages through the same host, manifest, review prompt and grant store as any later plugin. There is no private side door for the built-ins, so whatever the interface gets wrong, the built-ins get wrong too.
- **The interface is expensive to change.** Every plugin, the template (#30) and every later third-party plugin is written against it. A mistake fixed later means a new major version and one more compatibility layer kept in the host for as long as old plugins load. Zed now carries ten generated binding modules, `since_v0_0_1` to `since_v0_8_0`, with a branch for each when it loads an extension ([`crates/extension_host/src/wasm_host/wit.rs`](https://github.com/zed-industries/zed/blob/main/crates/extension_host/src/wasm_host/wit.rs), read for design only, because that crate is GPL).
- **The plugin order changed on 2026-10-20.** It is now runtime (#25), template (#30), Notes (#35), Forge (#42); the Fledge plugin (#32) is on hold until Fledge's own API is ready ([`docs/decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/decisions.md), 2026-10-20). Notes goes first because the owner will use it every day, so it proves the runtime and the shared document service (D28's CodeMirror bridge and safe save, built in #37) on a real workload before Forge adds a pseudoterminal, git (D29), language servers (D30) and Tree-sitter. So this note designs **what Notes needs now** and checks that **Forge's needs fit later** without a rewrite, and it no longer treats Fledge's "New lead" form as the first workload.

The research may run alongside Plan 1's remaining work because it is paper only; the host itself (#28, #29) waits for Plan 1 to close and for the owner's approval of this note (2026-10-20).

## Short answer

1. **Runtime: wasmtime with the component model and WIT**, host bindings generated with `wasmtime::component::bindgen!`, pinned exactly (`=`) on the **48.x long-term-support line**, with Cranelift, without Winch, the pooling allocator, WASI 0.3 async or `wasi-http`. wasmi and wasmer are rejected because neither supports the component model, which the owner's WIT decision requires. wasmtime's licence, `Apache-2.0 WITH LLVM-exception`, needs an explicit owner approval.
2. **Interface: one versioned WIT package, `syrinx:plugin@0.1.0`**, typed records everywhere except two named JSON channels, plain ids for anything that persists, host resources only for live capability objects, synchronous functions only, and pull-based search feeds whose cursor the host owns. Static facts (commands, view kinds, feeds, widgets, capability requests) live in a manifest read without running code; behaviour lives in exports.
3. **Capabilities: nothing is granted by default.** A TOML manifest asks for typed, scoped capabilities, each with a one-sentence reason; the owner reviews them at install, grants optional ones at first use, and can revoke any of them. The host owns every risky service (files with safe saves, watching, indexing, terminals, git, language servers, Tree-sitter); the plugin owns product logic. Spawning a process is named honestly as full trust.
4. **Plugin interface: a separate Rust type of web view**, in the same WebView2 environment as pages but a separate InPrivate profile, serving each plugin's files from Rust at a per-plugin origin under `.invalid` with a strict Content Security Policy the host sets, talking to its own module only through a Rust-validated message bridge. The Notes and Forge editor is a host-provided CodeMirror component. Palette forms and Home widgets stay declarative and are drawn by GPUI.
5. **Distribution: a folder with a signed, canonical file list**, reusing the design of Syrinx's own update format; built-ins bundled in Syrinx's signed release as separate packages; everything else installed from a folder the owner picks; no plugin downloads in v1; updates owner-initiated, strictly newer, from the same signer, and held inactive if they widen a capability.

The rest of this note gives the evidence, the conflicts between the five reports and how they were settled, and the decisions the owner needs to make.

## 1. The runtime

### What was compared

The issue named three options: wasmtime with the component model (Zed's model), wasmtime with a byte-buffer protocol (Extism's model), and another runtime such as wasmer or wasmi. The owner's 2026-10-15 decision already fixes WIT typed interfaces, a sandbox from day one, exact pins as for GPUI, no GPL code, a full audit of every transitive crate before any dependency is added, and D6's budgets: memory with 10 tabs within 10% of Edge's, idle CPU near zero, and GPUI chrome painting before WebView2 finishes starting. The byte-buffer option is covered under the interface (section 2), because it is an interface choice that any runtime can make.

### Recommendation

Embed **wasmtime** with `default-features = false, features = ["runtime", "cranelift", "component-model", "std", "parallel-compilation", "anyhow"]`, and `wasmtime-wasi` with only `["p2"]`. Pin it on the **48.x** LTS line. Turn on epoch interruption. Leave off Winch (the baseline compiler), the pooling allocator, WASI 0.3 and `component-model-async`, and `wasi-http`. Keep a cache of compiled plugins in Syrinx's local app-data folder (section 5) and never load compiled code that was downloaded. This is Zed's model: Zed's current `Cargo.toml` requires wasmtime `48` (a caret requirement, not an exact pin) with almost this feature set, plus `async`, `demangle` and `incremental-cache` ([zed `Cargo.toml`](https://github.com/zed-industries/zed/blob/main/Cargo.toml), lines 973–985).

**Reconciled: 48.x LTS, not 49.0.4.** The interface research cites wasmtime 49.0.4's documentation (released 2026-10-19; a 50.0.0 release candidate exists), and the capability research reads `Config` at a 51.0.0-dev docs page, while the runtime research recommends the 48.x LTS line. They do not disagree about the design, only about which documentation was at hand. The pin should be the LTS line: wasmtime releases a new major version on the 20th of every month and supports an ordinary release for two months, while "each release that is a multiple of 12 is considered an LTS release and is supported for 24 months" ([release process](https://docs.wasmtime.dev/stability-release.html)). The current LTS lines are 36.x (from 2025-08-20) and 48.x (from 2026-08-20). Following monthly majors would mean a pin move every two months, and each pin move of a core dependency needs the owner under `AGENTS.md`. Consequences: every API this note cites from 49.0.4 or later documentation was re-read on docs.rs at 48.0.7 during fact-checking (2026-10-20): `Linker`'s semver matching, `Component::component_type()`, the async incompleteness note, `StoreLimitsBuilder`'s defaults, `max_wasm_stack`'s 512 KiB default and the epoch "2-3x" wording are present with the same text, and `FsPerms` exists in `wasmtime-wasi` 48.0.7 (its behaviour is section 3's). #28 still re-checks any further API it relies on. The licence audit below, resolved at 49.0.4, was also repeated at 48.0.7 (the latest 48.x patch): the same 170 crates and the same licence strings, with Cranelift at 0.135.7 rather than 0.136.4.

**Reconciled: no async execution in 0.1.0.** The runtime research's recommended feature list has no `async`, but its audited graph included it, and the interface research proposed `bindgen!`'s `imports: { default: async }` so that a slow host import blocks only one plugin. Both goals are met without using async execution: each plugin instance already runs on its own worker thread (section 2), so a synchronous host import can simply block that worker while Rust does the work elsewhere, and with synchronous execution an expired epoch deadline traps, which is the behaviour wanted. The `async` feature is not the `component-model-async` feature whose advisories are avoided below. **Correction from fact-checking:** the Cargo feature cannot actually be left out while WASI 0.2 is linked, because `wasmtime-wasi` 48.0.7 depends on `wasmtime-wasi-io`, which turns on wasmtime's `async` feature unconditionally (its `Cargo.toml`; `cargo tree -e features -i wasmtime` shows it). So the decision is about use, not compilation: Syrinx uses the synchronous bindings and never enables `Config::async_support`, and the #27 audit is run on the graph as resolved, with `async` compiled in.

### Why not wasmi or wasmer

**wasmi**, the pure-Rust interpreter, has the cleanest licences and the smallest graph (15 crates, all MIT or Apache, and `Unlicense OR MIT` for `memchr`). It has no component model: its tracking issue [#897](https://github.com/wasmi-labs/wasmi/issues/897) is open, and the maintainer called the feature "not planned nor unplanned" on 2025-02-11. Only WASI 0.1 is offered through `wasmi_wasi`; third-party crates (`wasm_component_layer`, `wasmi_runtime_layer`) layer components over it but are not maintained by wasmi ⚠️. Choosing it would force the byte-buffer protocol the owner rejected in favour of WIT, and as an interpreter it is slow for Forge's language features. It also allocates the guest's whole initial memory up front (about 1.2 MiB per instance in the measurements below).

**wasmer** is rejected for three reasons. Its WASI Preview 2 issue [#4439](https://github.com/wasmerio/wasmer/issues/4439) was closed "not planned" on 2024-08-20, its 7.5.0 API crate has no component-model feature, and its direction is WASIX, its own POSIX-like extension. A secondary blog claims "Wasmer 5+ runs P2 components"; no primary source was found, so it is treated as unverified ⚠️. It has no published LTS or backport policy ⚠️ and no `SECURITY.md` in its repository (404 at the root and in `.github/`) ⚠️. Its graph is the largest (228 crates) and brings licences outside Syrinx's list: `self_cell 1.3.0` under `Apache-2.0 OR GPL-2.0-only` (Apache could be elected, but the election would have to be recorded), whole-crate MPL-2.0 (`colored 3.1.1`), ISC (`libloading`), 0BSD, CC0 and BSL-1.0 alternatives, a second Zlib crate (`zlib-rs`), and an unconditional `bindgen` and `clang-sys` build dependency used only for its V8 backend (`wasmer-7.5.0/build.rs`).

### Licences of every transitive crate

Each graph was resolved in a throwaway Cargo project outside the repository, for `x86_64-pc-windows-msvc`, with `cargo tree -e normal,build --target x86_64-pc-windows-msvc --format "{p}|{l}"`, which is the feature-resolved graph including build scripts and procedural macros (`cargo metadata`'s unpruned resolve overcounts; it lists `foldhash 0.1.5` for wasmi, which is never activated). Licence strings are crates.io metadata, not a reading of each `LICENSE` file ⚠️; the #27 audit reads the files, as earlier Syrinx audits did. cargo-deny was not used.

| Graph (Windows target) | Crates | Licence strings that are not plain MIT or Apache |
|---|---|---|
| wasmtime 49.0.4 `runtime, cranelift, component-model, std, parallel-compilation, async, anyhow` + wasmtime-wasi 49.0.4 `p2` | 170 | 31 × `Apache-2.0 WITH LLVM-exception`; 13 × `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`; 18 × `Unicode-3.0` (already approved, exact versions); `unicode-ident` `(MIT OR Apache-2.0) AND Unicode-3.0`; `foldhash 0.2.0` Zlib; `encoding_rs 0.8.42` `(Apache-2.0 OR MIT) AND BSD-3-Clause` (already in Syrinx); 3 × `Unlicense OR MIT` (elect MIT) |
| The same without wasmtime-wasi | 104 (103 without `async`) | 28 × LLVM-exception (27 without `async`, which drops `wasmtime-internal-fiber`); `unicode-ident`; `foldhash 0.2.0`; `encoding_rs`; 3 × `Unlicense OR MIT` |
| The first row, runtime only, without Cranelift | 142 | 19 × LLVM-exception; otherwise as the first row |
| wasmer 7.5.0 `sys, cranelift` | 228 | 15 × LLVM-exception; 2 × `MIT OR Apache-2.0 WITH LLVM-exception`; `Apache-2.0 OR GPL-2.0-only`; `MPL-2.0`; `ISC`; `0BSD` × 2; `BSD-3-Clause` (`bindgen`); `Zlib` × 2; CC0 and BSL-1.0 alternatives |
| wasmi 2.0.0 defaults | 15 | `Unlicense OR MIT` (`memchr`) |

Against Syrinx's current lockfile, the recommended wasmtime graph adds 59 name-and-version pairs, 48 of them entirely new crate names; the runtime-only graph would add 45, wasmer 102 and wasmi 9. No crate in any of the three graphs is GPL-only.

The licence findings for the owner:

1. **`Apache-2.0 WITH LLVM-exception` needs an explicit ruling.** It is the sole licence of 31 crates in the recommended graph: `cranelift-assembler-x64`, `-assembler-x64-meta`, `-bforest`, `-bitset`, `-codegen`, `-codegen-meta`, `-codegen-shared`, `-control`, `-entity`, `-frontend`, `-isle`, `-native` and `-srcgen` (all 0.136.4 at the audited 49.0.4); `pulley-interpreter` and `pulley-macros`; `regalloc2 0.15.2`; `target-lexicon 0.13.5`; `wasmtime` and `wasmtime-environ`; `wasmtime-internal-{component-macro, component-util, core, cranelift, fiber, jit-icache-coherence, unwinder, versioned-export-macros, wit-bindgen}`; `wasmtime-wasi` and `wasmtime-wasi-io`; and `winx 0.36.4` (crate licences from the [crates.io API](https://crates.io/api/v1/crates/wasmtime)). Syrinx's lockfile has no crate under that licence alone today; it appears only as an `OR` alternative, as in `rustix` (`docs/dev/fulltext-audit.md`). The exception only *adds* permissions to Apache-2.0: it waives the attribution requirement for compiled object code and covers compatibility with GPLv2-only code ([LLVM licence text](https://llvm.org/LICENSE.txt); [SPDX: LLVM-exception](https://spdx.org/licenses/LLVM-exception.html)). It is no less permissive than Apache-2.0, but `AGENTS.md` lists permitted licences by name and earlier approvals were exact in scope, so the owner should approve it explicitly, for the wasmtime and Cranelift family at the pinned versions.
2. **`unicode-ident` resolves to 1.0.27 in a fresh graph,** while the owner's Unicode-3.0 approval names **1.0.26 only**. Inside Syrinx's workspace the existing lockfile keeps 1.0.26, because wasmtime's requirement is `^1`. This needs no decision as long as `cargo update` is not allowed to move it; if a later pin forces 1.0.27, that is a fresh, narrow approval.
3. **The `foldhash 0.2.0` exception applies unchanged:** wasmtime pulls exactly the approved version (Zlib) through `hashbrown 0.17.1`.
4. **The Unicode-3.0 ICU crates are already approved.** `wasmtime-wasi` pulls 18 ICU and `zerovec` crates through `url` and `idna`, each already in Syrinx's lockfile at the versions approved on 2026-10-10 (`docs/dev/integrated-preview-dependencies.md`). Nothing new is needed while those versions hold.
5. The guest-side `wit-bindgen` 0.63.0 (in plugin repositories, not in `syrinx.exe`) is `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`, so MIT can be elected. wasmer 7.5.0 itself is MIT ([README](https://github.com/wasmerio/wasmer#license)); wasmi 2.0.0 is MIT or Apache-2.0 ([repository](https://github.com/wasmi-labs/wasmi)).

### Maintenance

- **wasmtime** (Bytecode Alliance) releases a major version on the 20th of every month, branched on the 5th ([release process](https://docs.wasmtime.dev/stability-release.html)); crates.io confirms monthly majors from 26.0.0 (2024-10-22) to 49.0.0 (2026-09-21). "The Wasmtime project guarantees backports and patch releases will be made for any discovered security issue" on supported releases; on 2026-10-19 alone 36.0.19, 48.0.7 and 49.0.4 were published together. The minimum supported Rust version is 1.95 for 48.x and 1.96 for 49.x, and the policy avoids raising it within a release's life; Syrinx's workspace is on 1.97, so both fit. `x86_64-pc-windows-msvc` is **Tier 1**, as are Cranelift, `component-model`, `component-model-async` and the WASI proposals; Pulley, wasmtime's portable interpreter, is Tier 2 and `aarch64-pc-windows-msvc` only Tier 3 ([stability tiers](https://github.com/bytecodealliance/wasmtime/blob/main/docs/stability-tiers.md)). Tier 1 promises continuous fuzzing and "CVEs and security releases ... as necessary".
- **wasmer** (Wasmer Inc.) released roughly monthly through 2026 (7.0.0 on 2026-01-28 to 7.5.0 on 2026-10-01), with irregular majors (5.0 in 2024-10, 6.0 in 2025-04, 7.0 in 2026-01). No LTS or backport policy and no tiered Windows platform statement were found ⚠️.
- **wasmi** (wasmi-labs, mainly Robin Freyler) released 1.0.0 on 2025-12-03, 1.1.0 on 2026-06-12 and 2.0.0 on 2026-09-01, and backported its December 2025 security fix to 0.47, 0.51 and 1.0. It has no LTS policy ⚠️, and security reports go to one maintainer's personal email ([SECURITY.md](https://github.com/wasmi-labs/wasmi/blob/main/SECURITY.md)), which makes it a single-maintainer project ⚠️ (inferred).

### Security record, about the last three years

**wasmtime** has 65 RustSec advisories dated after 2023-10-20 across `wasmtime`, `wasmtime-wasi`, `wasmtime-wasi-http` and `wasmtime-jit-debug` ([RustSec advisory-db](https://github.com/rustsec/advisory-db/tree/main/crates/wasmtime), cloned 2026-10-20): 5 in 2024, 3 in 2025 and **57 in 2026** (four more carry 2023 ids, RUSTSEC-2023-0090 to -0093, but are dated March to September 2023). The jump is explained by the project itself: on 2026-04-09 it published 12 at once, 11 of them "discovered by the Wasmtime team using new LLM-based tools" built as a "multi-agent harness", a batch that "is triple the total number of advisories published in all of 2025", and said LLM-based discovery will be required for future Tier 1 features ([Bytecode Alliance post](https://bytecodealliance.org/articles/wasmtime-security-advisories)). Large batches followed on 2026-09-24, 10-02, 10-14 and 10-19. wasmtime also counts any guest-triggerable host panic or memory exhaustion as a vulnerability, though not miscompiles that stay inside the sandbox ([what counts](https://docs.wasmtime.dev/security-what-is-considered-a-security-vulnerability.html)). For Syrinx this means a security patch roughly every one to three weeks, each a pin bump on the LTS line that must pass the gate.

Advisories that hit the recommended configuration (x86_64, Cranelift, component model, WASI 0.2):

- Code generation on x86_64: [RUSTSEC-2026-0006](https://rustsec.org/advisories/RUSTSEC-2026-0006.html) (`f64.copysign` segfault or out-of-sandbox load), [RUSTSEC-2026-0087](https://rustsec.org/advisories/RUSTSEC-2026-0087.html) (`f64x2.splat`) and [RUSTSEC-2026-0380](https://rustsec.org/advisories/RUSTSEC-2026-0380.html), a Cranelift bit-packing overflow on any architecture, CVSS 9.9, fixed in 36.0.19 and in 43 and later ([GHSA-897f-g5gm-fmxm](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-897f-g5gm-fmxm)).
- Component-model string transcoding: [RUSTSEC-2026-0091](https://rustsec.org/advisories/RUSTSEC-2026-0091.html), an out-of-bounds write or crash (Moderate, CVSS 5.9, CVE-2026-34942, per the Bytecode Alliance's April post), and [RUSTSEC-2026-0093](https://rustsec.org/advisories/RUSTSEC-2026-0093.html), a heap out-of-bounds read.
- WASI filesystem sandbox escapes through path tricks: [RUSTSEC-2026-0149](https://rustsec.org/advisories/RUSTSEC-2026-0149.html), [RUSTSEC-2026-0188](https://rustsec.org/advisories/RUSTSEC-2026-0188.html), [RUSTSEC-2026-0269](https://rustsec.org/advisories/RUSTSEC-2026-0269.html), [RUSTSEC-2026-0352](https://rustsec.org/advisories/RUSTSEC-2026-0352.html) and [RUSTSEC-2024-0438](https://rustsec.org/advisories/RUSTSEC-2024-0438.html) (Windows device names). This is a strong argument for giving plugins **no WASI preopened directories** at all and reaching files only through Syrinx's own host functions, which check paths in Rust against the owner's grant (section 3).

Advisories the recommended configuration avoids, which is why those features stay off even though async is Tier 1: Winch (RUSTSEC-2026-0086, -0089, -0094 and the critical -0095), aarch64 (RUSTSEC-2026-0096 and -0379), the pooling allocator (RUSTSEC-2026-0088), `component-model-async` and WASI 0.3 (about ten advisories on 2026-10-02 and 10-14, including a native stack buffer overflow, [RUSTSEC-2026-0327](https://rustsec.org/advisories/RUSTSEC-2026-0327.html)) and `wasi-http` (seven).

wasmtime's [security page](https://docs.wasmtime.dev/security.html) states that the call stack is inaccessible to the guest and there is no undefined behaviour, that linear memories get 2 GB guard regions and are zeroed between instances, that tables and `br_table` have Spectre mitigations, and that "Wasmtime won't segfault when using its public API". "Our fuzzers run continuously on OSS-Fuzz" ([fuzzing guide](https://docs.wasmtime.dev/contributing-fuzzing.html)); the April post notes there was no continuous aarch64 fuzzing. Disclosure follows the Bytecode Alliance policy: private GitHub reports, a typical 72-hour embargo from CVE issue, a pre-notification list and the [sec-announce](https://bytecodealliance.org/security) group ([SECURITY.md](https://github.com/bytecodealliance/wasmtime/blob/main/SECURITY.md)).

**wasmer** has nine GitHub advisories ([advisories](https://github.com/wasmerio/wasmer/security/advisories)): GHSA-4mq4-7rw3-vm5j (2023-12, High, filesystem sandbox not enforced in wasmer-cli before 4.2.4), GHSA-55f3-3qvg-8pv5 (2024-06, a symlink sandbox bypass), and seven on 2026-10-08, among them unbounded host allocations in WASI and WASIX, missing bounds checks on `memory.atomic.notify` and `wait32` before 7.3.0, patched in 7.4.0 ([GHSA-r6j4-873j-3vf7](https://github.com/wasmerio/wasmer/security/advisories/GHSA-r6j4-873j-3vf7)), and an LLVM-backend panic. None appears in RustSec under `wasmer` except through OSV, and no documented fuzzing programme was found ⚠️. Fewer advisories do not show it is safer; they are consistent with a smaller search effort ⚠️ (inference).

**wasmi** has two serious advisories, rated Critical and High on GitHub (OSV rates both High) ([advisories](https://github.com/wasmi-labs/wasmi/security/advisories)): [GHSA-75jp-vq8x-h4cq](https://osv.dev/vulnerability/GHSA-75jp-vq8x-h4cq) (2024-03, an out-of-bounds write for host-to-Wasm calls with more than 128 parameters) and [GHSA-g4v2-cjqp-rfmq](https://osv.dev/vulnerability/GHSA-g4v2-cjqp-rfmq) (2025-12, a use-after-free in linear memory, fixed in 1.0.1, 0.51.3 and 0.47.1). It has had two paid audits, SRLabs (v0.31, 2023-12) and Runtime Verification (v0.36–0.38, 2024-11) per its [README](https://github.com/wasmi-labs/wasmi), and a `fuzz` folder whose programme is undocumented ⚠️.

### Component model and WIT maturity

WASI 0.2 shipped in January 2024 with patches up to 0.2.12; WASI 0.3, with native async, shipped 0.3.0 on 2026-06-11 and 0.3.1 on 2026-08-11, with releases about every two months planned to 2027-12 and no date for WASI 1.0 ([WASI roadmap](https://wasi.dev/roadmap)). Rust's `wasm32-wasip2` target is **Tier 2** and outputs a component directly ([rustc platform page](https://doc.rust-lang.org/rustc/platform-support/wasm32-wasip2.html)). In wasmtime, both the component model and component-model async are Tier 1; `wit-bindgen 0.63.0` (2026-10-14) generates the guest side. Verified on Linux: a six-line WIT world with one import and one export, built with `wit-bindgen` for `wasm32-wasip2`, loaded through `bindgen!` and returned the expected string.

**A trap for the template:** a trivial Rust `wasm32-wasip2` guest imports 14 WASI interfaces (`wasi:cli/*`, `wasi:io/*`, `wasi:clocks/monotonic-clock`, at 0.2.0 and 0.2.9) because Rust's standard library links them. The host must therefore link `wasmtime-wasi`'s 0.2 implementation or provide stubs. The recommendation is to link it with an empty context: no preopened folders, no environment variables, no arguments and no network, with standard output and error sent to the same bounded, development-only log as the host's own `log` function. Building for `wasm32-unknown-unknown` and wrapping with `wasm-tools component new` would avoid WASI at the cost of `std` I/O in plugins ⚠️ (not tried). ⚠️ Exactly which WASI imports a current `wasm32-wasip2` standard library needs should be checked on a built component.

### Cost against D6 (measured on Linux)

Published figures first. Wasmtime 1.0 cut SpiderMonkey's instantiation from "about 2 milliseconds" to "5 microseconds" through copy-on-write mapping of the initial heap and lazy table initialisation ([Wasmtime 1.0 performance, 2022-09-06](https://bytecodealliance.org/articles/wasmtime-10-performance)). Epoch interruption costs about 10% of guest execution speed, and fuel metering costs more but is deterministic ([interrupting Wasm](https://docs.wasmtime.dev/examples-interrupting-wasm.html)).

**Measured on Linux x86_64, 4-core VM, not Windows ⚠️.** Treat these as orders of magnitude; #28 re-measures them on the owner's machine. The guest was a synthetic plugin: a 48 KB component (`opt-level="s"`, LTO, stripped) and a 21 KB core module doing the same string work, 200 live instances, medians of three runs.

| Runtime and path (Linux) | Compile once | Load precompiled | Instantiate (median) | First call | Warm call | Resident memory per live instance |
|---|---|---|---|---|---|---|
| wasmtime component, default allocator | 25–27 ms (Cranelift) | 0.19–0.28 ms (`Component::deserialize`, 139 KiB artefact) | 15–21 µs | 12–16 µs | 0.4–0.6 µs | 29–31 KiB (plus 4 GiB of *reserved* address space each) |
| wasmtime component, pooling allocator | same; engine creation 2 ms | same | 17–21 µs | 16–19 µs | 0.4–0.6 µs | 37–38 KiB |
| wasmtime component, epoch interruption on | 32–47 ms; 163 KiB artefact | 0.2–0.25 ms | 15–18 µs | 13 µs | 0.5 µs | 30 KiB |
| wasmtime core module (byte-buffer protocol) | 11–14 ms | — | 9 µs | — | — | 22 KiB |
| wasmer 7.5 core module (Cranelift) | 28–30 ms | 0.07–0.11 ms (92 KiB) | 13–17 µs | — | — | 21 KiB |
| wasmi 2.0 core module (interpreter, lazy translation) | 0.34 ms | n/a | 570–650 µs | 55–72 µs | 1.24 µs | **1,170 KiB** |

Also on Linux: a runtime-only wasmtime host without Cranelift loaded the precompiled component, linked WASI, instantiated it and made a call in 0.7–0.9 ms in total; compiling the 48 KB component with Cranelift at start-up raised process resident memory from 4.1 MiB to about 19 MiB, a transient peak.

| Host binary (Linux release builds, stripped, fresh target directory, 4 cores) ⚠️ | Clean release build | Stripped size | Added over a 0.33 MiB hello-world |
|---|---|---|---|
| wasmtime + Cranelift + component model + WASI 0.2 (recommended) | 202 s, 169 crates | 17.1 MiB | about 16.8 MiB |
| wasmtime runtime only, no Cranelift (not viable, see below) | 86 s | 4.7 MiB | about 4.4 MiB |
| wasmer 7.5 `sys, cranelift` | 247 s, 232 crates | 16.0 MiB | about 15.6 MiB |
| wasmi 2.0 | 24 s, 15 crates | 2.7 MiB | about 2.4 MiB |

MSVC binary sizes and build times on Windows will differ. Much of the recommended graph overlaps Syrinx's existing lockfile, so part of the build time is already paid. Zed sets `opt-level = 3` for wasmtime and Cranelift even in its dev profile (its `Cargo.toml`, `[profile.dev.package]`, lines 1091–1094); D38's `local` profile is already optimised. wasmtime's `runtime` feature compiles a small C helper through `cc`, which on Windows needs the MSVC compiler that the GPUI shader patch's Windows SDK requirement already implies ⚠️.

What this means against D6:

- **Memory.** An idle instance costs tens of KiB plus its own heap, and Syrinx will hold about three to ten long-lived instances. The engine and compiled code add an estimated 1–2 MiB per plugin, mapped from the artefact ⚠️ (estimate). That is noise against Phase 0's Edge baseline of 2,049 MB with ten tabs ([note 09](Background-09-phase-0-verdict)), as long as plugins are instantiated on first use, as Plan 2 already requires. Forge's real heap, with parsed trees and editor state, will dominate and is measured when Forge is built. The 4 GiB per-instance reservations are address-space reservations (`MEM_RESERVE`) on Windows, not committed memory; whether they count against the commit charge or show in Task Manager is unverified ⚠️. The plugin views' renderers (section 4) are a larger, unmeasured cost.
- **Start-up.** Compiling at launch is the real risk: Forge's guest could be megabytes, and Cranelift time grows with code size, possibly to seconds ⚠️ (extrapolated). So Syrinx compiles each plugin once, on a background thread, when it is installed or updated, stores the artefact in the local app-data folder and loads it at activation (section 5). First paint then never depends on plugins.
- **Idle CPU.** wasmtime runs nothing between calls. Epoch interruption needs a host timer that advances the epoch; it should tick only while a plugin call is in progress, never as a constant wake-up ⚠️ (design inference), to keep D6's "idle CPU near zero".
- **The pooling allocator** brought nothing at Syrinx's scale: slightly slower instantiation, more memory per instance, 2 ms more engine creation, and its own advisory. It suits servers that create thousands of instances a second.
- **A Cranelift-free `syrinx.exe` is not an option,** because compiled code may only come from the same wasmtime version and must never be downloaded (section 5), so Syrinx must be able to compile on the owner's machine.

| | wasmtime + component model | wasmer | wasmi |
|---|---|---|---|
| WIT typed interfaces (required since 2026-10-15) | Yes, Tier 1; Zed uses it | No (Preview 2 "not planned") | No (#897 open) |
| Licence | `Apache-2.0 WITH LLVM-exception` (needs a ruling); otherwise within existing approvals | MIT itself; a GPL alternative, MPL, ISC, 0BSD and others in its graph | Clean |
| Maintenance | Monthly; 24-month LTS; guaranteed security backports; Windows x64 Tier 1 | Roughly monthly; no LTS; no security policy file | Active; mainly one maintainer; no LTS |
| Security posture | Many advisories from aggressive fuzzing and LLM-assisted auditing under a broad definition; fast coordinated patches | Few advisories; little process evidence | Two serious (Critical and High) in three years; two audits |
| Per instance (Linux) | about 15–20 µs, about 30 KiB | about 15 µs, about 21 KiB | about 0.6 ms, about 1.2 MiB |
| Execution | Compiled (Cranelift) | Compiled | Interpreted |
| Size added (Linux) | about 17 MiB | about 16 MiB | about 2.4 MiB |

wasmtime is the only runtime that meets the owner's WIT requirement with a support policy Syrinx can pin to. Its costs are about 17 MiB of binary and a high advisory rate that means frequent patch bumps. Neither breaks D6, provided compilation is cached and plugins instantiate on first use.

## 2. The interface: what crosses the WIT boundary, and how it is versioned

### What exists in Syrinx today

No `Plugin` trait exists in code; `crates/` has no `syrinx-plugin-api`. There are placeholders the interface must fit: `CommandScope::Plugin { plugin }` and `CommandAction::Plugin { plugin, action }` in `crates/syrinx-core/src/commands.rs` ("no host exists yet"), with command ids as dot-separated lowercase ASCII segments such as `browser.new-tab`; `TabKind::{Web, PluginView, Note, File, Diff, Terminal}` and `TabMetadata { title, address, favicon, badge }` in `tabs.rs`; `BuiltInPlugin` with the stable ids `forge`, `notes` and `fledge` in `destinations.rs`; and `Entry { target, title, address, usage, open }` in `crates/syrinx-search/src/types.rs`, where `Target` is `Tab | Saved | History | Command` and so has no plugin-content target yet. Tier 1 search is the in-memory catalogue ranked by `frizbee` on every keystroke, with D48's budget of 5 ms at the 95th percentile for 100,000 rows; tier 2 is the full-text index, planned behind a `FullText` trait (P1-24; no such trait is in `crates/` yet, only the `fulltext-bench` bake-off crate). Plan 2 already requires owned, bounded index batches with stable ids, revisions and deletions, a persistent cursor that advances only after a durable commit, activation on first use, cheap manifest discovery, bounded queues, request identity, cancellation, "a timeout is not proof that a write was reversed", and no blanket `catch_unwind` ([Plan 2 at af3de53](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/plans/Syrinx%20v1%20%E2%80%94%20Plugins%20and%20Fledge%20%E2%80%94%20Plan.md)).

### What the component model can carry

From the WIT reference ([`WIT.md`](https://github.com/WebAssembly/component-model/blob/main/design/mvp/WIT.md)): `bool`, the integers `u8` to `u64` and `s8` to `s64`, `f32`, `f64`, `char` and `string`; `list<T>`, `option<T>`, `result<T, E>` and tuples; named `record`, `variant`, `enum` and `flags` types (and `map<K, V>` since WASI 0.3.1); **resources**, "an entity with a lifetime that can only be passed around indirectly via handle values", either owned (dropping it runs the destructor) or `borrow<T>`; and the async types `future<T>`, `stream<T>` and `async func`, shipped in WASI 0.3.0. Every value is copied between the plugin's memory and the host's under the canonical ABI, the component model's rule for laying values out in memory, so there are no shared pointers. A WIT `string` must be valid Unicode, and the ABI traps on malformed text ([`CanonicalABI.md`](https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md)). Plain values therefore already satisfy the "owned, serialisable values only" guardrail of `AGENTS.md`.

### Typed records, with two named JSON channels

Every call uses records, variants, enums, options, results, lists and strings. Typed calls are checked when the host links the plugin, so a mismatch fails at load with a clear error instead of deep inside a call; both sides are generated from one definition, which is the point of the owner's chosen form; and a command, a form, a search batch or a widget is easier for the host to bound and validate as a typed record than as free-form JSON.

- *Rejected: a JSON or byte payload everywhere* (Extism's model). Extism passes byte buffers and leaves the format to convention ([extism.org host functions](https://extism.org/docs/concepts/host-functions) ⚠️ not re-read on 2026-10-20; Extism is BSD-3-Clause, `extism 1.41.0` on crates.io). That gives up generated bindings, load-time type checks and the documentation a WIT file is, and every plugin would hand-write its own serialisation and versioning.
- *Rejected: typed everywhere with no JSON at all.* Two channels are opaque by nature, and each is named with its reason: **view messages**, between a plugin's HTML and JavaScript and its module, which Syrinx relays after checking the envelope (section 4) but does not interpret; and **the plugin's own settings**, stored as opaque bytes in its host-scoped namespace (Zed's `get-settings` likewise returns a JSON `string`). Everything Syrinx itself reads or draws (commands, forms, search entries, widgets, tab metadata) stays typed.

### Plain ids for identity, resources only for live capabilities

A resource handle is an index into one instance's handle table: it cannot go into the store, survive a restart or survive the instance being discarded after a trap. So everything that persists or is compared uses a plain id: command ids, view references, search entry ids, submission ids and widget ids, each either a validated `string` of dot-separated lowercase segments (matching `CommandId`) or a `u64` the host allocates.

Host-defined resources are allowed where a live, revocable object is the honest model and dropping it should release something: a terminal session, a language-server connection, a watched folder, later a stream of output. Forge's capabilities are the motivating case, and Zed uses resources the same way (`worktree`, `project` and `key-value-store` handed to the extension as `borrow<worktree>`). Each lives in the host's `ResourceTable`, which wasmtime's generated `Host*` traits index by `Resource<T>` ([`bindgen!` docs](https://docs.rs/wasmtime/49.0.4/wasmtime/component/macro.bindgen.html)), and the host revokes a grant by failing calls on the handle. Guest-defined resources (the plugin exporting a resource type) are not allowed in 0.1.0, because nothing in the v1 plans needs Syrinx to hold a handle into a plugin. Nothing from Syrinx's core is ever handed to a plugin as a resource: no tab, palette or database object crosses, only ids and snapshots, which keeps S3 (Rust owns state, and the tab model stays the one source of truth).

### Bounds

WIT has no length limits. So the host caps each plugin's memory with wasmtime's `ResourceLimiter`, which the `Config` documentation recommends to "ensure that the store's allocated heap size ... [is] bounded" ([`Config`](https://docs.rs/wasmtime/49.0.4/wasmtime/struct.Config.html)); `StoreLimitsBuilder::memory_size` caps each linear memory, whose default is unlimited, while instance, table and memory counts default to 10,000 ([`StoreLimitsBuilder`](https://docs.wasmtime.dev/api/wasmtime/struct.StoreLimitsBuilder.html)). The value comes from measurement in #28 ⚠️. The default `max_wasm_stack` is 512 KiB, and overflowing it traps. Every value lifted from a plugin is also validated against fixed limits before use (string length, batch length, number of form fields, view-message size); a value over a limit is a typed error, not a trap.

### Versioning

How WIT versions work:

- The version lives on the package, `package syrinx:plugin@0.1.0;`, as full semver, and "if a package contains a feature gate, its version must be specified" (`WIT.md`, "Package declaration" and "Feature Gates").
- Interface names carry it in the binary, as `syrinx:plugin/commands@0.1.0` (confirmed on Linux during fact-checking: a `wasm32-wasip2` component built with `wit-bindgen` 0.63.0 from `package fc:test@0.1.0` contains the names `fc:test/commands@0.1.0` and `fc:test/host@0.1.0`, read from the binary's strings rather than with `wasm-tools component wit`; bare world functions carried no such name).
- Feature gates mark change: `@since(version = 0.1.1)` marks an addition, after which "the item is not modified incompatibly going forward"; `@unstable(feature = …)` marks an item that "may change type or be removed at any time" and is hidden unless opted into; `@deprecated(version = …)` pairs with `@since`. Gates are resolved at build time, and "any item that refers to another gated item must also be compatibly gated".
- wasmtime's `Linker` consults "semver-compatible names" automatically: a host defining `a:b/c@0.2.1` satisfies a plugin importing `@0.2.0`, and the reverse, "because if an API was defined at 0.2.0 and 0.2.1 then it must be the same API" ([`Linker`](https://docs.rs/wasmtime/49.0.4/wasmtime/component/struct.Linker.html), "Names and semver"). ⚠️ The page does not state the 0.x rule; that 0.1.x and 0.2.x are treated as incompatible tracks, as semver says, is assumed.
- Subtyping "is only relaxed for `module`, `instance` and `component` types": a component may export more and import less, but records and variants must match exactly ([`Explainer.md`](https://github.com/WebAssembly/component-model/blob/main/design/mvp/Explainer.md), "Type checking").

What breaks a plugin, inferred from those rules (⚠️ each needs a host test in #28):

| Change | Effect |
|---|---|
| Add a function or interface with `@since(0.1.1)` | Compatible: old plugins do not import it; plugins built for 0.1.1 need a 0.1.1 or later host |
| Add a field to a record or a case to a variant | **Breaks** (exact match): add a new record or function with `@since`, or bump the minor |
| Rename or remove anything, or change a parameter type | **Breaks** |
| Change a sync `func` to an `async func` | **Breaks** |
| Change an `@unstable` item | Breaks plugins that opted in, by definition; built-ins never ship with an unstable feature on |
| The host stops providing an import a plugin uses | Instantiation fails, unless the host uses `define_unknown_imports_as_traps`, which defers failure to the call; not recommended |

The scheme:

1. **One versioned package, `syrinx:plugin`, starting at 0.1.0** and staying below 1.0 until Forge, the heaviest consumer, has used it. Within a minor, add only through `@since`; anything else bumps the minor and opens a new compatibility track.
2. **All exports are interfaces, never bare world functions.** Zed's world exports bare functions (`export init-extension: func();`), which carry no version, so Zed needs a custom `zed:api-version` section to tell versions apart (read for design only, in its GPL host). With interface exports the version is in the names, and the host can read it from `Component::component_type()` before instantiating ⚠️ (the API exists; not exercised).
3. **The manifest repeats the version** (section 3), and the host rejects a plugin whose manifest disagrees with its component's interface names (a #28 test).
4. **The host supports several tracks as Zed does:** one `bindgen!`-generated module per supported track (`wit/v0_1/`, `wit/v0_2/`), an enum over loaded instances, and thin translation from old tracks to the newest host logic. Syrinx publishes a support window; the recommendation is the current and previous minor while below 1.0.
5. **Unstable features ship gated.** Home widgets (#31) start as `@unstable(feature = home-widgets)` until the owner approves #31's plan; the template enables no unstable feature. ⚠️ wit-bindgen has a `features` option for opting in; `bindgen!`'s documentation page does not list one for the host, so whether the host can compile unstable items selectively is checked in wasmtime's source.
6. **The WIT files live in this repository,** because the host is the authority, and are published to the template as a versioned copy or a small guest crate, as Zed does with `zed_extension_api` (Apache-2.0). Where the guest crate lives is for #27.

### Static facts in the manifest, behaviour in exports

**Reconciled.** The interface research's draft world had the plugin list its commands, view types, feeds and widget types through exports read after activation, while the capability research put commands, tab types, search feeds and widgets in the manifest, and the interface research itself required the manifest to carry "its views and its static command list" so that discovery runs no code. With activation on first use, the palette must show a plugin's commands *before* the plugin has run, so the manifest must be the source. The rule adopted is: **anything Syrinx needs before a plugin runs is declared in the manifest; exports only do things.** The `list-commands`, `list-view-types`, `list-feeds` and `list-widget-types` exports are dropped from 0.1.0, which also removes a second copy of the same facts that could drift.

**Reconciled: search is pull-based.** The capability research's index capability had the plugin push batches through `submit(batch)`; the interface research rejected pushing, because then the plugin decides when a batch is "delivered" and cannot know whether the tier 2 commit happened. The pull design wins: the plugin calls `feed-changed`, the host pulls a bounded batch after its own cursor, commits it to tier 1 or tier 2, and only then records the cursor, which is exactly Plan 2's "advance a persistent source cursor only after ... the durable index commit". The capability research's other rules are kept: the host rejects stale revisions, namespaces ids by plugin and clears a feed's contributions when a vault switches. A live per-keystroke `query` export for tier 1 is also rejected, because it would put plugin execution inside D48's 5 ms budget.

### Draft world

This is a sketch for discussion, not an approved API, and has not been checked with `wasm-tools` or a WIT parser ⚠️. It merges the interface research's world with the interface-view bridge (section 4) and the capability names (section 3). Notes' capability interfaces (`documents`, `watch`, `drafts`, `open-external`) and Forge's (`terminal`, `git`, `lsp`, `parse`) are named but designed in #28 and the Notes and Forge slices; the Fledge interface is left out of 0.1.0 because Fledge is on hold.

```wit
package syrinx:plugin@0.1.0;

/// Shared value types. Ids are validated by the host: dot-separated lowercase
/// ASCII segments, at most a fixed length (bounds set in #28).
interface types {
    type command-id = string;          // e.g. "notes.new-note"; must start with the plugin id
    type request-id = u64;             // host-allocated, never reused across view or controller lifetimes
    type view-instance = u64;
    type timestamp = s64;              // Unix milliseconds

    variant plugin-error {
        invalid-input(string),
        not-found(string),
        denied(string),                // a capability the owner has not granted; names it
        revoked(string),               // a capability the owner withdrew while in use
        unavailable(string),           // a dependency is unavailable; retry may help
        internal(string),              // shown to the owner, never logged with owner data
    }

    /// Plan 2: a timeout is not proof a write was reversed.
    variant outcome {
        done(option<string>),
        failed(plugin-error),
        uncertain(string),             // reached the service, completion unknown; never resubmitted
    }

    enum icon { plugin, document, person, folder, terminal, search, star, warning }
}

/// What every plugin may ask of Syrinx. Capability interfaces are separate.
interface host {
    use types.{plugin-error, request-id, view-instance, timestamp};
    enum log-level { debug, info, warn, error }
    log: func(level: log-level, message: string);              // bounded; development builds only
    settings-get: func(key: string) -> result<option<list<u8>>, plugin-error>;
    settings-set: func(key: string, value: list<u8>) -> result<_, plugin-error>;
    open-view: func(kind: string, address: string) -> result<view-instance, plugin-error>;
    set-view-metadata: func(view: view-instance, title: string, badge: option<string>)
        -> result<_, plugin-error>;
    /// An event to the view's own interface (section 4). Never reaches a page web view.
    post-view-event: func(view: view-instance, topic: string, json: string) -> result<_, plugin-error>;
    feed-changed: func(feed: string);                           // the host pulls later
    @unstable(feature = home-widgets)
    widget-changed: func(widget: string);
    cancel: func(request: request-id);
    now: func() -> timestamp;
}

// Capability interfaces (imports), designed in #28 and the plugin slices:
// documents, watch, drafts, open-external (Notes); parse, terminal, git, lsp (Forge).

interface lifecycle {
    use types.{plugin-error};
    record activation { host-version: string, granted: list<string> }
    activate: func(info: activation) -> result<_, plugin-error>;   // on first use; epoch-bounded
    deactivate: func();                                             // best effort; also bounded
}

interface commands {
    use types.{command-id, plugin-error};
    run: func(id: command-id) -> result<_, plugin-error>;          // declared in the manifest
}

/// Declarative forms, drawn by GPUI in the palette's preview pane.
interface forms {
    use types.{command-id, plugin-error, request-id, outcome};
    variant field-kind {
        text(text-options), multiline(text-options), number(number-options),
        money(string), choice(list<choice-option>), toggle,
    }
    record text-options { max-length: u32, placeholder: option<string> }
    record number-options { min: option<f64>, max: option<f64>, step: option<f64> }
    record choice-option { value: string, label: string }
    record field { name: string, label: string, kind: field-kind, required: bool, initial: option<string> }
    record form-spec { title: string, fields: list<field>, submit-label: string }
    record field-value { name: string, value: string }
    record field-error { name: string, message: string }
    variant submit-result {
        finished(outcome),
        pending(request-id),           // outcome arrives through events.deliver
        invalid(list<field-error>),    // no effect; input stays, first failing field focused
    }
    form-for: func(id: command-id) -> result<form-spec, plugin-error>;
    validate: func(id: command-id, values: list<field-value>) -> list<field-error>;
    /// The host supplies submission-id so a repeated Enter cannot duplicate (P2-05).
    submit: func(id: command-id, submission-id: string, values: list<field-value>) -> submit-result;
}

/// Plugin tab views: HTML and JavaScript in a plugin view (section 4).
interface views {
    use types.{view-instance, request-id, plugin-error};
    record view-request { view: view-instance, request-id: request-id, method: string, json: string }
    opened: func(view: view-instance, kind: string, address: string) -> result<_, plugin-error>;
    /// A request from the view's interface, already checked by the host.
    message: func(req: view-request) -> result<string, plugin-error>;
    closed: func(view: view-instance);                             // drop subscriptions (P2-06)
}

/// Pull-based search feeds, declared in the manifest. The host owns cursors and commits.
interface search-feed {
    use types.{plugin-error, icon, timestamp};
    record catalogue-entry { id: string, revision: u64, title: string, subtitle: string,
                             icon: icon, last-used: option<timestamp> }
    record document { id: string, revision: u64, title: string, body: string }
    variant change { upsert-entry(catalogue-entry), upsert-document(document), delete(string), reset }
    record batch { changes: list<change>, next-cursor: string, more: bool }
    changes: func(feed: string, after: option<string>, max: u32) -> result<batch, plugin-error>;
    resolve: func(feed: string, id: string) -> result<string, plugin-error>;   // an address for open-view
}

interface events {
    use types.{request-id, outcome};
    deliver: func(request: request-id, result: outcome);
}

@unstable(feature = home-widgets)
interface widgets {
    use types.{plugin-error, icon};
    record stat { label: string, value: string, trend: option<string> }
    record list-item { title: string, subtitle: option<string>, icon: icon, target: option<string> }
    variant widget-body { stat(stat), items(list<list-item>), message(string) }
    snapshot: func(widget: string, width: u8, height: u8) -> result<widget-body, plugin-error>;
}

world plugin {
    import types;
    import host;
    export lifecycle;
    export commands;
    export forms;
    export views;
    export search-feed;
    export events;
    @unstable(feature = home-widgets)
    export widgets;
}
```

Notes on the parts:

- **Commands.** The host rejects duplicates and ids without the plugin's prefix before anything reaches the palette (P2-02), then adds them to tier 1 like built-in commands. `syrinx-core`'s `Enablement` is left out because it depends on browser state a plugin should not see; a later `@since` field could add a host-evaluated enum. `CommandAction::Plugin { plugin, action }` stays, and the host checks that `action` starts with `plugin`.
- **Forms.** A form is declarative (see section 4 for why). Field state lives in Rust (Plan 2, "Rendering and state"); the host checks `required`, lengths and number ranges, `validate` is the plugin's own check, and the host allocates `submission-id` and keeps it while a submission is pending, so a repeated Enter is a no-op. With Fledge on hold, #29 proves forms with a synthetic test plugin rather than "New lead"; the spec's example (Name, Company, Source, Est. value) still fits `form-spec` when Fledge resumes.
- **Search feeds.** `catalogue` entries map to tier 1 `Entry` rows and documents to tier 2. Revisions make replays idempotent, and `reset` covers Notes switching vaults (N1) and teardown. This needs a new `Target::Plugin { plugin, feed, id }` or similar in `syrinx-search`, per-feed namespaces in tier 2, and a palette scope decision (owner decision 8).
- **Widgets** are declarative snapshots, gated `@unstable` until #31's plan is approved. A web-view widget body is deliberately absent. Weather comes from a plugin that owns its own network grant (#31's settled decision); Syrinx makes no request of its own.
- **Tabs.** `TabKind::PluginView` is used; whether Notes' and Forge's `Note` and `File` kinds become views of those plugins is for #27.

### Lifecycle, errors, timeouts, re-entrancy and async

- **Load and activate.** Discovery reads the manifest only. On first use the host loads the cached compiled form (or compiles), links the interfaces the manifest requested, instantiates through `InstancePre` (`Linker::instantiate_pre`, which resolves imports once ahead of time), then calls `lifecycle.activate`.
- **Reconciled: link what the manifest requests, enforce what the owner granted.** The interface research proposed linking only the capability interfaces the owner granted; but an optional capability can be refused at run time (section 3), and an unlinked import makes instantiation fail. So the host links every capability interface the manifest *requests*, and each function returns `denied` until the owner grants it. A module that imports an interface its manifest did not request is refused at load ⚠️ (matching component imports to manifest requests is a design to prove in #28).
- **Two kinds of failure.** Ordinary failures are typed (`result<_, plugin-error>`, `outcome`) and shown to the owner. A **trap** (panic, out-of-bounds access, an expired deadline, a malformed string) is different: the component model's Component Invariant 1 says "any trap must lock down the entire containing store" ([`CanonicalABI.md`](https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md)). The host discards that `Store`, marks in-flight writes `uncertain`, shows the plugin as failed, and re-instantiates only on the next explicit use. Treating a trapped instance as unusable is conservative ⚠️ rather than a documented wasmtime requirement. No `catch_unwind` is involved; the sandbox is the isolation.
- **Timeouts.** Plan 2 said "a cooperative timeout cannot interrupt arbitrary non-yielding code". That was true of native Rust and **no longer holds inside WASM**, which answers #26's ⚠️. `Config::epoch_interruption` makes compiled code check the epoch against the store's deadline, `Engine::increment_epoch` advances it, and the guest traps when it passes (or, with async, yields through `epoch_deadline_async_yield_and_update`); the documentation calls it faster than fuel ("2-3x" in some measurements), while `consume_fuel` counts instructions for deterministic limits Syrinx does not need ([`Config`](https://docs.wasmtime.dev/api/wasmtime/struct.Config.html)). Zed enables it and yields every tick (`config.epoch_interruption(true)`, `store.epoch_deadline_async_yield_and_update(1)` in its GPL `wasm_host.rs`, read for design only). Syrinx sets a per-call deadline on every export; on expiry it traps and discards the instance, and treats any writes it started as `uncertain`. An epoch deadline does not interrupt a host function the guest is waiting on, so every host service bounds and cancels its own work (`host.cancel`).
- **Re-entrancy** means the host calling into a plugin again while the plugin is in the middle of a call. The specification says a callee "may reenter" during an import call, and that a host "must be careful to only perform recursive reentrance on components when explicitly supported by the component's documented API" ([`Concurrency.md`](https://github.com/WebAssembly/component-model/blob/main/design/mvp/Concurrency.md), "Reentrance"). Syrinx's contract: a plugin may call host imports at any time during an export, and **the host never calls a plugin export from inside one of that plugin's imports**; anything for the plugin is queued and delivered after the current export returns. With synchronous bindings Rust enforces this anyway, because the `Store` is mutably borrowed for the call, and calls into one plugin are serialised through its bounded queue.
- **Async.** No WIT `async` in 0.1.0. WASI 0.3 is the current preview, but wasmtime 49.0.4's `Config::wasm_component_model_async` documentation still says "Wasmtime's support for this feature is very incomplete" (fetched 2026-10-20), and moving a function to async later is a breaking change. Each plugin's `Store` runs on its own worker thread, never the UI thread, so a blocking host import stalls only that plugin. A later 0.2 track can adopt `async func`, `future` and `stream` for Forge's terminal output and language servers once wasmtime calls its support complete; concurrent exports on one instance must then be designed for.

### Mapping from the planned `Plugin` trait

| Plan 2's native trait design | WIT form |
|---|---|
| `syrinx-plugin-api` crate depending on no Syrinx crate | The `syrinx:plugin` WIT package, plus a thin guest crate generated with `wit-bindgen` |
| `Plugin` trait methods (manifest, tab types, commands, search provider) | Manifest file for static facts; exports `lifecycle`, `commands`, `forms`, `views`, `search-feed`, `events`, `widgets` |
| `Host` trait the plugin calls | Imports: `host` and one interface per capability, implemented as `bindgen!`'s generated `Host` traits in application wiring |
| Object-safe boxed async futures | Not needed: an enum of generated instance types per version track, each on its own worker |
| `FormSpec`, list and detail specs | `forms.form-spec` and friends, still declarative and GPUI-drawn |
| `IndexFeed` batches | `search-feed.changes` (pull), with the host owning the cursor |
| "Not a security sandbox" | Now a sandbox: memory limits, epoch deadlines, only requested capabilities linked, grants enforced per call |
| A registration list of built-ins | Installed plugin packages, built-ins included (section 5) |

`xtask`'s `check_plugin_dependency` changes meaning, because plugins leave the workspace: the checks become that `syrinx-core` stays free of wasmtime, that one host crate alone links it, and that the WIT package builds. That is #27's description and #28's code.

### Licences of the prior art

Zed's `crates/extension_api` and its `wit/since_v*/*.wit` files are Apache-2.0 and could be copied with attribution, but nothing is copied, because Zed's world is about language servers and debuggers, not Syrinx's palette, tabs and search. Zed's host (`crates/extension_host`, `crates/extension`) is **GPL-3.0-or-later**: its `wit.rs` and `wasm_host.rs` were read only to describe the patterns above, and no code may come from them. Fermyon Spin and wasmCloud (Apache-2.0), VS Code's experimental component-model support (`@vscode/wasm-component-model`, MIT) and Lapce's WASI plugins (Apache-2.0; note 03 found Lapce grants HTTP to every plugin) were not re-read on 2026-10-20 ⚠️; they support the same conclusion of typed WIT contracts and host-defined capability interfaces.

## 3. Capabilities, the manifest and the owner's grants

If this part is loose, the sandbox means nothing: a plugin granted "files" without a scope can read the whole profile, a grant that survives an update which quietly adds "run programs" hands new power to code the owner never reviewed, and a plugin whose web page can call the network freely bypasses every rule the WASM sandbox enforces.

### What the primary sources say

- **WASI gives no global namespace.** "Absolute paths are not permitted, and there is no global namespace"; resolution "is constrained to occur within the sub-filesystem referenced by the base handle", and "it's not permitted to use paths that temporarily step outside the sandbox". A symlink with an absolute target fails, and one that resolves outside fails with "access denied", checked at resolution time so it holds while other processes rename files ([wasi-filesystem, path-resolution.md](https://github.com/WebAssembly/wasi-filesystem/blob/main/path-resolution.md)). The guest learns its directories only from `preopens.get-directories` ([`wit/preopens.wit`](https://github.com/WebAssembly/wasi-filesystem/blob/main/wit/preopens.wit), commit `943f04e`). **WASI has no file-watching API** in 0.2 or the 0.3 draft (searched `wit/` and `wit-0.3.0-draft/` at `943f04e`), so a watcher must be a Syrinx interface.
- **wasmtime's `WasiCtxBuilder`.** `preopened_dir(host_path, guest_path, perms)`: "By default WebAssembly does not have access to the filesystem because there are no preopened directories", and "WASI will also prevent access outside of files provided here" ([wasmtime-wasi `WasiCtxBuilder`](https://docs.rs/wasmtime-wasi/latest/wasmtime_wasi/struct.WasiCtxBuilder.html), 49.0.4). The permission type changed: in 49.0.4 it is `FsPerms`, `ReadOnly` or `ReadWrite` ([`FsPerms`](https://docs.rs/wasmtime-wasi/latest/wasmtime_wasi/filesystem/enum.FsPerms.html)), while 36.0.0 took separate `DirPerms` and `FilePerms` ([36.0.0](https://docs.rs/wasmtime-wasi/36.0.0/wasmtime_wasi/struct.WasiCtxBuilder.html)). The change came in 48.0.0 (47.0.0's page still takes `DirPerms`; 48.0.0's takes `FsPerms`), so the recommended `=48.0.x` pin already has the new form; it shows this API still moves, which favours a Syrinx-defined file interface over raw preopens. Network is off by default: name lookup is disabled, `socket_addr_check` approves or rejects each address, and `inherit_network` grants everything and is never used. ⚠️ The 49.0.4 page says both that `allow_tcp` is off by default and that TCP is allowed with every address denied; the outcome is the same, but check the source. Environment variables and arguments are empty by default; `inherit_env` would copy the owner's environment, which can hold tokens, and is never used.
- **Does a junction or symlink escape a folder on Windows?** wasmtime-wasi 49.0.4 builds its filesystem on `cap-primitives ^4.0.3`. Reading cap-std at commit `b211211` ([cap-std](https://github.com/bytecodealliance/cap-std), 2026-10-19): on Windows every path is resolved by hand, one component at a time (`cap-primitives/src/windows/fs/open_impl.rs` calls `manually::open`), each opened with `FILE_FLAG_OPEN_REPARSE_POINT` (`windows/fs/oflags.rs`) so a reparse point is never followed by Windows. A link is recognised through `std::fs::FileType::is_symlink`, true on Windows for any reparse tag with the name-surrogate bit (`0x20000000`) set ([std `sys/fs/windows.rs`](https://github.com/rust-lang/rust/blob/master/library/std/src/sys/fs/windows.rs)); both symbolic links and **junctions** carry it. A junction's target is always absolute, and a link whose target begins with a drive prefix or root returns `escape_attempt()` (`fs/manually/open.rs`). Reserved device names, including the superscript variants added for [CVE-2024-51756](https://osv.dev/vulnerability/CVE-2024-51756) (fixed in cap-primitives 3.4.1), are refused before resolution, and directories opened during resolution drop `FILE_SHARE_DELETE`. **Conclusion, from source:** a junction or symlink inside the folder that points outside is refused; a junction pointing *inside* is also refused, because its target is absolute; a relative symlink that stays inside works. ⚠️ Not run on this machine: cap-std's Windows junction tests exist (`tests/fs.rs`, `symlink_junction`), but two are `#[cfg_attr(windows, ignore)]`. P3-01 proves it with generated fixtures (a junction out, a junction in, an absolute symlink, a relative symlink, and `..` through a link).
- **What path checks do not cover,** each ⚠️ to test in P3-01: **hard links** (a file in the vault hard-linked to one elsewhere is the same file; NTFS has no directory hard links, and a safe save that writes a new file and replaces should break the link rather than write through it; verify); **alternate data streams** (`note.md:hidden`, not an escape but a hiding place, so a `:` is refused in any folder-relative path); **cloud placeholders** (OneDrive files-on-demand are reparse points without the name-surrogate bit, read as ordinary files, and reading one can start a download; a vault may live in OneDrive, only the database may not under D13, so this is reported, not treated as an error); and **8.3 short names** (`NOTES~1.MD`), a second spelling that matters for identity and case-collision checks.

### Prior art

| System | Manifest | How powers are declared | How the user grants them | Lesson for Syrinx |
|---|---|---|---|---|
| **Zed** | `extension.toml` (`id`, `name`, `version`, `schema_version`, `authors`, `description`, `repository`), Rust for `wasm32-wasip2` ([developing extensions](https://zed.dev/docs/extensions/developing-extensions)) | `[[capabilities]]` with a `kind` and a scope: `process:exec` with command and argument patterns, `download_file` (host and path), `npm:install` (package) ([capabilities](https://zed.dev/docs/extensions/capabilities)) | No prompt documented; the user narrows `granted_extension_capabilities`, which grants everything by default; a call outside the grant returns an error | Typed, scoped requests that fail with an error are good; granting everything by default is not |
| **VS Code** | `package.json` with `contributes`, `activationEvents`, `capabilities` ([manifest](https://code.visualstudio.com/api/references/extension-manifest)) | No permissions; extensions run unsandboxed. **Workspace Trust** gates the folder: Restricted Mode blocks tasks and debugging ([Workspace Trust](https://code.visualstudio.com/api/extension-guides/workspace-trust)) | A trust prompt per folder | `contributes` is the model for declaring commands and views; Workspace Trust is the model for Forge |
| **Obsidian** | `manifest.json` with `minAppVersion` ([manifest](https://docs.obsidian.md/Reference/Manifest)) | None | Turning Restricted mode off trusts every community plugin | The rejected model; only the minimum-version field is worth copying |
| **Chrome extensions** | `manifest.json` with `permissions`, `host_permissions`, `optional_permissions` | Named permissions with host patterns | A warning at install; optional permissions requested at run time "from inside a user gesture" and removable; an update adding a warning-bearing permission disables the extension "until the user accepts the new permission" ([permission warnings](https://developer.chrome.com/docs/extensions/develop/concepts/permission-warnings), [permissions API](https://developer.chrome.com/docs/extensions/reference/api/permissions)) | The owner's named model: install-time review, optional run-time requests, held updates that widen power |
| **Tauri 2** | Capability files for named windows ([capabilities](https://v2.tauri.app/security/capabilities/)) | `allow` and `deny` scopes; "`deny` always supersedes the `allow` scope", and each command enforces its own scope ([scopes](https://v2.tauri.app/security/scope/)) | Chosen by the developer at build time | Deny wins; each host function checks its own scope |

The principles taken from them: nothing is granted by default (rejecting Zed's default); every request is typed, scoped and carries a shown reason; the owner chooses paths and the plugin never names one; a fixed deny list wins over any grant; widening is held and narrowing is silent; each host function enforces its own scope, because a grant table alone is not enforcement; and full-trust powers are named honestly, because running a program is not sandboxable by WASM.

### The manifest

**Reconciled: one file name.** The capability research called the manifest `syrinx-plugin.toml`, the distribution research `plugin.toml`, and both describe the same TOML file. This note uses **`syrinx-plugin.toml`**, which cannot be mistaken for another tool's file in a plugin repository; the package list (section 5) names it like any other file. TOML fits because Cargo, Zed and the template's own `Cargo.toml` use it, and it allows comments, which a template needs. Field names are proposals for #27, and the owner approves the public format. The draft below shows Notes, with synthetic values:

```toml
# What Syrinx needs to know about this plugin before running any of its code.
schema = 1                           # version of this manifest format

[plugin]
id = "notes"                         # stable; lowercase letters, digits, hyphens; unique
name = "Notes"
version = "0.1.0"                    # SemVer; must equal the release tag
description = "Markdown notes in an Obsidian-compatible vault."
authors = ["Example Author <author@example.invalid>"]
repository = "https://example.invalid/notes-plugin"
licence = "MPL-2.0"
min-syrinx = "0.9.0"                 # oldest Syrinx it runs on (Obsidian's minAppVersion)

[interface]
world = "syrinx:plugin/plugin@0.1.0" # must match the component's interface names

[entry]
module = "plugin.wasm"               # a component built for wasm32-wasip2; never precompiled
ui = "ui/"                           # served only into this plugin's own view
activation = ["on-command", "on-view", "on-search"]   # lazy start

[[contributes.commands]]
id = "notes.new-note"                # must start with the plugin id
title = "New note"
keywords = ["create"]
form = true                          # choosing it shows a declarative form

[[contributes.tab-types]]
id = "notes.note"
title = "Note"
ui-methods = ["open-note", "rename"] # the only view-message methods the host will relay

[[contributes.search-feeds]]
id = "notes.notes"
title = "Notes"
tiers = ["catalogue", "full-text"]   # tier 1 (frizbee) and tier 2

[[capabilities]]
kind = "storage"                     # this plugin's own namespaced settings
reason = "Remember your vault choice and editor settings."
required = true

[[capabilities]]
kind = "folder"
access = "read-write"
count = 1                            # one active vault (N1)
watch = true                         # also wants change events for that folder
reason = "Read and save the Markdown notes in the vault folder you choose."
required = true

[[capabilities]]
kind = "drafts"                      # host-held recovery drafts in local app data (N2)
reason = "Keep unsaved typing safe if Syrinx closes unexpectedly."
required = true

[[capabilities]]
kind = "index"
feeds = ["notes.notes"]
reason = "Let the command palette find notes by title and text."
required = true

[[capabilities]]
kind = "open-external"               # open a link in a normal page tab, on a click
reason = "Open web links in your notes as browser tabs."
required = false
```

Forge would add `folder` with `count = "many"`, and `terminal`, `git`, `language-servers` (D30's installed servers only) and `parse`, the first three being full trust. A future Fledge manifest would request `network` limited to named hosts and an `oauth` capability whose tokens the host holds and attaches, so the module never sees them (D23); that waits with Fledge ⚠️.

Before any code runs, Syrinx refuses a manifest with a named error if its `schema` or `world` version is unsupported, its id is a duplicate (P2-02), a contributed id lacks the plugin's prefix, a capability `kind` is unknown, a `reason` is missing, or a full-trust kind lacks `trust = "full"`. The host decides which kinds are full trust; a manifest cannot downgrade one.

### How the owner sees, grants and revokes

- **Install-time review.** One review sheet shows the name, version, author and repository, a short module hash, the publisher key and whether it is the same as before (section 5), the world version, the licence, and one row per capability with its reason in plain words ("Read and save files in one folder you choose"). Full-trust rows are grouped under a heading such as "Can run programs on your computer with your permissions", with a sentence saying the sandbox does not limit them. Required capabilities are on; optional ones start off. Declining a required capability means the plugin is not installed.
- **Run-time requests.** Optional capabilities are requested when first needed and only from the owner's own action (Chrome's user-gesture rule), such as the first click on a web link in a note. A folder grant opens the Windows folder picker at that moment. A refusal returns `denied` and the plugin shows its own fallback.
- **Settings and revocation.** A Plugins page lists each plugin's capabilities with a switch each, and each folder grant with its path and "Change folder". Revoking takes effect at once: the host closes directory handles, stops watches, ends terminal and language-server processes and cancels pending requests (observed completion, no automatic resubmission), and later calls return `revoked`. Removing a plugin removes its grants, its namespaced storage, its index contributions and its drafts (part of the grants model, owner decision 9).
- **Where grants live.** In Syrinx's SQLite store in local app data with D13's daily snapshots, keyed by plugin id, never in the plugin's own files, so a plugin cannot grant itself anything. A proposed shape for #28 to refine: `plugin_install(plugin_id, version, module_sha256, manifest_sha256, signer_key, state, installed_at, updated_at)`, `plugin_grant(plugin_id, capability, scope_json, granted_at, granted_for_manifest)`, and `plugin_folder(plugin_id, grant_id, path, access)`, kept apart so a path never appears in a scope string. Folder paths are owner data and never go into logs, panic messages, analytics (D39 excludes paths) or fixtures. No grant-history table is proposed, because D39 rejects detailed timelines.
- **The fixed deny list.** No folder grant may equal or contain Syrinx's data folder, its snapshot folder, the WebView2 user-data folder (it holds the owner's signed-in sessions), `%WINDIR%`, `%ProgramFiles%`, drive roots, or the root of the owner's user profile. The check runs on the canonical path, after the host resolves junctions itself, both at grant time and every time the folder is opened. The capability research left open whether to refuse the profile root or only warn ⚠️; this note recommends refusing it, since a plugin that truly needs the whole profile is not one Syrinx should run sandboxed.

**Reconciled: an update that widens a capability keeps the old version running.** The capability research recommended pausing the plugin until the owner accepts a widened required capability; the distribution research recommended installing the new files but not activating them, so the old version stays active, as Chrome keeps an escalating extension disabled rather than broken. The second is adopted, because pausing turns an update the owner has not looked at yet into a lost daily tool. The full rule, from both reports and section 5: same or fewer requests install, and grants no longer asked for are deleted rather than kept dormant; a narrowed request narrows the stored grant silently; a new capability or a wider scope or mode (read-only to read-write, more hosts, one folder to many, any new full-trust kind) holds the new version inactive behind a prompt showing only the difference ("Notes 0.4.0 also asks to: run processes"), and declining keeps the old version; sensitive capabilities (network, processes, terminal) always name exact hosts, commands or folders; a changed reason text with the same capabilities gives a one-line notice; a changed id is a different plugin; and a new world major is reviewed like a fresh install.

### The split between host and plugin

| Host (Syrinx, Rust, audited once, shared) | Plugin (WASM, its own repository) |
|---|---|
| Manifest parsing and validation, grants and the deny list | Product logic and state (S3 holds: state is in Rust, in the module) |
| Plugin-scoped storage (P2-04) | Markdown and YAML parsing, link resolution, Bases evaluation |
| The **document service**: containment, revisions, staged saves | Deciding what to save and when (N2's autosave for Notes, explicit save for Forge) |
| The **watch service** | Reacting to change events |
| The index into tier 1 and tier 2, deletions and the journal (P1-27) | Answering `changes` with batches |
| Plugin views: serving `ui/`, the bridge, isolation; the shared editor (section 4) | Its own HTML and JavaScript; driving the editor |
| Commands, palette forms and tab kinds | Command handlers and form descriptions |
| Tree-sitter parse service | Choosing grammars and queries |
| Terminal (ConPTY), git, the LSP client, processes | Asking for sessions and presenting results |
| Network with a host allow-list and host-held OAuth tokens | API logic on top |
| Memory caps, epoch deadlines, restart after a trap | Staying inside them |

Rejected: **raw WASI preopens and process access, with each plugin building its own safe save, watcher and LSP client** (each plugin would carry its own copy of the riskiest code, D28's "Rust owns disk" would be split across repositories, Forge could not reuse Notes' save service, and #37's "a defect here ships twice" would become *n* times); **generic `process:exec` the way Zed does, and nothing else** (simpler, but every full-trust power through one door with no typed limits); and **a host "Notes service" and "Forge service" with thin plugins** (this defeats separate repositories, because each plugin release would again need a Syrinx release).

### What Notes needs now

| Need | Provider | Why there |
|---|---|---|
| **One vault folder, read-write** (N1, P3-01) | A host **document service** scoped to the granted folder, not a raw WASI preopen | P3-06 needs staged, checked saves (write, flush, compare the fingerprint, replace, handle each failure) and an exact Windows containment policy (case collisions, reserved names, `:` streams). WASI cannot express "replace only if the disk revision matches", and its permission API still moves. Forge reuses the same service (#37). The host implements containment with cap-std's `Dir` plus Syrinx's own Windows checks, so there is one audited policy |
| Read-only bulk scanning | The same service, listing and reading with revision ids | One path for every read keeps revisions consistent. ⚠️ The cost of copying file bytes across the boundary for a large vault is unmeasured; measure on P3-02's synthetic vaults |
| **File watching** (P3-07) | A host **watch** interface with debounced `created`, `changed`, `removed`, `renamed` and `overflow-rescan` events | WASI has none. `ReadDirectoryChangesW` can overflow its buffer, so "everything may have changed, rescan" is part of the contract, along with delayed and missed events |
| **Editor** (D28, P3-04) | The host-provided CodeMirror component (section 4) | The checksum-matched save is a safety property Syrinx must own; line endings are mapped before checksums (trapstoknow, CRLF) |
| **Recovery drafts** (N2) | A host **drafts** store in Syrinx's local app data, per plugin and per document, never in the vault | A vault may sit in OneDrive or Obsidian Sync, and writing half-typed drafts beside it would sync them and could confuse Obsidian; D13 keeps Syrinx's own state out of synced folders. Drafts hold note text, so they stay out of logs and analytics |
| **Catalogue and full-text feed** (#52, #53, P3-03) | The pull-based `search-feed` with the host's `index` capability | Matches Plan 2's owned batches and #53's adapter; the host clears the old vault's contributions on a switch |
| Markdown and YAML parsing (P3-02) | **In the plugin**, for example `pulldown-cmark` built for `wasm32-wasip2` | Pure computation with no I/O, the safest thing to sandbox; keeps vault semantics in the plugin's repository. D28 says "Rust owns parsing", and WASM is still Rust. ⚠️ Parse speed in WASM is unmeasured |
| Images and attachments (P3-09) | The document service's resolver, and `open-external` for an explicit click | Containment applies to images too; previews never fetch remote resources, which the view's CSP also enforces |

**The watcher dependency.** The `notify` crate (8.2.0) is CC0-1.0, which is not on `AGENTS.md`'s licence list. This note recommends a small Syrinx wrapper over `ReadDirectoryChangesW` on the already-pinned `windows` bindings, which needs no licence exception and keeps the overflow behaviour under Syrinx's own tests; approving `notify` instead remains open to the owner.

### What Forge needs later, and why it fits

| Need | Provider | Trust |
|---|---|---|
| Project folders | The same document service, with `count = "many"` | Sandboxed |
| **Tree-sitter** | A host **parse service**: the plugin sends a document id and grammar name and receives trees or highlight spans; grammars are data in one registry | Sandboxed; parsing grants no file or process power |
| **Terminal** (ConPTY) | A host terminal service: Rust owns the pseudoterminal through [`CreatePseudoConsole`](https://learn.microsoft.com/en-us/windows/console/createpseudoconsole), and output streams through bounded queues straight to the tab's xterm.js view; the plugin asks for "a terminal in project folder X" and never names a program | **Full** |
| **git** (D29) | A host git service: reads in process behind `GitRead`; writes and network through `git.exe` with fixed argument shapes, literal pathspecs and no force flags | **Full** for writes and network |
| **Language servers** (D30) | A host LSP service that spawns only installed servers and runs the LSP client in Rust; the plugin gets typed completion, hover, diagnostics and rename | **Full** ⚠️ (how much project code a server runs varies) |
| Tasks (D31) | Through the terminal service | **Full** |

Tree-sitter belongs in the host because D28 says "Rust owns … parsing (Tree-sitter …)", running its C library inside the plugin would need a C toolchain for WASM in the template, and every keystroke's reparse would cross the boundary twice. ⚠️ Tree-sitter's optional `wasm` feature runs grammars compiled to WASM inside its own wasmtime, which could let a plugin ship grammars without native code; in tree-sitter 0.27.1 that feature depends on `wasmtime-c-api-impl ^48.0.1` (crates.io, 2026-10-20), the same line as the recommended pin, so it might share Syrinx's wasmtime rather than add a second, but whether its C-API layer and feature set fit was not checked; check before choosing. Each grammar's licence is audited separately, and Zed's GPL `languages` queries stay out. The terminal stream bypasses the module because routing every byte through it adds latency and copies with nothing gained. The interface's resources (a terminal session, an LSP connection) are exactly what section 2 allowed, and the async that terminal output and language servers will want arrives as a later `0.2` track, so Forge fits without changing 0.1.0's shape.

**Spawning a process is full trust.** A WASM sandbox cannot follow a process out of itself: once the host starts `pwsh.exe`, `git.exe` or `rust-analyzer.exe` for a plugin, that process has the owner's full rights, and Zed's `process:exec` scoping limits *which* program starts, not what it does. The project itself is code: `git status` through `git.exe` can run programs named in the repository's config (`core.fsmonitor`) ⚠️ (verify the exact keys and Git for Windows' `safe.directory` behaviour), commits run hooks that D29 keeps on purpose, language servers run build scripts, and tasks are commands by definition. So Forge asks two separate questions: the plugin grant, "Forge can run programs on your computer", asked once at install; and a per-project trust, "Do you trust the code in this folder?", asked when a project is opened. Until the owner says yes, Forge opens the project read-only for editing and highlighting, git reads go only through the in-process `GitRead`, and terminal, tasks, git writes and language servers stay off. This is VS Code's Restricted Mode, adapted. ⚠️ Whether the in-process git reader can execute repository config depends on the library chosen; note that libgit2's C source is GPL-2.0 with a linking exception even though `libgit2-sys` says MIT OR Apache-2.0, while `gix` is MIT OR Apache-2.0. What still helps short of a sandbox: a Windows Job object with kill-on-close (cleanup, not security), a clean environment with no Syrinx secrets, working directories confined to granted folders, and no inherited handles. An AppContainer or LPAC sandbox for language servers was not investigated ⚠️; it would likely block the toolchains servers need.

### Trust model

The built-ins (Forge, Notes and, when it resumes, Fledge) go through the same manifest, review sheet and grants as any plugin, which follows from the 2026-10-15 decision that the built-ins use the sandbox from the start; "built-in" changes only the label shown and grants nothing automatically, even when bundled. Third-party plugins (later, under D17) get sandboxed capabilities only in their first release: full-trust kinds are refused for them until the owner decides otherwise, and that decision needs signing or provenance. The module's SHA-256 is recorded at grant time. ⚠️ Without a signature there is no proof of who built a module; section 5 adds one.

## 4. The plugin interface: views, bridge, editor, forms

### Why the plugin view must be a different kind of web view

The decision puts a second kind of web view beside the page web views. Page web views carry the rules that keep Google sign-in working and keep untrusted sites out: no IPC handler, no injected script, no custom user agent and no request filter (`AGENTS.md`; `docs/dev/trapstoknow.md`, the `window.ipc` and user-agent traps). A plugin view needs exactly the channel a page must never have. If the two are not kept apart in structure rather than by convention, one refactor can hand a Google page a channel into Syrinx, and a plugin's interface can gain more than its manifest was granted.

What Syrinx does today, read from the code: `crates/syrinx-platform/src/browser/environment.rs` builds one shared Evergreen environment with `CoreWebView2EnvironmentOptions` (SmartScreen on through `--enable-features=msSmartScreenProtection`, `IsCustomCrashReportingEnabled` true, browser extensions off, the stable channel) and refuses to start if `WEBVIEW2_*` override variables are set. `browser/page.rs` builds each page with `with_environment(environment.clone())`, no `with_ipc_handler`, no `with_initialization_script`, no `with_custom_protocol`, devtools and password autosave off, and registers `AcceleratorKeyPressed` itself through `webview2-com`. No editor or plugin view exists yet (#37). The vendored wry shows how its features are built: `with_ipc_handler` adds a document-creation script defining `window.ipc` and registers `WebMessageReceived`, passing `args.Source()` as the request URI (`vendor/wry-0.57.0/src/webview2/mod.rs`, `attach_ipc_handler`); `with_custom_protocol` is a `WebResourceRequested` filter on that one web view for `http(s)://<scheme>.*`, using `AddWebResourceRequestedFilterWithRequestSourceKinds` (all source kinds, so workers and iframes are covered) where the runtime offers it, and with `with_https_scheme(true)` a navigation to `<scheme>://rest` becomes `https://<scheme>.rest` (`src/custom_protocol_workaround.rs`, `src/lib.rs`); and `with_profile_name` and `with_incognito` map to `ICoreWebView2ControllerOptions`' `ProfileName` and `IsInPrivateModeEnabled`, so "webviews with different profile names within the same environment have isolated cookies, storage, IndexedDB, cache, and other site data, while sharing the runtime" (`src/lib.rs`; [wry 0.57.0 `WebViewBuilder`](https://docs.rs/wry/0.57.0/wry/struct.WebViewBuilder.html)).

### Serving the plugin's files

WebView2 offers four ways to load local content ([Using local content in WebView2 apps](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/working-with-local-content)):

| Approach | A real origin | Syrinx sets the headers (CSP) | Serves from a verified package | Cost |
|---|---|---|---|---|
| `file://` | one origin per file path | no | no | fastest |
| `NavigateToString` | `null` origin, `about:blank` | no | no; 2 MB limit, no sub-resources | — |
| Virtual host mapping (`SetVirtualHostNameToFolderMapping`) | yes | **no**: CSP only through a `<meta>` tag the plugin author controls | no (needs a folder) | fast; Microsoft notes media can be slow |
| `WebResourceRequested` (wry custom protocol) | yes | **yes** | **yes**: Rust reads, verifies and answers | each request crosses to the host's UI thread; Microsoft says to limit filters to what must raise the event |

**Recommendation: wry's custom protocol on the plugin view only.** It is the only option where Syrinx, not the plugin, writes the security headers, and Rust serves straight from the installed package after checking each file's hash, so a plugin folder edited on disk is caught rather than served. Plugin interfaces are a handful of small files loaded once, so the per-request hop is acceptable. ⚠️ The time to load a CodeMirror bundle of a few hundred kB through this path is measured in the first spike. Rejected: virtual host mapping (a careless or hostile plugin could simply omit its CSP tag); WebView2's registered custom schemes, `CoreWebView2CustomSchemeRegistration`, because "any WebView2 environments sharing the browser process must be created with identical custom scheme registrations, otherwise the environment creation will fail" ([CoreWebView2CustomSchemeRegistration](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2customschemeregistration)), so adding one would change the environment every page uses; and `file://` and `NavigateToString`, which give no usable origin for message checks or headers.

**The origin** is one per plugin under the reserved `.invalid` top-level domain, for which users "MAY assume that queries ... will always return NXDOMAIN responses" ([RFC 6761 § 6.4](https://www.rfc-editor.org/rfc/rfc6761#section-6.4)): navigating to `syrinx-plugin://<plugin-id>.invalid/index.html` with `with_https_scheme(true)` yields `https://syrinx-plugin.<plugin-id>.invalid/index.html` ⚠️ (read from the vendored source, not yet run). `localhost` is rejected, although Tauri uses `http://tauri.localhost` on Windows, because Chromium resolves `localhost` to loopback without DNS: if the filter ever failed to register, a request would reach whatever local server listens on port 443, whereas under `.invalid` the failure fails closed. ⚠️ Whether Windows' resolver sends a DNS query for a `.invalid` name before failing is unverified; RFC 6761 says name resolution APIs "SHOULD always return immediate negative responses" and "SHOULD NOT send queries" for such names, and Syrinx never navigates the view before the filter is in place, which removes the question. One origin per plugin means the same-origin policy keeps one plugin's interface from another's DOM and storage, and the host can tell plugins apart by origin alone; under site isolation each origin probably gets its own renderer process, a memory cost.

**Reconciled: the request filter.** The capability research suggested a request filter on plugin views to block all non-local requests, and asked the owner to confirm it, since `AGENTS.md` forbids request filters; the interface-view research instead blocks the network with CSP and serves files through a custom protocol, which is itself a `WebResourceRequested` filter. Both reports read `AGENTS.md` the same way: its prohibition covers **page web views and Google's sites** ("Never inject scripts, add request filters, rewrite headers ... on Google's sites"; "Anything that touches a page web view's identity or contents (user agent, IPC handler, injected script, request filter)"). A filter on Syrinx's own plugin view, which loads only Syrinx-served content, is not that case. The design adopted is the second: **the only filter is wry's custom-protocol filter for the plugin's own scheme, on the plugin view type only**; network blocking is done by the host-set CSP, the `.invalid` origin and cancelled navigation, so no general "block everything else" filter is needed. #27's guardrail rewrite should say this in words, so a later reader who sees "request filter" in plugin-view code does not think a rule was broken, and the owner confirms it (owner decision 7).

### Environment and profile

| Option | Cookies and storage | Crash dumps (trapstoknow: "Crash dumps go to Microsoft unless Syrinx says otherwise") | Browser process | Verdict |
|---|---|---|---|---|
| Same environment, default profile (what pages use) | shared with the owner's browsing | inherited, correct | shared | **Rejected**: a plugin origin beside Google's cookies, and a profile-wide setting changed for plugins would reach pages |
| Same environment, separate InPrivate profile | isolated; nothing written to disk | inherited, correct | shared | **Recommended** |
| Separate environment (a second user-data folder) | isolated | set again, a second chance to get it wrong | a second browser process and helpers | **Rejected**: separate user-data folders "consumed more system resources including memory, CPU footprint, and disk space" ([multi-profile support](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/multi-profile-support)), against D6 |

One environment means one browser process with shared GPU and helper processes, and renderers allocated by site ([process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model)); it also keeps the trap "Each tab silently gets its own environment" from recurring, and the plugin view inherits exactly the options `environment.rs` sets. **Reconciled: one profile, not one per plugin.** The capability research suggested a separate profile, "and perhaps one per plugin"; one InPrivate profile for all plugin views is enough, because per-plugin origins already separate their storage, and each extra profile has an unmeasured memory cost ⚠️. InPrivate makes S3 structural: `localStorage`, IndexedDB and cache vanish with the view, so a plugin cannot quietly keep its real state in the browser instead of in Rust, and nothing an interface showed (a note, later a Fledge mail) lands in a WebView2 profile folder. The plain limit: a crash of the shared browser process (not a renderer) ends pages and plugin views together, as it already does for every page tab, and P1-16's recovery applies; a plugin renderer crash is contained to its view.

**Structural separation in Rust.** Plugin views are built from their own type (for example `PluginView` beside today's `Page`) in `syrinx-platform`, so the message handler, the protocol handler and the profile name can only be attached to that type. A unit test in the style of the existing `.update_in(` test refuses `with_ipc_handler`, `add_WebMessageReceived`, `with_custom_protocol` and `with_initialization_script` anywhere in `browser/page.rs`, turning the page rules into something the gate checks.

**Locked down,** following Microsoft's [security guidance](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/security) ("Treat all web content as insecure", "Restrict web content functionality"): `AreHostObjectsAllowed` and `AreDefaultScriptDialogsEnabled` false; devtools off outside development builds; the default context menu off or Syrinx's own; `NavigationStarting` (wry's `with_navigation_handler`) allows only the plugin's own origin, and an ordinary link becomes an explicit "open in a tab" request the host may refuse; new-window requests always denied; permission requests (camera, microphone, location, notifications) denied, with `SavesInProfile` false as for pages (trapstoknow, "A consent click grants a replacement prompt"); downloads denied; and D35's page-text capture never registers on a plugin view.

### The bridge

```
plugin interface (JavaScript, renderer process)
  chrome.webview.postMessage(json)                   // built into WebView2
        │
        ▼
Syrinx host (Rust, UI thread): WebMessageReceived, on the plugin view only
  1. Source() origin equals this view's plugin origin, exactly
  2. the view generation is current (not a closed or recreated view)
  3. size within the limit; parses as the envelope; method declared in the manifest
  4. request id not already pending; the per-view queue not full
        │  owned bytes, posted to the plugin's worker
        ▼
plugin module (wasmtime, Syrinx's process)
  views.message(view-request) -> result<string, plugin-error>
  (may call the host capabilities the owner granted)
        │
        ▼
Syrinx host: drop the reply if the view or its generation changed;
  otherwise PostWebMessageAsJson(reply) to that view
```

The interface never talks to the module directly, for four reasons. They are in different processes (the renderer in Edge's process tree, the module inside Syrinx), so any link is a message through the host, and the only question is whether the host inspects it; it must. The interface is **less trusted than its own plugin**: it renders untrusted data (note text, later an email, a file in a Forge project), and a script injection there must gain no more than the interface already has; because it can only send messages its plugin's Rust code then judges, an injection in a preview cannot read files the plugin was granted. The grants belong to the module, and a second path from JavaScript to the host would need its own grants and review and would put plugin logic in JavaScript, against S3 and against the rejection of Obsidian-style plugins. And running the WASM inside the web view's own JavaScript engine is rejected for the same reasons: the state would live in the browser and the host's capability checks would not apply.

**The channel** is `chrome.webview.postMessage` in and `PostWebMessageAsJson` out, with Syrinx's own `WebMessageReceived` handler registered through `webview2-com`, the same way `page.rs` registers `AcceleratorKeyPressed`. No initialization script is needed, because `window.chrome.webview` is built in. Microsoft's sample checks the sender with `args->get_Source` ("Always validate that the origin of the message is what you expect") and recommends building outgoing JSON with a JSON library through `PostWebMessageAsJson` ([interop of native and web code](https://learn.microsoft.com/en-us/microsoft-edge/webview2/how-to/communicate-btwn-web-native); [security](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/security)). The template ships a small JavaScript helper (request ids, promises, timeouts) in the plugin's own bundle, as VS Code's webviews call `acquireVsCodeApi()`; it is the plugin's file, not something Syrinx injects. wry's `with_ipc_handler` would work and is acceptable on plugin views, but it adds a document-creation script defining `window.ipc` and hands Rust only a string; writing the handler ourselves keeps "no initialization scripts in Syrinx" true everywhere. Rejected: `AddHostObjectToScript`, which projects a native COM object into JavaScript as a generic proxy, against Microsoft's "Avoid generic proxies. Design specific web messages" ([security](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/security); [`AddHostObjectToScript`](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2#addhostobjecttoscript)); and, for v1, Tauri's isolation pattern, a sandboxed iframe that inspects and AES-GCM-encrypts every message ([Tauri isolation](https://v2.tauri.app/concept/inter-process-communication/isolation/)), which guards against the plugin's own front-end dependencies but which Syrinx matches more simply by validating every message in Rust, and which Tauri documents as failing to load ES modules in sandboxed iframes on Windows.

**The envelope:**

```json
{ "v": 1, "kind": "request", "id": 42, "method": "open-note", "body": { } }
{ "v": 1, "kind": "response", "id": 42, "ok": true, "body": { } }
{ "v": 1, "kind": "event", "topic": "theme-changed", "body": { } }
{ "v": 1, "kind": "cancel", "id": 42 }
```

`v` is the bridge version, so an old plugin's interface keeps working after Syrinx changes the envelope. `id` is chosen by the interface and unique among its pending requests; the host rejects a duplicate and drops responses for ids no longer pending. `method` must be one the manifest declared for that view kind (`ui-methods`), so a typo or an attack costs no WASM call. The host sends only responses and events, never script. Because the trap "A recreated controller can reuse an old permission request number" applies equally to request ids across view recreation, the host allocates a view generation and checks it on every reply. Validation order before any WASM call: origin, generation, size, JSON shape, method, queue bound. A fixed cap per message (for example 1 MiB) is proposed ⚠️ (WebView2 documents no message size limit that was found; the number comes from the first spike), and large payloads go as host-served responses on the plugin's origin (`GET https://syrinx-plugin.<id>.invalid/data/<token>`) rather than inside a message. Every request is asynchronous for the interface; when a view closes or is recreated, the host cancels its pending requests and drops late replies, and a `cancel` reaches the plugin as a cancellation it may or may not honour.

**Reconciled with the interface's two JSON channels.** The interface research modelled this as `views.message(view, json)` plus `host.post-view-message`; the interface-view research as `handle-ui-message(view, request-id, method, body)` plus `post-event(view, topic, body)`. They are the same channel; the draft world in section 2 adopts the richer form (`views.message` takes a request record with the host-checked id and method, and `host.post-view-event` sends events), so the host's checks are visible in the types while the body stays a JSON string the plugin parses with `serde`. Typed WIT records per method were rejected, because they would bake every plugin's interface messages into Syrinx's WIT.

### Content Security Policy and network

Syrinx sets this header on every HTML response it serves for a plugin ([MDN: Content-Security-Policy](https://developer.mozilla.org/docs/Web/HTTP/Headers/Content-Security-Policy)):

```
default-src 'none';
script-src 'self';
style-src 'self' 'nonce-<per-load>';
img-src 'self' data: blob:;
font-src 'self';
connect-src 'self';
worker-src 'self';
frame-src 'none'; child-src 'none'; object-src 'none';
base-uri 'none'; form-action 'none'; frame-ancestors 'none'
```

**Reconciled:** the capability research proposed `default-src 'self'`; the stricter `default-src 'none'` with named `'self'` sources is adopted, since it allows nothing that was not listed. **No network from a plugin interface, ever**, even for a plugin that holds a network grant: `connect-src 'self'` lets it fetch only Syrinx-served data on its own origin, and a granted plugin reaches the network through its module's host function, where the grant is checked. That keeps one place where network use is decided. WebView2 documents that registered custom schemes "participate in CORS and adheres to CSP", and wry's rewrite is ordinary https, so normal CSP applies. No remote or inline script and no `'unsafe-eval'`: the scripts are the files in the reviewed package. CodeMirror 6 inserts its own style elements and supports CSP through the `EditorView.cspNonce` facet ([CodeMirror reference](https://codemirror.net/docs/ref/#view.EditorView^cspNonce)), so the host generates a nonce per load (the reference, read on 2026-10-20, describes the facet as providing "a Content Security Policy nonce to use when creating the style sheets for the editor") ⚠️ (confirm in the spike that CodeMirror runs without `'unsafe-eval'`). Remote images stay blocked in v1; fetching them later through a plugin's grant and serving them on its origin would be a network call made on the plugin's behalf, an owner decision. VS Code reaches a similar place with a recommended CSP `<meta>` tag from `default-src 'none'` and restricted `localResourceRoots` ([VS Code webview API](https://code.visualstudio.com/api/extension-guides/webview)); Syrinx is stricter by sending the policy as a header the plugin cannot remove.

### The Notes and Forge editor (D28)

**Reconciled: the editor is a host component.** The capability research placed "its HTML, JavaScript and CodeMirror configuration" on the plugin's side; the interface-view research recommended one host-provided CodeMirror component that the plugin drives. The host component is adopted. D28's exception to S3 is safety-critical: CodeMirror owns the live buffer and undo while typing, and "Rust is the authority for what is on disk and writes a save only after the two copies' checksums match". #37 lists what that protocol must survive (delayed, duplicate and stale messages, mixed line endings, UTF-16 to UTF-8 offsets, interrupted saves). If each plugin carried its own copy, each would have to get it right, and a buggy or hostile plugin could ship a CodeMirror that reports a matching checksum for text it never sent. Forge's plan already says it will reuse plan 3's document service (#37), and a host component makes that reuse literal. Syrinx ships one audited CodeMirror bundle on a Syrinx-owned editor origin; the document bridge (open, change sets with base revisions, checksums, save) runs between that view and the document service, never through the plugin; the plugin drives the editor through WIT (which document, read-only or editable, and the language data Rust computes: live-preview decorations, outline, completions, diagnostics). Notes' live-preview rules are computed in the plugin's Rust (pulldown-cmark) and sent as decoration ranges. The trade-off: a plugin cannot add its own CodeMirror extension JavaScript in v1, because that means loading plugin script into the trusted editor view, which needs its own design. Whether a plugin tab shows the editor and the plugin's own interface in one view or as two side-by-side child views is for #27; one view is cheaper (one renderer), two keep the origins apart ⚠️ (choose two only if the measurement shows the second renderer is cheap).

### Focus and keyboard

A plugin view is a WebView2 child window like a page, so the same traps apply and the same code handles both. Ctrl T, W, L and K must reach Syrinx while a plugin view has focus, so `AcceleratorKeyPressed` is registered on every plugin view through the shared function `page.rs` uses, keeping its rules (set `Handled`, post to the event loop, no cross-process COM calls inside the handler, ignore auto-repeat; trapstoknow, "Browser shortcuts never reach Syrinx while a page has focus"). Unlike pages, `AreBrowserAcceleratorKeysEnabled` is false on plugin views, so F5, Ctrl R and Ctrl P do not reload or print the interface and lose unsaved editor state; editing keys need a routing table, and which keys a plugin may claim is for #27. The palette, an owned GPUI popup, draws over a plugin view as over a page (trapstoknow, "GPUI draws above a WebView2 child window"; D7), and closing it returns focus by the same path (activate natively and synchronously when Syrinx is in front, never `activate_window` when another app is, then `WebView::focus()`). Reuse `page_took_focus` with its `chrome_has_keyboard_focus` check, because `GotFocus` does not prove the view holds the keys (the P1-36 and #57 traps). A hidden view may be suspended only after its load completes and never while CodeMirror holds unsaved text; an interface-only view holds no state of its own and can be destroyed when its tab closes. `set_bounds` is called whenever the page card changes, and F6 traversal follows P1-36's rules for pages.

### Memory per extra web view ⚠️

D6 requires memory with 10 tabs within 10% of Edge's, and D28 already accepted "One WebView2 renderer is used only while a Notes or Forge tab is open". By structure: the browser, GPU and helper processes are shared and cost nothing new; each view adds a controller and, with its own origin, most likely its own renderer, since a new domain "will usually start a new renderer process" ([process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model)); the InPrivate profile's in-memory state is unknown. **No figure has been measured on this machine**, none appears in the repository's docs, and none was found in Microsoft's documentation. The proposed measurement, run when the owner allows it in #28: a fresh synthetic profile with D6's 10 synthetic page tabs on `example.com`-style fixtures; add one plugin view serving a "hello" interface, then a CodeMirror view with a generated 1 MB document, then three plugin views; after each step sum the private working set across the process tree with `ICoreWebView2Environment8::GetProcessInfos` and record idle CPU for 60 s; and report whether suspending or destroying hidden plugin views brings it back down. Ways to keep the cost down, in order: one view per plugin, not per document; destroy interface-only views when their tab closes; suspend hidden editor views; keep palette forms and Home widgets out of web views.

### Palette forms and Home widgets stay declarative

Both the interface research and the interface-view research reached this independently. The 2026-10-15 decision says a plugin "draws its interface in HTML and JavaScript inside Syrinx's own local web view"; this note reads that as the form for **tabs**, not for palette forms, and the owner should confirm the reading, because it settles #29's open ⚠️. The palette is a GPUI popup created once and shown and hidden for speed (DW-90); a web view inside it would add a renderer to the palette's open path, a second child window whose focus must be routed inside an owned popup (the hardest focus code Syrinx has), and a slower first paint, while Rust owning the field state is what Plan 2 already settled. Home widgets (#31) start declarative for the memory reason: ten widgets as web views could be ten renderers on the page the owner opens most. A web-view widget type could come later for a plugin that truly needs one, as #31's decision.

### Prior art compared

| System | Interface | Channel to the backend | Sandbox |
|---|---|---|---|
| VS Code webviews ([docs](https://code.visualstudio.com/api/extension-guides/webview)) | HTML and JavaScript, "isolated contexts that cannot directly access local resources" | `postMessage` both ways through `acquireVsCodeApi()`; warns that `retainContextWhenHidden` "has high memory overhead" | The webview is sandboxed; the extension is not |
| Tauri isolation ([docs](https://v2.tauri.app/concept/inter-process-communication/isolation/)) | HTML and JavaScript | IPC through a sandboxed iframe that inspects and encrypts each message | Rust core trusted; front end untrusted |
| Obsidian ([plugin security](https://obsidian.md/help/plugin-security)) | Plugins run JavaScript in the app | Direct | None: "Obsidian cannot reliably restrict plugins to specific permissions or access levels" |
| **Syrinx (proposed)** | HTML and JavaScript in a plugin view: per-plugin origin, host-set CSP, InPrivate profile | `chrome.webview.postMessage`, Rust validation, then the plugin's WASM export | Interface untrusted; module sandboxed with granted capabilities only |

Two side notes for the owner. `AGENTS.md`'s "Only Syrinx's own local web views (the editors) talk to Rust" is widened by the 2026-10-15 decision to every plugin's interface, third-party ones included; #27 updates the guardrail text, as the decision says, and should state that the plugin view's protocol filter and message handler are allowed only on the plugin view type. And page web views already expose `window.chrome.webview` (WebView2's default `IsWebMessageEnabled` is true); with no handler registered, messages from a page go nowhere. Setting it false on pages would close that surface, but it changes page web view settings, so it is noted only, not recommended without the owner, and is unrelated to plugin views.

## 5. Distribution: packaging, loading, updating and review

Plugin code now reaches the owner's machine by a route that is not Syrinx's own signed update. A sandbox limits what a plugin does with its grants, but it does nothing if the bytes were swapped on the way or an update quietly asks for more; the route must not break the rule against "any network call Syrinx makes on its own" (the only exception so far being the owner-pressed update check against one compiled-in host, `syrinxapp.dev`, [`docs/decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/decisions.md), 2026-10-13); and the owner, or an agent working for the owner, needs a concrete checklist before saying yes. If this is skipped, whatever the template's CI happens to upload becomes the format by accident, and signatures and capability diffs have to be retrofitted onto installed plugins.

More terms: a **package** is everything one plugin version ships; **sideloading** is installing from a file the owner picked rather than a store; a **provenance attestation** is a signed statement, made by CI, that a file with a given SHA-256 was built by a given workflow at a given commit of a given repository (GitHub calls these artifact attestations); a **reproducible build** gives byte-identical output from the same source on different machines; **TOFU** (trust on first use) records who signed the first install and requires later updates from the same signer.

### The model to reuse: Syrinx's own update format

Syrinx already has a reviewed, tested design for installing signed bytes from a folder or one host and nothing else. `docs/update-format.md` defines a canonical signed manifest of file hashes (UTF-8, LF, one canonical byte form re-serialised and compared byte for byte, `file <sha256> <size> <path>` lines sorted by path, strict relative paths of ASCII letters, digits, `.`, `_` and `-`), of which only the manifest is signed; a 76-byte detached signature (magic `AUS1`, an 8-byte key id from the first bytes of SHA-256 over the public point, ECDSA P-256 `r || s`) checked with Windows CNG through the already-pinned `windows =0.62.2` crate *before* the manifest is parsed, with RustCrypto and `ring` rejected to avoid new audited packages; pinned keys only ("It never fetches a key, accepts one from the package or reads one from the registry or a settings file"); installing exactly the verified bytes from the verifier's handles; and refusing a version lower than *or equal to* the installed one. `docs/update-delivery.md` chose folders over archives ("Syrinx needs no archive parser, which would be new attack surface and probably a new dependency"), and only `DirectoryPayload` enforces "refuse any unlisted entry". The network path is one compiled-in HTTPS host, no redirects, no cookies, no identifier, a constant agent string, only after the owner presses a button, with only `syrinx-update-net` allowed to use WinHTTP. Since 2026-10-14 the release key is an encrypted GitHub Actions secret, with the accepted trade-off that whoever takes over the owner's GitHub account or the workflow can sign. The honest limit applies to plugins too: "The update path defends against the network, the host and corrupted downloads, not against malware already running as the owner".

### Prior art

- **Zed's registry builds the binary.** Publishing is a pull request to [`zed-industries/extensions`](https://github.com/zed-industries/extensions) adding the extension's public repository as a submodule and an `extensions.toml` entry whose `version` "matches the one set in `extension.toml` at the particular commit" ([publishing guide](https://zed.dev/docs/extensions/publishing/publishing-guide)); an update moves the submodule ([updating](https://zed.dev/docs/extensions/publishing/updating-and-maintenance)), so every release is reviewed as a pinned commit, and "every submission is reviewed, and not all of them make the cut" ([registry README](https://github.com/zed-industries/extensions)). CI checks a root licence against a list that includes GPLv3 and LGPLv3 ([licence requirements](https://zed.dev/docs/extensions/publishing/license-requirements)), so Syrinx, which allows no GPL, cannot copy it. The registry's CI ([`.github/workflows/ci.yml`](https://github.com/zed-industries/extensions/blob/main/.github/workflows/ci.yml)) builds for `wasm32-wasip2` and uploads the packages, so **the author's binary is never used** ⚠️ (whether packages are signed and how Zed verifies them was not read). Local development is "Install Dev Extension", shown as "Overridden by dev extension" ([developing extensions](https://zed.dev/docs/extensions/developing-extensions)). The lesson: build from a reviewed commit by a trusted builder; Syrinx gets this for one owner through provenance attestations and reproducible builds, without a registry.
- **Obsidian** reads `community-plugins.json` and downloads `manifest.json`, `main.js` and `styles.css` from the GitHub release tagged with the manifest version, learning updates from the default branch and minimum app versions from `versions.json` ([obsidian-releases README](https://github.com/obsidianmd/obsidian-releases)); submission verifies repository ownership and runs an automated review ([submit your plugin](https://docs.obsidian.md/Plugins/Releasing/Submit+your+plugin)), and Obsidian "automatically scans every plugin version" with manual review for "popular, featured, and flagged plugins" ([plugin security](https://obsidian.md/help/plugin-security)). Worth copying: version equals release tag, and a minimum app version. Not worth copying: a release asset can be replaced by whoever controls the author's account, and nothing on the user's machine notices.
- **VS Code Marketplace** signs every extension at publication and VS Code verifies on install, with malware scanning and, since 1.97, a trust prompt for third-party publishers ([extension runtime security](https://code.visualstudio.com/docs/configure/extensions/extension-runtime-security)). ⚠️ The format is undocumented by Microsoft; third parties say it is PKCS #7 over per-file SHA-256 digests produced by the proprietary `@vscode/vsce-sign` ([node-ovsx-sign](https://github.com/filiptronicek/node-ovsx-sign)), and researchers bypassed the "verified" badge with crafted VSIX files ([OX Security](https://www.ox.security/blog/can-you-trust-that-verified-symbol-exploiting-ide-extensions-is-easier-than-it-should-be/)). "Install from VSIX…" exists without security guidance ([publishing extensions](https://code.visualstudio.com/api/working-with-extensions/publishing-extension)). The lesson: a per-file hash list with one signature is Syrinx's existing shape, and a signature proves only who published, which is why review and the capability prompt still matter.
- **Tauri's updater** requires a signature that "cannot be disabled", with the public key embedded as a literal ("It **cannot** be a file path!") and the private key in CI ([Tauri updater](https://v2.tauri.app/plugin/updater/)). ⚠️ The page does not name the algorithm; Tauri is understood to use minisign, Ed25519 over a BLAKE2b-512 prehash with a *trusted comment* covered by a second signature, which can hold "application-specific metadata such as the intended file name, timestamps, resource identifiers, or version numbers to prevent downgrade attacks" ([minisign](https://jedisct1.github.io/minisign/)). The useful idea is to sign the plugin id and version together with the hashes; adopting minisign itself would add an Ed25519 crate for no gain, so it is rejected.
- **Chrome** disables an extension whose update adds a warning-bearing permission "until the user accepts the new permission" ([permission warnings](https://developer.chrome.com/docs/extensions/develop/concepts/permission-warnings)).
- **Sigstore and GitHub attestations.** an attestation records the workflow and "the repository, organization, environment, commit SHA, and triggering event for the artifact" ([artifact attestations](https://docs.github.com/en/actions/concepts/security/artifact-attestations)), and `actions/attest` needs `id-token: write`, `contents: read` and `attestations: write` ([using artifact attestations](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations)); "as of version 4, `actions/attest-build-provenance` is simply a wrapper on top of `actions/attest`" and "new implementations should use `actions/attest`" ([attest-build-provenance README](https://github.com/actions/attest-build-provenance)). Public repositories sign through the Sigstore Public Good instance with a public transparency log; private repositories use GitHub's own instance with none; attestations alone give SLSA v1.0 Build Level 2, and Level 3 needs a reusable workflow; they are "not a guarantee that an artifact is secure", and "generating attestations alone doesn't provide any security benefit, the attestations must be verified for the benefit to be realized" ([artifact attestations](https://docs.github.com/en/actions/concepts/security/artifact-attestations)). **"To use artifact attestations in private or internal repositories, you must be on a GitHub Enterprise Cloud plan"**; on Free, Pro and Team plans they are available for public repositories only (the using page and the attest-build-provenance README both say so) ⚠️, and whether the Fledge-Software organisation has that plan is unknown. Verification is `gh attestation verify <file> -R <owner>/<repo>`, or offline with `gh attestation download`, `gh attestation trusted-root` and `--bundle --custom-trusted-root`, which compares repository *names* only, so confirm the repository *id* separately ([verify offline](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/verify-attestations-offline)); cosign's equivalent is `cosign verify-blob` ([cosign verify](https://docs.sigstore.dev/cosign/verifying/verify/)). The lesson: provenance is checked at *review* time with the `gh` CLI outside Syrinx; verifying Sigstore bundles inside Syrinx would need X.509, transparency-log proofs and a fetched, rotating trusted root, a large new dependency and a new network host, rejected for v1.
- **Reproducible WASM builds.** Rust builds are often reproducible once the toolchain is matched and paths are normalised ([reproducible-builds.org, Rust](https://reproducible-builds.org/docs/rust/)); commit `Cargo.lock`, build with `--locked` (otherwise Cargo "may resolve to newer compatible versions and produce a different Wasm") and remap paths with `--remap-path-prefix` ([Internet Computer reproducible builds](https://docs.internetcomputer.org/guides/canister-management/reproducible-builds.md)); build-directory references can survive in compressed `.rustc` metadata ([Yocto discussion](https://patchwork.yoctoproject.org/comment/13986)), and linker and debug-symbol settings matter ([Tock #1666](https://github.com/tock/tock/issues/1666)). ⚠️ No primary source confirms that `wasm32-wasip2` output, including the component wrapping step, is reproducible across Windows and Linux; the template's CI tests it before anyone relies on it.

### Package format: a folder with a signed, canonical file list

```text
notes-0.3.0/
  syrinx-plugin.toml       the manifest (section 3)
  plugin.wasm              the component; never a precompiled .cwasm
  ui/index.html            interface files, served by the custom protocol
  ui/app.js
  ui/app.css
  LICENSE
  THIRD-PARTY-LICENSES     licences of every bundled crate and JavaScript package
  package                  canonical file list in update-format's shape
  package.sig              detached signature
```

`package` reuses `docs/update-format.md`'s rules exactly, with its own first line so it can never pass as a Syrinx update:

```text
syrinx-plugin-package 1
id notes
version 0.3.0
world syrinx:plugin/plugin@0.1.0
file 3f0c…(64 hex) 1048576 plugin.wasm
file 9a1b…(64 hex) 2048 syrinx-plugin.toml
file …                     ui/app.js
```

A folder, because the update path chose folders to avoid an archive parser, and `DirectoryPayload` already refuses unlisted entries, checks sizes and hashes and hands back verified handles; Windows Explorer unzips natively, so the owner's step is "Extract All…, then Install plugin from folder…". The id and version sit inside the signed bytes (minisign's trusted-comment idea), so a validly signed package cannot be renamed into another plugin's slot or replayed as an older version. The world version is repeated so the checklist and prompt can show it without parsing the component. The interface files are hashed too, because they are code, and the custom-protocol handler serves them only after checking. A release also carries a zip of exactly that folder (`syrinx-plugin-notes-0.3.0.zip`), `SHA256SUMS` and the attestation; the zip is transport only and nothing in Syrinx reads it. The signature is a P-256 signature in the `AUS1` shape with a distinct magic such as `SPP1`, verified with the same CNG code. The magic only catches a misplaced file early: `docs/update-format.md` warns that it "sits outside the signed digest, so anyone can relabel" a signature. What keeps a plugin package from passing as a Syrinx update, or the reverse, is the signed first line (`syrinx-plugin-package 1` against `syrinx-update-manifest 1`), each parser requiring its own header and the byte-exact canonical reparse, together with the separate keys. Rejected: a single-file `.syrinx-plugin` archive (nicer to double-click, but it needs a zip parser in Syrinx, and registering a file type touches the registry, which is the owner's; it can come later as its own decision); and the component alone with the manifest in a custom section, as Zed embeds its version (discovery would have to parse WASM, and interface files would need a second channel anyway).

### The compiled-code cache

wasmtime can save its native code (`Component::serialize`, `Engine::precompile_component`) and load it later (`Component::deserialize`, `deserialize_file`). The safety facts: `deserialize` is `unsafe` because "if fed invalid input or used improperly this could lead to memory safety vulnerabilities"; "Arbitrary input could, for example, replace valid compiled code with any other valid compiled code"; the blob "is only lightly validated" and "should not ... be exposed to arbitrary user input"; a blob from another wasmtime version "will deterministically and safely produce an Err" ([`Module`](https://docs.wasmtime.dev/api/wasmtime/struct.Module.html); the component methods share this, [`Component`](https://docs.wasmtime.dev/api/wasmtime/component/struct.Component.html)); if `Engine::precompile_compatibility_hash` "matches between two `Engine`s then binaries from one are guaranteed to deserialize in the other" ([`Engine`](https://docs.wasmtime.dev/api/wasmtime/struct.Engine.html)); and `deserialize_file` memory-maps the file, so "any changes to the file on disk may change future instantiations". A cache file is native code that bypasses the sandbox if tampered with, so:

1. A package never contains precompiled code; install refuses any `.cwasm` or anything `Engine::detect_precompiled` recognises. Native code is only produced on the owner's machine, from a verified `plugin.wasm`.
2. The cache lives in the local data folder (`%LOCALAPPDATA%\Syrinx\data\PluginCache\`), under P1-03's storage lease, never roaming or synced (D13), outside every backup.
3. The key is the SHA-256 of the verified `plugin.wasm`, the engine's `precompile_compatibility_hash` (believed, not confirmed, to cover CPU features ⚠️) and Syrinx's own version, so a Syrinx upgrade, a configuration change such as turning on epoch interruption, or a data folder copied to another machine all miss and recompile.
4. **Reconciled: read with `deserialize`, not `deserialize_file`.** The runtime research proposed loading with `Component::deserialize_file`; because that maps the file and a later edit on disk could reach a mapped page, the distribution research's rule is adopted: read the bytes, check their SHA-256 against the value Syrinx recorded when it wrote them, and pass the checked bytes to `deserialize`. A mismatch deletes the entry and recompiles.
5. Stale entries are deleted on uninstall, update and at start-up.

The honest limit is the same as for updates: a program running as the owner can write to the data folder and plant native code between Syrinx's write and its next read, and the recorded hash lives in the same user's storage; rule 4 narrows that, and no per-user design closes it. wasmtime's own on-disk cache (`Config::cache_config_load` and the `cache` feature) is not recommended ⚠️, because it manages its own directory and eviction outside Syrinx's lease and recorded hashes. The fallback, if the owner prefers no native code on disk, is to compile at every activation; with activation on first use, that cost is paid once per session per plugin, not at start-up.

### Loading

| Route | What it is | Network | Verdict |
|---|---|---|---|
| **Bundled with Syrinx** | Syrinx's release includes the current built-in packages as payload files listed in Syrinx's own signed manifest | None beyond Syrinx's approved update | Recommended for built-ins (owner decision) |
| **Installed from a folder the owner chose** | "Install plugin from folder…" in the Plugins section and the palette | None | Recommended for v1 |
| **Fetched from a GitHub release** | Syrinx downloads the zip from `github.com/<org>/<repo>/releases/...` | A new host, a redirect to `release-assets.githubusercontent.com` (observed 2026-10-20), a token for a private repository | Rejected for v1 |

**Bundled built-ins are still separate packages.** The 2026-10-15 decision rejected *compiling* built-ins into `syrinx.exe`; shipping their packages *beside* it does not bring that coupling back. The weekly release workflow pins each built-in by version and SHA-256 in a small file in Syrinx-app (for example `plugins.lock`), downloads that release in CI, verifies its attestation there, and lists its files in Syrinx's own update manifest, so the Syrinx release key covers them. The built-in loads through the same host, prompt and grants as any plugin, and a bundled plugin still asks for its capabilities on first activation. A plugin release still needs no Syrinx release: the owner can install a newer Notes from a folder, and the installed copy overrides the bundled one, as Zed shows "Overridden by dev extension", until a Syrinx release bundles an equal or newer version. ⚠️ This changes the release workflow, which needs the owner's yes under `AGENTS.md`, and makes CI (not the owner's machine) download from GitHub. Rejected: shipping no plugins, which is simpler but leaves a fresh install without Notes until the owner fetches it.

**Sideloading.** (1) The owner picks a folder; Syrinx copies it into `PluginStaging\` with `FolderSource`'s handle discipline. (2) It verifies `package.sig`, then `package`, then every listed file under `DirectoryPayload`'s rules. (3) It reads `syrinx-plugin.toml` without compiling, checking that id, version and world match the signed list, that the world is in the support window, and that the minimum Syrinx version is met. (4) It shows the review sheet. (5) On yes, it copies from the verified handles into `Plugins\<id>\<version>\` and records the package hash, the signer key and the grants; on no, it deletes the staging folder. (6) The previous version's folder is kept until the new one has activated once, so a plugin that fails to start can be rolled back. A **developer mode** for plugin authors may allow an unsigned folder, as Zed's dev extensions do, but only as an explicit owner setting, off by default, with a persistent "unsigned development plugin" label, and never for a folder that also holds a valid signature from a different key.

**Fetching** is not in v1. Any fetch must reuse what the owner approved on 2026-10-13: an owner-pressed button, one compiled-in HTTPS host, no redirects, no credentials, signed statements and `syrinx-update-net` as the only network crate. `github.com` would be a second host, its release downloads redirect (which the update path refuses by design), and a private repository would need a token in Syrinx, the reasons GitHub Releases were rejected for Syrinx itself (2026-10-13 and 2026-10-14). If the owner later wants "Check for plugin updates", the recommended form is a signed index at `https://syrinxapp.dev/plugins/v1/latest` with the same statement rules as `updates/v1/latest` and files under `plugins/v1/<id>/<version>/files/...`, published by the owner's workflow; that widens the only network exception, so it is its own decision and a `check-deps` review.

### Updating

Updates are owner-initiated only: no background check, no check at start-up, no timer. In v1 an update is "Install plugin from folder…" with a newer version, and bundled built-ins update with Syrinx. An update is accepted only if it has the same id and a strictly higher version (a deliberate downgrade is uninstall then install, with grants starting over); is signed by the key recorded at first install or by a successor key the previous version's manifest announced (the rotation shape of `PRODUCTION_KEYS`), with a different key refused as "This update is signed by a different publisher" (bundled built-ins are covered by the Syrinx release key instead); targets a world still in the support window ("This plugin needs a newer Syrinx" otherwise); and passes the capability diff of section 3. The recommended signing arrangement is a P-256 key per plugin repository held as an encrypted Actions secret, created outside any agent session and never seen by an agent, mirroring the owner's 2026-10-14 choice for Syrinx. Rejected: hash pinning only (safe, but no continuity between versions, and an agent cannot tell "same publisher" from a hash); Sigstore verification inside Syrinx (a large dependency and a fetched trusted root); and minisign (a second algorithm and crate for no gain). Grants that name the owner's folders or accounts are data and stay out of logs, fixtures and chats.

### Reviewing a plugin before installing

A checklist the owner can follow, and an agent can run on the owner's behalf; an agent never installs, it reports, and the owner decides. Everything is checked without running the plugin.

- **Identity and source:** the repository is the expected one by owner, name *and* id (`gh api repos/<owner>/<repo> --jq .id`, since names can be reused after a rename); the release tag equals `version` in the manifest at that commit and points at a commit on the default branch; the signer key id matches the one recorded at first install or an announced successor.
- **Licence:** `LICENSE` is MIT, Apache-2.0, BSD or MPL-2.0 (Syrinx's rule, not Zed's list); `THIRD-PARTY-LICENSES` lists every bundled crate (from `cargo deny` or `cargo about`) and every JavaScript package, with no GPL; an unknown licence stops the install and goes to the owner.
- **Interface version:** the world is in the support window, matches the manifest and enables no unstable feature the owner has not approved.
- **Capabilities:** the list, or for an update the diff, read line by line: why this plugin needs each one, and whether the scope is as narrow as possible; network, process and terminal requests name exactly what they reach and the README explains each; a plugin with no stated reason for network access is declined.
- **Provenance:** `gh attestation verify plugin.wasm -R <owner>/<repo>` succeeds, and the attestation names the repository's own release workflow at the tagged commit, triggered by the tag push; the same for the zip, and `SHA256SUMS` matches; the offline variant if needed.
- **Reproducibility** (recommended for built-ins and any plugin requesting a sensitive capability): check out the tag, build with the pinned `rust-toolchain.toml` and `cargo build --locked --release --target wasm32-wasip2`, and compare the SHA-256 with the release; a mismatch is reported but is not proof of tampering while reproducibility is unconfirmed ⚠️.
- **Code:** read the diff since the last reviewed tag, not only the latest commit: new dependencies in `Cargo.lock` and `package.json`, new uses of granted capabilities, `build.rs`, and any CI script; `ui/` holds no remote script, remote font or `eval` (the CSP would block them, but their presence means the plugin expects to do something forbidden); no secret, token or real personal data.
- **Package hygiene:** only listed files, no `.cwasm`, no executable other than `plugin.wasm`, no path outside the rules.

### The template's CI (#30)

Two workflows, both written only after the owner confirms them (`AGENTS.md`: CI scripts), with every third-party action pinned by full commit SHA. **`ci.yml`, on every push and pull request:** install the toolchain from `rust-toolchain.toml` with `wasm32-wasip2`; `cargo fmt --check`, `cargo clippy -D warnings` and host-target unit tests; `cargo build --locked --release --target wasm32-wasip2`; validate that the component's world equals the manifest's and exports the required interfaces ⚠️ (tooling such as `wasm-tools component wit`, or a small checker Syrinx publishes, needs its own licence check); **contract tests** against a wasmtime-based test host that Syrinx publishes, implementing `syrinx:plugin` with synthetic capabilities, running the lifecycle (activate, command, form submit, view open and close, deactivate) plus negative tests (an ungranted capability fails with the typed error, an oversized message is refused), all on synthetic data (`example.com`, `.test`, `.invalid`); a licence check with Syrinx's allow-list for crates and `ui/` packages ⚠️ (tool choice is the template's own audit); and a reproducibility check, building twice in different directories. **`release.yml`, on a version tag only:** refuse unless the tag equals the manifest version; run `ci.yml`; assemble the folder, write `package` and sign it with the repository's key; zip it and write `SHA256SUMS`; attest `plugin.wasm` and the zip with `actions/attest`, with `id-token: write`, `contents: write` and `attestations: write` scoped to that job (a reusable workflow for SLSA Build Level 3); and publish a GitHub release. ⚠️ On private repositories without GitHub Enterprise Cloud the attestation step fails; the choice is public plugin repositories (needed anyway for a public registry later) or relying on the package signature and reproducibility alone.

## Conflicts between the five reports, and how they were settled

| Point | What the reports said | Settled as |
|---|---|---|
| wasmtime version | Interface and capability research cite 49.0.4 (and 51.0.0-dev) docs; runtime research recommends the 48.x LTS | Pin `=48.0.x`; re-check every cited API and repeat the licence audit at that pin (section 1) |
| wasmtime `async` feature | Runtime feature list has none; its audited graph and the interface research's async host imports assume it | No async execution in 0.1.0; synchronous imports on a per-plugin worker. The `async` Cargo feature is still compiled in, because `wasmtime-wasi` forces it, so the audit covers the graph with it (section 1) |
| Request filter on plugin views | Capability research: a filter to block non-local requests, owner to confirm; interface-view research: CSP plus custom protocol | Only wry's custom-protocol filter, on the plugin view type; CSP, `.invalid` origin and cancelled navigation block the network. `AGENTS.md` forbids filters on page web views and Google's sites only; #27 says so in words (section 4) |
| Profile for plugin views | Capability research: separate profile, perhaps one per plugin; interface-view research: one InPrivate profile | One separate InPrivate profile, same environment, per-plugin origins (section 4) |
| CSP | `default-src 'self'` against `default-src 'none'` with named sources | The stricter policy (section 4) |
| Editor | Plugin bundles its CodeMirror configuration; or a host-provided component | Host-provided component, no plugin JavaScript in it in v1 (section 4) |
| Commands, views, feeds, widgets | Listed by exports after activation; or declared in the manifest | Declared in the manifest; the listing exports are dropped (section 2) |
| Index feed | Plugin pushes `submit(batch)`; or host pulls after its own cursor | Pull, so the cursor moves only after a durable commit (section 2) |
| Linking capability imports | Link only granted interfaces; but optional grants come at run time | Link every requested interface; enforce grants per call; refuse unrequested imports (section 2) |
| Denied and revoked errors | `capability-denied` and `capability-revoked` against `plugin-error.denied` | `plugin-error` gains `revoked` beside `denied` (section 2) |
| Widened update | Pause the plugin; or keep the old version active | Keep the old version active, new one held (section 3) |
| Manifest file name | `syrinx-plugin.toml` against `plugin.toml` | `syrinx-plugin.toml` (section 3) |
| Cache loading | `deserialize_file`; or `deserialize` after a hash check | `deserialize` after a hash check; key includes Syrinx's version (section 5) |
| View messages | `views.message(view, json)`; or `handle-ui-message(view, request-id, method, body)` | One channel with the richer request record (sections 2 and 4) |
| First workload | Fledge's "New lead" (written before 2026-10-20) | Fledge is on hold; #29 proves forms with a synthetic plugin, and Notes is the first real plugin |

## Recommendation

Syrinx should embed wasmtime, pinned exactly on the 48.x long-term-support line with Cranelift, the component model and WASI 0.2, and with Winch, the pooling allocator, `component-model-async` and WASI 0.3, and `wasi-http` all off (wasmtime's plain `async` feature is compiled in because `wasmtime-wasi` requires it, but Syrinx runs synchronously). It is the only runtime that meets the owner's WIT decision with a support policy Syrinx can pin to, and its costs, about 17 MiB of binary and a security patch every few weeks, fit D6 as long as plugins are compiled once at install, cached in the local data folder under hash checks, and instantiated on first use. Its licence, `Apache-2.0 WITH LLVM-exception`, is a strict superset of Apache-2.0's permissions and should be approved by name, while `unicode-ident` stays at the approved 1.0.26 and the already approved `foldhash` and ICU crates carry over unchanged.

The contract between Syrinx and every plugin, the built-ins included, is one WIT package, `syrinx:plugin@0.1.0`, kept below 1.0 until Forge has used it, with the host loading the current and previous minor. Everything Syrinx reads or draws crosses as typed records; only view messages and a plugin's own settings are opaque. Identity is always a plain id; resources exist only for live host capabilities such as a terminal or a language-server connection. Functions are synchronous in 0.1.0, each plugin runs on its own worker under a memory cap and a per-call epoch deadline, a trap discards the instance and marks its writes uncertain, and the host never re-enters a plugin from inside its own import. Everything Syrinx must know before a plugin runs (commands, view kinds, feeds, widgets, capability requests) sits in `syrinx-plugin.toml`; exports only do things. Search is pulled by the host, which owns the cursor, so no plugin code runs on a keystroke and D48's budget cannot be broken by a plugin.

Plugins get nothing by default. Each capability is typed, scoped and explained in one sentence; the owner reviews them at install, grants optional ones at first use, chooses every folder in a system picker, and can revoke any of them at once, with a fixed deny list that no grant overrides. The host owns every risky service once, for all plugins: a document service with safe, revision-checked saves and Windows containment, a watcher written on the existing `windows` bindings (so no CC0 exception is needed), recovery drafts in local app data, the search index, and later the parse, terminal, git and language-server services that Forge needs. Running any program is called what it is, full trust; Forge asks for it once and then asks per project whether the owner trusts that code, and third-party plugins cannot get it until signing and provenance exist.

A plugin's interface is HTML and JavaScript in a separate kind of web view, built from its own Rust type, in the same WebView2 environment as pages but a separate InPrivate profile, served by Rust from the verified package at a per-plugin `.invalid` origin with a strict policy the host sets, with no network of its own and no navigation away. It talks only to its own module, through a message bridge that Rust validates on every message. The editor Notes and Forge use is one host-provided CodeMirror component, because D28's checksum-matched save is a property Syrinx must own. Palette forms and Home widgets stay declarative and are drawn by GPUI. None of this touches a page web view, and a gate test should prove it.

Plugins arrive as folders with a signed, canonical file list in the shape of Syrinx's own update format, signed per plugin repository with a P-256 key and trusted on first use. Built-ins ship as separate packages bundled in Syrinx's own signed release; everything else is installed from a folder the owner picks; Syrinx downloads no plugin in v1; updates are owner-initiated, strictly newer and from the same signer, and an update that widens a capability waits, inactive, while the old version keeps working. Before installing, the owner or an agent runs the review checklist with provenance checked by the `gh` CLI outside Syrinx. The template's CI builds, contract-tests against a published test host, checks licences and reproducibility, and on a tag signs, attests and releases, ideally from public repositories so attestations work on any GitHub plan.

The first proofs belong in #28, on the owner's Windows machine: the instantiation, memory and compile-time figures above re-measured for a realistically sized guest; the plugin view's memory against D6; the custom-protocol URL form, the CSP with CodeMirror and the bridge's checks; and the junction and hard-link fixtures for the document service in P3-01. Notes then exercises the whole design on the owner's daily notes before Forge adds its heavier, full-trust services on the same foundations.

## Decisions needed from the owner

Ordered by what blocks #27, #28 and #29 first.

1. **Runtime and pin:** embed wasmtime with the component model, pinned `=48.0.x` (LTS to about 2028-08) with the feature set above (Syrinx calls plugins synchronously; wasmtime's `async` feature is still compiled in, because `wasmtime-wasi` requires it, so the dependency audit covers the graph with it), and allow its security patch releases within that line as routine pin bumps that pass the gate? *Recommended: yes.* Nothing else meets the WIT decision with a pinnable support policy.
2. **Licence:** approve `Apache-2.0 WITH LLVM-exception` for the wasmtime and Cranelift family (about 31 crates) at the pinned versions? *Recommended: yes.* The exception only adds permissions to Apache-2.0.
3. **Measurement and D6:** may #28 run the Windows measurements described here (runtime start-up, memory and compile time, and plugin-view memory), and do plugin views count inside D6's 10%-of-Edge budget? *Recommended: yes to the run; report plugin views separately, with the 10 page tabs still within budget.* Every number here is from Linux or unmeasured.
4. **Interface rules:** approve one `syrinx:plugin@0.1.0` package with typed WIT and two named JSON channels, plain ids for identity, resources only for live host capabilities, no WIT async in 0.1.0, static facts in the manifest, and a support window of the current and previous minor below 1.0? *Recommended: yes.* These fix what every plugin is written against.
5. **Palette forms and widgets:** confirm that "draws its interface in HTML" means plugin tabs, while palette forms stay declarative in GPUI and Home widgets start declarative and `@unstable` until #31? *Recommended: yes.* It settles #29's open question without a renderer in the palette.
6. **Plugin views:** approve a separate plugin-view type in the same environment with one InPrivate profile, a per-plugin `.invalid` origin served by wry's custom protocol, the host-set CSP, and no network from interfaces even with a network grant? *Recommended: yes.* It isolates plugins from browsing and adds no browser process.
7. **Guardrail wording:** may #27 state that the custom-protocol filter and message handler are allowed on the plugin-view type only, and remain forbidden on page web views? *Recommended: yes.* The `AGENTS.md` ban is about page web views and Google's sites, and saying so prevents a misreading.
8. **Plugin search content:** show plugin feeds as their own palette scope chip (for example "Notes", shown only while that plugin is installed) as well as under Everything? *Recommended: yes.* It matches the spec's chips; it is `design/README.md` territory, so it is the owner's.
9. **Grants model:** nothing granted by default, an install review sheet, optional grants at first use, the fixed deny list including the profile root, an update that widens a capability held inactive while the old version keeps running, and removing a plugin deletes its grants, storage, index contributions and drafts? *Recommended: yes.* It is the Chrome model the owner named, without breaking a daily tool.
10. **Full trust:** use the wording "Can run programs on your computer with your permissions", add Forge's per-project trust question with a read-only restricted mode, and refuse full-trust capabilities to third-party plugins until signing and provenance exist? *Recommended: yes.* A sandbox cannot follow a process it starts.
11. **Package and signing:** a folder with a signed canonical file list (no archive parser in Syrinx), a P-256 key per plugin repository as an Actions secret, trust on first use with announced rotation, and an unsigned developer mode off by default? *Recommended: yes.* It reuses the reviewed update-format code.
12. **Loading:** bundle the built-in packages in Syrinx's signed release (a release-workflow change), offer "Install plugin from folder…", and fetch no plugins over the network in v1? *Recommended: yes.* A fresh install has Notes, and no new network path is added.
13. **Compiled-code cache:** allow it in the local data folder under the hash rules above, rather than compiling at every activation? *Recommended: allow it.* Compiling a large guest at every activation could take seconds.
14. **Plugin repositories and template CI:** make the plugin repositories public so attestations work on any plan, and approve the `ci.yml` and `release.yml` described here before an agent writes them? *Recommended: public, and yes.* Private attestations need GitHub Enterprise Cloud, and CI changes are the owner's.
15. **Notes' editor and data:** make the editor a host-provided CodeMirror component with no plugin JavaScript inside it in v1; keep recovery drafts in local app data outside D13's snapshots, deleted once the save they protect succeeds or after at most 7 days; and treat note text in tier 2 as a rebuildable cache outside the snapshots, with no seven-day expiry because the files are the truth? *Recommended: yes to all three.* Each keeps D28's save and D13's snapshot rules intact for Notes. ⚠️ The body argues the first point (section 4) and where drafts live (section 3); the draft lifetime and the tier-2 retention rule are proposed here without a full argument, so #37 and #36 should test them before the owner settles them.

## Still unverified (⚠️, in one place)

- All performance and size figures were measured on Linux; Windows numbers come from #28. Cranelift compile time for a realistically sized guest, the commit-charge effect of 4 GiB reservations, plugin-view and InPrivate-profile memory, the custom-protocol load time for a CodeMirror bundle and the cost of copying vault bytes across the boundary are all unmeasured.
- APIs cited from 49.0.4 or later documentation that have not been re-checked on 48.x (`Config`, `Linker`, `StoreLimitsBuilder`, `Component::component_type` and `WasiCtxBuilder`'s `FsPerms` were checked on 48.0.x docs); licence strings are crates.io metadata, not `LICENSE` files.
- Interface names carrying the version in a component built on Windows (shown on Linux); the 0.x semver rule in `Linker`; host-side unstable-feature selection in `bindgen!`; `Component::component_type()` for reading versions; matching component imports to manifest requests; which WASI imports a `wasm32-wasip2` standard library needs; the WIT sketches themselves (not parsed).
- The rule that a trapped instance is unusable (conservative, not documented); `allow_tcp`'s default (`FsPerms` replaced `DirPerms` and `FilePerms` in 48.0.0, checked on docs.rs).
- Junction and symlink refusal through cap-std on this machine; hard links, `:` streams, cloud placeholders and 8.3 names in a vault.
- The plugin-view URL form from wry's rewrite; DNS behaviour for `.invalid`; CodeMirror under the CSP without `'unsafe-eval'` (its `cspNonce` facet exists); a suitable message size cap.
- `precompile_compatibility_hash`'s coverage of CPU features; reproducibility of `wasm32-wasip2` components across operating systems; the Fledge-Software organisation's GitHub plan; Zed's packaging internals.
- Whether Tree-sitter's `wasm` feature (on `wasmtime-c-api-impl ^48.0.1`) fits Syrinx's wasmtime build; whether `git status` or the chosen in-process git reader executes repository config; whether language servers can be sandboxed further.
- wasmer's Preview 2 claim, its LTS and security process; wasmi's maintainer model and fuzzing; Extism's documentation and the other prior art not re-read on 2026-10-20.

## Sources

- **Syrinx:** [`docs/decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/decisions.md) (2026-10-13, 2026-10-14, 2026-10-15, 2026-10-16, 2026-10-20); [`docs/architecture-decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/architecture-decisions.md) (D6, D12, D13, D16, D17, D23, D24, D28–D31, D34, D35, D39, D40, D48); [`AGENTS.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/AGENTS.md); `docs/dev/trapstoknow.md`; `docs/update-format.md`; `docs/update-delivery.md`; `docs/dev/integrated-preview-dependencies.md`; `docs/dev/fulltext-audit.md`; `crates/syrinx-core/src/{commands,tabs,destinations}.rs`; `crates/syrinx-search/src/types.rs`; `crates/syrinx-platform/src/browser/{environment,page}.rs`; `vendor/wry-0.57.0`; [Plan 2 at af3de53](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/plans/Syrinx%20v1%20%E2%80%94%20Plugins%20and%20Fledge%20%E2%80%94%20Plan.md); issues [#25](https://github.com/Fledge-Software/Syrinx-app/issues/25) to #31, #35 to #42, #52, #53; [note 03](Background-03-core-architecture), [note 09](Background-09-phase-0-verdict).
- **wasmtime:** [release process and LTS](https://docs.wasmtime.dev/stability-release.html); [stability tiers](https://github.com/bytecodealliance/wasmtime/blob/main/docs/stability-tiers.md); [security](https://docs.wasmtime.dev/security.html); [what counts as a vulnerability](https://docs.wasmtime.dev/security-what-is-considered-a-security-vulnerability.html); [fuzzing](https://docs.wasmtime.dev/contributing-fuzzing.html); [SECURITY.md](https://github.com/bytecodealliance/wasmtime/blob/main/SECURITY.md); [Bytecode Alliance security policy](https://bytecodealliance.org/security); [April 2026 advisories](https://bytecodealliance.org/articles/wasmtime-security-advisories); [1.0 performance](https://bytecodealliance.org/articles/wasmtime-10-performance); [interrupting Wasm](https://docs.wasmtime.dev/examples-interrupting-wasm.html); [`Module`](https://docs.wasmtime.dev/api/wasmtime/struct.Module.html); [`Component`](https://docs.wasmtime.dev/api/wasmtime/component/struct.Component.html); [`Engine`](https://docs.wasmtime.dev/api/wasmtime/struct.Engine.html); [`Config`](https://docs.wasmtime.dev/api/wasmtime/struct.Config.html) and [`Config` 49.0.4](https://docs.rs/wasmtime/49.0.4/wasmtime/struct.Config.html); [`StoreLimitsBuilder`](https://docs.wasmtime.dev/api/wasmtime/struct.StoreLimitsBuilder.html); [`bindgen!`](https://docs.rs/wasmtime/49.0.4/wasmtime/component/macro.bindgen.html); [`Linker`](https://docs.rs/wasmtime/49.0.4/wasmtime/component/struct.Linker.html); [`WasiCtxBuilder`](https://docs.rs/wasmtime-wasi/latest/wasmtime_wasi/struct.WasiCtxBuilder.html), [`FsPerms`](https://docs.rs/wasmtime-wasi/latest/wasmtime_wasi/filesystem/enum.FsPerms.html), [36.0.0 `WasiCtxBuilder`](https://docs.rs/wasmtime-wasi/36.0.0/wasmtime_wasi/struct.WasiCtxBuilder.html); [cap-std](https://github.com/bytecodealliance/cap-std).
- **Advisory data:** [RustSec advisory-db](https://github.com/rustsec/advisory-db); [OSV](https://api.osv.dev); the individual RUSTSEC, GHSA and CVE links above.
- **wasmer and wasmi:** [wasmer](https://github.com/wasmerio/wasmer), [its advisories](https://github.com/wasmerio/wasmer/security/advisories), [#4439](https://github.com/wasmerio/wasmer/issues/4439); [wasmi](https://github.com/wasmi-labs/wasmi), [its advisories](https://github.com/wasmi-labs/wasmi/security/advisories), [#897](https://github.com/wasmi-labs/wasmi/issues/897), [SECURITY.md](https://github.com/wasmi-labs/wasmi/blob/main/SECURITY.md).
- **Standards:** [`WIT.md`](https://github.com/WebAssembly/component-model/blob/main/design/mvp/WIT.md); [`Explainer.md`](https://github.com/WebAssembly/component-model/blob/main/design/mvp/Explainer.md); [`Concurrency.md`](https://github.com/WebAssembly/component-model/blob/main/design/mvp/Concurrency.md); [`CanonicalABI.md`](https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md); [component-model README](https://github.com/WebAssembly/component-model/blob/main/README.md); [WASI README](https://github.com/WebAssembly/WASI/blob/main/README.md); [WASI roadmap](https://wasi.dev/roadmap); [wasi-filesystem path resolution](https://github.com/WebAssembly/wasi-filesystem/blob/main/path-resolution.md) and [WIT](https://github.com/WebAssembly/wasi-filesystem/tree/main/wit); [rustc `wasm32-wasip2`](https://doc.rust-lang.org/rustc/platform-support/wasm32-wasip2.html); [RFC 6761](https://www.rfc-editor.org/rfc/rfc6761#section-6.4); [MDN CSP](https://developer.mozilla.org/docs/Web/HTTP/Headers/Content-Security-Policy).
- **Licences:** [LLVM licence](https://llvm.org/LICENSE.txt); [SPDX LLVM-exception](https://spdx.org/licenses/LLVM-exception.html); crates.io metadata as of 2026-10-20 (wasmtime and wasmtime-wasi 49.0.4, cranelift-codegen, wit-bindgen 0.63.0, wit-parser, wasmer 7.5.0, wasmi 2.0.0, extism 1.41.0, notify 8.2.0 CC0-1.0, portable-pty 0.9.0 MIT, git2 0.21.0, libgit2-sys 0.18.8 and gix 0.89.0 MIT OR Apache-2.0, tree-sitter 0.27.1 MIT, cap-primitives 4.0.3).
- **WebView2 and wry:** [local content](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/working-with-local-content); [interop of native and web code](https://learn.microsoft.com/en-us/microsoft-edge/webview2/how-to/communicate-btwn-web-native); [security](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/security); [custom scheme registration](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2customschemeregistration); [multiple profiles](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/multi-profile-support); [process model](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model); [`AddHostObjectToScript`](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2#addhostobjecttoscript); [`CreatePseudoConsole`](https://learn.microsoft.com/en-us/windows/console/createpseudoconsole); [wry 0.57.0 `WebViewBuilder`](https://docs.rs/wry/0.57.0/wry/struct.WebViewBuilder.html); [CodeMirror `cspNonce`](https://codemirror.net/docs/ref/#view.EditorView^cspNonce).
- **Prior art:** Zed ([`Cargo.toml`](https://github.com/zed-industries/zed/blob/main/Cargo.toml), [extension API](https://github.com/zed-industries/zed/tree/main/crates/extension_api), [extension host, GPL, design only](https://github.com/zed-industries/zed/tree/main/crates/extension_host), [capabilities](https://zed.dev/docs/extensions/capabilities), [developing](https://zed.dev/docs/extensions/developing-extensions), [publishing](https://zed.dev/docs/extensions/publishing/publishing-guide), [updating](https://zed.dev/docs/extensions/publishing/updating-and-maintenance), [licence requirements](https://zed.dev/docs/extensions/publishing/license-requirements), [registry](https://github.com/zed-industries/extensions) and its [CI](https://github.com/zed-industries/extensions/blob/main/.github/workflows/ci.yml)); VS Code ([manifest](https://code.visualstudio.com/api/references/extension-manifest), [Workspace Trust](https://code.visualstudio.com/api/extension-guides/workspace-trust), [webviews](https://code.visualstudio.com/api/extension-guides/webview), [runtime security](https://code.visualstudio.com/docs/configure/extensions/extension-runtime-security), [publishing](https://code.visualstudio.com/api/working-with-extensions/publishing-extension)); Obsidian ([manifest](https://docs.obsidian.md/Reference/Manifest), [plugin security](https://obsidian.md/help/plugin-security), [releases](https://github.com/obsidianmd/obsidian-releases), [submission](https://docs.obsidian.md/Plugins/Releasing/Submit+your+plugin)); Chrome ([permission warnings](https://developer.chrome.com/docs/extensions/develop/concepts/permission-warnings), [permissions API](https://developer.chrome.com/docs/extensions/reference/api/permissions)); Tauri ([capabilities](https://v2.tauri.app/security/capabilities/), [scopes](https://v2.tauri.app/security/scope/), [isolation](https://v2.tauri.app/concept/inter-process-communication/isolation/), [updater](https://v2.tauri.app/plugin/updater/)); [Extism](https://extism.org/docs/concepts/host-functions); [minisign](https://jedisct1.github.io/minisign/); [node-ovsx-sign](https://github.com/filiptronicek/node-ovsx-sign); [OX Security](https://www.ox.security/blog/can-you-trust-that-verified-symbol-exploiting-ide-extensions-is-easier-than-it-should-be/).
- **Supply chain:** GitHub [artifact attestations](https://docs.github.com/en/actions/concepts/security/artifact-attestations), [using them](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations), [verifying offline](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/verify-attestations-offline), [attest-build-provenance](https://github.com/actions/attest-build-provenance); [cosign verify](https://docs.sigstore.dev/cosign/verifying/verify/); [reproducible-builds.org, Rust](https://reproducible-builds.org/docs/rust/); [Internet Computer reproducible builds](https://docs.internetcomputer.org/guides/canister-management/reproducible-builds.md); [Yocto discussion](https://patchwork.yoctoproject.org/comment/13986); [Tock #1666](https://github.com/tock/tock/issues/1666).
