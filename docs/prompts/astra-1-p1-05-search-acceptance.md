# Astra task 1: finish P1-05 by meeting D48, or prove why the design can't

Paste `docs/prompts/astra-common-rules.md` above this prompt.

## Why this matters

The command palette (Ctrl K) ranks open tabs, saved pages and history locally
as the owner types. D48 sets its speed budget: a complete query over 100,000
synthetic rows must finish within **5 ms at the 95th percentile**, with
maxima reported. This covers ranking, boosts, scope counts and highlight
positions. Slice P1-05 is the only open slice ahead of the rest of Plan 1,
and it has stalled. The latest production run passes **10 of 26** benchmark
cases. Two days of micro-experiments are logged in `docs/p1-05-search.md`;
most were rejected or left test-only. Small tweaks to the current path are
not converging.

## Your branch

`codex/p1-05-d48`, in your own worktree (see the common rules). Stay inside
`crates/atlas-search` and `crates/xtask/src/benchmark*`.

## Reading list (stay inside it)

- `docs/plans/Atlas v1 — Plan.md`: grep for `D48` and for `P1-05`, and read
  only those rows and paragraphs.
- `docs/p1-05-search.md`: lines 1–80 (scope, boundaries, corpus and
  thresholds), then lines 1948–2348 (the last two days of experiments and
  their results). Skim the headings (`grep -n "^#"`) to see what has already
  been tried and rejected. **Don't repeat a rejected experiment.**
- `docs/trapstoknow.md`, section "Local search": four frizbee traps.
- `docs/p1-05-frizbee-maintenance-review.md`, the summary only: why frizbee
  is vendored and patched.
- The source: `crates/atlas-search/src/catalogue.rs`, `membership.rs` and
  `score_cache.rs`. Read `range_index/` only if you decide to promote it.

## What to do

1. **Reproduce the current number first.** Run the benchmark exactly as
   `docs/p1-05-search.md` and `docs/LOCAL-DEV.md` describe, three times in
   fresh processes, and record the 26 case results. If you get something
   other than about 10/26, stop and report the discrepancy before changing
   anything.
2. **Step back to the architecture.** Build a profile of where time goes, per
   case, for the failing cases. Then write a short design note in
   `docs/p1-05-search.md` (a new dated section, at most 80 lines). It names
   the two or three structural options that could reach 5 ms p95, with
   estimated gains, what each costs in memory and mutation speed, and why.
   Examples: an incremental prefix cache keyed on the previous query while
   typing; precomputed per-field trigram or character-bag prefilters; doing
   counts and highlights only for the visible top N with exact counts
   computed lazily; or splitting work across threads. Pick one and say why.
3. **Implement the chosen option behind the existing public API.** Results,
   counts and highlights must stay identical: the 1,040-sample exactness
   comparison must still pass. Add regressions for anything new.
4. **Measure** with three fresh-process benchmark runs. D48 passes only if
   all 26 cases meet 5 ms p95 in all three runs.
5. If you can't reach 26/26 with a reasonable design, **say so plainly**. Then
   write the evidence-backed case to the owner for one of: a revised budget,
   a smaller corpus, or limiting exact counts to the visible rows. **Do not
   change D48 yourself, and do not relax any assertion.**

## Constraints

- Keep the corpus, the vendored frizbee's `safe_read` feature, exact counts
  and highlights, and the 5 ms limit. These are D48 and the approved
  frizbee review.
- Pass `&str` to frizbee's generic APIs, never `&String`. The other type
  multiplies compile time; see the traps.
- A new dependency means stop and audit (see the common rules).
- Delete experiments that lose, rather than leaving them as test-only
  modules. The crate already carries a lot of unpromoted experimental code.
  If you think some of it should be removed, list it in your report; don't
  delete existing modules without saying so.

## Done means

Either D48 passes 26/26 in three fresh-process runs, with the gate green, the
design note written and the ledger row for P1-05 updated; or a clear
negative result with the owner-facing recommendation. Both are acceptable
outcomes. Silent partial progress is not.
