# Next session: continue Syrinx v1 from the 2026-10-14 checkpoint

The hand-off from the 2026-10-13/14 session on EZ-LAPTOP-01: first how the
owner starts, then, from **Brief** down, what the new session reads. Where
the files it names disagree with this prompt, the files win.

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
work below comes from it; plugins (Plan 2) start only once Plan 1 closes. The
repository is on GitHub at `Fledge-Software/Syrinx-app`.

### 1. Read before anything else

`CLAUDE.md` loads `AGENTS.md`, `docs/trapstoknow.md` and `docs/spec.md`. Then
read, as `CLAUDE.md` says: `docs/decisions.md`; `docs/plans/Syrinx v1 —
Plan.md` ("Decisions taken", "Open threads", the "Deferred work" register and
the issue map); `design/README.md` before changing anything I see;
`docs/LOCAL-DEV.md`; `docs/releasing.md` (GitHub now runs the gate on every
push and pull request, PR #4); and `docs/implementation-status.md`, its
current checkpoint and the P1-39 rows. For update work, also read
`docs/proposals/p1-39-update-delivery.md` and `docs/p1-38-update-format.md`,
whose P1-39a, P1-39b and P1-39c sections record my answers of 2026-10-14.

### 2. Where things stand

`git fetch origin` and fast-forward local `main` first. At hand-off
`origin/main` was `b44c19a`; `cargo xtask check` last passed locally at
`6dac3e1` with 1,305 tests and the smoke run, and PR #4 since changed only
the GitHub workflows and `docs/releasing.md`. Run the gate on `main` before
new work and record the count in the ledger's checkpoint.

The 2026-10-13/14 session merged, each after independent review and a green
gate:

| Slice | What it did |
| --- | --- |
| DW-61 | One Tab stop and arrow keys in the sidebar tree and the peek; F6 lands on the active row |
| DW-7 | Customize → Site permissions → Manage lists remembered choices, each with Forget |
| Fixture-timeout diagnosis | The old hidden-tab suspension timer froze two never-shown test pages; my 2026-10-12 timer change already fixed it (20/20 passes; the reproduction fails 3/3 on `bbe59d3` and passes 3/3 on main) |
| DW-64, DW-65 | The certificate suite fails within 5 s of a dropped connection; passing runs delete their synthetic profiles; the gate now runs the fixture's own unit tests |
| P1-39a | The signed latest-version statement, its verifier, `cargo xtask package` writing the manifest, and `cargo xtask sign-update` |
| P1-39b | "Install update from file…", `syrinx-apply.exe`, the pre-update snapshot, Restart now / Later and rollback |
| P1-39c | The button-pressed "Check for updates" over WinHTTP (`syrinx-update-net`, the one D9 exception) |

Every real update still fails closed, because `PRODUCTION_KEYS` stays empty
until my P1-39d.

**This machine (EZ-LAPTOP-01)** has Rust 1.97.1, VS 2022 Build Tools with
the Windows SDK, Git for Windows, and Python 3.13 per-user. The certificate
fixture needs `%LOCALAPPDATA%\Programs\Python\Python313` first on `PATH`,
because a Microsoft Store `python.exe` alias also exists. Node and pnpm are
not installed.

### 3. Rules learned last session

- **Another session also changes `main`.** On 2026-10-14 one rebased local
  `main` onto `origin/main`, giving every local commit a new hash, and pushed
  a merge before its gate ran. Before merging a slice, check that its base is
  still an ancestor of `main` (`git merge-base --is-ancestor`); if it is not,
  replay the slice's own unpushed commits with `git rebase --onto main
  <old-base>` and re-gate. You may push `main` after a green gate, as a
  fast-forward only: `git fetch` first, never force-push, never push ungated
  work.
- **Agents stop when I interrupt a tool call.** Afterwards, check
  `ListAgents`, running `cargo` and `page_close` processes and each
  worktree's `git status` before assuming an agent is still working. A
  stopped agent cannot be resumed; launch a replacement in its existing
  worktree when I ask.
- **No synthetic keystrokes or clicks unless Syrinx's own window, by process
  id, is confirmed in the foreground.** On 2026-10-14 a window check sent
  Ctrl K into my Claude app.
- **Serialize native fixture runs with every build:** check for a running
  `page_close` before building, and never run timing experiments while
  anything compiles.
- **`git merge` cannot read `-F -` in this shell;** write the message to a
  file in the scratchpad first. Python is not available for edits; prefer
  the Edit tool, and never pipe Unicode through PowerShell (trap).
- **Reviews catch real bugs.** P1-39b's first review found the helper could
  never rename the program folder. Keep every slice on its own branch and
  worktree, with an independent reviewer and the gate after each `--no-ff`
  merge.

### 4. Agent work, in work order

Use subagents, each in its own worktree on its own branch.

1. **Ask me first about AN-01.** The approved analytics proposal
   (`docs/proposals/an-01-02-local-analytics.md`) puts AN-01 after all of
   P1-39, including my P1-39d. The last session recommended starting AN-01
   now, since it needs neither the keys nor the host; I had not answered.
   Start it only on my yes.
2. **DW-60:** announce the palette's moving selection to screen readers
   (P1-36 follow-up, needed before P1-40). Rerun the UI Automation probe in
   `docs/p1-36-accessibility.md`; my NVDA check follows.
3. **DW-62:** show the focus ring only after keyboard input (P1-36
   follow-up).
4. **DW-66:** a "Reset update records" action for an unreadable
   `Updates\state` (P1-39b follow-up, which I chose on 2026-10-14).
5. **DW-68:** make `cargo xtask check-deps` check the resolved `windows`
   features from `cargo metadata`'s `resolve.nodes`, so a third-party crate
   cannot switch on networking unseen (P1-39c follow-up).
6. **Ledger tidy:** many rows in `docs/implementation-status.md` still say
   "merged into the integration branch" or "awaiting merge" for work that is
   on `main`. Correct them so my picture of progress is accurate.

Still blocked, so do not start them:
- P1-24 and P1-26 wait on P1-23's quiet-machine runs and engine choice,
  which must also resolve the lru advisory.
- AN-02 waits on AN-01.
- DW-63 waits on my choice of key; it proposes Ctrl W on a focused row, with
  Backspace as the alternative.
- DW-5 belongs to Plan 2.
- DW-67 is deferred until after P1-40.

If everything above is done or blocked, ask me before moving on.

### 5. The owner's own items (point at them; never do them)

- **Quiet-machine runs, P1-23 first:** three fresh-process runs per engine
  and corpus with `run-comparison.ps1`, with Cargo and rustc not running;
  P1-24 and P1-26 wait on them. Pause every agent first. Then D48's three
  runs for P1-05 (`cargo xtask bench`), and P1-06's idle reruns and
  eight-suspended-tab measurement.
- **Palette acceptance three times:** `cargo xtask palette-acceptance` in the
  foreground, keyboard and mouse untouched (P1-33).
- **Manual window and NVDA checks** listed in each slice's row, including
  DW-61's NVDA check 13 (Shift Tab into the tree, and the peek, which no
  agent drove) and the new Customize sections (Site permissions, Updates).
- **DW-63's key choice** for "Unload page" on a focused sidebar row.
- **P1-39d:**
  - create the two release keys, a primary on a hardware key and an offline
    backup, outside the repository and every agent session;
  - have a session pin both public keys (DW-30);
  - make `syrinxapp.dev` serve `/updates/v1/` with its full certificate
    chain;
  - run the first real `sign-update` and update myself;
  - keep my own record of the highest statement serial issued, and re-sign
    `latest` at least every 60 days.
- **Old Atlas folders:** `%LOCALAPPDATA%\Atlas\data`,
  `%LOCALAPPDATA%\AtlasProductPreview` and `%LOCALAPPDATA%\Programs\Atlas`
  may hold signed-in WebView2 profiles. I delete them by hand when nothing in
  them is needed.
- **Owner-only slices:** P1-18 (site and sign-in acceptance) and P1-40 (a
  default-browser week, after P1-39d and AN-02).

Throughout: recommend directly, put undecided product questions to me, and
write commits and reports in `AGENTS.md`'s "Writing voice".
