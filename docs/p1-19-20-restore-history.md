# P1-19 session restore and P1-20 history

Status: **implemented on branch `claude/p1-19-20-restore-history`, awaiting
independent review and the owner's merge approval.** The owner approved
running Plan 1 slices in parallel on 2026-10-11; this branch was cut from the
integration tip `9c90d91`. Gate evidence is at the end of this note.

## Why

P1-19 exists because a browser that loses its tabs when it crashes cannot be
trusted with daily use, and because restoring by loading every saved
destination would make start-up slow and noisy (D42, D43). P1-20 exists
because navigation must become searchable memory: the palette ranks only
local tabs, bookmarks and history (D9), and history starts empty and fills as
the owner browses (D10). Both are prerequisites for P1-21's folder UI and for
P1-27, which must clear history and keep it cleared across backup restores
(D47).

## P1-19: what was already there, and the gaps

Most of session restore already existed in source:

- `atlas-store` commits the whole tab model, saved rows, associations, order,
  selection and identity watermarks in one transaction with a compare-and-save
  revision (`crates/atlas-store/src/records.rs`).
- The session worker (`crates/atlas-app/src/session.rs`, `Worker::restore`)
  loads the committed snapshot at start-up and rebuilds the search catalogue
  from it. Restored tabs are reported with an empty `created` list, so the UI
  does not treat them as new pages.
- The UI (`ui/navigation_intents.rs`) creates a native page only for the
  active tab. Background tabs and unloaded saved rows stay logical until the
  owner selects them, so restore never loads every saved destination.
- The worker tests already covered clean restarts in detail.

What was missing was proof of the two properties the plan names: recovery
after an **unclean** stop, and that start-up loads **only** the active page.

### Crash and restart proof

`crates/atlas-app/src/session/crash_restart_tests.rs` runs a real unclean
stop. The test re-executes its own test binary as a child process (the same
pattern as the smoke watchdog's subprocess test) with a disposable directory
under the system temp folder. The child drives the real session worker through
a script: two saved pages, the second navigated live and then closed so its
row is unloaded, an ordinary tab with a reported title, and a selection that
is not the last tab. After every scripted change is acknowledged, it queues
one more change and immediately calls `std::process::exit`. That skips every
destructor: the store is not shut down, the write-ahead log is not
checkpointed and the clean-shutdown marker is never written, which is what a
killed process leaves behind.

The parent then starts a normal session on the same directory and checks:

- `previous_shutdown` is `Unclean`, so the store ran its `quick_check`;
- the restored model equals the acknowledged state, or the acknowledged state
  plus the whole queued change, never anything in between (expected states
  are computed by replaying the script through the worker's own `apply_model`);
- saved rows keep their order and fixed targets, the unloaded row stays
  unloaded, the open saved row keeps its association, and the ordinary tab
  keeps its title and address;
- closed-tab history is in-session only (D42), so reopen does nothing after
  restart, and opening the unloaded row loads its saved address, not the
  later live one, under a fresh tab identity above the restored watermark;
- a clean stop afterwards marks the next start clean and keeps the new state.

`ui/navigation_intents.rs` gains a test that a restored snapshot with a
background tab and an unloaded saved row asks the page host for the active
page only.

Back and forward lists are not restored; the plan does not promise them.

## P1-20: history

### Storage: schema version 3

**Migration number: version 3** (`user_version = 3`), the next free number on
this base after P1-21's version 2. It follows P1-21's style: it only creates
its own objects, alters no earlier table, runs in one transaction with a
foreign-key check and a hook that re-reads and validates the session, and any
failure rolls back to the intact version 2 file.

| Object | Holds |
| --- | --- |
| `history_urls` | One row per visited address: `id`, `address` (unique), latest `title`, `visit_count`, `last_visited`. |
| `history_visits` | One row per visit: `id`, `url_id` (foreign key), `visited_at`. |
| `history_visits_by_url` | Index on `(url_id, visited_at)` so a clear can recompute totals without scanning every visit per address. |

Times are Unix seconds, the unit of D47's deletion ranges. Both tables use
`AUTOINCREMENT`, so an identity removed by a clear is never reused; a stale
palette result can then fail to open, but can never open a different page.
History starts empty on upgrade (D10). Schema validation now matches indexes
as well as tables exactly. History rows are not loaded or validated as a
whole at start-up (they can be large); SQLite's constraints guard their shape
and each read decodes and checks identities.

`Store` gains `record_visit`, `retitle_history`, `history_entries`,
`recent_visits` and `apply_history_deletions`. Writes go through the existing
single writer thread; reads use the two reader connections. History writes do
not change the session's compare-and-save revision.

Older builds refuse a version 3 database (`FutureSchema`) and skip version 3
backups, as P1-21 arranged; downgrading below this branch is unsupported, as
before.

### Recording: no duplicates from stale or repeated events

`crates/atlas-app/src/session/history.rs` turns page callbacks into visits:

- A visit is recorded when a live **web** tab reports an address different
  from the one it last reported. Opening a tab or submitting an address is
  intent, not a visit; the page's own report of its address is.
- Repeated or replayed address callbacks for the current address are the same
  visit. A title callback updates the address's title once and is skipped when
  unchanged.
- Callbacks for a closed tab are ignored, because the model no longer has the
  tab (tab identities are never reused).
- Restored tabs are seeded with their restored address, so reloading them at
  start-up is not counted as a new visit.
- Only `http://` and `https://` addresses are history; Home (`about:blank`)
  moves the cursor, so leaving and returning is a new visit.

WebView2 reports `SourceChanged` for fragment and history-API navigations too,
so those count as visits. A reload of the same address is not counted. If a
title callback arrives before its page's address callback, the earlier visit
briefly takes the new title until the next title callback corrects it.

A failed history write to storage stops the session like a failed save. An
unindexable row or an unavailable clock is reported and the session continues.

### Search: incremental tier 1 updates through the public API

No `atlas-search` change was needed. The catalogue already had
`Target::History(NonZeroU64)`, `Scope::History`, `Usage { visits, last_used }`
and incremental `upsert`/`remove`. At start-up the worker upserts every
`history_urls` row once; after each visit or retitle it upserts only that
row, with the visit count and last visit feeding the existing frequency and
recency boosts. One row per address keeps repeated visits from flooding the
palette. Activating a history result in the palette opens a new tab at that
address (`SessionCommand::OpenHistory`).

### The history list (Ctrl H)

`crates/atlas-app/src/ui/history_list.rs` shows the 200 most recent visits in
the page card, newest first, each with its title (or address before a title is
known), address and a coarse age. Clicking a row opens it in a new tab. Ctrl H
or the toolbar's History button toggles the list; switching tabs closes it.
While the list is open no page is shown beneath it, because GPUI cannot draw
over a WebView2 child window (see `docs/trapstoknow.md`). Only the newest read
fills the list, so a slow reply cannot overwrite a newer one.

**Limitation:** Ctrl H works while Atlas's own interface has focus. While a
page has focus, shortcuts reach Atlas only through `AcceleratorKeyPressed` in
`atlas-platform/src/browser/page.rs`, which maps a fixed list. Adding Ctrl H
there is a one-line change in the area another agent is rewriting, so it was
left for that work; the toolbar button works in both cases.

### D47: cleared history must not come back

The existing pure contract in `atlas-store/src/deletion.rs` applies directly:
its only category is `History` and its ranges are half-open Unix-second
intervals. `Store::apply_history_deletions` is the history participant: it
removes visits inside each range, drops addresses left without visits and
recomputes the rest. It is idempotent. The test
`cleared_history_stays_cleared_when_an_older_copy_is_restored_and_replayed`
records history, keeps a closed copy of the database as an older recovery
point, clears a range through the journal's request/apply/complete order,
then opens the older copy and replays `DeletionLog::replay_after(0)`. The
cleared visits are gone and the result matches the live database.

What P1-27 still owns: persisting the journal and its generation barrier
outside restored state, a clear-history UI, removing cleared rows from the
running catalogue, clearing WebView2's own data, and calling this participant
on the staged copy during recovery. Until then recovery keeps refusing backups
with a nonzero deletion generation, and this branch adds no clearing UI.

## Tests

All data is synthetic (`example.test` addresses, made-up titles) and every
database is disposable.

- `crates/atlas-store/src/engine/tests/history.rs`: visits merge by address,
  list newest first, keep known titles, retitle without recreating removed
  entries and survive restart without touching the session revision; invalid
  visits are refused; a version 2 database upgrades with its session intact
  and empty history; a failing version 3 migration (conflicting table, or a
  validation-hook failure) rolls back to version 2; damaged version 2 rows are
  refused before migrating, leaving the file byte-identical; the D47 replay
  test above.
- `crates/atlas-app/src/session/history_tests.rs`: recording through the
  worker, with replayed address and title callbacks, Home, a late callback for
  a closed tab, history search, opening a result, restart and the restored
  tab's reload; and a missing entry as a recoverable failure.
- `crates/atlas-app/src/session/crash_restart_tests.rs`: the crash proof
  above, which also checks that the acknowledged visit survives.
- `crates/atlas-app/src/ui/history_list.rs`: stale replies, status text,
  labels and ages.
- `crates/atlas-platform/src/storage/recovery/tests.rs`: the newer-schema
  marker moves to version 4, and the version 1 downgrade fixture also drops
  the history tables and now expects migration to version 3.

One existing worker test, `removed_saved_row_stays_removed_after_live_reopen_stale_callbacks_and_restart`,
now also sees the live navigation as a history row. Its search helper was
updated to require exactly one tab row, no saved row and one history row at
the live address, rather than a total of one.

## Gate

`cargo xtask check` passed on this machine on 2026-10-12 (exit 0): dependency
and deferred-work checks, formatting, Clippy, 538 tests and the smoke launch.
The log is `target/p1-19-20-gate.log`. One earlier warm-up build in this
worktree hit Windows Application Control (os error 4551) on the `rust-i18n`
build script; an unchanged retry built normally. No setting was changed.

The window was not inspected by an agent: the development build is not an
application the screen-control tool can be granted, so the owner checks below
cover what a passing gate cannot.

## Owner checks

1. Run `cargo atlas`, browse a few sites, press Ctrl H (with the sidebar or
   toolbar focused) or click History, and confirm the list shows them newest
   first with titles; click one and confirm it opens in a new tab.
2. Ctrl K, type part of a visited site's name, and confirm the history result
   appears and opens.
3. Close Atlas normally, start it again, and confirm tabs, saved rows (closed
   ones still unloaded) and history are back, and that only the active page
   loads.
4. End Atlas from Task Manager while several tabs are open, restart, and
   confirm the footer says the session was recovered after an interrupted
   exit and nothing acknowledged is missing.
