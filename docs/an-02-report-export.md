# AN-02: usage report preview and explicit export

**Status:** built on `claude/an-02-report-export` (2026-10-14). It stays open
until the owner has looked at the preview in the running app, which is the
approved proposal's done condition for something the owner sees, and until an
independent review approves it and the owner approves the merge.

## Why this slice exists

AN-01 made Syrinx count its own use, on this computer only, while the owner has
counting switched on (D39). Those counts are useful only once they can leave the
machine, and D39 allows that only when the owner explicitly exports them. The
approved proposal, `docs/proposals/an-01-02-local-analytics.md` ("Report preview
and export (AN-02)"), therefore asks for two things before anything leaves: the
owner must be able to read exactly what would leave, and there must be exactly
two ways out, both a click of the owner's. If this slice were skipped, P1-40's
week of use could produce counts nobody could look at, and a later "quick"
export would be designed without the guarantee that what the owner reads is
what leaves.

## What was built

### One text, read back into a table (`crates/syrinx-core/src/usage/`)

- `UsageFile::report_text(period, today)` (`codec.rs`) returns the exact report
  text of the current period (ending today, the day of export) or of the
  previous, completed period (ending on the day it closed); `None` when no
  period has closed yet. It is AN-01's `report()`, unchanged: UTF-8 without a
  byte-order mark, LF line endings, two-space indentation, a final newline,
  keys in schema order and every field present.
- `read_report(text)` (`codec.rs`, types in `report.rs`) turns that text back
  into the table the preview draws. It parses with AN-01's strict reader,
  requires the exact key list of the schema, and then re-renders what it parsed
  and refuses the text unless the result is byte for byte the input. A CR, a
  byte-order mark, other spacing, a missing, extra or reordered key, or
  another schema version is therefore refused rather than shown. This is how
  "the table is generated from the same bytes" is enforced in the product,
  not only in a test.

### The worker builds the text (`crates/syrinx-app/src/session/usage.rs`)

`SessionCommand::UsageReport { request_id, period }` is answered with
`SessionEvent::UsageReport`. The session worker, which already owns the usage
file, merges the counts recorded since the last write into its in-memory file
(they are saved at the next ordinary write, never counted twice), builds the
text and sends it. Building a report reads and writes no file. With counting
off the answer is `UsageError::Off`; asking for the previous period before one
has closed is `UsageError::NoPrevious`. `UsageStatus` now also says whether a
previous period exists, so the window can offer it.

### What the owner sees (`crates/syrinx-app/src/ui/usage_preview.rs`)

- Customize's Usage report section gains **View report**, unavailable (dimmed,
  not a Tab stop, and described as unavailable to screen readers) while
  counting is off. The switch's note is restored to the approved sentence,
  "Nothing is sent anywhere. You can view, export or delete the counts at any
  time."
- View report replaces Customize's choices, as Site permissions does, with a
  back button. It offers **Current period** and **Previous period** (the
  latter only once a period has closed), the report as a grouped table (About
  this report, Features, Commands, Timings, Errors, in the file's order, values
  exactly as written), and a **Show exact file** switch that shows the text
  line by line in the monospace face.
- **Save report…** opens Windows' own save dialog on a dedicated thread (the
  route P1-22's import uses), suggesting
  `syrinx-usage-report-<period_start>-to-<period_end>.json`. Syrinx passes no
  folder, keeps none, and asks Windows not to add the file to its recent items;
  Windows asks before replacing a file. The same thread writes the shown bytes
  and flushes them; a failure is shown under the buttons and names no path.
- **Copy report** puts the same text on the clipboard through GPUI's
  `write_to_clipboard`, then reads the clipboard back and says whether Windows
  took it, because GPUI only logs a failed write.
- The suggested prompt is shown below, with **Copy prompt**, and a line saying
  that Syrinx sends nothing and that a report identifies its sender.
- Save and Copy are unavailable while the report is loading, failed, or a save
  is running, and the view closes if counting is switched off.

No dependency, `windows` feature, vendor patch or network call was added. The
save dialog uses `IFileSaveDialog`, which the platform crate's existing
`Win32_UI_Shell` feature already provides, in `syrinx_platform::file_dialog`
beside the open dialog. GPUI's own `prompt_for_new_path` was considered and
not used: it offers no way to keep the file out of Windows' recent items, and
it logs rather than returns some of its failures.

## Tests

All tests use synthetic counts and synthetic folders; no real usage report was
created.

- `crates/syrinx-core/src/usage/tests.rs`:
  `a_report_is_written_byte_for_byte_in_the_approved_format` compares a
  synthetic report with a second, hand-written rendering built from the
  approved key lists and the proposal's `about` text, byte for byte, and checks
  no byte-order mark, CR or tab, one final newline and two-space indentation;
  `the_preview_table_holds_exactly_the_report_fields` checks that the table's
  envelope and key paths equal the approved schema exactly;
  `the_preview_refuses_any_text_but_the_exact_report` (CRLF, BOM, no final
  newline, other indentation, an extra key, a missing key, another version);
  `a_completed_period_reports_until_the_day_it_closed`.
- `crates/syrinx-app/src/session/usage_tests.rs` (real session workers):
  `a_report_request_answers_with_the_exact_counts_and_writes_nothing` (no file
  in the profile folder is added or changed by a request, unsaved counts are in
  the report, no private fragment leaks, and the merged counts are saved once);
  `there_is_nothing_to_export_while_off_or_before_a_period_closes`;
  `a_completed_period_is_reported_until_the_day_it_closed`.
- `crates/syrinx-app/src/ui/usage_preview.rs`: `the_export_is_the_preview` (the
  exact view's lines, the table, the clipboard item's text and a file written by
  the save path are all the shown bytes, compared without normalising);
  `the_suggested_name_carries_the_two_period_dates`;
  `only_the_latest_request_is_shown`;
  `nothing_is_exportable_while_off_failed_loading_or_saving`;
  `only_the_export_buttons_reach_the_clipboard_or_a_chosen_file`, which scans
  this crate's product code: the clipboard is written in one place, reached
  only from Copy report and Copy prompt, and the save dialog and the write are
  reached only from Save report….
- `crates/syrinx-app/src/ui/usage_report.rs`:
  `turning_counting_off_closes_the_report_and_view_report_needs_counting`.
- The gate's existing dependency check confirms that no network crate was
  added.

## Questions for the owner

- **Copy prompt is a third clipboard button.** The proposal asks for the
  prompt to be shown "with a way to copy it" and also says the two export
  buttons are the only ways a report leaves. Copy prompt copies only the fixed
  prompt, never report content. Recommendation: keep it.
- **Windows may still remember the dialog's folder.** Syrinx passes no folder
  and stores nothing, and the file is kept out of Windows' recent items, but the
  Windows save dialog itself may reopen in the folder last used by Syrinx.
  Stopping that would mean forcing a fixed folder every time, which the
  proposal does not ask for. Recommendation: accept it as Windows' behaviour;
  `docs/usage-report.md` says so.
- **A completed period's report ends on the day it closed**, which is also the
  first day of the current period, as AN-01 decided. Recommendation: keep.

## Verification on this machine (2026-10-14)

- **Window check.** `target\local\syrinx.exe` was launched visibly with a fresh
  `--test-data-dir` under `target\an-02\`. Clicks were posted only after
  confirming, by process id, that Syrinx's window was in the foreground, and
  captures were cropped to the Customize panel. They are in `target\an-02\`:
  `01-section-off.png` (switch off, the restored note, View report dimmed),
  `02-section-on.png` (counting since today, Reset and View report),
  `03-report-table.png` (the table: Current period selected, Previous period
  dimmed, About this report and the first features, all zero),
  `04-exact-file.png` and `05-exact-file-scrolled.png` (Show exact file on:
  the exact text in the monospace face). UI Automation found Save report…,
  Copy report and Copy prompt by name, with Previous period not focusable.
  The bottom of the view (the export buttons and the prompt) was not captured,
  and Save and Copy were not pressed in the running app: Syrinx lost the
  foreground before then, and pressing Copy would have replaced the owner's
  clipboard. Their behaviour is covered by the tests above. This is not the
  owner's visual acceptance.
- **Gate.** `cargo xtask check` passed with 1,400 test results (summed over
  every `test result` line), none failed or ignored, and the smoke run passed.
