# Next session: continue Syrinx v1 from the 2026-10-13 checkpoint

The hand-off from the 2026-10-13 cloud session: first how the owner starts,
then, from **Brief** down, what the new session reads. Where the files it
names disagree with this prompt, the files win.

## How to start (for the owner)

1. Open PowerShell in `C:\Users\<user>\Documents\GitHub\Syrinx-app`. To keep
   long sessions from running out of context, set
   `$env:CLAUDE_AUTOCOMPACT_PCT_OVERRIDE = '50'` first.
2. Start Claude Code there and paste:

   ```
   Read docs/next-session-prompt.md from the "Brief" heading down and follow it in order. Stop and ask me before anything it lists as mine.
   ```

---

## Brief

Syrinx (called Atlas until 2026-10-12) is a browser-first workspace app for
Windows 11. Plan 1 (engine, browser core and palette) is still open, so all
work below comes from it. The repository is on GitHub at
`Fledge-Software/Syrinx-app` (`docs/decisions.md`, 2026-10-13): push only the
branch you were assigned, never `main` unless I ask, and open a pull request
only when I ask.

### 1. Read before anything else

`CLAUDE.md` loads `AGENTS.md`, `docs/trapstoknow.md` and `docs/spec.md`. Then
read, as `CLAUDE.md` says: `docs/decisions.md` (the three 2026-10-13 entries:
the GitHub remote, the hawk logo and its `#FF612E` accent, and P1-39's
approval); `docs/plans/Syrinx v1 — Plan.md` ("Decisions taken", "Open
threads", "Deferred work" and the issue map); `design/README.md` before
changing anything I see; `docs/LOCAL-DEV.md`; and the "Current checkpoint —
2026-10-13" in `docs/implementation-status.md`.

### 2. Sync `main`

`git fetch origin` and fast-forward local `main` to `origin/main`. On
2026-10-13 it was at `53af11f`. Then check whether the P1-39 approval branch
has merged: `git merge-base --is-ancestor b41c3bd origin/main`. Its two
commits, `3d14dd7` (the owner's approval recorded in `decisions.md`, the
plan and the ledger) and `b41c3bd` (P1-40 now waits on P1-39d; both
issue-map copies agree), are on `ccr-99a24605-ajkyhv`. If they are not on
`main`, tell me and ask whether to merge that branch through the normal
review and gate before P1-39a starts; without it, `main`'s ledger still
reads as though P1-39 waits on my decision.

### 3. Run the gate on `main` first

Run `cargo xtask check` on `main` before any new work, and record the test
count in the checkpoint of `docs/implementation-status.md`. This matters
because the cloud session merged `53af11f` at my request **without the
gate** (it runs only on Windows), and DW-7's three Store-level tests in
`crates/syrinx-store/src/engine/tests/permissions.rs` have never run
anywhere. The last recorded gate passed 1,014 tests, before the P1-05,
P1-23 and `53af11f` merges. If it fails, fix the cause on its own branch;
never weaken a test.

### 4. Agent work, in work order

Use subagents, each in its own worktree on its own branch. Each slice merges
to `main` with `--no-ff` only after an independent review approves it and
the gate passes after the merge. Serialize every native fixture run with the
gate (trap: "Do not build an example while its executable is running").

1. **DW-61: wire the roving-focus model into the sidebar and the peek.**
   `crates/syrinx-ui/src/tree_focus.rs` already decides the single Tab stop
   and where Up, Down, Home, End, Left and Right go. Wire it into
   `crates/syrinx-app/src/ui/sidebar_view.rs` and `ui/peek.rs`, following
   the module's own cautions: build the `TreeRow`s in the same loop that
   draws the rows (`rows()` skips a row whose lookup fails), and after every
   row change reset the focused id with `tab_stop(...)`, or
   `focus_after_collapse` for a collapse. F6 should land on the active row.
   Mind the GPUI traps on `tab_stop(true)` and the accessibility tree; rerun
   the UI Automation probe in `docs/p1-36-accessibility.md`. My NVDA check
   follows.
2. **DW-7: the settings control to forget a site permission.** The store
   half (`Store::forget_permission`) is merged. Add the control to P1-29's
   settings surface with UI tests in `syrinx-app`, then remove the
   `TODO(DW-7)` marker in `crates/syrinx-store/src/permissions.rs`.
3. **The fixture-timeout diagnosis** (`docs/fixture-timeout-diagnosis-plan.md`).
   Start with Step 0: 20 unchanged, untraced runs of the native fixture on
   current `main`, keeping each run's output and each failed run's synthetic
   directories under `target/`. Then the shared instrumentation, then
   candidates 1 and 2. Never change an assertion or a deadline. Record
   results in the plan and the P1-11 row.
4. **P1-39a**, per the approved split in
   `docs/proposals/p1-39-update-delivery.md`: the signed latest-version
   statement and its verifier in `syrinx-update`, `cargo xtask package`
   emitting P1-38's `manifest`, and `cargo xtask sign-update` tested with
   in-memory keys. No network, and no real keys: those are mine (DW-30).
   P1-39b and P1-39c follow in order; AN-01 follows P1-39.

Still blocked, so do not start them: P1-24 and P1-26 wait on P1-23's
quiet-machine runs and engine choice, which must also resolve the lru
advisory. If everything above is done or blocked, ask me before moving on.

### 5. The owner's own items (point at them; never do them)

- **Quiet-machine runs, P1-23 first:** three fresh-process runs per engine
  and corpus with `run-comparison.ps1`, with Cargo and rustc not running,
  because P1-24 and P1-26 wait on its result. Then D48's three runs for
  P1-05 (`cargo xtask bench`) and P1-06's idle reruns and
  eight-suspended-tab measurement.
- **Palette acceptance three times:** `cargo xtask palette-acceptance` in the
  foreground, keyboard and mouse untouched (P1-33; confirms DW-90).
- **Manual window checks** listed in each slice's row of
  `docs/implementation-status.md`, including the branding: taskbar, Alt Tab
  and title bar on a light taskbar, and the Haze/Glass accent adjustment.
- **DW-30's release keys:** a primary on a hardware key and an offline
  backup, created outside the repository and every agent session. Production
  updates (P1-39d) wait on them and on the `syrinxapp.dev` host set-up.
- **Old Atlas folders:** `%LOCALAPPDATA%\Atlas\data`,
  `%LOCALAPPDATA%\AtlasProductPreview` and `%LOCALAPPDATA%\Programs\Atlas`
  may hold signed-in WebView2 profiles. Syrinx never touches them; I delete
  them by hand when nothing in them is needed (`docs/LOCAL-DEV.md`).
- **Owner-only slices:** P1-18 (site and sign-in acceptance) and P1-40 (a
  default-browser week, after P1-39d).

Throughout: recommend directly, put undecided product questions to me, and
write commits and reports in `AGENTS.md`'s "Writing voice".
