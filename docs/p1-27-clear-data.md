# P1-27: clear browsing data

Status: implemented on `claude/p1-27-clear-data` (2026-10-12), awaiting
independent review and the owner's merge approval. The full-text slices
P1-23 to P1-26 are not done, so this slice clears what exists today: Atlas's
history database, the tier-1 search catalogue and WebView2's own profile data.

## Why

Before this slice, "clear history" could not be offered honestly. Atlas keeps
its own copy of every visit (P1-20) beside WebView2's copy, the running search
catalogue holds a row for each remembered address, and the daily backups
(D13) hold older copies of the database. Clearing only WebView2 would leave
Atlas's history searchable; clearing only the database would let a restored
backup bring the history back. D47 settled the last point: Atlas keeps a
local deletion record of cleared date ranges, never addresses or page text,
and restoration must honour it instead of discarding older backups.

## What the owner sees

The toolbar's **History** button (or Ctrl H) opens the history list, whose
header now has **Clear browsing data…**. The dialog takes the page card's
place, the way the history list does, so no page sits under GPUI drawing.

- **What to clear:** Browsing history (on by default), Cookies and site data
  (off by default, because it signs the owner out of Google and most other
  sites) and Cached files.
- **Time range:** last hour (default), last 24 hours, last 7 days, last 4
  weeks, all time.
- **Clear data** runs the clear. The dialog says "Clearing…" and then names
  exactly what was cleared and, separately, every part that failed and why,
  for example: "Cleared Atlas's browsing history (14 visits; 12 sites no
  longer listed) from the last hour. The page engine's visited-page records,
  cookies and site data were not cleared: WebView2 refused (error
  0x80070005)." The visit count covers partial removals too: a site visited
  both inside and outside the range loses only the visits inside it and stays
  listed. The same summary appears in the footer, so it is not lost if the
  dialog was closed meanwhile.
- **No endless "Clearing…".** If the history part cannot be queued, or the
  session worker stops before answering, the history part is reported as not
  cleared ("Local storage is unavailable") instead of waiting. If WebView2
  clears the data but its temporary controller then fails to close, the clear
  is reported as done and the close failure is named separately, with its
  error code.

There is no command registry on this base (the palette's command work is
P1-30/P1-31), so the dialog is not yet a palette command. Registering it
belongs to the palette slices once their registry exists.

## How a clear runs

Two parts run in parallel and report separately:

1. **Atlas's history** (session worker, `crates/atlas-app/src/session/deletions.rs`):
   1. append a *requested* record to the journal and flush it;
   2. `Store::apply_history_deletions` removes the visits in the range and
      addresses left without visits, and records the clear's generation in
      the database in the same transaction;
   3. the running catalogue removes every removed `Target::History` row and
      recomputes every changed one, and the visit recorder forgets its
      retractable visits and removed entries, so a late "navigation failed"
      cannot retract a visit that is already gone;
   4. append a *completed* record and flush it.

   If Atlas stops between steps 1 and 4, the next start finishes the clear
   before anything reads history.
2. **WebView2's data** (`crates/atlas-platform/src/browser/clear_data.rs`):
   a hidden controller on the shared environment reaches the profile every
   page uses and calls `ICoreWebView2Profile2::ClearBrowsingDataInTimeRange`
   (or `ClearBrowsingData` for all time). Browsing history maps to
   `BROWSING_HISTORY`, cookies and site data to `ALL_SITE` (cookies plus every
   kind of site storage) and cached files to `DISK_CACHE`. The controller
   never navigates; no script, IPC handler or user agent touches any page.

WebView2's data is not part of the journal: it is not in Atlas's backups, so
a restore cannot bring it back. A failure there is reported and can be retried.

## The journal and the generation barrier

`atlas.deletions`, beside `atlas.db` and `atlas.lifecycle` in the data
directory, holds P1-04's fixed-size, checksummed records
(`crates/atlas-platform/src/storage/deletions.rs`). The running session holds
it with an exclusive handle; appends are flushed before the caller acts, and
the folder itself is flushed after the file is first created, so the new
directory entry survives a power cut.

- A zero-length file or a torn header (a crash during creation) holds no
  record and is initialised.
- A tail shorter than one record (a crash in the middle of an append) is cut
  off when the journal opens. That append never finished its flush, so no
  caller acted on it: a cut-off request was never applied, and a cut-off
  completion is simply written again by the next start's reconciliation.
- A complete record whose checksum fails is refused, never cut off: it was
  flushed, so something may already have acted on it.

Schema version 5 adds `history_deletion_state`, the newest clear generation
the database has applied, and the index `history_visits_by_time`, so range
clears and retention find visits by time instead of scanning every visit.
The generation is the barrier against a lost journal: every start compares
it with the journal and

- replays any journal clear the database lacks (a restored or older file);
- finishes an interrupted clear;
- stops before opening anything else if the database is ahead of the
  journal, because then the journal has lost records and a later restore
  could resurrect history. The owner is offered the repair below.

Each backup's completion record now carries the generation it holds, read
from the copy's own database, instead of the old fixed zero.

## When the journal cannot be trusted

Two start-up states would otherwise lock the owner out of Atlas for good:
a damaged complete record, and a database ahead of its journal (the journal
was deleted, or replaced by an older copy). Atlas cannot reconstruct the
lost ranges, and guessing could let a backup restore history the owner
cleared. Instead, the start-up card says, in plain words, that Atlas's record
of cleared history is damaged (or missing or older than the data) and that
continuing requires clearing **all** browsing history; saved pages, open
tabs and settings are kept. Nothing is changed unless the owner presses
**Clear all history and continue**; closing Atlas instead leaves every file
as it was.

On confirmation (`DeletionJournal::rebuild`, `session/history_repair.rs`):

1. the damaged journal's bytes are preserved beside it as
   `atlas.deletions.damaged-<time>`, the way D46 keeps damaged databases;
2. the new journal is written to `atlas.deletions.rebuild` and flushed. It
   holds one completed **all-time** clear for every generation up to the
   database's, then one more all-time clear as a *requested* record;
3. the old journal is released, the new file is renamed over it in one
   atomic step, and the folder is flushed;
4. the start-up's normal reconciliation applies the requested all-time clear
   and completes it, so the live database is empty of history before
   anything reads it.

A crash at any step is safe. Before the rename, the damaged journal is still
in place, so the next start offers the repair again, and the next repair
removes the leftover `.rebuild` file first. After the rename, the journal is
complete and valid and its requested clear is still pending, so the next
start finishes it without asking. The first version truncated the journal in
place before writing, which a crash could leave empty, and ran the all-time
clear as a separate step that a crash could skip without anyone noticing.

Why this honours D46 and D47: D47 requires that a restore can never bring
cleared history back. Each rebuilt generation claims all time, a superset of
whatever the lost record cleared, so replaying it onto any backup removes at
least what the owner originally removed. Older backups stay usable as
recovery points for everything else, as D47 intended, rather than being
discarded. D46 requires the owner's confirmation before Atlas destroys
state to recover from damage, and keeps damaged files; the repair asks
first, explains what is lost and keeps the damaged journal. The rejected
alternatives were refusing to start until someone repaired the file by hand
(the original behaviour, which leaves the owner no way back into Atlas) and
silently resetting the journal (which breaks D47).

## Recovery replay

Recovery no longer refuses backups with a nonzero generation. It reads the
journal, refuses (without trying an older copy) if the journal cannot cover
a backup's generation, and on confirmation:

1. copies the backup into a fresh staging bundle and validates it;
2. replays every journal clear after the backup's generation onto the staged
   copy (`atlas_store::replay_history_deletions`, in rollback-journal mode so
   the bundle keeps only its database file), checking first that the copy's
   own generation matches its completion record;
3. rebinds the same file, checks no SQLite side file was left, validates it
   again and only then writes the recovery intent and installs it.

A copy older than schema 5 cannot record a generation; it is still cleared
by the replay, and the normal start replays again after migrating it, which
deletes nothing twice. An interrupted recovery's resume path accepts the
backup's generation, because its stage was replayed before the intent existed.

## History retention (DW-4)

The owner decided on 2026-10-12 that history is kept for 90 days. Without a
bound, every visit stays forever and start-up loads every remembered address
into the search catalogue, so both the database and start-up time grow
without limit (D48 plans for 100,000 rows).

Expiry is an ordinary D47 range clear, not a separate deletion path:

- At start-up, after reconciliation and before the catalogue loads,
  `retention::expire_at_startup` clears `[0, end)`, where `end` is the
  cutoff, or earlier if more than 2,000 visits are older than it
  (`Store::history_expiry_end`). Expired addresses therefore never reach the
  catalogue.
- **The cutoff is 90 days before the earlier of the clock and the newest
  stored visit.** A clock that jumps a year ahead therefore cannot expire the
  most recent 90 days of real browsing; without the cap it would journal a
  clear that also replays onto every backup.
- While the session is idle, the check that also runs the daily backup calls
  `Worker::expire_while_idle`. A backlog left by the bound continues one batch
  per idle check; once nothing has expired, the next expiry waits a day.
- The next due time is saved in the settings table (`history.expiry_due`,
  kept out of the appearance settings handed to the window). A start within
  a day of the last expiry skips it, so restarting many times a day adds
  nothing to the journal. A saved time more than a day ahead, left by a clock
  that moved back, is ignored.
- Each expiry is journaled, applied and completed like an owner's clear, so a
  restored backup replays it and old visits cannot come back. Nothing is
  journaled when nothing has expired.
- A journal refusal during an idle expiry is shown once and retried the next
  day; a storage failure stops the session, as it does for an owner's clear.

The rejected alternatives were a visit-count cap (the owner chose an age),
and unjournaled pruning, which a backup restore would undo.

**Journal growth.** Each clear adds two 48-byte records, and the journal
refuses new clears at 16 MiB, which is about 174,000 clears. At one expiry a
day plus the owner's own clears that is centuries, so compaction (folding old
completed clears together) is not needed and is not registered as deferred
work.

**Measured cost** (2026-10-12, this machine, `local` profile, 100,000
synthetic visits to 10,000 addresses spread over 200 days, one 2,000-visit
batch; a throwaway timing test that is not part of the suite):

| Step | Without the time index | With `history_visits_by_time` |
| --- | --- | --- |
| Find the batch end (`history_expiry_end`) | 13.7 ms | 0.2–0.3 ms |
| Apply the clear (`apply_history_deletions`) | 47 ms | 40–51 ms |
| Load 10,000 addresses for the catalogue | 5 ms | 5–7 ms |

Applying a clear is dominated by recounting every remembered address after
the visits go, not by finding them. Together with the journal's two flushed
records and the saved due time, start-up expiry stays well under a tenth of
a second at D48's scale.

## Known limits

- Open tabs keep their addresses and their back and forward lists, and the
  in-session "reopen closed tab" list is not cleared. Saved pages are
  bookmarks, not history.
- If both the journal and every backup newer than a clear are lost, and the
  database is replaced by an older copy by hand, nothing can detect it.
- Ranges use the computer's clock. If the clock was wrong when a page was
  visited, or is wrong when the owner clears, "last hour" means the last hour
  by the clock's current reading, so a visit stamped in the future or the
  far past can fall outside the chosen range. "All time" is unaffected.
- A retention batch that stops at its 2,000-visit bound leaves older visits
  in the catalogue until the idle checks catch up, a few minutes after the
  first start with a large backlog.
- Visits recorded while the clock was far ahead carry that wrong time. Once
  one exists, retention measures 90 days back from it, so real visits more
  than 90 days older than that wrong visit can expire. The cap protects
  against a jumped clock, not against browsing for a while under one.

## Tests

All on disposable synthetic data with reserved domains:

- store: generation recording, the change report, idempotent replay, the
  version 4 → 5 migration, staged replay on version 5 and version 4 copies,
  mismatch and missing-generation refusals, no side files after replay;
- platform: journal creation, exclusivity, durable appends, refusal of a
  rejected record, empty-file and torn-header initialisation, torn-tail
  truncation, refusal of a damaged complete record until a rebuild that keeps
  its bytes; recovery of
  a damaged primary from a backup with a nonzero generation that replays a
  later clear, and refusal when the journal lacks the backup's clear;
- session: a clear updates search immediately, survives restart, and leaves
  the recorder unable to retract cleared visits; an invalid range is
  refused without stopping the session; a damaged journal and a missing
  journal behind the database each offer the repair, a declined repair
  changes nothing, a confirmed one clears all history for good; a clear whose
  request was journaled before a crash finishes at the next start;
- retention: the bounded expiry end (cutoff, limit, shared oldest second);
  start-up expiry of visits older than 90 days, journaled once, and a second
  start within the day that journals nothing even with a newly expired
  visit; a clock a year ahead that expires nothing; idle expiry in bounded
  batches with the catalogue following, then waiting a day; the version 5
  migration's time index;
- repair crash safety: stopping after the replacement is written leaves the
  damaged journal refused and the repair repeatable; a repeated repair
  removes the leftover file; stopping after the rename leaves the all-time
  clear requested, and the next start applies it without asking;
- dialog: matching spans for both parts, stale replies ignored, partial
  failure wording with WebView2's error code, a close failure reported apart
  from a successful clear, a stopped session ending the wait, cache-only
  clears.

## Owner checks

Use a disposable Atlas data folder if you prefer not to clear your own.

1. Visit a few sites, open History (toolbar or Ctrl H), choose **Clear
   browsing data…**, keep "Browsing history" and "Last hour", and clear. The
   summary names the number of visits and sites; the history list and the palette's
   History results no longer show them.
2. Restart Atlas. The cleared sites stay gone from History and the palette.
3. Tick only "Cached files" and clear: the summary says cached files were
   cleared and you stay signed in to your sites.
4. Tick "Cookies and site data" and clear "Last hour": you are signed out of
   sites you signed in to within that hour. (Expect to sign in to Google
   again yourself afterwards.)
5. Close the dialog while a clear runs: the outcome still appears in the
   footer.
6. Only with a disposable data folder: close Atlas, delete `atlas.deletions`
   from it after clearing at least once, and start Atlas. The start-up card
   asks to clear all browsing history; closing Atlas there changes nothing,
   and confirming opens Atlas with an empty history.
