# Plugin system

Syrinx is a browser first, but its reason to exist is that code (Forge),
notes (Notes) and the owner's business software (Fledge) live inside it as
plugins that open as tabs and share one palette and one search. **Nothing of
the plugin system is built yet.** This page explains the decision that
shapes it and the order the work will happen in. The work itself is tracked
in issues, linked below.

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

## The order of work

The decision fixes the order. **After Plan 1 closes**, which is its last
slice, the owner's one-week daily-driver acceptance: first the plugin
interface and host, then the template, then each plugin. Starting the
interface and template before Plan 1 closes was rejected. On 2026-10-16 the
owner allowed one exception: the research note on the WASM runtime may
start now.

1. **[#26](https://github.com/Fledge-Software/Syrinx-app/issues/26), the
   research note.** It compares the WASM runtime and interface choices (for
   example wasmtime and the component model, their licences and
   maintenance), designs the host capabilities and measures start-up and
   memory cost against D6. It is reviewed with the owner before Plan 2's
   slices are rewritten. It can start now.
2. **[#25 Epic: Plugin runtime](https://github.com/Fledge-Software/Syrinx-app/issues/25)**:
   the rewrite of the Plan 2–4 slice boundaries and `AGENTS.md`'s plugin
   rules for WASM (#27), the interface, registration, lifecycle and scoped
   storage (#28), plugin commands with forms in the palette and plugin tabs
   (#29), the template repository's contents (#30), and a plan for a
   customisable Home page with widgets (#31).
3. **[#32 Epic: Fledge plugin](https://github.com/Fledge-Software/Syrinx-app/issues/32)**:
   the first plugin built from the template, a "New lead" form in the
   palette that works against a stand-in client. Fledge is reached only
   through a client interface, and the real client waits for Fledge's own
   Stage C (D24). The owner's acceptance of Plan 2 here unblocks Notes.
4. **[#35 Epic: Notes plugin](https://github.com/Fledge-Software/Syrinx-app/issues/35)**:
   local Markdown files, wikilinks, backlinks, typed properties and a table
   view.
5. **[#42 Epic: Forge plugin](https://github.com/Fledge-Software/Syrinx-app/issues/42)**:
   a project, editor and syntax highlighting, a terminal, git, language
   servers, tasks and split editors.

## Background

The earlier comparison of plugin designs (a Rust trait, dynamic libraries,
WebAssembly and out-of-process plugins, and what Zed and Lapce do) is
[Background 03](Background-03-core-architecture). It predates the
2026-10-15 decision and recommended a statically registered Rust trait,
which that decision replaced. The editor, syntax and language-server
research for Notes and Forge is
[Background 04](Background-04-forge-and-notes), and Fledge's API surface is
[Background 05](Background-05-fledge-api).
