# Local catalogue search

`Catalogue` is a derived metadata index for P1-05. Callers feed entries from
the core model, saved-page records, history and the command registry. It
performs no I/O and owns no browsing or plugin lifecycle. Distinct targets
remain distinct even if their text or addresses match. A saved page and its
live tab may therefore both appear until the later palette chooses its
presentation policy.

`upsert` changes one target; `remove` removes one. Only a real mutation advances
the generation. Failed validation leaves all rows and that generation intact.
Query results identify the generation they observed. Entry, result, catalogue
and usage diagnostics redact content; callers must keep text out of their own
logs as well.

Each query supplies its scope, display limit, typo budget, blank-query behavior,
time and boost weights. There are no implied UI defaults. Only the empty string
is blank. Whitespace and characters such as `!`, `^` and `$` remain literal
query characters; frizbee's query-language parser is not used.

Titles and addresses are matched separately, and a row receives the better
field score. A query cannot span those fields. Matching uses frizbee 0.13.0's
smart case and Unicode behavior: uppercase queries require matching case;
non-ASCII queries use Unicode matching. There is no normalization, accent
folding or full Unicode case folding. Long scoring windows can use frizbee's
greedy fallback, so optimal alignment is not promised for every long field.

Counts include all matching targets, once per target, before scope filtering
or limiting. The selected scope is ranked by descending score and then ascending
`Target` order. A linear partition selects the finalists; only those are sorted
and highlighted. Matcher state and ranking capacity are reused. Batch matching
uses temporary string references without duplicating catalogue text.

The final score is the best fuzzy field score plus these nonnegative additions:

- Open entries receive exactly the open weight.
- Frequency contributes `frequency * bit_length(visits) / 32`, rounded down.
  Zero visits contribute zero; `u32::MAX` visits receive the whole weight.
- Recency contributes `recency * max(window - age, 0) / window`, rounded down.
  Age is `max(now - last_used, 0)` in seconds. Unknown use contributes zero;
  future timestamps have age zero. The window is explicitly nonzero.

Each addition is bounded by its `u16` weight. The combined score fits `u32`,
including extreme timestamps and counters. Boosts never introduce nonmatches.

Highlight ranges are ascending, merged UTF-8 byte ranges into the unchanged
title/address. frizbee can return continuation-byte positions; Syrinx widens
these to whole Unicode scalars before producing a range. Ranges are safe for
Rust string slicing but do not promise whole grapheme clusters.

frizbee's candidate filter and best-scoring traceback can disagree about the
typo budget. If an accepted field has no trace, one lazy secondary matcher
recovers its matched-character positions without a traceback typo cap. This
changes no membership, count or score, and never highlights an originally
unmatched field. Its positions describe a partial alignment, not a complete
proof of the query's typo budget. A genuine zero-score or long-window fallback
can still have no matched-character positions; Syrinx never fabricates them.

The matcher uses `u32` indices, so entries and individual fields cannot exceed
that range. Query bytes cannot exceed `Scoring::default().max_needle_len()`
(3,639 for the audited pin), which bounds pattern compilation. Accepted queries
can still saturate the matcher's `u16` scores; repeated delimiter/capitalization
patterns exceed its documented unsaturated-score estimate. These limits return
structural errors, never silent truncation.
Normal Rust allocation failure behavior still applies.

The workspace benchmark measures the complete synthetic 100,000-entry query
path, including counts, boosts, finalist highlights and owned result strings.
The owner-approved gate is p95 below 5 ms with the maximum reported as well;
the measured result belongs in the benchmark report, not a library claim.

# Full-text index (tier 2)

`fulltext::FullText` is the durable word index (P1-24, #53) on Tantivy 0.27.0,
which the owner chose over SQLite's FTS5 on 2026-10-24 (#52). It lives in a
folder the caller opens and owns exclusively. That folder is a cache: it must be
local, outside D13's backups and outside any synced folder, and it can always
be rebuilt from its sources.

Every document belongs to one namespace. `Namespace::page()` holds captured page
text, which expires seven days after capture (D40) and follows D47's history
clears. `Namespace::feed(name)` holds a plugin's search feed, such as Notes';
it never expires and is cleared only by deleting its documents or by `reset`.
Expiry, clears and resets never touch another namespace, and the same id in two
namespaces names two documents.

Writes are staged and then committed. `stage` decides each change against what
the index already holds: a newer revision replaces, the same revision is a
no-op, and an older revision, or one at or below a deletion's tombstone, is
refused. Page text already older than seven days is refused too. Nothing staged
is visible or durable until `commit` returns; a crash or `discard` before then
loses all of it. `expire`, `reset` and `apply_clears` refuse to run while
changes are staged, and commit on their own.

Every commit rewrites the segments that hold deleted rows and deletes the
files no commit uses, because Tantivy otherwise keeps a deleted document's
stored text on disk until a merge. A replacement can rewrite a large segment,
so callers commit in batches rather than per page. If a file cannot be deleted
yet (on Windows, another program may hold it open), the commit still stands,
`commit` returns `CleanupPending`, `cleanup_pending()` stays true, and the next
commit or `retry_cleanup` tries again. Opening the index finishes a cleanup a
crash interrupted. A failed `discard` leaves the handle refusing all work
(`NeedsReopen`), because Tantivy has already released the folder's lock. `apply_clears` stores the last applied journal
generation in the same commit as its deletions, so replaying the journal after
a crash or a backup restore applies each clear exactly once.

Search does not filter by age, so whoever owns the index calls `expire` at
start-up and at least daily. A feed's tombstones never expire; `reset` is how
a feed starts afresh.

Queries match every word in the title or the body, after folding case and
accents; `prefix_last` matches the last word as a prefix. A query returns at
most 1,000 hits. Each hit carries a snippet of at most 160 bytes, cut at a
character boundary, with highlighted byte ranges. Snippet terms
for a prefix are expanded by hand, since Tantivy's own prefix query leaves them
out (see `docs/dev/trapstoknow.md`). Documents, hits, requests and errors
never print ids, titles or text.
