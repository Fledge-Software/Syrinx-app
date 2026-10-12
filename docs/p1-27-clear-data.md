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
  for example: "Cleared Atlas's browsing history (12 sites) from the last
  hour. The page engine's visited-page records, cookies and site data were
  not cleared: …". The same summary appears in the footer, so it is not lost
  if the dialog was closed meanwhile.

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
it with an exclusive handle; appends are flushed before the caller acts. A
torn tail is refused, never truncated. A zero-length file (a crash during
creation) holds no record and is initialised.

Schema version 5 adds `history_deletion_state`, the newest clear generation
the database has applied. It is the barrier against a lost journal: every
start compares it with the journal and

- replays any journal clear the database lacks (a restored or older file);
- finishes an interrupted clear;
- refuses to start if the database is ahead of the journal, because then the
  journal has lost records and a later restore could resurrect history.

Each backup's completion record now carries the generation it holds, read
from the copy's own database, instead of the old fixed zero.

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

## Known limits

- Open tabs keep their addresses and their back and forward lists, and the
  in-session "reopen closed tab" list is not cleared. Saved pages are
  bookmarks, not history.
- If both the journal and every backup newer than a clear are lost, and the
  database is replaced by an older copy by hand, nothing can detect it.
- History retention (DW-4) is not added: the policy (maximum age or row
  count, and its value) is an owner decision. The pruning machinery is ready:
  a retention clear is a range clear through the same participant.
  Recommendation: keep 90 days, matching Chromium-based browsers.

## Tests

All on disposable synthetic data with reserved domains:

- store: generation recording, the change report, idempotent replay, the
  version 4 → 5 migration, staged replay on version 5 and version 4 copies,
  mismatch and missing-generation refusals, no side files after replay;
- platform: journal creation, exclusivity, durable appends, refusal of a
  rejected record, empty-file initialisation, torn-tail refusal; recovery of
  a damaged primary from a backup with a nonzero generation that replays a
  later clear, and refusal when the journal lacks the backup's clear;
- session: a clear updates search immediately, survives restart, and leaves
  the recorder unable to retract cleared visits; an invalid range is
  refused without stopping the session;
- dialog: matching spans for both parts, stale replies ignored, partial
  failure wording, cache-only clears.

## Owner checks

Use a disposable Atlas data folder if you prefer not to clear your own.

1. Visit a few sites, open History (toolbar or Ctrl H), choose **Clear
   browsing data…**, keep "Browsing history" and "Last hour", and clear. The
   summary names the number of sites; the history list and the palette's
   History results no longer show them.
2. Restart Atlas. The cleared sites stay gone from History and the palette.
3. Tick only "Cached files" and clear: the summary says cached files were
   cleared and you stay signed in to your sites.
4. Tick "Cookies and site data" and clear "Last hour": you are signed out of
   sites you signed in to within that hour. (Expect to sign in to Google
   again yourself afterwards.)
5. Close the dialog while a clear runs: the outcome still appears in the
   footer.
