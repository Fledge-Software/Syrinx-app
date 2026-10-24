# Owner checklist: what only the owner can do

Each item here needs the owner's own accounts, hands, eyes or decision, and
each unblocks agent work or closes a slice. Agents point at these items and
never do them (`AGENTS.md`, "Operator work"). Code work, including the agent
half of each item, is tracked in GitHub issues on `Fledge-Software/Syrinx-app`,
not here. This list was first the owner's half of the 2026-10-14 hand-off
(`docs/owner-next-steps.md`, [at af3de53](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/owner-next-steps.md)).

## How to use it

Work through it alone, or start Claude Code in the repository and paste:

```
Read docs/owner-checklist.md and walk me through it one item at a time, in order. Give me the exact commands, tell me what to look for and record what I report in the slice's GitHub issue. Never run a measurement, a check or a sign-in for me, and stop all building agents before a quiet-machine run.
```

Record results as pass/fail and timings only, never account contents.

---

## Order

### 1. Finish P1-39d (in progress)

**Changed 2026-10-14:** you keep no release key on a device. The weekly
workflow signs with a key held as a GitHub Actions secret and re-signs
`latest` every week; the backup is an encrypted file in your password
manager (`docs/decisions.md`, 2026-10-14). The release fixes are already
on `main`.

- **Approve the workflow change first.** The agent half of issue #55 is on
  the branch `feat/55-workflow-signing`, committed but not pushed, because
  changing the release workflow needs your yes. It signs each release,
  re-signs `latest` in quiet weeks and produces the `updates/v1/` files as a
  workflow artifact (`docs/releasing.md`, "Signed updates"). Publishing those
  files to `https://syrinxapp.dev/updates/v1/` (404 on 2026-10-14) still
  waits for your decision on where they go while the repository is private.
- **Then create the two keys, outside any agent session** (commands below).
  Until an agent pins them, releases stay unsigned with a warning, so
  setting the secret first breaks nothing.
- **Give an agent only the four public lines** the script prints (two
  points, two identifiers) to pin in `PRODUCTION_KEYS` (issue #55). An
  independent reviewer recomputes the identifiers and checks them against
  your record.
- After the next release, update an installed Syrinx yourself with Check
  for updates; record it in P1-39d's GitHub issue.

#### Creating the release keys (one time)

First create the environment that holds the secret, so only workflow runs
on `main` can read it. In GitHub: this repository → Settings →
Environments → New environment, named exactly `release-signing`. Under
"Deployment branches and tags" choose "Selected branches and tags" and add
`main` only. Add no other secret there. The script below then stores the
primary key in that environment, not as a repository-wide secret.

Close every agent session first. Use a normal PowerShell window in your
Syrinx-app checkout, on a `main` that includes the merged workflow change,
with `gh` signed in to an account that may set this repository's secrets.
`new-release-key.ps1` makes each key in memory, sends its private half to
exactly one place and writes nothing to disk; read it before running it.
`-ExecutionPolicy Bypass` applies to that one run only and changes no
setting.

```powershell
git pull
gh auth status

# Optional: a throwaway key through the workflow's own import and delete commands.
powershell -NoProfile -ExecutionPolicy Bypass -File .github\scripts\new-release-key.ps1 -Rehearse

# Primary: straight into SYRINX_RELEASE_KEY in the release-signing environment (via standard input).
powershell -NoProfile -ExecutionPolicy Bypass -File .github\scripts\new-release-key.ps1 -Role Primary

# Backup: onto the clipboard; paste it into a new secure note named
# "Syrinx release key (backup)", save, press Enter, and the script clears the clipboard.
powershell -NoProfile -ExecutionPolicy Bypass -File .github\scripts\new-release-key.ps1 -Role Backup

# Check: lists the secret's name and date, never its value.
gh secret list --repo Fledge-Software/Syrinx-app --env release-signing
```

Each key run ends with two lines, `<Role> public point: …` (128 hex digits)
and `<Role> identifier: …` (16 hex digits). They are public: copy both
pairs into the password manager note as your separate record, and paste
the four lines to an agent. The backup is marked so Windows keeps it out
of clipboard history and the cloud clipboard, and the script clears the
clipboard when you press Enter or Ctrl+C. If you ever lose the primary, put
the backup's text into the same secret (`gh secret set SYRINX_RELEASE_KEY
--env release-signing --repo Fledge-Software/Syrinx-app`, which asks for the
value: paste it and press Enter); installed copies already trust it.

### 2. Quiet-machine runs (unblock P1-24 and P1-26)

Stop every agent and build first; check with
`Get-Process cargo,rustc -ErrorAction SilentlyContinue` that nothing is
listed. Close other heavy programs.

1. **P1-23 full-text bake-off (first; P1-24 and P1-26 wait on it).**

   ```powershell
   cargo build -p fulltext-bench --profile local --offline --locked
   powershell -NoProfile -File crates/fulltext-bench/run-comparison.ps1 -Label p1-23-quiet-<date> -QuietWaitMinutes 20
   ```

   Results land in `target/p1-23-quiet-<date>-*.csv`; note
   `$env:NUMBER_OF_PROCESSORS` beside them. Details:
   [`docs/p1-23-fulltext-bakeoff.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-23-fulltext-bakeoff.md).
2. **P1-05 search (D48) and AN-01's timings.** Run `cargo xtask bench` three
   times, each a fresh process. It reports the 100,000-row search p95
   against 5 ms, and AN-01's per-update overhead (100 ns budget) and file
   load (1 ms budget). Details: [`docs/p1-05-search.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-05-search.md), end of file.
3. **P1-06 idle CPU.** Rerun `docs/dev/vendor-reviews/p1-06-evidence/measure.ps1` on the
   patched `syrinx.exe` with a fresh `--test-data-dir` (300 s idle, target
   under 0.5% of one core), and the eight-suspended-tab measurement.
   Details: `docs/dev/vendor-reviews/p1-06-gpui-idle-results.md`.

### 3. Palette acceptance (P1-33)

Three times, in the foreground, without touching keyboard or mouse:

```powershell
cargo xtask palette-acceptance
```

Reports go to `target\p1-33\`. Check that the opening p95 is under one
refresh interval, `escape_returns_page_focus` passes 20 of 20 and
`enter_activates_selected_tab` passes every time. Then the hand checks in
[`docs/p1-33-palette-acceptance.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-33-palette-acceptance.md), "Owner checks" 3 and 4 (a playing
video under the palette, Alt Tab, a second monitor).

### 4. Screen reader and visual checks (P1-36 and its follow-ups)

With NVDA installed and running, work through the thirteen checks in
[`docs/p1-36-accessibility.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-36-accessibility.md), "Owner checks". The ones that keep rows open:

- **Check 7 (DW-60):** Ctrl K, type, arrow through rows. NVDA should read
  each row once, as "n of m". Also try Shift Tab and decide owner question
  3 (announce rows only after arrows, as built, or after every keystroke).
- **Check 12 (DW-62):** click a toolbar button, no ring; press Tab or F6
  from a page, ring appears; in each of the five palettes; also in the
  palette, peek and import windows; Ctrl or Shift alone does not bring the
  ring back.
- **Check 13 (DW-61):** Shift Tab back into the sidebar tree, and the peek.

### 5. Window checks of the Settings sections

On a fresh profile (`cargo syrinx --test-data-dir <new folder>`; the alias already ends
in `--`, so a second `--` reaches Syrinx and the test profile is not used):

- **Settings › Privacy, site permissions:** remember a choice on a test site, Forget it, and
  see the site ask again.
- **Settings › Usage report (AN-01):** the switch is off; switching on shows
  "Counting since …"; switching off asks first and deletes the counts.
- **Settings › Updates:** the host sentence, Check for updates and Install from file…
  appear. Reset update records appears only with a damaged
  `Updates\state` (DW-66).

Then the per-slice window checks still listed in
[`docs/implementation-status.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/implementation-status.md) (sidebar widths, folders, peek, import,
home and settings, history, clear data, restore after restart).

### 6. Owner-only slices

- **P1-18:** sign in to your Google accounts (password with phone prompt,
  passkey, Workspace), play YouTube, Disney+ and Hulu, check the daily
  sites, and browse one crash-free day. Also check whether your Workspace
  admin console turns on device checks.
- **P1-40 (closes Plan 1):** a week with Syrinx as your default browser,
  after P1-39d and AN-02, with the D6 memory and start-up evidence.
  Install and register with the setup program first (`docs/installing.md`).

### 7. Housekeeping when convenient

- Before making the repository public: the pre-publication checklist in
  `docs/releasing.md`.
