# Rules every Astra session follows

Paste this block above each Astra task prompt. It keeps three parallel Astra
sessions and Claude's subagents from colliding, and keeps each session inside
a 258k-token context.

## Context budget

Your context is 258k tokens. The repository's docs are much larger than that.
Read only the files and line ranges your task lists. Never read a whole file
over 600 lines; use `grep -n "^#"` to find headings and read only the section
you need. In particular, never read all of `docs/p1-05-search.md` (2,348
lines), `docs/handoff-integrated-preview-2026-10-10.md` (1,166 lines) or
`docs/plans/Atlas v1 — Plan.md` (1,248 lines) end to end. Keep command output
short with `tail`, `grep` or `Select-Object -Last`.

## Read first, in this order

1. `AGENTS.md`: the rules (writing readable code, guardrails, test data).
2. `docs/trapstoknow.md`: the traps already found. Read the sections that
   touch your area.
3. `docs/decisions.md`: the owner's decisions since the spec. These win over
   everything else.
4. `docs/implementation-status.md`, lines 520–555 only: the Plan 1 slice
   table.
5. Your task's own reading list.

## Where you work

- **Never work in `C:\Users\<user>\Documents\GitHub\Atlas-app` itself.** That
  checkout belongs to the integration branch. Create your own worktree:

  ```powershell
  git -C C:\Users\<user>\Documents\GitHub\Atlas-app worktree add ..\Atlas-wt\<slice> -b codex/<slice> codex/integrated-browser-preview
  ```

- Use one branch per slice, named in your task. Commit to that branch as you
  reach verified checkpoints. Write commit messages that explain why before
  what, as in "Writing voice" in `AGENTS.md`.
- **Never merge, rebase another branch, push or add a remote.** The owner
  approves every merge. The repository is local only.
- Other agents are working in parallel on these slices: P1-05 search, the
  P1-06 GPUI idle patch, P1-07 design tokens, the P1-11–P1-14 browser
  essentials, the P1-21 folder model, the P1-22 import parser and the P1-23
  full-text bake-off. Stay inside the files your task names. If you must
  change a shared file (the workspace `Cargo.toml`, `Cargo.lock`,
  `crates/atlas-app/src/ui.rs` or `crates/atlas-platform/src/lib.rs`), keep
  the change minimal and say so in your final report.

## Gate and evidence

- The gate is `cargo xtask check` (see `docs/LOCAL-DEV.md`). Run it before
  you call the slice done, and write the log under your worktree's `target/`.
- Windows Application Control sometimes blocks locally built executables
  (os error 4551). Record the block; never change a security setting, and
  never sign or relocate binaries to get around it.
- `TODO`, `FIXME` and `HACK` markers are allowed only as `TODO(DW-n)`, with
  the row added first to "Deferred work" in `docs/plans/Atlas v1 — Plan.md`.
  The gate rejects any other form.
- Never skip, `#[ignore]`, loosen or delete a test. Never silence Clippy with
  `#[allow]`. Never swallow errors with `let _ =`, `.ok()` or a catch-all
  default.
- Use only synthetic test data: `example.com`, `.test`, `.invalid` and
  made-up text. Never read a real browser profile or the owner's Atlas data.

## Stop and ask, instead of acting

Stop and report to the owner, without doing it, if your task needs any of
these:

- a new dependency, or a `Cargo.lock` change that shifts many transitive
  packages. Write the maintenance, security and licence audit, then stop;
- moving the GPUI, wry or `webview2-com` pins, or any vendor patch not
  already approved in `AGENTS.md`;
- a change to a decision in `docs/decisions.md`, the spec or a plan's
  "Decisions taken";
- anything touching a page web view's identity: user agent, IPC handler,
  injected script or request filter.

## Final report

End with a short report:

- what is done, with the gate log path and its result;
- what is not done, and why;
- any shared file you touched;
- which ledger rows in `docs/implementation-status.md` should change.

Update that ledger row yourself, on your branch only.
