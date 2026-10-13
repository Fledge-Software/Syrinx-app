# P1-19 session restore and P1-20 history

Status: **implemented on branch `claude/p1-19-20-restore-history`, awaiting
independent review and the owner's merge approval.** The owner approved
running Plan 1 slices in parallel on 2026-10-11; this branch was cut from the
integration tip `25a4fc7`. Gate evidence is at the end of this note.

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

- `syrinx-store` commits the whole tab model, saved rows, associations, order,
  selection and identity watermarks in one transaction with a compare-and-save
  revision (`crates/syrinx-store/src/records.rs`).
- The session worker (`crates/syrinx-app/src/session.rs`, `Worker::restore`)
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

`crates/syrinx-app/src/session/crash_restart_tests.rs` runs a real unclean
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

### Storage: schema version 4

**Migration number: version 4** (`user_version = 4`). This branch first took
version 3, but the integration merge on 2026-10-12 brought in P1-28/P1-29's
preferences migration (`settings`, `quick_destinations`) as version 3, so
history was renumbered to run after it. It follows P1-21's style: it only
creates its own objects, alters no earlier table, runs with a foreign-key
check and a hook that re-reads and validates the session, and a failure rolls
the migration back. A version 2 file upgrades through preferences (3) and then
history (4).

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

`Store` gains `record_visit` (returning the visit and address identities),
`retitle_history`, `remove_visit`, `history_entries`, `recent_visits` and
`apply_history_deletions`. Writes go through the existing
single writer thread; reads use the two reader connections. History writes do
not change the session's compare-and-save revision.

Older builds refuse a version 4 database (`FutureSchema`) and skip version 4
backups, as P1-21 arranged; downgrading below this branch is unsupported, as
before.

### Recording: no duplicates from stale or repeated events

`crates/syrinx-app/src/session/history/tracker.rs` decides, without storage,
which page callbacks are visits and which address a title names;
`session/history.rs` applies those decisions to the store and catalogue.

- A visit is recorded when a live **web** tab reports an address different
  from the one it last reported. Opening a tab or submitting an address is
  intent, not a visit; the page's own report of its address is.
- Repeated or replayed address callbacks for the current address are the same
  visit, and so is a change of the fragment (`#part`) alone. A reload of the
  same address is not counted.
- Address changes made by script (`pushState`) are visits, as in other
  browsers, but returning to an address this tab recorded within the last
  10 seconds is the same visit, so quick back-and-forth does not multiply
  rows.
- A failed navigation is not a visit. WebView2 reports the new address before
  the navigation's result, so the visit is recorded and then retracted
  (`Store::remove_visit`) when the UI forwards a failed `NavigationCompleted`
  as `SessionCommand::ObserveNavigation`; the address row disappears from
  the catalogue if no visit remains. Script-made changes have no navigation
  result and stay recorded. One known gap: a failed reload after a script-made
  change retracts that scripted visit.
- Callbacks for a closed tab are ignored, because the model no longer has the
  tab (tab identities are never reused).
- Restored tabs are seeded with their restored address, so reloading them at
  start-up is not counted as a new visit.
- Only `http://` and `https://` addresses are history; Home (`about:blank`)
  moves the cursor.

**Titles name the right address.** A page can report its next title just
before its next address; a single-page app often does. A title reported on
its own is therefore held for 300 ms (`TITLE_SETTLE`). If the tab's address
changes within that time, the title goes to the new visit; otherwise it is
written to the current address, at the next command or when the worker's
wait expires, and any title still held at shutdown is written then. A title
reported in the same callback as an address belongs to that address. The
price is that a page whose title changes just before the owner navigates
away keeps its older title (never the next page's), and the next page shows
the carried title until its own arrives. Duplicate titles write nothing.

A failed history write to storage stops the session like a failed save. An
unindexable row or an unavailable clock is reported and the session continues.

### Search: incremental tier 1 updates through the public API

No `syrinx-search` change was needed. The catalogue already had
`Target::History(NonZeroU64)`, `Scope::History`, `Usage { visits, last_used }`
and incremental `upsert`/`remove`. At start-up the worker upserts every
`history_urls` row once; after each visit or retitle it upserts only that
row, with the visit count and last visit feeding the existing frequency and
recency boosts. One row per address keeps repeated visits from flooding the
palette. Activating a history result in the palette opens a new tab at that
address (`SessionCommand::OpenHistory`).

### The history list (Ctrl H)

`crates/syrinx-app/src/ui/history_list.rs` shows the 200 most recent visits in
the page card, newest first, each with its title (or address before a title is
known), address and a coarse age. Clicking a row opens it in a new tab. Ctrl H
or the toolbar's History button toggles the list; switching tabs closes it.
While the list is open no page is shown beneath it, because GPUI cannot draw
over a WebView2 child window (see `docs/trapstoknow.md`). Only the newest read
fills the list, so a slow reply cannot overwrite a newer one.

**Ctrl H while a page has focus:** shortcuts reach Syrinx from a focused page
only through `AcceleratorKeyPressed` in `syrinx-platform/src/browser/page.rs`,
which maps a fixed list. A parallel P1-13 session is adding the Ctrl H
mapping there; the coordinator will connect it to this list
(`toggle_history` in `ui.rs`) at integration. Until then Ctrl H works while
Syrinx's own interface has focus, and the toolbar button works in both cases.

**Unbounded growth (DW-4, done in P1-27's follow-up: 90-day retention, see
`docs/p1-27-clear-data.md`):** every visit is kept and every remembered address
is loaded into the catalogue at start-up. Retention and pruning before D48's
100,000-row scale are registered as DW-4 in the plan's "Deferred work" table
(owning slice P1-27, or a P1-20 follow-up), with a `TODO(DW-4)` at the load
in `Worker::restore`.

### D47: cleared history must not come back

The existing pure contract in `syrinx-store/src/deletion.rs` applies directly:
its only category is `History` and its ranges are half-open Unix-second
intervals. `Store::apply_history_deletions` is the history participant: it
removes visits inside each range, drops addresses left without visits and
recomputes the rest. It is idempotent. The test
`cleared_history_stays_cleared_when_an_older_copy_is_restored_and_replayed`
records history, keeps a closed copy of the database as an older recovery
point, clears a range through the journal's request/apply/complete order,
then opens the older copy and replays `DeletionLog::replay_after(0)`. The
cleared visits are gone and the result matches the live database.

**Deletion is not wired into the running app yet.** Nothing in Syrinx calls
`apply_history_deletions` outside tests. P1-27 owns all of it: persisting the
journal and its generation barrier outside restored state, a clear-history
UI, calling the participant on the live database, updating the running
tier-1 catalogue (removing or recomputing every affected `Target::History`
row, which this participant does not report today), clearing WebView2's own
data, and replaying completed clears on the staged copy during recovery.
Until then recovery keeps refusing backups with a nonzero deletion
generation. P1-27 has since done all of this; see `docs/p1-27-clear-data.md`.

## Tests

All data is synthetic (`example.test` addresses, made-up titles) and every
database is disposable.

- `crates/syrinx-store/src/engine/tests/history.rs`: visits merge by address,
  list newest first, keep known titles, retitle without recreating removed
  entries and survive restart without touching the session revision; invalid
  visits are refused; a version 2 database upgrades through preferences to
  version 4 with its session intact and empty history; a failing version 4
  migration (conflicting table, or a validation-hook failure) leaves no
  history tables behind; damaged version 2 rows are
  refused before migrating, leaving the file byte-identical; the D47 replay
  test above.
- `crates/syrinx-store/src/engine/tests/history.rs` also covers
  `remove_visit`: it recomputes an address's totals, drops an address with no
  visit left, and is idempotent.
- `crates/syrinx-app/src/session/history/tracker.rs` (pure, with injected
  times): a title reported just before its address names the new visit (the
  review's regression); a settled title names the current address; forced
  and repeated settling; fragments, replays and quick scripted returns are
  not visits, while a return after the window is; a failed navigation
  retracts only its unconfirmed visit and drops the error page's title;
  restored and closed tabs record nothing.
- `crates/syrinx-app/src/session/history_tests.rs`: recording through the
  worker with replayed address and title callbacks, a settled title, a
  fragment, a quick return through Home, a failed navigation retracted from
  storage and search, a late callback for a closed tab, history search,
  opening a result, restart and the restored tab's reload; and a missing
  entry as a recoverable failure.
- `crates/syrinx-app/src/session/crash_restart_tests.rs`: the crash proof
  above, which also checks that the acknowledged visit survives.
- `crates/syrinx-app/src/ui/history_list.rs`: stale replies, status text,
  labels and ages.
- `crates/syrinx-platform/src/storage/recovery/tests.rs`: the newer-schema
  marker moves to version 5, and the version 1 downgrade fixture also drops
  the history and preference tables and now expects migration to version 4.

One existing worker test, `removed_saved_row_stays_removed_after_live_reopen_stale_callbacks_and_restart`,
now also sees the live navigation as a history row. Its search helper was
updated to require exactly one tab row, no saved row and one history row at
the live address, rather than a total of one.

## Gate

`cargo xtask check` passed on this machine on 2026-10-12 (exit 0): dependency
and deferred-work checks, formatting, Clippy, 538 tests and the smoke launch.
The log is `target/p1-19-20-gate.log`. After the review fixes it passed
again (exit 0, 545 tests, smoke included); that log is
`target/p1-19-20-review-gate.log`, written after one Clippy correction in a
new test. After merging `codex/integrated-browser-preview` (preferences as
schema version 3, the GPUI idle patch and the sidebar slice) and renumbering
history to version 4, it passed again (exit 0, 606 tests, smoke included);
that log is `target/p1-19-20-merge-gate.log`. One earlier warm-up build in this
worktree hit Windows Application Control (os error 4551) on the `rust-i18n`
build script; an unchanged retry built normally. No setting was changed.

The window was not inspected by an agent: the development build is not an
application the screen-control tool can be granted, so the owner checks below
cover what a passing gate cannot.

## Owner checks

1. Run `cargo syrinx`, browse a few sites, press Ctrl H (with the sidebar or
   toolbar focused) or click History, and confirm the list shows them newest
   first with titles; click one and confirm it opens in a new tab.
2. Ctrl K, type part of a visited site's name, and confirm the history result
   appears and opens.
3. Close Syrinx normally, start it again, and confirm tabs, saved rows (closed
   ones still unloaded) and history are back, and that only the active page
   loads.
4. End Syrinx from Task Manager while several tabs are open, restart, and
   confirm the footer says the session was recovered after an interrupted
   exit and nothing acknowledged is missing.
