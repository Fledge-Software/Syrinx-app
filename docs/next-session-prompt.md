# Next session: continue Syrinx v1 from the 2026-10-14 evening checkpoint

The agent half of the hand-off from the second 2026-10-14 session on
EZ-LAPTOP-01; the owner's half is `docs/owner-next-steps.md`. First how
the owner starts, then, from **Brief** down, what the new session reads.
Where the files it names disagree with this prompt, the files win.

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
read, as `CLAUDE.md` says: `docs/decisions.md` (five new 2026-10-14 entries:
DW-62's focus-ring route, DW-68's pinned network allow-list, and AN-01's
early start, command list and switch note); `docs/plans/Syrinx v1 —
Plan.md` ("Decisions taken", "Open threads", the "Deferred work" register
and the issue map, including the new AN-01/AN-02 rows); `design/README.md`
before changing anything I see; `docs/LOCAL-DEV.md`; `docs/releasing.md`;
and `docs/implementation-status.md`, whose current checkpoint is
2026-10-14. For AN-02 also read `docs/proposals/an-01-02-local-analytics.md`
and `docs/an-01-local-analytics.md`; for DW-69, the "P1-39b" and "DW-66"
sections of `docs/p1-38-update-format.md`.

### 2. Where things stand

`git fetch origin` and fast-forward local `main` first. At hand-off
`origin/main` was `79e96bc`, and `cargo xtask check` passed there with
1,378 tests and the smoke run. Run the gate on `main` before new work and
record the count in the ledger's checkpoint.

This session merged, each after independent review (re-reviewed after
fixes) and a green gate:

| Slice | What it did |
| --- | --- |
| Ledger tidy | Every row names the merge that brought its slice to main; checkpoint moved to 2026-10-14 |
| DW-66 | Customize → Updates offers "Reset update records…" only while `Updates\state` is unreadable; start-up discards a staged update beside an unreadable record (my decision) |
| DW-60 | The palette is a combo box that owns the query and rows, so screen readers hear the arrowed row (open until my NVDA check 7) |
| DW-62 | Syrinx's own keyboard-or-pointer flag shows the focus ring only after keys (open until my visual check 12) |
| DW-68 | `check-deps` reads the resolved `windows`/`windows-sys`/`winapi` features; eight third-party network entries are allowed, pinned to their requesters; the source scan refuses socket APIs in Syrinx crates |
| AN-01 | Local usage counting, off until switched on; switching off deletes and stays off |

I reported on 2026-10-14 that P1-39 is finished apart from release bugs I am
working out elsewhere. `PRODUCTION_KEYS` on main was still empty at
hand-off, so do not mark DW-30 or P1-39d done until the pinned keys are on
main.

**This machine (EZ-LAPTOP-01)** has Rust 1.97.1, VS 2022 Build Tools with
the Windows SDK, Git for Windows, and Python 3.13 per-user. The certificate
fixture needs `%LOCALAPPDATA%\Programs\Python\Python313` first on `PATH`.
Node and pnpm are not installed.

### 3. Rules learned

Last session's rules still hold: another session may change `main` (check
`git merge-base --is-ancestor` before merging and replay with `rebase
--onto` if needed); agents stop when I interrupt; no synthetic keystrokes or
clicks unless Syrinx's own window, by process id, is in the foreground;
serialize native fixture runs with builds; `git merge`/`commit` messages
come from a file; never pipe Unicode through PowerShell. New this session:

- **Pushing `main` can be blocked by the session's permission check.** Ask
  me, then push only as a fast-forward after a green gate.
- **Four agents building at once took hours each.** Every worktree builds
  its own `target`. Run at most two building agents at a time; reviewers
  read only and do not build.
- **`docs/implementation-status.md` and `docs/decisions.md` conflict on
  almost every merge** (the P1-36 and P1-39 rows, the 2026-10-14 entries).
  Keep main's side, re-add the branch's note by hand, and rewrite files
  with .NET UTF-8 (no BOM), never through a PowerShell pipe.
- **Reviews keep finding real bugs**: AN-01's first review found that a
  failed delete would silently resume counting, DW-68's found a bypassable
  library-name scan. Re-review every fix commit that adds logic.
- **A register row stays Open until my manual check passes**, as DW-60,
  DW-61 and DW-62 do; "built" goes in the row's text.

### 4. Agent work, in work order

Use subagents, each in its own worktree on its own branch, with an
independent review, then a `--no-ff` merge and the gate.

1. **DW-69:** finish an interrupted discard of a staged update (write the
   staging mark before removing `pending`, with a crash-point test). P1-39b
   follow-up; the row has the design.
2. **AN-02:** the report preview and owner-initiated JSON export, as the
   approved proposal defines, restoring the approved switch sentence ("You
   can view, export or delete the counts at any time"). AN-02 must land
   before P1-40.
3. **Accessible text fields:** DW-60 found that the find bar, folder and
   tile forms and Customize's colour field report the window, not the
   field, as focused (`docs/p1-36-accessibility.md`, "DW-60"). Add a DW row
   first, then apply DW-60's container pattern to each field and rerun the
   UI Automation probe.

Still blocked, so do not start them:
- P1-24 and P1-26 wait on P1-23's quiet-machine runs and engine choice,
  which must also resolve the lru advisory.
- DW-63 waits on my choice of key (Ctrl W on a focused row, or Backspace).
- DW-5 belongs to Plan 2; DW-67 waits until after P1-40.
- P1-39d, P1-18 and P1-40 are mine.

If everything above is done or blocked, ask me before moving on.

### 5. The owner's own items

They are listed, in order and with commands, in `docs/owner-next-steps.md`.
Point at the item that blocks your next step and wait; never do one. Before
I start a quiet-machine run, stop every building agent when I ask.

Throughout: recommend directly, put undecided product questions to me, and
write commits and reports in `AGENTS.md`'s "Writing voice".
