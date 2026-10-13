# Integrated browser preview: dependency review

The owner approved an integrated browser preview out of the normal slice order so Atlas can be used and assessed as a window. This report makes the dependency additions reviewable before they enter the product. It does not mark P1-05's search performance or the full workspace gate complete.

Status on 2026-10-10: the isolated Windows graph resolves offline from the existing Cargo cache. The licence, maintenance, RustSec and cached-source checks below are complete for this proposed graph. The owner explicitly approved the exact licence exceptions listed below on 2026-10-10. No product manifest or lockfile was changed by this audit, and no dependency was downloaded or built.

## Pins and resolved features

The proposed direct pins are `gpui-pre =0.3.8`, `gpui-pre-platform =0.3.8`, `gpui-component =0.7.1`, `wry =0.57.0`, `webview2-com =0.39.1`, `raw-window-handle =0.6.2` and `futures =0.3.34`. Address parsing can directly use `url =2.5.8` (MIT OR Apache-2.0), already present in this graph. Every resolved `gpui-pre-*` package remains at snapshot `0.3.8`. These preserve the engine and interface pins used by the Phase 0 spike; they do not upgrade that stack. Published GPUI metadata identifies Zed source commit `279fe070bb389b79652e52065b2f001edcc0b11b`.

The isolated manifest at `target/integrated-preview-audit/Cargo.toml` disables default features on GPUI, its platform crate, the component crate and wry. However, `gpui-component 0.7.1` enables GPUI's defaults through its own dependency: the effective `gpui-pre` features include `font-kit`, `windows-manifest`, `wayland` and `x11`. Cargo's Windows target filter excludes the Linux/macOS implementation dependencies; feature names alone are not evidence that their code is compiled on Windows. The component's optional speech, tree-sitter and inspector features are not selected.

`cargo metadata --offline --filter-platform x86_64-pc-windows-msvc` succeeds. Traversing normal and build dependencies yields 522 packages including local Atlas/audit packages. The isolated lock includes inactive targets too, so its larger package count must not be presented as the Windows compiled graph. `target/integrated-preview-audit/active.json` records each reachable package's exact version, licence, selected features and source path. The isolated manifest includes the current storage/search crates and the approved local SQLite patch. Every existing product registry name/version is retained in the proposed resolution. The eventual product lock diff still needs a separate check before addition.

All 516 registry archive SHA256 values match their cached crates.io index records, and none is marked yanked in that cache. The audit also compared 2,450 cached manifests, published commit records, build scripts and licence/notice files against those archives: no differences. This is a cached-index integrity check, not a fresh online index fetch. `archive-checks.json` and `source-checks.json` under the isolated audit directory record the results. The proposed isolated lock SHA256 is `ffb02ca2f68416e2898454463c98d41ecd6e71016509d38b30d04844432808b3`; it identifies this proposal, not a later product lock.

## Approved licence scope

The repository normally permits MIT, Apache-2.0, BSD and whole MPL-2.0 files. The owner approved the following exact additional scope for this integrated preview. It does not approve arbitrary packages using those licences later.

| Licence | Exact packages or bundled asset scope |
| --- | --- |
| Unicode-3.0 | `icu_collections 2.3.0`, `icu_locale_core 2.3.0`, `icu_normalizer 2.3.0`, `icu_normalizer_data 2.3.0`, `icu_properties 2.3.0`, `icu_properties_data 2.3.0`, `icu_provider 2.3.1`, `litemap 0.8.3`, `potential_utf 0.1.6`, `tinystr 0.8.4`, `writeable 0.6.4`, `yoke 0.8.3`, `yoke-derive 0.8.4`, `zerofrom 0.1.8`, `zerofrom-derive 0.1.8`, `zerotrie 0.2.5`, `zerovec 0.11.8`, `zerovec-derive 0.11.6` |
| Zlib | `slotmap 1.1.1`, `zlib-rs 0.6.8` |
| CC0-1.0 | `notify 8.2.0` |
| bzip2-1.0.6 | `libbz2-rs-sys 0.2.5` |
| ISC | Lucide SVG assets bundled in `gpui-kit-assets 0.7.1`; preserve `LICENSE-LUCIDE`, including the separate MIT notice for Feather-derived icons. The crate's Apache-2.0 metadata does not replace these asset terms. |
| 0BSD | `enum-iterator 2.3.0`, `enum-iterator-derive 1.5.0`; explicitly recorded although these are permissive BSD-family licences. |

The already approved `unicode-ident 1.0.26` Unicode-3.0 component and `foldhash 0.2.0` Zlib dependency retain their versions. SQLite remains public domain with its existing approved wrapper/vendor terms; this report does not relabel it MIT. `option-ext 0.2.0` is a whole MPL-2.0 crate, within the existing rule. Where a package offers an allowed alternative, use that alternative: for example, `raw-window-handle` offers MIT or Apache-2.0 as well as Zlib, so it needs no new Zlib exception. No GPL package was found in the reachable Windows normal/build graph. No bundled font files were found in the component/base/assets packages inspected.

The native WebView2 loader is separate from the Rust bindings' MIT licence. The pinned bindings' [updater source](https://github.com/wravery/webview2-rs/blob/edc2caf886175ccaebe86078c9cfe1ae2a187328/crates/update-bindings/src/main.rs) records Microsoft SDK `1.0.3800.47`; its [official package licence](https://www.nuget.org/packages/Microsoft.Web.WebView2/1.0.3800.47/License) has BSD-3-Clause terms. Preserve Microsoft's notice with redistributed binaries. That fits the existing BSD rule and does not require a new exception. The approved system Evergreen runtime remains a distinct installed Microsoft component.

## Maintenance and security evidence

The [GPUI Kit release history](https://github.com/longbridge/gpui-kit/releases) records `0.7.1` on 2026-10-05 and its deliberate GPUI `0.3.8` pin. The published component source records commit `87d10ae5e1299d1be18670a37570c5402f952e34`. These are snapshot releases: exact pins remain essential, and a later release is not permission to upgrade.

The upstream [wry release record](https://github.com/tauri-apps/wry/releases/tag/wry-v0.57.0), [WebView2 bindings source](https://github.com/wravery/webview2-rs/tree/edc2caf886175ccaebe86078c9cfe1ae2a187328), [futures release history](https://github.com/rust-lang/futures-rs/releases) and [raw-window-handle release history](https://github.com/rust-windowing/raw-window-handle/releases) support the chosen published versions. The bindings use Windows `0.62`; older Windows binding versions also remain transitively required by other crates. They coexist and must not be treated as interchangeable Rust handle types.

The active graph was cross-checked against the current [RustSec package index](https://rustsec.org/packages/) and the applicable packages' advisory version ranges. No applicable vulnerability was found in the selected releases. Four maintenance notices remain: [`instant 0.1.13`](https://rustsec.org/advisories/RUSTSEC-2024-0384.html), [`paste 1.0.15`](https://rustsec.org/advisories/RUSTSEC-2024-0436.html), [`rustybuzz 0.20.1`](https://rustsec.org/advisories/RUSTSEC-2026-0206.html) and [`ttf-parser 0.25.1`](https://rustsec.org/advisories/RUSTSEC-2026-0192.html). These are inherited risks, including font parsing/shaping dependencies, to record and monitor; this audit does not silently replace or patch them. This review used primary advisory pages, not a claim that `cargo audit` ran or that unknown vulnerabilities cannot exist.

Several recent advisories explicitly exclude the resolved versions: [`bytes 1.12.1`](https://rustsec.org/advisories/RUSTSEC-2026-0007.html), [`crossbeam-epoch 0.9.21`](https://rustsec.org/advisories/RUSTSEC-2026-0204.html), [`event-listener 5.4.2`](https://rustsec.org/advisories/RUSTSEC-2026-0221.html), [`memmap2 0.9.11`](https://rustsec.org/advisories/RUSTSEC-2026-0186), [`anyhow 1.0.104`](https://rustsec.org/advisories/RUSTSEC-2026-0190), and [`time 0.3.55`](https://rustsec.org/advisories/RUSTSEC-2026-0009.html). The malicious [`arrayref 0.3.10`](https://rustsec.org/advisories/RUSTSEC-2026-0260.html) is absent; the selected `0.3.9` is in that advisory's unaffected range. This is evidence about these exact releases, not a claim that all future vulnerabilities are excluded.

## Product graph comparison

After approval, the real workspace resolution retained every package name
and version in the previous committed lockfile. The filtered product metadata
contains no registry package outside the reviewed Windows graph, and all
resolved GPUI snapshot packages remain exactly `0.3.8`. The additional
workspace member in the traversal is local tooling, not a new registry
dependency. The complete lockfile also records inactive-platform packages;
its size is not the number of libraries compiled on Windows.

## Build-script boundary and next checks

No build script ran during metadata resolution. The graph contains 57 custom build targets. Focused source review found compiler/version probes, generated tables, local native compilation and bundled resource copying, without a network fetch in the selected build paths. The component/assets scripts generate Rust tables from bundled icons. `gpui-pre` embeds its Windows manifest with the resource compiler; `vswhom-sys` compiles a bundled Visual Studio discovery helper. `webview2-com-sys` copies the bundled loader variants into its output directory and selects the target architecture; the upstream SDK-updater utility is not part of this dependency graph and is not run during a normal build.

GPUI's Windows shader build invokes the Windows SDK shader compiler when its build helper has debug assertions disabled. Atlas's current local profile retains assertions, so this is distinct from simply saying “release-style build.” `rav1e` and `av-scenechange` have assembly-related script branches, but their assembly features are not selected here. None of this authorizes an installation or a Windows setting change. Registry-source comments remain outside Atlas's tracked-source marker check; no new vendor or blanket lint exception is needed.

The dependency proposal is ready for product addition within this approved scope. Compare the actual product lock and feature selection against this proposal, run the normal gate, and test the visible window. Re-audit any additional packages or changed versions. Offline resolution and source review prove neither compilation nor a working browser, and they do not waive the existing search performance or security-policy execution barriers.
