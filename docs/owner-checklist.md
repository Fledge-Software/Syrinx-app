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

### 1. Finish P1-39d (paused by you on 2026-10-24)

**Paused 2026-10-24:** you chose not to set up the signing key yet, because
it adds a monthly cost and the previews have no users who need signed
updates, so weekly previews stay unsigned until you do this item
(`docs/decisions.md`, 2026-10-24). The agent half waits as draft PR #119.

**Changed 2026-10-14:** you keep no release key on a device. The weekly
workflow signs with a key held as a GitHub Actions secret and re-signs
`latest` every week; the backup is an encrypted file in your password
manager (`docs/decisions.md`, 2026-10-14). The release fixes are already
on `main`.

- Wait for the agent slice (a GitHub issue) that adds workflow signing and
  publishes `https://syrinxapp.dev/updates/v1/` (it returned 404 on
  2026-10-14). It will give you the commands for the one-time key creation.
- Then, outside any agent session: create the key, put it in the GitHub
  secret and the backup in your password manager, and give an agent only
  the two public points and identifiers to pin (issue #55).
- After the next release, update an installed Syrinx yourself with Check
  for updates; record it in P1-39d's GitHub issue.

### 2. Quiet-machine runs

Stop every agent and build first; check with
`Get-Process cargo,rustc -ErrorAction SilentlyContinue` that nothing is
listed. Close other heavy programs.

1. **P1-23 full-text bake-off: done.** The quiet-machine comparison ran on
   2026-10-21 (results on #52), and on 2026-10-24 you confirmed its choice,
   Tantivy, pinned at 0.27.0 (`docs/decisions.md`, 2026-10-24). Nothing is
   left to run; P1-24 (#53) builds on it.
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
[`docs/p1-36-accessibility.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/p1-36-accessibility.md), "Owner checks". The ones that still wait on you, named by
their items in Plan 1's old deferred-work register (`DW-n`, read at
`af3de53`):

- **Check 7 (DW-60):** Ctrl K, type, arrow through rows. NVDA should read
  each row once, as "n of m". Also try Shift Tab and decide owner question
  3 (announce rows only after arrows, as built, or after every keystroke).
- **Check 12 (DW-62):** click a toolbar button, no ring; press Tab or F6
  from a page, ring appears; in every preset (Graphite, Paper and, since
  2026-10-21, Glass) and in one custom theme, which the 2026-10-16 decision
  asks for in place of the five fixed palettes; also in the palette, peek
  and import windows; Ctrl or Shift alone does not bring the ring back.
- **Check 13 (DW-61):** Shift Tab back into the sidebar tree, and the peek.

### 5. Window checks of the Settings sections

The Settings page itself passed your window check on 2026-10-24 (#91,
`docs/decisions.md`); the section checks below are still to do.

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
- **#130 (background playback):** with your own accounts, play a YouTube
  video and a Hulu episode in a background tab for several minutes each,
  through an ad break; the sound must never stop. Then pause one, wait two
  minutes and switch back: the page must still be there, paused where you
  left it (a sleeping tab wakes when shown; nothing in the window marks the
  sleep itself). The native audio suite proves the timing; run it on a
  quiet machine:
  `cargo run --offline --locked --profile local --package syrinx-platform --example page_close -- --audio-only`.
  It plays a faint 30 Hz tone for about 20 seconds (most speakers cannot
  reproduce it) and must print that the tone page stayed awake while hidden
  and then slept about a minute after the tone stopped. Record pass or fail
  and the printed seconds in #130.

### 7. Downloads and see-through themes (PR #145: #143, #151)

PR #145 merged on 2026-10-24; do these once `cargo xtask check` passes on
this machine. Record
each as pass or fail, with the line the fixture prints where one is named,
in #143 (downloads) or #151 (themes); record the two verdicts in
`docs/decisions.md` too.

1. **The downloads fixture.** Run
   `cargo run --package syrinx-platform --example page_close --profile local --offline --locked -- --downloads-only`
   and read four results:
   - WebView2's own download dialog never opened.
   - The self-closing popup's download completed, and the popup then
     closed.
   - Case 4's line, "Closing a popup's web view mid-download kept the
     download" or "... stopped the download". It decides whether a popup
     stays open until its download ends (`docs/decisions.md`, 2026-10-24,
     entry (2)): record which.
   - Case 6's final state and reason for `synthetic-tool.exe`, which shows
     what SmartScreen does while the dialog is hidden.

   Then run the whole fixture once, the same command without
   `--downloads-only`, and record in #143 how long the run took from start
   to finish beside its result (for example `$start = Get-Date`, then the
   command, then `(Get-Date) - $start`, which keeps the fixture's output
   on screen). Run it on a quiet machine, as section 2 describes, and
   straight after the `--downloads-only` run, so `cargo run` rebuilds
   nothing and the time measures the fixture, not the compiler. The whole
   run includes #130's audio suite, which needs that quiet machine, and the
   certificate suite, which starts `python.exe` from the path. The
   fixture ends itself after 600 s in case its message pump is stuck. Its
   suites and shutdowns may use up to 545 s (seven 45 s suites,
   the 150 s audio suite and eight 10 s shutdowns), but creating the
   profiles, the WebView2 environments and the synthetic servers is not in
   that sum, so only a measured run shows how much of the 55 s margin is
   left. If a full run ever trips the 600 s watchdog, the bound is raised;
   a suite is never dropped to fit under it (PR #145's re-review and the
   reply to it, 2026-10-24).
2. **A real Drive download, signed in yourself.** One list only, with
   progress and then "Done"; then pause, resume and cancel a large
   download. While a popup's download runs, switch to another tab for more
   than ten seconds and check the download keeps progressing (a hidden
   popup is suspended about five seconds after its navigation settles, and
   nobody yet knows whether a suspended web view keeps downloading).
3. **A same-tab download link** on any ordinary site: the status line says
   "Downloading <name>." and never "The page could not be loaded. Try
   reloading.", and Settings › Usage report counts no navigation failure for
   it. If either appears, the download navigation is being counted as a
   failed page load; report it on #143.
4. **Close a downloading popup with Ctrl W**, then let the download finish:
   nothing else happens (no tab closes, no message).
5. **A flyout under a screen reader.** With the P1-36 UI Automation probe
   or NVDA attached, open the Downloads flyout with two downloads listed:
   the `local` build does not panic, and each row's buttons are read with
   their labels.
6. **An Acrylic theme over a bright window.** Make a theme with Acrylic,
   Dark, 90 % and Home See-through, with a bright window behind Syrinx: the
   title bar, sidebar and Home show that window frosted, web pages stay
   solid, and the selected tab's title and Home's text are readable. Switch
   to Light: the frost turns light and Home stays readable.
7. **The dark-Acrylic surface floor.** Under dark Acrylic above about 60 %,
   the selected row, fields, the search pill and the Solid Home card keep a
   0.343 backing (`DARK_ACRYLIC_SURFACE_FLOOR`). Accept it or say what to
   change.
8. **The presets.** A saved Haze still shows Home Solid at its canvas card
   tone, and Glass at 50 % looks close enough to the canvas.
9. **The flyout while its rows change.** With it open, start a second
   download and let one finish: each change reopens the flyout at the same
   corner, because its window is sized to its rows. Say whether that flicker
   is acceptable or should become an in-place resize. Also check that it
   hangs under the Downloads button at a narrow and a wide window, since its
   corner is computed from the title bar's layout rather than read from the
   button.

### 8. Window checks of the 2026-10-24 merges (#76, #85, #51)

On 2026-10-24 you told the coordinating agent to merge the open pull
requests. Three of them took their issue's recommended answer before
anyone had looked at the result in a window. The `docs/decisions.md`
entries that record those answers wait for your confirmation in PR #162.
Record pass or fail, or what to change, in the issue named (a comment on
a closed issue is enough).

1. **A saved Home tab is called "Home" (#76, PR #100).** Since #91,
   Ctrl D refuses Home, so only a Home page saved before 2026-10-23 has
   such a row; if your profile has none, ask an agent for a synthetic
   profile that does. The saved row reads "Home" in the sidebar, in the
   peek and in the palette, both while its tab is open and after you
   close it, and typing "about:blank" in the palette still finds it.
2. **The palette and the peek have rounded corners (#85, PR #103).** In
   Graphite and in Paper, open Ctrl K over a Home tab, then collapse the
   sidebar and open the peek. Look for clean corners and a soft shadow,
   with no black corners and no double shadow. `design/README.md` records
   the look once you say yes.
3. **The restore-backup screen (#51, PR #102).** An agent prepares a
   synthetic profile with a verified backup and a deliberately damaged
   database, never your own; start Syrinx on it with the window visible.
   The screen is drawn in Graphite whatever theme is chosen, shows the
   backup's date in your local time followed by UTC, and offers
   **Restore this backup** and **Cancel and exit**. Say whether its
   wording (listed in PR #102) and its two choices are what you want;
   #51 stays open until you do.

### 9. Housekeeping when convenient

- The repository is public now. The pre-publication checklist in
  `docs/releasing.md` records what was checked on 2026-10-13 and what has
  changed since; anything in it that still matters to you, such as the
  third-party skills under `.claude/skills/` or what the history shows, is
  yours to decide.
