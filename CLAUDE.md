@AGENTS.md
@docs/dev/trapstoknow.md

# Syrinx

Syrinx is a browser-first workspace app: a Windows desktop browser with a
vertical-tab sidebar, where Notes, Forge (code) and Fledge will run as
built-in plugins that open as tabs. Plugins are sandboxed WebAssembly
components, each in its own repository, with the built-in ones bundled in
Syrinx's release (owner, 2026-10-15 and 2026-10-21); none is built yet,
and the Fledge plugin is on hold until Fledge's API is ready (2026-10-20).
It was called Atlas, a working name, until the owner named it Syrinx on
2026-10-12; the docs and wiki say Syrinx throughout. Pages render in WebView2 driven from
Rust through wry, and Syrinx's own interface is drawn in Rust with the
conditionally approved GPUI base (D37's idle-patch gate still applies).

- The product spec, the decisions already made and the phased plan are in
  `docs/spec.md`, loaded below. Its "Decisions already made" are not to be
  relitigated.
- The owner's decisions since the spec are in `docs/decisions.md`; where it
  and the spec disagree, it wins. The v1 decisions D1–D49, each argued with
  what was rejected, are in `docs/architecture-decisions.md`. Work is tracked
  in GitHub issues on `Fledge-Software/Syrinx-app`.
- The design target and the decisions made since the spec are in
  `design/README.md`. The live canvas is the source of truth for the design;
  `design/canvas/` is a snapshot of it.

# At the start of every session

`AGENTS.md` (the rules) and `docs/dev/trapstoknow.md` (the traps) are loaded
above, and `docs/spec.md` below. Before picking up work, also read:

- `docs/decisions.md`: the owner's decisions since the spec, which win over
  it.
- `docs/architecture-decisions.md`: the v1 decisions, which are settled.
  Where they disagree with the files above, those files still win.
- The GitHub issue you are working, and its epic.
- `design/README.md`, before changing anything the owner sees.
- The wiki's Background page behind the area you are working in, for the
  reasoning (notes 07 and 08 correct parts of the earlier ones).
- `docs/dev/LOCAL-DEV.md`: how to run the app and the gate on this machine.

# Writing readable code

- **Write code a human can read.** Follow "Writing readable code" in
  `AGENTS.md`:
  - a one-line *why* at the top of each file;
  - a short *what* comment above each block;
  - early returns instead of nesting;
  - named helpers instead of long functions.

  A long explanatory header is not a substitute.

## Working agreements

- **The repository is on GitHub at `Fledge-Software/Syrinx-app` (owner,
  2026-10-13).** Push only the branch your session was assigned, never
  `main` unless the owner asks, and open a pull request only when the owner
  asks. Syrinx is MPL-2.0, and the owner has made the repository public. A weekly
  preview release runs from `.github/workflows/weekly-release.yml`; see
  `docs/releasing.md` and `docs/decisions.md`.
- **Local implementation is approved (D41; workflow updated 2026-10-12).**
  Build each slice on its own branch in a local worktree. Once an
  independent review approves it and `cargo xtask check` passes, merge it to
  `main` (see `docs/decisions.md`). GitHub is not a prerequisite. The owner-requested planning baseline is
  committed as `2c80273`; the scaffold records the exact development commands
  in `docs/dev/LOCAL-DEV.md`.
- **Commit only when the owner asks.** (The v1 plans were the one
  exception, committed to their branch as they were written; they were
  removed on 2026-10-16 and are read at `af3de53`.)
- **Work the phases in order.** Phase 0, the feasibility spike, came first
  and is done: its verdict, go with GPUI, is the wiki's Background 09
  (2026-10-10), and the owner approved the four v1 plans the same day.
  Take the next piece of work from the earliest phase or plan with open
  work, and if that is blocked, ask before skipping ahead. See "Work
  order" in `AGENTS.md`.
- **Syrinx is a separate project from Fledge-App.** Fledge reaches Syrinx only
  as a plugin talking to Fledge's API; don't put Syrinx code or docs in the
  Fledge-App repository.
- **Fixing a PR means committing on that PR's branch** (once the repository
  is on GitHub). Conflict resolutions, CI fixes and review-comment fixes go to
  the branch the pull request already tracks, never to a side branch or a
  second pull request that "supersedes" the first. Merge the base branch into
  the PR branch and push; do not rebase, amend or force-push somebody else's
  branch.
- **Build and test on this machine.** Syrinx is a Windows desktop app with no
  server or deploy. Run `cargo xtask check` as defined in `docs/dev/LOCAL-DEV.md`:
  dependency checks, formatting, Clippy, tests and an application smoke run
  under D38's `local` profile. After changing anything the owner sees, run
  the app and look at the window.
- **Hands off Google's pages.** Never set a custom user agent, attach an IPC
  handler or inject scripts on a page web view; the rules and their reasons
  are under "Page web views and Google" in `AGENTS.md`.
- **Never paste real personal data anywhere**: not the owner's history,
  bookmarks, notes, page text, cookies, Fledge data or credentials, and not
  into a test, a fixture, a log line, a commit, a plan or a chat with an
  agent. Generate test values that cannot belong to anyone. The owner, not an
  agent, signs in to real accounts. The full rule is under "Test data" in
  `AGENTS.md`.
- **Usage analytics stays local (D39).** The owner approved aggregate
  feature/command counts, timings and error categories, with explicit report
  export and model suggestions reviewed by the owner. No automatic uploads,
  identifiers, content or detailed event timeline. Do not collect or export
  real usage while developing; review the schema and slice breakdown first.

@docs/spec.md
