# Astra task 3: P1-23, choose the full-text engine with evidence

Paste `docs/prompts/astra-common-rules.md` above this prompt.

## Why this matters

Tier 2 search (page text, notes, plugin content) needs a full-text index. D18
leaves the choice between SQLite FTS5 and Tantivy to a measured bake-off on
one synthetic corpus, with retention costs included: page text expires after
seven days (D40) and must never be restored from backups. The choice shapes
P1-24 to P1-27 and later the Notes plugin. Picking by taste would get
re-litigated.

## Branch

`codex/p1-23-fulltext-bakeoff`. Put all code in a new **benchmark-only**
crate, `crates/fulltext-bench`, that no product crate depends on. If
`crates/xtask/src/dependency_policy.rs` needs to allow it, change that and
nothing else. Product crates stay untouched.

## Reading list (stay inside it)

- `docs/plans/Syrinx v1 — Plan.md`: grep for `D18`, `D35`, `D40`, `P1-23`,
  `P1-24` and `FullText`, and read those rows and paragraphs.
- `docs/decisions.md`: the D40 and Q2 entries (retention and separation from
  the durable store).
- `docs/research/03-core-architecture.md`: the search section only (grep
  `FTS5`). Note that this research note assumes Tauri; ignore that part.
- `docs/dependency-audit.md`: read the first audit entry as the template,
  then grep `rusqlite` to see how the bundled SQLite is pinned. FTS5 may
  already be compiled in; check the vendored
  `vendor/libsqlite3-sys-0.38.2` build flags before assuming anything.
- `docs/trapstoknow.md`, section "Storage".

## What to do

1. **Audit first, then stop.** Write `docs/p1-23-fulltext-audit.md`:
   - FTS5: what the pinned SQLite already includes, and any build-flag change
     it needs. A build-flag change counts as a change to the approved SQLite
     patch, so it needs owner approval.
   - Tantivy: an exact version, its licence, maintenance and security record,
     and the full transitive dependency list with licences, from
     `cargo tree` in a scratch copy, not the real lockfile.

   **The owner approved Tantivy on 2026-10-11** (see `AGENTS.md`,
   "Approved dependency"). Add the exact audited version to the bench crate
   only. Stop and ask only if a transitive package has a licence outside
   the permissive list, or if FTS5 needs a SQLite build-flag change.
2. **Corpus.** Generate the corpus deterministically from a seed: synthetic
   pages on `example.test` domains with made-up text, sized to the plan's
   stated scale. Include multilingual and accented text, long pages and
   near-duplicates.
3. **Measure the same operations on both engines:**
   - bulk index;
   - incremental add and update;
   - delete;
   - seven-day expiry sweep;
   - query latency p50/p95/max for typical palette queries;
   - index size on disk;
   - recovery: interrupted write with a process kill in a disposable
     directory, then reopen and verify;
   - confirming expired or deleted text can't come back through replay.

   Use three fresh-process runs.
4. **Write the report** as `docs/p1-23-fulltext-bakeoff.md`. Include the
   method, the raw numbers, the failures and a reasoned recommendation that
   names what was rejected and why. The owner reviews it before P1-24
   starts, so don't start P1-24.

## Constraints

- Only synthetic data. No page text from any real site.
- No network fetches at run time.
- Record the corpus seed and the exact command lines so anyone can reproduce
  the numbers.

## Done means

The audit is written and both engines are measured, unless a licence or
build-flag question is waiting on the owner, stated plainly. The report has a
recommendation, the gate is green on the branch, and the P1-23 ledger row is
updated.
