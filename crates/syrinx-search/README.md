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
