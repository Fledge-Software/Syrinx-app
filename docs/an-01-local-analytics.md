# AN-01: local usage counting

**Status:** implemented on `claude/an-01-local-analytics` (2026-10-14), awaiting
independent review, the owner's review of the frozen command list and an
owner-approved merge. The owner started AN-01 on 2026-10-14, ahead of P1-39d,
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
  **The owner reviews this list** (the proposal makes that a done condition):
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
  60 days are ever kept. A completed period reports the last day of its 30-day
  span as its end.
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
- If Windows cannot report today's date, an existing file is left unread for
  that run and switching on is refused with a message; nothing is guessed.

### What the owner sees (`crates/syrinx-app/src/ui/usage_report.rs`)

Customize gains a **Usage report** section between Site permissions and
Updates, with the approved switch text, "Count how I use Syrinx, on this
computer only", and its note. The switch is off by default. Switching on
starts a period today and shows "Counting since YYYY-MM-DD. Periods last 30
days." with a **Reset** button. Switching off and Reset each ask first ("Turn
off and delete" / "Reset counts", or Cancel). The switch reuses Customize's
existing switch, now shared as `customize::switch_control`.

### Where each field is recorded

| Field | Recorded at |
|---|---|
| `app.started`, `error.unclean_shutdown`, `error.storage_open`, `error.recovery_offered` | The worker, once the usage file is loaded at start-up |
| `tab.created`, `tab.closed`, `tab.reopened`, `tab.switched_keyboard`, `saved.saved`, `saved.removed`, `saved.opened_unloaded`, `folder.created`, `folder.removed`, `import.completed` | The worker, after a command is accepted (a tab-opening command only if it opened a tab, any other only if it changed the model) |
| `tab.switched_sidebar` | A sidebar or peek row activation |
| `palette.opened_ctrl_k`, `palette.opened_ctrl_l` | The Ctrl K and Ctrl L keys, from the chrome, a page or the peek; a key that only refocuses an open palette is not counted |
| `palette.dismissed`, `palette.chose_*` | How each palette entry ended; Alt Enter is `chose_web_search`, any other submitted text `chose_address` |
| `palette.scope_changed` | A different scope chip chosen |
| `command.*` | Commands run from the palette; `>import bookmarks` and plugin commands count as `command.other` |
| `nav.back`, `nav.forward`, `nav.reload` | The shared handler of the toolbar, keys and palette commands |
| `sidebar.set_*`, `sidebar.peek_shown` | A sidebar state that settled on a new value; a peek popup installed |
| `home.tile_opened`, `favorite.opened` | A Quick access or Favorites tile opened |
| `find.opened`, `settings.opened` | A new find bar; Customize opened |
| `startup.first_frame_ms` | `main` to the window's first render |
| `startup.session_restored_ms` | `main` to the first session state reaching the window |
| `palette.open_to_paint_ms` | The palette entry starting to its first presented frame |
| `search.tier1_ms`, `store.save_ms`, `backup.snapshot_ms` | The worker's query, durable save and daily snapshot |
| `tab.switch_to_visible_ms` | A tab selection to its page being shown; a selection that ends on Home is not timed |
| `error.storage_write`, `error.backup_failed` | A failed durable save or daily snapshot |
| `error.renderer_failed`, `error.browser_process_exited`, `error.webview_creation`, `error.navigation_failed` | The page host's crash, engine-recovery, creation-failure and failed-navigation events |
| `error.import_refused` | A bookmark file the parser refused |
| `error.usage_file_reset` | A damaged usage file set aside |

Five fields stay zero because what they count does not exist yet or cannot be
recorded: `palette.chose_page_text`, `search.tier2_ms` and
`page_text.captured`/`error.page_text_capture` wait for P1-24/P1-26 (no
full-text results or captures exist), and `error.storage_location_refused`
cannot be recorded, because the refused folder is the one that would hold the
count.

Timings and the start-up facts are measured only while counting is on; the
worker reads no extra clock otherwise.

## Tests

All tests use synthetic counts, reserved `.test` addresses and generated
folders; no real usage was collected.

- `crates/syrinx-core/src/usage/tests.rs` (11 tests) and two `compile_fail`
  doc tests: slot order and size, the frozen command list against the
  registry, a report whose key set equals the schema's compiled list with only
  the four envelope text fields not numbers, bucket edges, disabled recording,
  saturation at the maximum, 30-day rotation with at most two periods, a
  round trip under 32 KiB even with every counter at its maximum, every
  truncation and a list of damaged variants refused without a panic, and
  calendar dates.
- `crates/syrinx-app/src/session/usage.rs` (3 tests): with no file nothing is
  counted or armed however long Syrinx idles; a write waits for a change and
  five minutes and goes through the replacement file; switching on without a
  date changes nothing.
- `crates/syrinx-app/src/session/usage_tests.rs` (6 tests, real
  session workers on disposable storage): counting off writes nothing; on
  counts every worker call site into the file and off deletes it; a run with
  private-looking addresses, a note body and a path leaves no five-character
  fragment of them in the file or its report; Reset deletes both periods; a
  damaged file is set aside, counted once and counting continues; no backup
  holds the usage file and a confirmed restore leaves its counts in place
  while recording `error.storage_open` and `error.recovery_offered`.
- `crates/syrinx-platform/src/calendar.rs`: the local date reads as ISO digits.
- The smoke run (`--smoke`, always a fresh profile with counting off) now
  fails if anything was counted or a usage file exists when it ends.
- `cargo xtask bench` now also measures one update's cost (the mean of 16
  consecutive updates per sample, 200,000 samples) for a counter, a timing and
  a switched-off call, and fails at a 99th percentile of 100 ns or more.

## What remains

- **The owner reviews the frozen command list above.** Until then AN-01 is
  not done by the proposal's own condition.
- **AN-02**: the preview, Save report…, Copy report, the suggested prompt and
  `docs/usage-report.md`. The approved switch note already says the counts can
  be viewed and exported; that is true only once AN-02 lands, which the plan
  puts before P1-40.
- The start-up cost of loading the file (proposal budget: under 1 ms at the
  95th percentile) is not yet measured, and the overhead figures below come
  from a busy machine; record both on a quiet machine before review.
- The owner's visual check of the Usage report section and its confirmations.

## Verification on this machine (2026-10-14)

- `cargo xtask check` passed with 1,328 test results (summed over every
  `test result` line, including the release-tool steps), none failed or
  ignored, and the smoke run passed with its new no-counting check.
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
  Toggle to a click.
