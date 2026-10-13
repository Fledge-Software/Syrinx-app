@AGENTS.md
@docs/trapstoknow.md

# Syrinx

Syrinx is a browser-first workspace app: a Windows desktop browser with a
vertical-tab sidebar, where Forge (code), Notes and Fledge run as built-in
plugins that open as tabs. It was called Atlas, a working name, until the
owner named it Syrinx on 2026-10-12; older records still say Atlas. Pages render in WebView2 driven from
Rust through wry, and Syrinx's own interface is drawn in Rust with the
conditionally approved GPUI base (D37's idle-patch gate still applies).

- The product spec, the decisions already made and the phased plan are in
  `docs/spec.md`, loaded below. Its "Decisions already made" are not to be
  relitigated.
- The owner's decisions since the spec are in `docs/decisions.md`; where it
  and the spec disagree, it wins. The v1 plan, with every decision argued,
  is `docs/plans/Syrinx v1 — Plan.md`.
- The design target and the decisions made since the spec are in
  `design/README.md`. The live canvas is the source of truth for the design;
  `design/canvas/` is a snapshot of it.

# At the start of every session

`AGENTS.md` (the rules) and `docs/trapstoknow.md` (the traps) are loaded
above, and `docs/spec.md` below. Before picking up work, also read:

- `docs/decisions.md`: the owner's decisions since the spec, which win over
  it.
- `docs/plans/Syrinx v1 — Plan.md`: its "Decisions taken" table, its "Open
  threads" and the part of the plan the work you are taking comes from. The
  owner approved all four plans on 2026-10-10; their decisions are settled.
  Where they disagree with the files above, those files still win.
- `design/README.md`, before changing anything the owner sees.
- The research note in `docs/research/` behind the area you are working in,
  for the reasoning (notes 07 and 08 correct parts of the earlier ones).
- `docs/LOCAL-DEV.md`, once the scaffold has created it: how to run the app
  and the gate on this machine.

# Writing readable code

- **Write code a human can read.** Follow "Writing readable code" in
  `AGENTS.md`:
  - a one-line *why* at the top of each file;
  - a short *what* comment above each block;
  - early returns instead of nesting;
  - named helpers instead of long functions.

  A long explanatory header is not a substitute.

## Working agreements

- **This repository is local only for now: no git remote and no pushes.**
- **Local implementation is approved (D41; workflow updated 2026-10-12).**
  Build each slice on its own branch in a local worktree. Once an
  independent review approves it and `cargo xtask check` passes, merge it to
  `main` (see `docs/decisions.md`). GitHub is not a prerequisite. The owner-requested planning baseline is
  committed as `d79fc29`; the scaffold records the exact development commands
  in `docs/LOCAL-DEV.md`.
- **Commit only when the owner asks.** (The v1 plan may be committed as it
  is written, to its local branch.)
- **Work the phases in order.** Phase 0, the feasibility spike, comes first;
  stop after it and report what worked, what didn't and what you recommend.
  After it, take the next piece of work from the earliest phase or plan with
  open work, and if that is blocked, ask before skipping ahead. See "Work
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
  server or deploy. Run `cargo xtask check` as defined in `docs/LOCAL-DEV.md`:
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
