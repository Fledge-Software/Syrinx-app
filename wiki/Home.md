# Syrinx

Syrinx is a browser-first workspace for Windows 11. It is a desktop web
browser in the style of Arc and Zen, with a vertical-tab sidebar and one
command palette (Ctrl K), where code (Forge), notes (Notes) and the owner's
business software (Fledge) will open as tabs beside ordinary web pages. The
point is that one person never needs a separate IDE, notes app and browser
open at once, and that all of them share one search and one source of
context. It is a browser first, not an IDE with a browser attached: the
default experience on launch is browsing.

Pages render in Microsoft Edge WebView2 (the system's embedded Chromium-based
web engine), driven from Rust through the `wry` library. Syrinx's own
interface, the sidebar, Home page and palette, is drawn in Rust with GPUI,
the toolkit Zed's editor is built on. Rust owns all state and logic; the
interface is thin.

Syrinx is early. The browser core (tabs, saved pages, history, session
restore, the palette and local search) is being built and used now. Forge,
Notes and Fledge are designed but not yet built. A preview build is
published every Thursday on the
[releases page](https://github.com/Fledge-Software/Syrinx-app/releases).
Previews are unsigned, so Windows SmartScreen warns before the first run.
Syrinx is licensed under the Mozilla Public License 2.0.

The product was called Atlas, a working name, until the owner named it
Syrinx on 2026-10-12, and the wiki and the docs now say Syrinx
throughout.

## Where things are

Each kind of information has one home, so that nothing is kept in two
places that can drift apart.

- **[`docs/`](https://github.com/Fledge-Software/Syrinx-app/tree/main/docs)
  in the repository** holds what describes the app and how it is operated:
  the owner's product spec (`docs/spec.md`), the owner's decisions since the
  spec (`docs/decisions.md`, which wins where the two disagree), the
  architecture decisions D1–D49 with their alternatives
  (`docs/architecture-decisions.md`), and notes on releasing, installing,
  updates, page-text indexing, the usage report and the theme editor.
- **[`docs/dev/`](https://github.com/Fledge-Software/Syrinx-app/tree/main/docs/dev)**
  holds contributor material that tools and agents read from disk: how to
  build and run the gate (`LOCAL-DEV.md`), the traps that have cost real time
  (`trapstoknow.md`), dependency audits, reviews of the patched vendored
  dependencies and the deferred-work register.
- **[Issues](https://github.com/Fledge-Software/Syrinx-app/issues)** are the
  only place for planned and outstanding work: epics, slices, bugs and
  questions waiting on the owner. See [How we track work](How-we-track-work).
- **This wiki** holds the background reading: how the pieces fit together,
  the research that led to the decisions, ideas that are not yet plans, and
  the project's history with links to records that have been retired from
  the repository.
- **`AGENTS.md`** at the repository root holds the rules for anyone, person
  or agent, changing the code, and `design/` holds the design target and the
  brand assets.

## Pages

- [Getting started](Getting-started): what you need and the three commands.
- [Architecture](Architecture): a tour of the crates and how pages, the
  interface, storage, search and updates work.
- [How we track work](How-we-track-work): issues, epics, labels, branches and
  the gate.
- [Plugin system](Plugin-system): the 2026-10-15 decision to build plugins as
  sandboxed WebAssembly, and the order the work will happen in.
- [Product and services](Product-and-services): what stays free, the planned
  paid services and their prices, and the one Syrinx account.
- Background research notes, written before and during Phase 0:
  - [01 Shell and engine](Background-01-shell-and-engine)
  - [02 Browser v1 scope](Background-02-browser-v1-scope),
    with appendices [02a Tauri and wry facts](Background-02a-tauri-wry-facts)
    and [02b Default browser, import and signing](Background-02b-default-browser-import-signing)
  - [03 Core architecture](Background-03-core-architecture)
  - [04 Forge and Notes](Background-04-forge-and-notes)
  - [05 Fledge API](Background-05-fledge-api)
  - [06 Engine and UI performance](Background-06-engine-and-ui-performance)
  - [07 Google sign-in and DRM](Background-07-google-sign-in-and-drm)
  - [08 GPUI and wry on Windows](Background-08-gpui-and-wry-on-windows)
  - [09 Phase 0 verdict](Background-09-phase-0-verdict)
- Ideas that are not plans yet:
  [Mobile sync](Ideas-Mobile-sync) and
  [A third build for Widevine](Ideas-Widevine-third-build).
- [History](History): the timeline, and every retired document with a
  permanent link.
