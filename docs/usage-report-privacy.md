---
status: approved by the owner, 2026-10-12
slices: AN-01 (local aggregate counters), AN-02 (report preview and export)
decisions: D39; D13, D37, D40 and D47 constrain it
date: 2026-10-12
---

# AN-01 and AN-02: Syrinx's local usage report

## Why this proposal exists

On 2026-10-10 the owner replaced the blanket "no telemetry" rule with D39:
Syrinx may count how it is used, on this computer only, so that the owner can
later hand a report to a model and get suggestions for improving Syrinx. D39
also said what that may not include: no account or device identifiers, no
addresses, searches, file paths or document text, no detailed timeline of
events, and no automatic upload. And it said that nothing is collected until
the owner has reviewed a concrete schema, the on/off, reset and retention
behaviour, and the slices that build it.

The readiness report sketched two slices, AN-01 and AN-02, and a sample
shape "for review, not a finalized schema". Plan 1's Issue map says these
sit "outside this approved map" until the owner approves their boundaries.
This document is the concrete proposal those notes ask for. It writes no
code and collects nothing.

If this step were skipped, the first implementation would decide what
counts as private by itself, one call site at a time. That is how a
"harmless" free-form label or error string ends up carrying an address.

## Terms used here

- **Counter**: a number that only goes up, such as "palette opened".
- **Timing histogram**: for one measured operation, a fixed row of counters,
  one per time range (bucket). Syrinx adds one to the bucket the measurement
  falls in and keeps nothing else: not the measurement, not when it happened.
- **Error category**: a counter for a fixed kind of failure, such as
  "storage write failed". Never an error message, code or path.
- **Period**: the span of days the counters cover, from a start date to now.
- **Report**: the counters of one period, written as a JSON file only when
  the owner presses Export or Copy.

## Principles the schema follows

1. **Every field is compiled in.** The schema is a set of Rust enums. There
   is no API that takes a string name, so no call site can invent a field,
   and no plugin can add one. A new field means a new schema version, which
   goes back to the owner.
2. **Counters are updated in place; no events are kept.** There is no queue
   of events "to aggregate later". The only thing ever stored is the current
   number in each fixed slot.
3. **Nothing describes content or identity.** No addresses, titles, search
   text, note or code text, paths, command names typed by the owner, plugin
   names, error strings or codes, account or device identifiers, install or
   session identifiers, or hashes of any of these.
4. **Nothing profiles sensitive behaviour, even as a count.** For example,
   there is no counter of "pages skipped because they were banking sites":
   that would be a monthly count of the owner's banking visits. Counts are
   about Syrinx's features, not about which parts of the owner's life they
   touched.
5. **Off is truly off.** When collection is off, the recording calls do
   nothing and no file exists.

## The fixed schema (version 1)

Every field below is in every report, zeros included, so a report's shape
never varies with what the owner did. Names are `area.event`. Each field is
justified by the question it helps answer; anything that could not be
justified was left out (see "Rejected fields").

### Report envelope

| Field | Type | Why |
|---|---|---|
| `schema_version` | integer, `1` | Lets a later Syrinx or a model read old reports correctly. |
| `syrinx_version` | string, Syrinx's own version (`0.1.0`) | Ties numbers to a build, so a regression can be traced to a change. Not an identifier: every copy of a build has the same one. |
| `period_start` | date, `YYYY-MM-DD`, local calendar | Needed to read counts as rates. Day precision only. |
| `period_end` | date, `YYYY-MM-DD` | The day of export, or the day a completed period closed. |
| `days_used` | integer | Days in the period on which Syrinx started at least once; turns totals into per-day rates. Syrinx stores only the last day it counted, never a list of days. |
| `about` | fixed text compiled into Syrinx | Explains the units and bucket edges to the reader or model. Identical in every report. |

### Feature counters

| Field | Counts | Question it answers |
|---|---|---|
| `app.started` | Syrinx launches | Baseline for every rate. |
| `tab.created` | New ordinary tabs (Ctrl T, new-window adoption, palette) | How tab-heavy is use? |
| `tab.closed` | Tab closures | |
| `tab.reopened` | Ctrl Shift T reopenings (D42) | Is the 20-entry stack used, and is it enough? |
| `tab.switched_sidebar` | Tab selections by clicking the sidebar | Mouse versus keyboard navigation. |
| `tab.switched_keyboard` | Tab selections by Ctrl Tab | |
| `saved.saved` | Save (pin) actions (D43) | Is saving used? |
| `saved.removed` | Remove or Unpin | |
| `saved.opened_unloaded` | Opening a saved row whose page was unloaded | Does unloading save memory without annoying? |
| `folder.created` | New folders (P1-21) | |
| `folder.removed` | Folder removals | |
| `import.completed` | Bookmark imports finished (P1-22), not entries | Is import a one-off? |
| `palette.opened_ctrl_k` | Palette opened with Ctrl K | Main palette entry point. |
| `palette.opened_ctrl_l` | Palette opened with Ctrl L | Address-entry use. |
| `palette.dismissed` | Palette closed without running anything (Esc or click away) | Does the palette fail to find things? |
| `palette.chose_tab` | Chose an open-tab result | Which result kinds matter, to tune ranking. |
| `palette.chose_saved` | Chose a saved page | |
| `palette.chose_history` | Chose a history entry | |
| `palette.chose_page_text` | Chose a tier 2 full-text result | Is D18's recall used enough to justify its cost? |
| `palette.chose_command` | Ran a command | |
| `palette.chose_address` | Navigated to a typed address | |
| `palette.chose_web_search` | Used Search the web (Alt Enter) | |
| `palette.scope_changed` | Scope-chip changes | Are the chips useful? |
| `nav.back` | Back | Toolbar and shortcut use. |
| `nav.forward` | Forward | |
| `nav.reload` | Reload | |
| `sidebar.set_wide` | Sidebar settled at wide | Which D44 width states are used. |
| `sidebar.set_compact` | Settled at favicon-only | |
| `sidebar.set_hidden` | Settled at hidden | |
| `sidebar.peek_shown` | Hover-peek shown (P1-17) | Does peek trigger by accident? |
| `home.tile_opened` | Quick access tile opened (P1-28) | |
| `favorite.opened` | Sidebar Favorite opened (D45) | |
| `find.opened` | Syrinx's own find opened (Ctrl K route, P1-13) | |
| `page_text.captured` | Pages captured for full text (P1-26) | Cost against `palette.chose_page_text`. One total only; skips are not split by reason (principle 4). |
| `settings.opened` | Settings or Customize opened | |

### Command counters

Each built-in command in the command registry (P1-30) has one compiled-in
counter, `command.<id>`, where `<id>` is the command's fixed internal
identifier (for example `command.tab.duplicate`). The list is generated from
the registry's built-in table at build time and frozen into the schema
version, so adding a command is a schema change. Any other command, such as
a later third-party plugin's, is counted only in `command.other`, never by
name. The full list is attached to AN-01's review, because the registry does
not exist yet.

### Timing histograms

Two fixed bucket scales are used. Each bucket's name is its upper edge in
milliseconds, and the last bucket counts everything above the previous edge.

- **Interaction scale** (12 buckets): `le_1`, `le_2`, `le_5`, `le_8`,
  `le_16`, `le_33`, `le_50`, `le_100`, `le_250`, `le_500`, `le_1000`,
  `gt_1000`. 5 ms is D48's search target; 16 and 33 ms are one and two
  frames at 60 Hz, the palette's one-frame target.
- **Start-up scale** (10 buckets): `le_100`, `le_250`, `le_500`, `le_750`,
  `le_1000`, `le_1500`, `le_2000`, `le_3000`, `le_5000`, `gt_5000`. D6's
  start-up comparison sits in this range.

| Field | Scale | Measures | Why |
|---|---|---|---|
| `startup.first_frame_ms` | Start-up | Process start to Syrinx's first painted frame | D6 start-up in real use. |
| `startup.session_restored_ms` | Start-up | Process start to restored tabs listed (P1-19) | Whether restore slows start-up. |
| `palette.open_to_paint_ms` | Interaction | Ctrl K to palette painted | Phase 4's one-frame target. |
| `search.tier1_ms` | Interaction | One keystroke's complete tier 1 ranking (D48's path) | D48 in daily use, not only the benchmark. |
| `search.tier2_ms` | Interaction | One full-text query | P1-24's store in real use. |
| `tab.switch_to_visible_ms` | Interaction | Tab selection to page shown | Suspension (P1-15) cost. |
| `store.save_ms` | Interaction | One session-worker write | Storage stalls. |
| `backup.snapshot_ms` | Start-up | One daily snapshot (D13) | Whether backups disturb use. |

### Error categories

| Field | Counts |
|---|---|
| `error.storage_open` | The database could not be opened |
| `error.storage_write` | A durable write failed |
| `error.storage_location_refused` | The data folder was refused (synced folder, D13) |
| `error.backup_failed` | A daily snapshot failed |
| `error.recovery_offered` | Syrinx offered a backup restore (D46) |
| `error.unclean_shutdown` | The previous run did not exit cleanly |
| `error.renderer_failed` | A page's renderer process failed (P1-16) |
| `error.browser_process_exited` | The shared WebView2 browser process exited |
| `error.webview_creation` | A page web view could not be created |
| `error.navigation_failed` | A top-level navigation failed (network or engine) |
| `error.import_refused` | A bookmark file was refused (P1-22) |
| `error.page_text_capture` | A page-text capture failed after being judged eligible |
| `error.usage_file_reset` | The usage file was unreadable and was started afresh |

### Rejected fields

| Proposed field | Why it is left out |
|---|---|
| Install, device, session or account identifiers, or hashes of them | D39. One person's hashed ID is still an identifier. |
| Timestamps of individual events, or a per-day breakdown | D39's "no detailed timeline". `days_used` is one number. |
| Windows build, WebView2 runtime version, screen size, locale | Not identifiers, but they add fingerprinting detail and the owner knows them; they can be added by hand when sharing. |
| Error messages or HRESULT codes | Messages can contain paths and addresses; codes invite free-form growth. Categories are enough to point at an area. |
| Per-site, per-domain or per-category page counts | Content-adjacent (principle 4). |
| Counts of typed characters, query lengths or result positions | Edges towards reconstructing what was searched for. |
| Names of plugins or user-defined commands | User-chosen text. |

## Collection controls

**Where.** Settings, in a "Usage report" section, beside the privacy settings.

**Default: off.** Nothing is counted until the owner turns it on. The
switch reads "Count how I use Syrinx, on this computer only", with one line
under it: "Nothing is sent anywhere. You can view, export or delete the
counts at any time."

**Turning it on** creates the usage file and starts a period on that day.

**Turning it off** stops counting immediately and deletes the usage file,
after a confirmation that says so. Off means no data on disk, so there is
no state where counting is off but old counts linger unnoticed. (The
alternative, keeping counts while paused, is rejected because it leaves a
file the owner may forget.)

**Reset** clears all counters, deletes the previous completed period and
starts a new period today, after a confirmation.

**View report** opens the preview described under AN-02.

## Retention

- **A period lasts at most 30 days.** On the first start after 30 days, the
  current period becomes the *previous* period, and a new one begins.
- **Syrinx keeps at most two periods**: the current one and the previous one.
  Starting a new period deletes the period before the previous one. The
  longest anything is kept is therefore about 60 days.
- 30 days was chosen because a month gives enough use to see patterns and
  matches how the owner is likely to review it. D40's seven days is a rule
  for page text and is deliberately not reused here, as the readiness report
  asked.
- **Backups.** The usage file is not part of D13's daily snapshots and is
  never restored from them, so a restore can never bring back counts the
  owner reset or deleted (the same reasoning as D47). Losing usage counts in
  a disaster is harmless.
- **Location.** The file lives in Syrinx's local (not roaming) app-data
  folder beside the database, behind the same synced-folder guard (D13).

## Storage and overhead

**In memory.** One fixed array of 32-bit atomic counters, indexed by the
schema enums: well under 4 KiB for version 1. Recording is one relaxed
atomic increment, saturating at the maximum rather than wrapping. A timing
is bucketed with a few comparisons and one increment. No lock, no
allocation, no system call, no string.

**When off.** The recorder holds one flag read at each call; when off it
returns immediately and the array is never written. No file is created and
no timer is armed.

**On disk.** A single small JSON file (`usage.json`, under 32 KiB) holding
both periods, rewritten whole through a temporary file and an atomic replace.
It is written by the existing session worker, never on the interface thread:
at most once every five minutes, and only if a counter changed, plus once at
clean shutdown. A crash loses at most five minutes of counts.

**Idle cost.** The flush timer is armed only after a counter changes, so an
idle Syrinx wakes up zero extra times. This protects D37's 0.5% idle-CPU bar.

**Budget, checked by AN-01's tests:**

| Measure | Budget |
|---|---|
| One counter or timing update, on the calling thread | 99th percentile under 100 ns on this machine |
| Extra memory | Under 16 KiB |
| Extra idle wake-ups | Zero |
| Disk writes | At most one per five minutes while in use, under 32 KiB each |
| Start-up cost of loading the file | Under 1 ms at the 95th percentile |
| A malformed or oversized usage file | Refused, renamed aside and counted once in `error.usage_file_reset`; never crashes Syrinx |

## Report preview and export (AN-02)

**Preview.** "View report" shows the selected period (current or previous)
as a readable table, grouped as above, with a "Show exact file" switch that
displays the precise text that would be exported. The table is generated
from the same bytes, so what the owner reads is what leaves.

**Export.** Two explicit buttons:

- **Save report…** opens the standard Windows save dialog with the
  suggested name `syrinx-usage-report-<period_start>-to-<period_end>.json`.
  The owner chooses the folder; Syrinx remembers nothing about it.
- **Copy report** puts the same text on the clipboard, for pasting into a
  model chat.

Nothing is exported, written outside Syrinx's folder or put on the clipboard
in any other way. Syrinx makes no network call for this feature, now or in
AN-02.

**Exact file format.**

- UTF-8 without a byte-order mark, LF line endings, two-space indentation,
  and a final newline.
- Keys in the fixed order of the schema; every field present, zeros
  included.
- No other content: no file metadata, no comments, no path, no user name.
  (The file system will record a creation time, as for any file; Syrinx
  adds none.)

A sample with synthetic numbers (abbreviated where marked; the real file
lists every field):

```json
{
  "schema_version": 1,
  "syrinx_version": "0.1.0",
  "period_start": "2026-11-02",
  "period_end": "2026-11-30",
  "days_used": 24,
  "about": "Aggregate counts from Syrinx, kept on the owner's computer. Counters are totals for the period. Timing buckets are named by their upper edge in milliseconds; gt_N counts everything above N. No addresses, searches, text, paths or identifiers are recorded.",
  "counts": {
    "app.started": 31,
    "tab.created": 412,
    "tab.closed": 398,
    "tab.reopened": 17,
    "palette.opened_ctrl_k": 655,
    "palette.dismissed": 140,
    "palette.chose_tab": 201,
    "palette.chose_page_text": 9,
    "sidebar.set_compact": 4,
    "page_text.captured": 1873,
    "...": "every other feature counter, zeros included"
  },
  "commands": {
    "command.tab.duplicate": 3,
    "command.other": 0,
    "...": "every built-in command"
  },
  "timings": {
    "search.tier1_ms": {
      "le_1": 2104, "le_2": 611, "le_5": 140, "le_8": 22, "le_16": 6,
      "le_33": 1, "le_50": 0, "le_100": 0, "le_250": 0, "le_500": 0,
      "le_1000": 0, "gt_1000": 0
    },
    "startup.first_frame_ms": {
      "le_100": 0, "le_250": 3, "le_500": 22, "le_750": 5, "le_1000": 1,
      "le_1500": 0, "le_2000": 0, "le_3000": 0, "le_5000": 0, "gt_5000": 0
    },
    "...": "every other histogram"
  },
  "errors": {
    "error.renderer_failed": 1,
    "error.unclean_shutdown": 0,
    "...": "every other category"
  }
}
```

(The `"..."` entries exist only in this document to shorten the sample; a
real report never contains them.)

## How a model gets the report

Nothing is integrated. The owner exports or copies the report and pastes it
into whichever model chat they choose. Syrinx has no model provider, API key,
upload, background analysis or automatic interface change, and none is
proposed. The preview shows a suggested prompt the owner can copy with it:

> This is a usage report from Syrinx, a desktop browser I use. It contains
> only aggregate counts and timing buckets for one period, described in its
> "about" field. Suggest up to five changes that might make Syrinx faster or
> easier for me, each tied to the numbers that support it. Say what each
> number cannot tell you, and do not assume why I did something from how
> often I did it.

The owner reviews any suggestion and decides what becomes work; suggestions
enter the plan like any other request. As D39 records, removing identifiers
does not make a one-person report anonymous: whoever receives it can infer
it came from the owner.

## Implementation slices and their tests

### AN-01: local aggregate counters

**Delivers.** The version 1 schema as enums in a new module of
`syrinx-core` (no window needed, so it is testable headlessly); the
atomic recorder; the usage file with its two periods, rotation and atomic
writes through the session worker; the Settings switch, Reset and their
confirmations; and the recording calls at each call site that exists by then.

**Tests.**

- **No private field can be recorded.** The recorder's public API takes
  only schema enums and durations; there is no function accepting a string,
  path or URL. A `compile_fail` doc test (Rust's built-in way to assert that
  an example does not compile, needing no new dependency) proves that passing a `&str` or `String` to the recorder is a
  type error. A second test serialises a report and asserts that its set of
  keys equals the schema's compiled key list exactly, and that every value
  is a number, apart from the envelope's fixed fields.
- **A private value cannot leak through a known field.** A synthetic run
  drives every call site with generated private-looking values
  (`https://private.example.test/…`, a synthetic note body, a
  `C:\synthetic\path`) and asserts that none of those strings, nor any
  substring longer than four characters of them, appears in the usage file
  or the report.
- **Disabled collection does nothing.** With collection off, a synthetic
  workload drives every call site: every counter stays zero, no usage file
  exists in the fresh synthetic profile, and no flush timer was armed. The
  app's existing smoke run is repeated with the switch off and asserts the
  same.
- **Off deletes, Reset clears.** Turning off deletes the file; Reset
  zeroes both periods and starts today.
- **Rotation and retention.** With an injected clock: a period closes after
  30 days; at most two periods exist; the oldest is deleted.
- **Backups never carry the file**, and a restore of an older snapshot
  leaves the current usage file untouched.
- **Damaged files.** Truncated, oversized, unknown-version and malformed
  files are renamed aside, counted once, and never crash start-up.
- **Saturation.** Counters stop at the maximum instead of wrapping.
- **Overhead.** A microbenchmark in `xtask bench` checks the 100 ns 99th
  percentile; an idle test confirms no timer is armed without changes.

**Done when** the owner has reviewed the final command list, the full gate
passes, and the overhead budget is met on this machine.

### AN-02: preview and explicit export

**Delivers.** The preview (table and exact text), Save report… and Copy
report, the suggested prompt, and a short `docs/usage-report.md` explaining
for the owner what is in a report and what is not.

**Tests.**

- **The export is the preview.** The bytes written by Save and placed on the
  clipboard are identical to the bytes the preview shows.
- **Exact format.** UTF-8 without a byte-order mark, LF endings and a final
  newline, fixed key order; checked byte for byte, normalising nothing (see
  the line-ending trap in `docs/dev/trapstoknow.md`).
- **No extra metadata.** The exported JSON's key set equals the schema
  exactly; no path, user name or time beyond the two period dates.
- **Nothing leaves without a click.** No code path writes a report outside
  Syrinx's folder or touches the clipboard except from the two buttons; the
  dependency check confirms no network crate was added.
- **Empty and off states.** With collection off, the buttons are disabled
  and nothing can be exported.

**Done when** the owner has looked at the preview in the running app (it is
something the owner sees) and the full gate passes. No real usage report is
created during development: tests use synthetic counters only.

## Where AN-01 and AN-02 go in the plan

The Issue map's 40 rows were approved without analytics, and its note says
AN-01/AN-02 stay "outside this approved map" until the owner approves their
boundaries. This proposal asks for exactly that, as an **amendment that
adds two rows without renumbering any P1 label**:

| ID | Blocked by | Then blocks |
|---|---|---|
| AN-01 | P1-39, and the owner's approval of this proposal | AN-02 |
| AN-02 | AN-01 | P1-40, unless the owner decides to run the week of use without analytics |

Why there:

- **After P1-39.** By then every plan 1 feature exists, so the recording
  calls can be placed once, in final code, instead of being threaded through
  features still being built. Placing AN-01 earlier would mean revisiting
  every later slice to add its call sites.
- **Before P1-40.** The week as the owner's default browser is the first
  real use. If the owner switches counting on, that week produces the first
  report, at the moment it is most useful.
- **Not a blocker if declined.** If the owner chooses not to count, P1-40
  waits only for P1-39, as in the approved map.
- **Parallel work (2026-10-11 decision).** The headless part of AN-01 (the
  enums, recorder, file and tests in `syrinx-core`) depends on no later slice
  and could be built on its own branch in parallel once approved; the call
  sites and the Settings section still wait for P1-39. This proposal does
  not ask for that; it is noted so the option is visible.

If approved, the Issue map gains these two rows in its Phase 5 table, P1-40's
"Blocked by" gains "AN-02 (if analytics is enabled)", and the readiness
report's "Proposed analytics amendment" points here as its concrete schema.

## Questions for the owner

Each is a yes or no. The recommendation is in brackets.

1. Do you approve the version 1 schema above (envelope, feature counters,
   command counters, timing histograms with their bucket edges, and error
   categories), with the rejected fields left out? *(Recommended: yes.)*
2. Should collection be off until you turn it on? *(Recommended: yes.)*
3. Should turning collection off also delete the stored counts, rather than
   keeping them paused? *(Recommended: yes.)*
4. Should counts cover periods of at most 30 days, with only the current and
   previous period kept? *(Recommended: yes.)*
5. Should the usage file be left out of daily backups and never restored
   from them? *(Recommended: yes.)*
6. Is a JSON file, saved through Save report… or copied with Copy report,
   with you pasting it into a model yourself, the right way to share it,
   with no model or upload built into Syrinx? *(Recommended: yes.)*
7. Do you approve adding AN-01 after P1-39 and AN-02 before P1-40, as two
   new rows that renumber nothing, with P1-40 waiting for AN-02 only if you
   choose to count? *(Recommended: yes.)*
