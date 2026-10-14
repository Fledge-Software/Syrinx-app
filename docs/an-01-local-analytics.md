# AN-01: local usage counting

**Status:** implemented on `claude/an-01-local-analytics` (2026-10-14); the
independent review's required changes are made in a second commit, and the
slice awaits re-review, a quiet-machine overhead and file-load run, and an
owner-approved merge. The owner approved the frozen command list as built on
2026-10-14. The owner started AN-01 on 2026-10-14, ahead of P1-39d,
because it needs neither the release keys nor the update host.

## Why this slice exists

D39 lets Syrinx count how it is used, on this computer only, so the owner can
later hand a report to a model and ask what to improve. The approved proposal,
`docs/proposals/an-01-02-local-analytics.md`, fixed what may be counted before
any code existed, because a collector designed call site by call site is how a
"harmless" label ends up carrying an address. AN-01 builds the counting side of
that proposal: the schema, the in-memory recorder, the file that holds two
periods, the switch and Reset, and the calls at every place that already
exists. AN-02 adds what the owner sees of a report and the two ways it can
leave the machine (Save report… and Copy report); none of that is built here.

If this slice were skipped, P1-40's week of real use would produce no report,
and a later collector would be retrofitted into finished code.

## What was built

### The schema and recorder (`crates/syrinx-core/src/usage*`)

- `usage/schema.rs` is version 1 of the schema exactly as approved: 35 feature
  counters, one counter per built-in command plus `command.other`, eight timing
  histograms on the two approved bucket scales, and 13 error categories. Every
  field is an enum variant; there is no function that takes a name, so no call
  site and no plugin can invent a field.
- The command counters are the registry's 20 built-in commands, frozen as
  `COMMAND_KEYS`. `command_index` matches every `BrowserAction`, so a new
  built-in command fails to compile until the schema is changed, and a test
  checks that the frozen keys equal `command.` plus each registry identifier.
  **The owner approved this list as built on 2026-10-14** (the proposal's
  done condition):
  `browser.new-tab`, `close-tab`, `reopen-tab`, `reload`, `back`, `forward`,
  `edit-address`, `next-tab`, `previous-tab`, `save-page`, `show-history`,
  `find`, `downloads`, `print`, `zoom-in`, `zoom-out`, `zoom-reset`,
  `clear-data`, `install-update`, `check-updates`, and `command.other`.
- `usage/recorder.rs` holds one fixed array of 159 relaxed `AtomicU32`
  counters (well under 1 KiB). Recording is one saturating atomic increment,
  with no lock, allocation, system call or string. While switched off, every
  call returns after one flag read and writes nothing. Two `compile_fail` doc
  tests prove that passing a `&str` or `String` to the recorder is a type
  error.
- `usage/period.rs` keeps a local calendar date (`UsageDate`, day precision
  only), a period (start day, the last day Syrinx started in it, days used,
  counts) and the file's two periods. A start on or after day 30 closes the
  current period, which replaces (deletes) the previous one, so at most about
  60 days are ever kept. A completed period records the day it actually closed
  (`closed_on`, the first start on or after day 30) and reports that as its
  `period_end`, as the proposal says; the current period has none, because
  its end is the day of export.
- `usage/codec.rs` writes `usage.json` and a report in one fixed JSON shape
  (UTF-8, LF, two-space indentation, final newline, every field present,
  zeros included) and reads back only that shape with a strict reader of its
  own: no arrays, escapes, signs or extra keys, a 32 KiB limit, and a version
  check before anything else is trusted. No dependency was added; `serde_json`
  is not linked into `syrinx-core`. `report()` produces the exact report text
  AN-02 will show and export; AN-01 uses it only in tests.

### The file and the controls (`crates/syrinx-app/src/session/usage.rs`)

- The file lives in the `State` folder beside `syrinx.db`, behind the same
  D13 synced-folder guard as the database. Its existence is what "on" means:
  switching off deletes it (and any set-aside damaged copy), so there is no
  state in which counting is off but counts linger.
- Switching off is durable before anything is deleted: the worker stops the
  recorder, writes and flushes an empty `usage.off` marker, deletes the usage
  files, and removes the marker last. If a file cannot be deleted (another
  program holds it), counting is already off, the window says so, and every
  later start sees the marker, stays off and retries the deletion. Without the
  marker a failed delete would have left `usage.json` behind and counting
  would have resumed at the next start. Reset and switching on finish any
  such leftover deletion first; a Reset that cannot delete the old counts
  leaves counting off rather than bringing them back.
- Only the session worker reads or writes it. Each write goes to
  `usage.json.new`, is flushed, then renamed over the file. Writes happen at
  most once every five minutes and only after a counter changed, on the
  worker's existing wake-ups (it adds none, which keeps D37's idle bar), and
  once at a clean exit, including after a storage failure.
- Backups copy only the database (`VACUUM INTO`), so the usage file is never
  in a snapshot and a restore never brings counts back (D47's reasoning).
- A damaged, truncated, oversized or other-version file is renamed to
  `usage.json.damaged` (replacing an older one), counting starts afresh and is
  written at once so it stays on, `error.usage_file_reset` is counted once,
  and the owner sees a non-fatal notice. Start-up never fails because of it.
- Today's date comes from Windows' local calendar. If Windows cannot report it,
  the error code (never a path) is logged and the UTC day is used, at most a
  day off. Only with no system clock at all is there no date: then an
  existing file keeps counting as it is (no new day, no rotation) and says so,
  rather than showing "off" while the file stays; a damaged file is still set
  aside, and switching on is refused with a message.
- Every status the worker reports carries its real state together with any
  problem (`SessionEvent::Usage { status, problem }`), so a failed Reset or
  switch-off can never leave "Counting since" on screen.

### What the owner sees (`crates/syrinx-app/src/ui/usage_report.rs`)

Customize gains a **Usage report** section between Site permissions and
Updates, with the approved switch text, "Count how I use Syrinx, on this
computer only". Its note is shortened, by the owner's decision of 2026-10-14
(`docs/decisions.md`), to "Nothing is sent anywhere. You can delete the
counts at any time." until AN-02 adds viewing and exporting and restores the
approved sentence. The switch is off by default. Switching on
starts a period today and shows "Counting since YYYY-MM-DD. Periods last 30
days." with a **Reset** button. Switching off and Reset each ask first ("Turn
off and delete" / "Reset counts", or Cancel). The switch reuses Customize's
existing switch, now shared as `customize::switch_control`.

### Where each field is recorded

| Field | Recorded at |
|---|---|
| `app.started`, `error.unclean_shutdown`, `error.storage_open`, `error.recovery_offered` | The worker, once the usage file is loaded at start-up |
| `tab.created`, `tab.closed`, `tab.reopened`, `saved.saved`, `saved.removed`, `saved.opened_unloaded`, `folder.created`, `folder.removed`, `import.completed` | The worker, after a command is accepted (a tab-opening command only if it opened a tab, any other only if it changed the model) |
| `tab.switched_sidebar` | A sidebar or peek row activation |
| `tab.switched_keyboard` | Ctrl Tab or Ctrl Shift Tab pressed with more than one tab open; the palette's Next and Previous tab commands count only as commands |
| `palette.opened_ctrl_k`, `palette.opened_ctrl_l` | The Ctrl K and Ctrl L keys, from the chrome, a page or the peek; a key that only refocuses an open palette, and the palette's Edit address command, are not counted |
| `palette.dismissed`, `palette.chose_*` | How each palette entry ended; submitted text counts by where it goes: an address is `chose_address`, a search (Alt Enter, or plain Enter on words) `chose_web_search`, and refused text neither |
| `palette.scope_changed` | A different scope chip chosen |
| `command.*` | Commands run from the palette; `>import bookmarks` and plugin commands count as `command.other` |
| `nav.back`, `nav.forward`, `nav.reload` | The shared handler of the toolbar, keys and palette commands |
| `sidebar.set_*`, `sidebar.peek_shown` | A sidebar state that settled on a new value; a peek popup installed |
| `home.tile_opened`, `favorite.opened` | A Quick access or Favorites tile opened |
| `find.opened`, `settings.opened` | A new find bar; Customize opened |
| `startup.first_frame_ms` | `main` to the window's first render; not recorded on a start where a recovery or history-repair prompt waited on the owner |
| `startup.session_restored_ms` | `main` to the first session state reaching the window; skipped the same way |
| `palette.open_to_paint_ms` | The palette entry starting to its first presented frame |
| `search.tier1_ms`, `store.save_ms`, `backup.snapshot_ms` | The worker's query, durable save and daily snapshot |
| `tab.switch_to_visible_ms` | A tab selection to the selected tab's page being shown. The timer keeps the tab it waits for (named by the request, or learned from the next state) and ignores other pages; a selection that changes nothing (the same tab, Ctrl Tab with one tab) or ends on Home is dropped |
| `error.storage_write`, `error.backup_failed` | A failed durable save or daily snapshot |
| `error.renderer_failed`, `error.browser_process_exited`, `error.webview_creation`, `error.navigation_failed` | The page host's crash, engine-recovery, creation-failure and failed-navigation events; a navigation that failed only because it was stopped or replaced (WebView2's `OperationCanceled`, now reported as `cancelled` on `NavigationCompleted`) is not counted |
| `error.import_refused` | A bookmark file the parser refused |
| `error.usage_file_reset` | A damaged usage file set aside |

Five fields stay zero because what they count does not exist yet or cannot be
recorded: `palette.chose_page_text`, `search.tier2_ms` and
`page_text.captured`/`error.page_text_capture` wait for P1-24/P1-26 (no
full-text results or captures exist), and `error.storage_location_refused`
cannot be recorded, because the refused folder is the one that would hold the
count.

Timings and the start-up facts are measured only while counting is on. While
off, the worker's only usage work on each existing wake-up is one check that
no write is waiting; it reads no clock for usage.

## Tests

All tests use synthetic counts, reserved `.test` addresses and generated
folders; no real usage was collected.

- `crates/syrinx-core/src/usage/tests.rs` (11 tests) and two `compile_fail`
  doc tests: slot order and size, the frozen command list against the
  registry, a report whose key paths equal a literal golden list of every
  approved key string (copied from the proposal, not generated from the code
  under test) with only the four envelope text fields not numbers, bucket edges, disabled recording,
  saturation at the maximum, 30-day rotation with at most two periods, a
  round trip under 32 KiB even with every counter at its maximum, every
  truncation and a list of damaged variants refused without a panic, and
  calendar dates.
- `crates/syrinx-app/src/session/usage.rs` (6 tests): with no file nothing is
  counted or armed however long Syrinx idles; a write waits for a change and
  five minutes and goes through the replacement file; switching on without a
  date changes nothing; an existing file keeps counting without a date; a
  switch-off while another handle holds `usage.json` without delete sharing
  stays off, stays off at the next start while still held, and finishes the
  deletion once released; a Reset that cannot delete reports the off status it
  left.
- `crates/syrinx-app/src/ui/usage_report.rs` (5 tests): only the selected
  tab's page ends a switch timing; a switch learns its target from the state;
  a switch that changes nothing or ends on Home is dropped; nothing is timed
  while off; a start that waited on a prompt records no start-up timings.
- `crates/syrinx-app/src/session/usage_tests.rs` (6 tests, real
  session workers on disposable storage): counting off writes nothing; on
  counts every worker call site into the file and off deletes it; a run with
  private-looking addresses, a note body and a path leaves no five-character
  fragment of them in the file or its report (this drives the worker's call
  sites; the window's call sites cannot carry text at all, which the
  recorder's type and its `compile_fail` tests guarantee); Reset deletes both
  periods; a
  damaged file is set aside, counted once and counting continues; no backup
  holds the usage file and a confirmed restore leaves its counts in place
  while recording `error.storage_open` and `error.recovery_offered`.
- `crates/syrinx-platform/src/calendar.rs`: the local date reads as ISO digits.
- The smoke run (`--smoke`, always a fresh profile with counting off) now
  fails if anything was counted or a usage file exists when it ends.
- `cargo xtask bench` now also measures one update's cost for a counter, a
  timing and a switched-off call, and fails at a 99th percentile of 100 ns or
  more. The 99th percentile is taken over samples that are each the mean of
  16 consecutive updates (200,000 samples), because one update is shorter than
  the clock's own cost; it is not a percentile of single updates. It also
  times loading the largest possible file (both periods, every counter at its
  maximum): read, decode and one start, 2,000 runs, failing at a 95th
  percentile of 1 ms or more.

## What remains

- **AN-02**: the preview, Save report…, Copy report, the suggested prompt,
  `docs/usage-report.md`, and restoring the switch's approved note. The plan
  puts it before P1-40.
- The overhead and file-load figures below come from a busy machine and are
  provisional; record a quiet-machine run before review.
- The owner's visual check of the Usage report section and its confirmations.

## Verification on this machine (2026-10-14)

- `cargo xtask check` passed with 1,328 test results on the first commit and,
  after the review fixes, with 1,336 (summed over every `test result` line,
  including the release-tool steps), none failed or ignored, and the smoke
  run passed with its new no-counting check.
- After the review fixes, `cargo xtask bench` (busy machine; provisional
  until a quiet-machine run): update p99s of 12.5 ns per counter, 12.5 ns per
  timing and 6.25 ns switched off; loading the largest file took 0.173 ms at
  the 95th percentile, under the 1 ms budget.
- `cargo xtask bench`, run while other sessions' gates were building: the
  usage recorder's 99th percentiles were 18.75 ns per counter update, 25.0 ns
  per timing update and 12.5 ns per switched-off call, all under the 100 ns
  budget. The same run's search benchmark missed its 5 ms p95 on four cases;
  that benchmark is D48's, unchanged here, and the machine was not quiet.
- **Window check.** `target\local\syrinx.exe` was launched visibly with a fresh
  `--test-data-dir` under the session's scratch folder. A UI Automation
  client found the Customize button and the switch by name; Customize was
  opened and the switch pressed by clicks posted only to Syrinx's own window
  after confirming, by process id, that it was in the foreground, and the
  panel was scrolled the same way. Captures showed: the Usage report section
  between Site permissions and Updates with the approved text and the switch
  off, and no `State\usage.json`; after pressing the switch, the switch on,
  "Counting since 2026-10-14. Periods last 30 days." with Reset, and the file
  present; pressing it again showed "Turn off counting? This deletes all
  usage counts on this computer." with "Turn off and delete" and Cancel;
  confirming (through UI Automation's Invoke) turned the switch off and
  deleted the file. Syrinx closed cleanly (exit code 0). This is not the
  owner's visual acceptance. UI Automation's Toggle on the switch did nothing,
  as for Customize's existing Reduce transparency switch; GPUI does not map
  Toggle to a click. The check was repeated after the review fixes on a fresh
  profile with the same results; the note now reads "Nothing is sent
  anywhere. You can delete the counts at any time."
