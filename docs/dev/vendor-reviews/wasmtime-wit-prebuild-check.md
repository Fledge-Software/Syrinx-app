# wasmtime 48 and the draft WIT: the pre-build check for #27

Syrinx's plugins are WebAssembly components (owner decision of 2026-10-15),
run by wasmtime pinned on its 48.x long-term-support line (decision 1 of
2026-10-21) against one typed interface, the `syrinx:plugin` WIT package.
Before #28 builds the host crate, the owner wanted three things known rather
than assumed: what the wasmtime dependency brings into Syrinx's graph, that
the draft interface actually parses and loads on Windows, and that the two
sandbox tools the design leans on, the epoch deadline and the memory cap, do
what research note 10 says they do. Decision 2 of 2026-10-24 folded these
"Plugin runtime 0" checks into #27 and allowed them before Plan 1's exit on
one condition: throwaway builds outside the repository only, nothing under
`crates/`, nothing built in this repository. This report records what was
checked on 2026-10-24, how, what came out, and what #28 must do differently.
The dependency audit itself is `docs/dev/wasmtime-audit.md`.

**Terms.** A *component* is a WASM binary in the component-model format,
exchanging typed values. A *world* lists what a component imports (what Syrinx
offers) and exports (what the plugin offers). `wasmtime::component::bindgen!`
generates the host side of a world from the `.wit` files; `wit_bindgen::generate!`
generates the guest side. A *store* holds one instance's state; a *trap* is an
unrecoverable stop (a deadline, an out-of-bounds access, a Rust abort).
`@since(version = x)` marks an addition that stays compatible from that version
on; `@unstable(feature = f)` marks an item hidden unless the feature is on.

## What was checked, and how

Everything ran in the session's scratchpad folder, outside the repository, on
this Windows 11 machine, with the `1.97.0` toolchain the plugin template pins
(`syrinx-plugin-template/rust-toolchain.toml`), which already had the
`wasm32-wasip2` target installed on this machine; nothing was installed. The throwaway had three parts:

- **A WIT probe**, a twenty-line program over `wit-parser 0.262.0` (the parser
  wasmtime and `wasm-tools` share; the `wasm-tools` command is not installed
  here) that parses a WIT folder twice, once as a guest build does (no
  unstable features) and once as wasmtime 48's `bindgen!` does
  (`all_features = true`, `wasmtime-internal-component-macro-48.0.7/src/bindgen.rs`,
  line 199), and prints the `plugin` world's imports and exports with their
  stability. It made each WIT iteration a two-second check instead of a
  wasmtime compile.
- **A host crate** declaring exactly the pin #28 will add, `wasmtime =48.0.7`
  with `default-features = false, features = ["runtime", "cranelift",
  "component-model", "std", "parallel-compilation", "anyhow"]` and
  `wasmtime-wasi =48.0.7` with `["p2"]`, and nothing else, so its lockfile is
  the audited closure. It holds three `bindgen!` modules over three WIT
  variants: **0.1.0** as the template ships it (`syrinx-plugin-template/wit/plugin.wit`,
  with the template's three findings applied: `events.deliver`'s parameter is
  `outcome`, every item of `widgets` is gated, and `widgets` is not exported);
  **0.1.1**, the same package with Notes' drafted `documents` and `watch`
  interfaces added as `@since(version = 0.1.1)` imports; and **0.1.1 plus one
  since-gated export**, `watch-sink`, kept only to show what an added export
  does to 0.1.0 plugins. The engine is configured as #28 will configure it:
  Cranelift, `epoch_interruption(true)`, the default allocator, no on-disk
  cache; WASI 0.2 linked with an empty context through
  `wasmtime_wasi::p2::add_to_linker_sync`; `StoreLimitsBuilder::memory_size`
  at 8 MiB with 16 instances, 4 memories and 8 tables; a thread advancing the
  epoch every 10 ms and a deadline of 50 ticks, half a second, set before every
  export call.
- **Three guests** built with `wit-bindgen 0.63.0` for `wasm32-wasip2`
  (`opt-level = "s"`, LTO, `panic = "abort"`, as the template builds): a *full*
  0.1.0 plugin implementing every export, with the commands `test.echo`,
  `test.fail`, `test.spin` (an endless loop), `test.grow` (grows memory a page
  at a time and reports when the host refuses) and `test.alloc` (keeps
  allocating 1 MiB `Vec`s, what a real plugin does by accident); a *minimal*
  0.1.0 plugin implementing only `lifecycle` and one command, with default
  stubs for `forms`, `views`, `search-feed` and `events`; and a *0.1.1* plugin
  that calls `documents` and `watch` from one command. They are 66 KB, 61 KB and
  75 KB.

Eleven tests drove the host against the guests; all eleven pass. The
dependency audit used the same crate's lockfile, resolved twice (fresh, and
seeded from Syrinx's own `Cargo.lock`), the extracted archives for their
licence files, a clone of the RustSec database at commit `21d909e4` of
2026-10-24, and crates.io for maintenance; its method is in
`docs/dev/wasmtime-audit.md`. The throwaway's build folders were deleted
afterwards.

One thing cost time and is worth a sentence so nobody mistakes it for an
Application Control block: the first host build failed to link wasmtime's
build scripts with `LNK1104: cannot open file`. The failing output path was
exactly 260 characters, Windows' `MAX_PATH`, because the scratchpad sits six
folders deep. A profile named `x` and a target folder named `t` brought the
longest build-script path (`wasmtime-internal-component-macro`'s) to 247 and
the build went through. The repository's own paths are short; this is a
scratchpad problem only.

## Results against the issue's list

| #27's check | Result | Recorded in |
|---|---|---|
| Resolve the graph for `x86_64-pc-windows-msvc`, record `async` and the features kept off | Done: 197 third-party crates in the lockfile, 170 compiled on Windows; `async` is on because `wasmtime-wasi-io`'s `std` feature requires it; Winch, the pooling allocator, `component-model-async`, WASI 0.3 and `wasi-http` are absent from the graph | `wasmtime-audit.md`, "The pin" |
| Audit every transitive crate, licence read from the archive | Done: every crate is MIT, Apache-2.0, BSD-3-Clause or an existing narrow approval; 32 crates under `Apache-2.0 WITH LLVM-exception` alone, all inside the family the owner approved; fourteen of them ship no licence file and were checked against the `bytecodealliance/wasmtime` repository's `LICENSE` at tag `v48.0.7` | `wasmtime-audit.md`, "Licences" |
| The narrow approvals still hold | Yes, in the seeded resolution: `unicode-ident 1.0.26`, `foldhash 0.2.0`, the eighteen Unicode-3.0 crates at Syrinx's versions. A fresh resolution would move `unicode-ident` to 1.0.27 and the ICU crates to 2.4.0, so #28 adds the pin without a blanket `cargo update` | `wasmtime-audit.md`, "Fresh against seeded" |
| Advisories at the pinned patch, and the cadence | 116 matches, one applicable, and it is Syrinx's own `tokio 1.53.2` (RUSTSEC-2026-0386, Unix-only functions), which `wasmtime-wasi` would compile for the first time; wasmtime's 64 and `wasmtime-wasi`'s 9 advisories are all patched or unaffected at 48.0.7. Seven 48.0.x patches in two months, each a routine pin bump under decision 1 of 2026-10-21 | `wasmtime-audit.md`, "Advisories" |
| Audit the guest side | Done: `wit-bindgen 0.63.0`, MIT elected from its triple licence, 32 crates, no applicable advisory; `wasm-tools` not used | `wasmtime-audit.md`, "Guest side" |
| Check the WIT draft with the template's findings applied; answer the Windows points; confirm one world with default stubs | All three: see the next two sections | below |
| Review the paper shapes of Notes' capability interfaces | `documents` and `watch` reviewed and run; two names must change before they parse; `drafts`, `editor` and `open-external` have no paper shapes yet | "Notes' paper shapes" |
| One end-to-end run with a trap on an expired deadline and a refused growth | Done, with one addition: a real allocation past the cap traps rather than failing typed | "The end-to-end run" |
| The owner confirms the exact 48.0.x patch | Open: 48.0.7 is recommended | "Open questions" |

## The WIT draft on Windows

**It parses and loads.** The 0.1.0 package as the template ships it parses
with and without features, `bindgen!` generates the host for it, and both
0.1.0 guests instantiate and answer every export that was called. With
`all_features` on, `host` has ten functions (the gated `widget-changed`
included); without, nine. No export of `widgets` appears in either reading,
which is the template's finding 3 holding on Windows.

**Interface names carry the version, and `Component::component_type()`
exposes them before instantiation.** The full guest reports the imports
`syrinx:plugin/types@0.1.0` and `syrinx:plugin/host@0.1.0` and the exports
`syrinx:plugin/lifecycle@0.1.0`, `commands@0.1.0`, `forms@0.1.0`,
`views@0.1.0`, `search-feed@0.1.0` and `events@0.1.0`; the 0.1.1 guest
reports the same names at `@0.1.1` plus `documents@0.1.1` and `watch@0.1.1`
among its imports. A host can therefore read a component's track from its
names, as note 10's scheme assumed (⚠️ now resolved).

**Which WASI 0.2 imports Rust's standard library brings**, for
`wasm32-wasip2` on `rustc 1.97.0`: fourteen interfaces, all at `0.2.9`, the
same fourteen the template found on Linux: `wasi:io/poll`, `wasi:io/error`,
`wasi:io/streams`, `wasi:clocks/monotonic-clock`, and `wasi:cli/stdout`,
`stderr`, `stdin`, `environment`, `exit`, `terminal-input`,
`terminal-output`, `terminal-stdin`, `terminal-stdout` and
`terminal-stderr`. No filesystem, socket, HTTP or random interface. The
template's test host also allows `wasi:random`; nothing imported it here.

**One world with default guest stubs works** (decision 5 of 2026-10-24). The
minimal guest, whose `forms`, `views`, `search-feed` and `events` are stubs
returning `not-found` or nothing, instantiates on the 0.1.0 host, runs its
one command, and answers `forms.form-for` and `search-feed.changes` with
`plugin-error.not-found`. A plugin therefore implements only what its
manifest declares, provided the template ships those defaults.

**Gated imports work both ways.** With `LinkOptions::home_widgets(false)`
the host does not link `widget-changed`; with it on, the host links one more
function than the guest imports, and the guest still loads, because an
instance may be given more than it asks for.

## Notes' paper shapes

Decision 6 of 2026-10-24 adds `documents`, `watch`, `drafts`, `editor` and
`open-external` to the package with `@since(0.1.x)` in the Notes slices,
after their paper shapes are reviewed here. Notes 1 drafted two of them, in
`Syrinx-Notes-Plugin/docs/host-contract-gaps.md` sections 1 to 3; the other
three are explicitly "not in Notes 1" (section 11), so there is nothing to
review yet and they go through the same probe when #37 drafts them.

Both drafted shapes were added to the 0.1.1 package verbatim first. Two
things stopped the parser, both names:

1. **`documents.list` is a WIT keyword.** `list: func() -> result<list<entry>, …>`
   fails with "expected `type`, `resource` or `func`, found keyword `list`",
   the same class of fault as the draft's `result` parameter that the template
   renamed to `outcome`. The throwaway calls it `list-entries`; the Notes
   repository chooses the final name (its `DocumentsApi::list` trait method
   mirrors it).
2. **`vault-state` names both a type and a function.** WIT keeps one namespace
   per interface, so `variant vault-state` and `vault-state: func()` are a
   "duplicate name". The throwaway renames the function `current-vault`.

With those two renames both interfaces parse, the gates read as
`Stable { since: 0.1.1 }`, and the 0.1.1 guest used them end to end: it
called `current-vault`, `list-entries`, `read` on the first entry and
`watch.poll(16)`, and logged `vault=Granted("Synthetic vault") entries=2
first_bytes=8 events=3 renames=1` against two synthetic documents and three
scripted events (a change, a rename and `overflow-rescan`). The variant
`event` with its `renamed(tuple<document-id, document-id>)` case, the `use
documents.{document-id}` across two since-gated interfaces, and the
`choose-vault` function all generate and link as drafted. The world's
`import documents;` and `import watch;` lines carry the gate as well as the
interfaces; that form is what was tested.

For the WIT package's documentation, the identifiers wit-parser 0.262.0
treats as keywords, which no interface, type, field or function may use
without `%` escaping: `as`, `async`, `bool`, `borrow`, `char`, `constructor`,
`enum`, `error-context`, `export`, `f32`, `f64`, `flags`, `from`, `func`,
`future`, `import`, `include`, `interface`, `list`, `map`, `option`, `own`,
`package`, `record`, `resource`, `result`, `s8`, `s16`, `s32`, `s64`, `static`,
`stream`, `string`, `tuple`, `type`, `u8`, `u16`, `u32`, `u64`, `use`,
`variant`, `with`, `world`.

Two observations for #36, not parse faults: `list-entries` returns the whole
vault in one call (the signature has no `max` or cursor), while #98 row 4
already asks for per-call budgets; a bounded shape would fit that better. And
bindgen turns the `document-id` and `command-id` aliases into `String`, so the
host's generated call signatures take `&String` where a `&str` would be
expected; harmless, but visible in #28's code.

## The version rules, observed

Note 10's table "What breaks a plugin" was inferred from the specifications;
three of its rows are now observed on Windows under wasmtime 48.0.7:

- **An import added with `@since(0.1.1)` is a compatible addition.** The
  0.1.0 full guest instantiates on the 0.1.1 host, whose linker defines
  `host@0.1.1`, `documents@0.1.1` and `watch@0.1.1`, and runs its commands and
  form. wasmtime's `Linker` matched the guest's `host@0.1.0` import to the
  host's `@0.1.1` definition, so semver-compatible naming works across a
  minor below 1.0, which the documentation left unstated (⚠️ resolved).
- **A plugin built for 0.1.1 needs a 0.1.1 host.** On the 0.1.0 host it is
  refused at instantiation with a usable message: "component imports instance
  `syrinx:plugin/documents@0.1.1`, but a matching implementation was not found
  in the linker: instance export `current-vault` has the wrong type: function
  implementation is missing".
- **An export added in a minor breaks every older plugin, `@since` or not.**
  With `watch-sink` exported `@since(version = 0.1.1)`, the 0.1.0 guest fails
  with "no exported instance named `syrinx:plugin/watch-sink@0.1.1`", exactly
  the template's finding 3 for `@unstable`, because `bindgen!` resolves every
  gate and `instantiate` requires every export it knows. **So within one
  bindgen module, 0.1.x may only add imports.** A new export, Home widgets
  included, needs a new minor track with its own `bindgen!` module (note 10's
  scheme, item 4) or a separate world; the template's wording that widgets can
  "return later as an `@since` addition" holds only if that addition is an
  import, and #31 should plan for a track, not a gate. Notes' five
  capabilities are all imports, so decision 6 is unaffected.

## The end-to-end run

- **An expired deadline traps.** `test.spin` trapped after 504.8 ms against a
  500 ms budget, with `Trap::Interrupt` ("wasm trap: interrupt"), under a
  ticker advancing the epoch every 10 ms. A further call on the same store
  then failed with "wasm trap: cannot enter component instance": **wasmtime
  itself refuses to re-enter a trapped component instance**, so note 10's
  rule that a trapped store is discarded is not merely conservative, it is
  the only thing that works. The real host must therefore keep its
  re-instantiation path cheap (the compiled-code cache of #137 is what makes
  it so) and mark in-flight writes `uncertain` as planned.
- **The memory cap refuses growth.** With `StoreLimitsBuilder::memory_size`
  at 8 MiB, `test.grow` started at 17 pages (1.06 MiB, Rust's initial memory)
  and was refused after 111 more, at exactly 128 pages; `memory.grow`
  returned -1 to the guest, the guest reported it as a typed
  `plugin-error.unavailable`, and the instance stayed usable (`test.echo`
  ran afterwards). That is the shape when a guest checks the result itself.
- **A real allocation past the cap traps.** `test.alloc`, which pushes 1 MiB
  `Vec`s, ended in `Trap::UnreachableCodeReached` ("wasm `unreachable`
  instruction executed"): Rust's allocator calls `handle_alloc_error`, which
  aborts under `panic = "abort"`. This is how an ordinary plugin meets the
  cap, so #28's "`test.grow` refused by the memory cap" test should expect a
  trap for ordinary code and a typed error only for a guest that probes
  `memory.grow` itself; after the trap the store is unusable, as above.

Nothing here is a measurement: the eleven tests, compiling three components
with Cranelift included, ran in 1.7 s on this machine, but #28's Windows
measurements (decision 3) are the figures to design from.

## What #28 must do differently, or write down

1. **Move `tokio` to a patched version in the same change** (a fresh
   resolution picks 1.54.1). `wasmtime-wasi` makes Syrinx compile tokio for
   the first time, and the version Syrinx's lockfile holds, 1.53.2, is in
   RUSTSEC-2026-0386's range. The affected functions are Unix-only, so Windows
   never compiles them, but the first build that compiles tokio should
   compile a clean one. This is a one-crate lockfile move, listed here for the
   owner's yes.
2. **Add the pin without `cargo update`.** The seeded resolution keeps every
   existing version; a fresh one moves `unicode-ident` to 1.0.27 and the
   Unicode-3.0 ICU crates to 2.4.0, both outside their exact approvals. Expect
   48 new crate names and two new versions (`gimli 0.33.0`, `object 0.39.1`)
   in the Windows build, and add the 32 `Apache-2.0 WITH LLVM-exception`
   crates to `xtask`'s dependency policy by exact name and version, as the
   template's `licences.toml` does.
3. **Write the addition rule into the WIT package:** within a minor, add
   imports only, each `@since(version = 0.1.x)`, gated on the interface and on
   the world's `import` line; a new export opens a new minor track with its
   own `bindgen!` module. Carry the correction to #31 (widgets).
4. **Design for "a trapped store cannot be re-entered".** Both the deadline
   and the memory cap end in a trap for ordinary code; the host discards the
   store, marks writes `uncertain`, and re-instantiates on next use.
5. **Make the memory-cap contract test two tests**: a `memory.grow` probe
   (typed error, instance survives) and an allocation (trap, instance
   discarded).
6. **Parse the WIT in the gate with `wit-parser 0.254.2`**, the version
   wasmtime 48 already pulls, so the check adds no crate; 0.262.0 was used
   here only because it was already cached. Ship the keyword list above with
   the package so Notes and Forge authors do not rediscover `list` and
   `result`.
7. **Rename before freezing:** `documents.list` and the `vault-state`
   function cannot stay as drafted; the names are the Notes repository's to
   choose (#36), but the host half must use the same ones.

## Open questions for the owner

1. **Confirm `wasmtime =48.0.7` and `wasmtime-wasi =48.0.7` as the pin**
   (#27's last box). It is the latest 48.x patch today; a newer 48.0.x on the
   day #28 lands is a routine bump under decision 1 of 2026-10-21.
2. **Approve moving `tokio` from 1.53.2 to a patched version** (1.54.1) with
   #28's lockfile change, as item 1 above argues.
3. **Where the audit file is committed.** #27's text says "draft it as
   `docs/dev/wasmtime-audit.md`; #28 commits it"; this pull request commits
   the draft now so #28 starts from a reviewed file and only updates it
   against the real lockfile. Keep it so, or hold it for #28?
4. **The names of `documents.list` and the `vault-state` function.** The
   throwaway used `list-entries` and `current-vault`; the Notes repository's
   trait and tests mirror whatever is chosen, so settling it now saves #36 a
   round trip.
5. **State the "imports only within a minor" rule as a contract rule.** It
   narrows decision 6's `@since(0.1.x)` to imports, which Notes' five
   capabilities already are, and it changes how #31 brings widgets back.
