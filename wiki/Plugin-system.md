# Plugin system

Syrinx is a browser first, but its reason to exist is that code (Forge),
notes (Notes) and the owner's business software (Fledge) live inside it as
plugins that open as tabs and share one palette and one search. **Nothing of
the plugin system is built in Syrinx yet**; the plugin template's first
contents are in its own repository, `Fledge-Software/syrinx-plugin-template`.
This page explains the decisions that shape it and the order the work
happens in. The work itself is tracked in issues, linked below.

## The 2026-10-15 decision

On 2026-10-15 the owner decided (recorded in
[`docs/decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/decisions.md)):

> **Plugins are built from a template repository, each in its own
> repository, as sandboxed WebAssembly loaded at run time; no plugin work
> starts until Plan 1 closes (changes D17 and D34).**

The owner wants a plugin template made first, then Forge, Notes and Fledge
each built from it, the way Obsidian's sample-plugin template and Zed's
extensions work. The decision has four parts.

- **Repositories.** A template repository on GitHub, marked as a template,
  and one repository per plugin made from it. The owner creates the
  repositories; agents prepare their contents. A template folder inside this
  repository was rejected, as were the `crates/plugin-*` workspace crates
  the earlier plans describe.
- **Form.** Each plugin is Rust compiled to **WebAssembly** (WASM), a
  portable bytecode that Syrinx runs inside a sandbox, so a module can touch
  nothing outside itself except through functions the host gives it. This
  is the Zed model. The functions and the values that cross them will be
  described in **WIT** (WebAssembly Interface Types), so host and plugin are
  generated from one definition.
- **Host capabilities.** A plugin reaches files, git, processes and the
  network only through host functions its manifest asks for and the owner
  grants, the way Chrome extensions declare permissions. Its interface is
  HTML and JavaScript inside Syrinx's own local web view, as Notes and Forge
  already planned for CodeMirror (D28), never a page web view. State stays
  in Rust, in the plugin's WASM module and in Syrinx's core.
- **What changes.** D17's sandbox is no longer deferred until a third party
  writes a plugin: the built-in plugins use it from the start. D34's
  "`Plugin` trait" becomes a WIT interface and host; Plan 2 otherwise stays
  thin.

The rejected alternatives, and why:

- **Compiling the built-in plugins into `syrinx.exe`.** Every plugin release
  would need a Syrinx release, which defeats separate repositories.
- **Obsidian-style JavaScript plugins.** No sandbox, and state would live
  outside Rust.
- **Native DLLs.** No sandbox, and Rust has no stable binary interface.

The owner chose this knowing its cost: Forge needs the host to provide real
capabilities (terminals, git, language servers, Tree-sitter), as Zed's
host does.

## What has been decided since

The owner's later decisions, all in
[`docs/decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/decisions.md),
fill in the 2026-10-15 outline:

- **2026-10-20.** The Fledge plugin is on hold until Fledge's own API is
  ready, because a stand-in client written now would encode guesses the
  real API may not keep. The plugin order becomes runtime, template, Notes,
  Forge. The research note and the template may start alongside Plan 1's
  remaining work; the plugin host still waits for Plan 1's exit.
- **2026-10-21.** Research note 10 (#26) is approved with its fifteen
  decisions, among them: wasmtime with the component model, pinned on its
  48.x line; one versioned WIT package, `syrinx:plugin`; plugin tabs drawn
  in HTML in a plugin view, a web view type separate from page web views,
  while palette forms and Home widgets are declarative and drawn by GPUI;
  nothing granted by default; and signed folder packages, with the
  built-in plugins bundled in Syrinx's release, an "Install plugin from
  folder…" command and no plugin fetched over the network in v1. The owner now accepts Plan 2 at the runtime epic's exit
  rather than in the Fledge epic.
- **2026-10-24.** Phase 2 is approved and re-cut for WASM plugins: the host
  crate is `crates/syrinx-plugin-host`, the only crate that links wasmtime,
  and the WIT package lives at `wit/v0_1/` in the Syrinx repository.

## The order of work

The 2026-10-15 decision held all plugin work until Plan 1 closes, which is
its last slice, the owner's one-week daily-driver acceptance. The order in
force since 2026-10-20 is: the plugin runtime, then the template, then
Notes, then Forge. The research note and the template may run alongside
Plan 1; the plugin host (#28, #29) still waits for Plan 1's exit.

1. **[#26](https://github.com/Fledge-Software/Syrinx-app/issues/26), the
   research note.** It compares the WASM runtime and interface choices and
   designs the host capabilities; the Windows measurements of their cost
   fall to #28. The owner approved it on 2026-10-21. Until the owner
   publishes it here as Background 10, it is
   [`docs/dev/background-10-wasm-plugin-runtime-draft.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/dev/background-10-wasm-plugin-runtime-draft.md).
2. **[#25 Epic: Plugin runtime](https://github.com/Fledge-Software/Syrinx-app/issues/25)**:
   the re-cut of the Plan 2–4 slices and `AGENTS.md`'s plugin rules for
   WASM, with its pre-build checks (#27); the host crate, WIT 0.1.0, a test
   component and the test host (#28); the manifest and signed packages,
   lifecycle, grants and the compiled-code cache (#134 to #137); palette
   commands and forms (#29), the plugin view, plugin tabs, installing from
   a folder and plugin search feeds (#138 to #141); the template
   repository (#30); bundling the built-in plugins (#142, outside the
   epic's exit); and a customisable Home page with widgets (#31, #92).
   The owner's acceptance of Plan 2 at this epic's exit unblocks Notes.
3. **[#35 Epic: Notes plugin](https://github.com/Fledge-Software/Syrinx-app/issues/35)**:
   local Markdown files, wikilinks, backlinks, typed properties and a table
   view. Each slice (#36 to #41) has a Syrinx host half and a plugin half
   in the Notes repository.
4. **[#42 Epic: Forge plugin](https://github.com/Fledge-Software/Syrinx-app/issues/42)**:
   a project, editor and syntax highlighting, a terminal, git, language
   servers, tasks and split editors. It starts only once Notes is live as
   the owner's daily notes tool.
5. **[#32 Epic: Fledge plugin](https://github.com/Fledge-Software/Syrinx-app/issues/32)**,
   on hold: a "New lead" form in the palette on a stand-in client. Fledge
   is reached only through a client interface, and the real client waits
   for Fledge's own Stage C (D24). It resumes when Fledge's API is ready
   and the owner says so.

## Background

The earlier comparison of plugin designs (a Rust trait, dynamic libraries,
WebAssembly and out-of-process plugins, and what Zed and Lapce do) is
[Background 03](Background-03-core-architecture). It predates the
2026-10-15 decision and recommended a statically registered Rust trait,
which that decision replaced. The WASM runtime research behind the
2026-10-21 decisions is research note 10, linked under #26 above. The
editor, syntax and language-server research for Notes and Forge is
[Background 04](Background-04-forge-and-notes), and Fledge's API surface is
[Background 05](Background-05-fledge-api).
