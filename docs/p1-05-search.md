# Measure local search before connecting the palette

P1-05 gives the future palette a tested in-memory catalogue without reading
the owner's browsing data or building its interface early. D9 keeps typed
queries local. The approved slice follows P1-04's owner-approved `7710c06`
merge and uses branch `codex/p1-05-search`, based on completion record
`33dade0`. Commits and merges for P1-05 need separate approval.

## Scope and boundaries

`crates/syrinx-search` holds derived searchable records, typed destinations,
incremental replacement/removal and the complete query path. The shared
`syrinx-core` tab and saved-page identities remain authoritative. History and
command identities are opaque inputs; this crate neither collects history
nor registers commands. It contains no database, network call, UI toolkit,
plugin host, full-text index or timer.

Saved destinations and live pages may have different titles and addresses
(D43). Do not merge records merely because their text matches. The later
palette integration in P1-31 will choose visible deduplication and connect
results to shared open/focus actions. This slice must preserve stable typed
targets and must not mutate the tab model.

The caller supplies the query scope, reference time, output limit, typo
allowance, empty-query behavior and boost weights. They are explicit inputs,
not new UI defaults. The catalogue counts all matching records before
filtering the displayed scope or limiting the result list. Frequency,
recency and open-state boosts can reorder matches but cannot manufacture
matches. Stable target order breaks ties. All content-bearing diagnostic
formatting is redacted.

The exact matcher is `frizbee =0.13.0`, with `std` and `safe_read` only.
`docs/dependency-audit.md` records its provenance, permissive licence,
maintenance and scoped advisory check. The safe-read feature disables the
library's deliberate allocation overreads; the measured build must keep it
enabled. Literal query construction avoids introducing undocumented query
operators. Returned highlights must be valid UTF-8 byte ranges over the
original title or address, including multibyte characters.

## Performance evidence

D48 records the owner's chosen acceptance rule: 95% of repeated complete
100,000-row queries must finish within 5 ms, with the worst sample reported
as well. `cargo xtask bench` measures matching, ranking with boosts,
uncapped scope counts, result construction and returned highlights together.
Corpus creation and result validation belong outside the timed interval.
An upstream matcher benchmark cannot establish this result.

The reproducible corpus contains 100,000 synthetic records spanning live
tabs, saved destinations, history and commands, with varied text lengths,
Unicode, frequency and recency. Queries cover broad, narrow, absent,
address-only, typo and Unicode matches, plus scope restrictions. The
benchmark policy uses 50 results and one typo from four characters; these
are workload parameters for this slice, not settled palette defaults.

Run the optimized `local` profile on this machine with no build process
running concurrently. Warm up, interleave repeated queries and retain all
samples, including failed runs. Report each case's median, 95th percentile
and maximum. Three fresh-process runs check repeatability without silently
averaging a slow case into easier queries.

Machine observed on 2026-10-10: AMD Ryzen AI 9 HX 370 with Radeon 890M,
24 logical processors, Windows version `10.0.26300.0`, Rust/Cargo `1.97.1`.
No Windows, power-policy or antivirus setting is changed for measurement.

The first 24 focused tests passed. The initial benchmark then stopped during
warmup: its long near-match had valid membership but no highlight positions.
The pinned matcher's constrained traceback can stop before any matching cell.
An independently reviewed fallback now retries highlighting only for accepted
fields with empty traces, preserving original membership, counts and scores.
Two additional regressions cover the failure and genuine zero-score matches
that remain present even when no trace is available. Such highlights are
matched-character positions, not a complete proof of the query's typo budget.

The two failed warmup attempts produced no timings. Their original empty JSON
files and diagnostic record remain under `target/p1-05-bench-*`; no passing
performance claim follows from them. After the fallback, all 26 focused
tests passed and the first complete benchmark run passed its correctness
validations. The complete `cargo xtask check` remains pending.

### First complete run, 2026-10-10

The performance gate **failed 25 of 26 cases**. Only the blank query met
the per-case p95 limit of less than 5 ms. The worst individual sample was
383.737 ms. This is initial evidence to guide profiling, not completion of
P1-05; the threshold and workload remain unchanged.

The run used five warmup passes and 40 measured passes per case. The raw
report, including all 1,040 measured samples, remains at
`target/p1-05-bench-run-1.json` (an ignored local artifact). Its SHA-256 is
`14a8cdc54f51e215e9c5344836e9720d3cf81f47ed7b3c872cdb9133b6318c57`.
Values below are rounded to three decimal places; the report retains
nanoseconds.

| Case | p95 (ms) | Maximum (ms) |
|---|---:|---:|
| blank | 2.833 | 3.012 |
| broad-character | 37.066 | 45.980 |
| typed-two | 39.154 | 45.574 |
| typed-three | 47.898 | 49.863 |
| typed-four | 58.940 | 61.811 |
| word | 42.413 | 42.617 |
| missing-character | 38.962 | 41.006 |
| transposed-characters | 38.687 | 44.863 |
| uppercase | 10.874 | 11.230 |
| subsequence | 49.239 | 53.806 |
| common-word | 39.861 | 43.228 |
| two-words | 51.887 | 52.211 |
| address-only | 35.964 | 39.399 |
| address-suffix | 55.678 | 58.284 |
| rare-identity | 34.154 | 36.629 |
| composed-accent | 77.880 | 83.656 |
| decomposed-accent | 88.435 | 95.607 |
| cjk | 17.227 | 18.243 |
| emoji | 16.339 | 17.247 |
| no-match | 10.310 | 10.853 |
| long-match | 352.522 | 383.737 |
| long-near-match | 322.657 | 331.403 |
| tabs-scope | 41.721 | 44.113 |
| saved-scope | 37.899 | 40.079 |
| history-scope | 43.135 | 43.378 |
| commands-scope | 41.133 | 45.367 |

### Native CPU diagnostic, 2026-10-10

A separate build used process-only `RUSTFLAGS=-C target-cpu=native` under
`target/p1-05-native-probe`. It kept `safe_read`, assertions and overflow
checks. This diagnostic does not change the product's CPU requirement or
replace the ordinary-build acceptance run. It also **failed 25 of 26 cases**;
only the blank query passed. The worst sample was 382.378 ms. Native CPU
flags did not resolve the measured failure.

All 1,040 corresponding samples had exactly equal result checksums, match
counts and hit counts; queries, scopes and workload metadata also matched.
Integer values were compared without floating-point conversion. The raw
report is `target/p1-05-bench-native-1.json`, SHA-256
`16f7a47147a532ddb0d949f15880899a4ac53e8cfe4127e70d0e40d141ddd03c`.
Representative timings follow; both raw reports preserve every case.

| Case | Ordinary p95 / max (ms) | Native p95 / max (ms) |
|---|---:|---:|
| blank | 2.833 / 3.012 | 2.354 / 2.508 |
| word | 42.413 / 42.617 | 41.322 / 45.018 |
| uppercase | 10.874 / 11.230 | 13.310 / 15.037 |
| composed-accent | 77.880 / 83.656 | 81.398 / 82.896 |
| no-match | 10.310 / 10.853 | 12.720 / 15.264 |
| long-match | 352.522 / 383.737 | 341.882 / 382.378 |
| long-near-match | 322.657 / 331.403 | 315.120 / 342.689 |

## Gate and unsigned execution checkpoint

Formatting and the direct full-workspace Clippy command passed. Across
attempts, 132 tests passed: 30 core, 40 platform unit, four backup integration,
18 storage integration, 26 search and 14 xtask tests. Windows blocked the
store unit-test executable before its 38 tests ran. The subsequent engine
identity test and full-workspace documentation-test stage were not reached.
This is partial evidence, not a passing full
gate. Unchanged retries of the main `xtask` executable and store unit-test
executable returned Windows error 4551.

`Cargo.toml` now sets `profile.local.build-override.opt-level = 3` so build
helpers use D38's optimized route. Assertions and overflow checks remain
inherited; no security settings changed. SQLite's Clippy build helper then
executed, but that sequence does not prove the cause of its earlier block.

The owner has no signing certificate and explicitly requires Syrinx to stay
unsigned. Today's signature checks returned `NotSigned` for the actual app,
main xtask, store-test executable and native probe. The current unsigned app
launched directly, printed `Syrinx 0.1.0` and exited with status 0. A filtered
Code Integrity event 3077 matched the blocked execution to policy
`VerifiedAndReputableDesktop`, status `0xc0e90002`. These observations neither
make a certificate a development prerequisite nor establish an unsigned
exemption; they do not explain why these particular images received
different outcomes.

The narrowly scoped signature, binary-hash and policy-event evidence is
preserved locally in `target/p1-05-unsigned-execution-evidence.json`.

There are no active Cargo or shell sessions at this checkpoint. P1-05 still
needs a complete passing gate, performance work and the three-run acceptance
evidence. Investigate the measured matcher cost before repeating an unchanged
failed benchmark. Preserve all failures, keep Windows security unchanged and
keep the app unsigned. P1-05 has no commit or merge approval.

## Parallel matcher diagnostic before D49

A temporary two-worker and four-worker matcher probe retained all 1,040
baseline result checksums, hit counts and match counts in each run. It still
missed D48: the ordinary word query's p95 was 27.9214 ms with two workers and
15.7392 ms with four. The four-worker run overlapped the briefly opened old
prototype and is diagnostic evidence only. The two ignored reports are
`target/p1-05-bench-parallel-2.json` (SHA-256
`d9463db958c83a1c18d85c7a5b81ef77468571fc5909f638da409e5868a27bc9`)
and `target/p1-05-bench-parallel-4.json` (SHA-256
`5cad8cce78276d2ffea61fb824cd3c00c6736f98032b397fdfeaf446f42b4888`).
The temporary probe code was removed; no production worker-count or CPU-flag
change was retained. The proposed eight-worker probe was not run because
the owner subsequently approved D49's integrated browser priority. Search
performance remains open, and the preview must not be called a passed P1-05.

## Read-only bottleneck investigation, 2026-10-10

The measured cost is predominantly matching and scoring, before Syrinx chooses
its final results. In `crates/syrinx-search/src/catalogue.rs`, `rank_matches`
passes every title and every address to frizbee's batch matcher. The pinned
0.13.0 source, `src/matcher/algo.rs`, runs its candidate filter and then the
scoring path for every accepted field in `match_list_into_impl`. That path
uses Smith-Waterman within its supported window and a greedy fallback beyond
that window.
Syrinx already uses `IndexAsc` to avoid the library's score sort, partially
selects its own best results and computes highlights only for those finalists.
Reducing those final allocations alone does not address the dominant work.

The earlier diagnostic measured about 33 ms for the direct matcher over
100,000 short synthetic fields and about 45 ms for a full query returning no
finalists. These are diagnostic timings, not additional acceptance runs. The
baseline's long query still admits 10,000 long titles, which each enter the
scoring path. The published package's `BENCHMARKS.md`, under "All Match", also
reports 11.90–21.05 ms for one typo over 100,000 fields as median field length
varies from 16 to 128. That upstream Linux workload and machine differ from
Syrinx's; these numbers explain why the dependency's broad-match cost cannot
be assumed to meet D48, rather than predict this machine's performance. The
audited source package and checksum are recorded in `docs/dependency-audit.md`.

The next optimization must avoid unnecessary scoring while retaining exact
membership and counts. A possible route is an incrementally maintained field
index that rejects impossible candidates, followed by proven score bounds
that skip scoring only when a candidate cannot beat the current last
finalist. Those bounds must include Syrinx's boosts and stable target tie
breaks. Counts still require exact membership across every scope, including
accepted zero-score matches, and final highlights must keep the existing
Unicode and empty-trace behavior. Frizbee's candidate filter is private
(`src/lib.rs`); its public batch API does not expose a membership-only pass.
An exact indexed implementation therefore needs separate equivalence tests,
including the pinned filter's case, Unicode and typo semantics. This is an
engineering direction, not an implemented optimization or a 5 ms prediction.

A character-presence index can cheaply reject some impossible matches, but
cannot by itself solve common-character queries or the long matching corpus.
The current evidence does not identify a small wrapper or CPU-flag change
that meets every case. D48 remains unchanged: full-query per-case p95 below
5 ms on all 100,000 entries. Truncating fields, dropping candidates or scopes,
changing typo semantics, or caching this benchmark's repeated answers would
not satisfy it. No source, dependency, profile or measurement changed during
this read-only investigation.

## Conservative ASCII candidate-filter experiment, 2026-10-10

To avoid sending provably impossible fields through the full matcher,
`crates/syrinx-search/src/candidate.rs` now maintains character-presence masks
beside the catalogue entries. For wholly ASCII query and field strings, it
rejects a field only when the number of distinct absent query characters
exceeds the typo budget. Each absent character requires at least one query
deletion under the pinned prefilter's rule. ASCII letter case is folded to
admit a conservative superset; Unicode in either string bypasses rejection.
`catalogue.rs` filters title and address independently, then restores each
match's original row index before merging. Upserts and swap-removals keep
the masks aligned with entries. No dependency, scoring policy, benchmark
query or result limit changed.

Independent source review found no defect in that conservative rule or its
integration. The focused test run passed 34 tests and failed two new
differential tests with `InvalidMatchRange`. Those tests compare complete
results against the same catalogue with every mask bypassed. Investigation
confirmed that both paths fail on the synthetic NUL query (`\0`): the matcher
returns highlight indices equal to field byte length, which the range guard
correctly rejects. The diagnostic log is
`target/p1-05-mask-diagnostic.log`. A source correction remains under review
and the failures remain unresolved. The test success requirements have not
been relaxed. This checkpoint is not a passing correctness gate.

The diagnostic benchmark retained exactly the baseline's result checksum,
match count and hit count for all 1,040 corresponding samples. It passed
only two of the 26 timing cases: blank and no-match. The worst sample was
378.0657 ms. Five warmup passes and 40 measured passes per case used the
unchanged 100,000-row workload. No compiler ran during measurement, but two
old GPUI prototype windows were running. This is neither acceptance evidence
nor a controlled before-and-after experiment proving a timing regression or
improvement. Selected p95 values follow; the raw report preserves every case
and sample.

| Case | Baseline p95 (ms) | Candidate-filter diagnostic p95 (ms) |
|---|---:|---:|
| blank | 2.833 | 3.119 |
| broad-character | 37.066 | 42.903 |
| word | 42.413 | 45.999 |
| address-only | 35.964 | 31.633 |
| composed-accent | 77.880 | 93.931 |
| no-match | 10.310 | 3.739 |
| long-match | 352.522 | 363.860 |
| long-near-match | 322.657 | 329.924 |

The ignored local report is `target/p1-05-bench-mask-1.json`, SHA-256
`fa818f95fac760119b4261c97451c70f627e1e322ef601dfac9926b21db6c876`.
The earlier failures and reports remain preserved. Exact result behavior,
the unresolved tests, and D48's per-case p95 below 5 ms all remain requirements
for completing P1-05; this incremental filter does not complete the slice.

## Exact membership experiment, 2026-10-10

Avoiding full scoring for every accepted row first requires a cheap way to
count every match correctly. The test-only `src/membership.rs` implements
zero/one-query-deletion membership with the pinned matcher's smart-case and
Unicode counterpart rules. It tracks the furthest matched prefix for each
deletion budget; a field character advances each state at most once. Budgets
above one explicitly request a fallback. It neither ranks nor highlights.

Six differential tests passed against the public frizbee API, including short
exhaustive ASCII/Unicode strings, all 128 ASCII bytes, literal NUL, case-mapping
exceptions and long fields across chunk boundaries. Independent source review,
focused Clippy and formatting passed. Logs are
`target/p1-05-membership-tests.log` and `target/p1-05-membership-clippy.log`.
The focused command filtered out the other 36 search tests; it does not resolve
the two previously failing NUL-highlight tests or constitute a full search gate.

That six-test result precedes the production integration below; it is not
evidence for the subsequent code changes.

## Bounded ranking experiment, 2026-10-10

To preserve complete counts while avoiding unnecessary scoring, the current
uncommitted catalogue uses exact membership for zero/one-deletion queries,
then scores candidates in descending order of a conservative score bound.
It stops only when every remaining candidate loses to the displayed cutoff,
including boost and Target-order ties. Larger typo budgets retain the batch
matcher. A zero display limit still counts all matching scopes. The exhaustive
test oracle bypasses masks, membership and pruning.

Independent source review rejected the matcher's amortized `18*n + 22`
estimate as a ranking bound: adjacent delimiter and capitalization bonuses
can exceed it. The implementation instead uses `min(65535, 20*n + 20)`,
where `n` is query byte length, covering both scalar and byte-based matcher
paths. Saturated scores remain valid; the existing query-length limit does
not guarantee unsaturated scores. Regressions cover both the counterexample
and saturation, along with boosts, scope counts, Unicode and greedy zero
scores. No input limit, scoring policy or dependency has changed.

The complete focused run in `target/p1-05-bounded-tests.log` passed 45 tests
and failed the same two NUL-highlight regressions, with none ignored or
filtered. All new bound, saturation and membership tests passed. Focused
Clippy, workspace formatting and diff checks passed. The separately proposed
dependency correction remains necessary and awaits approval.

The new benchmark executable built successfully, but Windows Application
Control refused to launch it in both restricted and normal desktop contexts.
A preceding PowerShell invocation failed while
configuring redirected output; neither attempt produced timing samples.
The build log is `target/p1-05-bounded-build.log`; the attempted report names
`p1-05-bench-bounded-1.json` and `p1-05-bench-bounded-2.json` are not benchmark
evidence. The loose bound can still require scoring every long-query match;
no new latency result or acceptance claim is available.

## Field-specific bound review, 2026-10-10

The current bound assumes that every matched query unit receives a boundary
bonus. A field with few word or case boundaries cannot supply that many.
The next justified experiment is to cache each field's count `B` of adjacent
byte positions that satisfy either of these disjoint conditions:

- An ASCII delimiter is followed by a non-delimiter. Here a delimiter is an
  ASCII byte that is neither a letter nor a digit, exactly as in the scorer.
- An ASCII lowercase letter is followed by an ASCII uppercase letter.

For the pinned default scoring, a conservative field bound is
`min(65535, 16*n + 4*min(n, B) + 12 + exact)`, where `exact` is eight only
when the **complete original query and field bytes are equal**, otherwise
zero. Use query byte length for `n`. For a non-ASCII query whose original
field is at most 1,024 bytes, query scalar count is also safe: the scorer's
trimmed field cannot exceed the 1,024-byte dynamic-programming cutoff.
Longer original fields must retain the byte bound unless the actual trimmed
window is independently known; their Unicode scorer can use the byte-based
greedy fallback. Smart-case restrictions can only remove possible matches
from this upper bound. The row bound is the larger field bound plus the
same exact Syrinx boost, with the existing Target tie comparison.

The source proof is in the cached `frizbee 0.13.0` package:
`src/smith_waterman/algo/ascii.rs:66-153` and
`src/smith_waterman/algo/unicode.rs:99-202` add boundary bonuses only at
those field positions. A scoring path advances through columns, so each
position contributes at most once. It matches at most `n` query units;
gap and mismatch transitions add no positive score. The greedy path in
`src/smith_waterman/greedy.rs:26-83` obeys the same bound and may disable
some leading delimiter bonuses. Trimming removes boundary opportunities;
its initial previous-byte masks are zero, so it cannot invent new ones.
Saturation or wrapping additions cannot exceed the uncapped sum.

The exact bonus needs particular care. `src/matcher/algo.rs:319-324` sets
`include_exact` only when the adjusted start is zero and the end equals the
original field length. The wrapper at lines 249-251 then also requires byte
equality with the trimmed field, after either scorer returns. Together these
conditions require equality with the original field; a matching substring
with leading or trailing text cannot earn the bonus. Mere length equality
is insufficient. The same check occurs in the highlight wrapper.

This source review preceded the implementation and measurement below. The
earlier `_aB_aB` counterexample still rules out `18*n + 22`; counting actual
boundary positions avoids its false amortization. Keep the coarse bound for
any case not covered by this proof. In particular, do not tighten the base
term using the number of real field characters while the NUL-padding defect
is unresolved: the current scorer can reward padded NUL positions. Those
zero padding bytes add no boundary bonus, so the proposed boundary count
does not conceal or excuse the failing highlight tests. Differential tests
must cover trimming, adjacent boundaries, Unicode at the greedy cutoff,
NUL, saturation and exact ties before integration. The unchanged full-query
benchmark must establish whether this additional metadata and bound actually
help; no 5 ms prediction follows from the proof.

## Field-bound implementation and diagnostic result, 2026-10-10

`candidate.rs` now caches each field's boundary count alongside its presence
mask; existing updates and swap-removals keep both aligned. `QueryBound`
computes the reviewed field bound and caps it by the coarse byte bound.
The catalogue uses the larger title/address bound before adding its unchanged
boost. Independent implementation review passed. The full search suite now
passes 47 of 49 tests, with only the same two NUL-highlight failures. Added
tests verify trimming, exact bonuses, adjacent boundaries, Unicode around
the 1,024-byte cutoff, saturation, ties and metadata replacement. Focused
Clippy and formatting passed. The combined application/search test log is
`target/integrated-preview-field-bounds-navigation-tests.log`.

The rebuilt benchmark ran successfully as a process but failed D48: only
the blank case passed, one of 26 cases. All 1,040 sample checksums, hit counts
and match counts equal the original baseline exactly. The worst sample was
382.8664 ms. No compiler ran during measurement; previously launched preview
processes remained alive. This diagnostic is not a controlled same-session
A/B trial or the three required acceptance runs.

| Case | Original baseline p95 (ms) | Field-bound diagnostic p95 (ms) |
| --- | ---: | ---: |
| blank | 2.833 | 3.181 |
| broad-character | 37.066 | 31.247 |
| word | 42.413 | 67.141 |
| address-only | 35.964 | 42.697 |
| composed-accent | 77.880 | 104.705 |
| no-match | 10.310 | 13.137 |
| long-match | 352.522 | 24.490 |
| long-near-match | 322.657 | 365.637 |

The report is `target/p1-05-bench-field-bounds-1.json`, SHA-256
`2a5bbbacbc9b8e752cb7c6ca43db62201068c15b066fac0485f12df0a34638b2`;
its stderr preserves the failed acceptance result. The experimental path
has not established a general performance improvement: the long-match case
is much faster in this run, but many cases are slower. Keep P1-05 unfinished,
retain the exhaustive oracle and obtain profiling evidence before selecting
another optimization. The earlier blocked benchmark binaries and matcher
failures remain recorded; no security setting or benchmark requirement changed.

## Stage attribution, 2026-10-10

To select the next experiment from measurements, the private synthetic test
in `src/catalogue/profiling.rs` times setup, membership/candidate creation,
heap/exact scoring, ordering and highlights/results. Production queries use
a no-op observer; clocks and reporting exist only in tests. Independent
source review, the focused test and focused Clippy passed. The test compares
complete results against the exhaustive implementation outside timing.

`target/p1-05-stage-profile-1.log` records a distinct 100,000-row synthetic
corpus, two warmups and five measured passes. These are median milliseconds,
not D48's acceptance workload or percentiles. Instrumentation also adds
per-scored-row counters, so treat these as attribution rather than a promise
about production latency.

| Query / scope | Membership and candidates | Heap and exact scoring | Complete query |
| --- | ---: | ---: | ---: |
| broad / Everything | 25.349 | 3.620 | 29.218 |
| broad / Commands | 22.989 | 0.036 | 23.081 |
| word / Everything | 28.988 | 7.974 | 35.837 |
| word / Commands | 23.106 | 0.054 | 23.240 |
| Unicode / Everything | 72.521 | 5.931 | 78.704 |
| Unicode / Commands | 68.960 | 0.050 | 69.118 |
| long / Everything | 10.224 | 23.183 | 36.380 |
| long / Commands | 10.779 | 0.230 | 11.261 |

All-scope membership dominates the short and Unicode cases even when only
64 command rows can be scored. Long queries also spend substantial time in
exact scoring. A narrowly scoped next experiment may accept literal substring
matches before running the existing ordered matcher: a literal match already
satisfies zero deletions. Misses must retain the old path. This can add a scan
to nonliteral cases, so only the unchanged full benchmark can justify adoption.
The two existing NUL-highlight failures and failed D48 gate remain open.

## Literal-substring experiment rejected, 2026-10-10

The positive substring check preserved results and passed independent review.
Its full suite passed 49 of 51 tests; only the same two NUL-highlight tests
failed, with none ignored or filtered. Focused Clippy and formatting passed.
The diagnostic corpus improved, but the unchanged acceptance workload did
not establish a general gain: only 1/26 cases passed. All 1,040 checksums,
hit counts and match counts still equalled the original baseline.

`target/p1-05-bench-substring-1.json` records a 396.0581 ms worst sample.
Its SHA-256 is
`47bbc6564b46913d3ade2ed8cc96b7a75cb4c274ce5c4e5e41973d41981acab8`.
Word p95 rose from about 67.14 to 68.72 ms and long-near-match from 365.64
to 385.29 ms, while broad-character fell from 31.25 to 24.44 ms and
long-match from 24.49 to 21.24 ms. This was not a controlled interleaved A/B
trial, so the changes are not a precise causal estimate. They do not justify
adding the extra failed-substring scan to production queries.

The shortcut was therefore removed, restoring the preceding membership
implementation. Keep the new literal/nonliteral semantic regression and
stage diagnostic; neither changes the acceptance workload. The rejected
candidate's tests and benchmark are historical evidence, not measurements
of the restored working tree. The restoration's test log is
`target/p1-05-restored-membership-tests-1.log`.
That full restored-tree run finished with 49 passing tests and the same two
NUL-highlight failures, none ignored or filtered. The retained profiler and
literal/nonliteral regression pass. The production shortcut is absent.

## Unicode queries against ASCII fields, 2026-10-10

The next specialization uses the existing cached ASCII status of each field.
The pinned matcher's Unicode case pairs preserve UTF-8 width, so a non-ASCII
query scalar cannot match an ASCII field. More such scalars than the allowed
deletions make membership impossible. With one scalar and the one-deletion
budget, that deletion is forced: the remaining original case pairs must match
as an exact subsequence, with no further deletion. Keeping those original
pairs matters for a query such as `Éa`; deleting `É` must not make `a`
case-insensitive. No normalization or transliteration is introduced.

`membership.rs` prepares that ASCII projection once per Unicode query;
`catalogue.rs` selects it only when `FieldMasks` proves the field is ASCII.
Other fields, ASCII queries and unsupported typo budgets retain their prior
paths. Independent review passed. The full search suite passes 50/52 tests,
with only the same two NUL-highlight failures and no ignored/filtered tests.
The combined log is `target/integrated-preview-watchdog-unicode-tests-1.log`.
The synthetic profiler's Unicode Commands membership median fell from
68.960 to 43.388 ms; this is attribution evidence, not D48 acceptance or a
controlled causal A/B result.

The unchanged full workload then ran in
`target/p1-05-bench-unicode-ascii-1.json`, SHA-256
`f1c07004131cef47b97ed1911a172d3e8cff6b98d6810c41f8b039da083ec00f`.
All 1,040 sample checksums, hit counts and match counts equal the original
baseline. The measured Unicode cases improve substantially, so this narrow
specialization is retained. It does not solve the performance gate: only
1/26 cases passes and the worst sample is 390.8081 ms. No compiler ran during
measurement; the earlier preview processes were still open. This remains a
diagnostic comparison, not three acceptance runs or a controlled A/B test.

| Case | Previous field-bound p95 (ms) | ASCII-field specialization p95 (ms) |
| --- | ---: | ---: |
| composed accent | 104.71 | 77.49 |
| decomposed accent | 107.78 | 81.83 |
| CJK | 82.81 | 10.91 |
| emoji | 83.05 | 10.89 |
| word | 67.14 | 67.40 |
| long near match | 365.64 | 377.91 |

The unchanged ASCII/long-query matching paths still need work. A reviewed,
unimplemented candidate is a bitset of reachable prefixes for ASCII queries
up to 63 bytes, with 256 byte masks prepared once per query. Separate exact
and one-deletion states could remove dependent prefix lookups in the hot
loop. Before adoption it needs exhaustive membership/full-result comparison,
62/63/64-byte boundaries, repeated bytes, literal NUL, smart-case and Unicode
field tests, plus the unchanged benchmark. Longer/Unicode queries and larger
typo budgets retain existing paths. Do not substitute this proposal for evidence
or revive the rejected substring experiment.

## Short ASCII membership bitsets, 2026-10-10

To reduce dependent lookups in the measured scan, original ASCII queries of
1–63 bytes now prepare a boxed table of 256 byte masks once per query.
`membership/bitset.rs` tracks reachable query prefixes in a 64-bit value,
with separate exact and one-deletion loops. Each match transition reads the
previous state; the deletion transition advances the new exact prefix once.
This prevents one field byte from satisfying two repeated query characters.
Bit 63 is the completion flag for a 63-byte query, with no shift by 64.
NUL is an ordinary real field byte, never padding. Empty/long ASCII queries,
Unicode queries and unsupported typo budgets keep their preceding paths.

Independent source review and focused Clippy pass. The full search suite
passes 53/55 tests; only the same two NUL-highlight regressions fail, with
none ignored or filtered (`target/p1-05-bitset-tests-1.log`). New tests compare
roughly 930,000 short-input cases with the scalar algorithm and exercise the
public matcher around 0/1/62/63/64 bytes, smart case, repeated bytes, deletion
positions and ASCII queries on Unicode fields. The other complete catalogue
oracles and the stage profiler pass; the two NUL oracles remain failing.

The unchanged full benchmark preserves all 1,040 baseline checksums, hit
counts and match counts. Its report is `target/p1-05-bench-bitset-1.json`,
SHA-256 `a60d4da721bc221f75f8cb621b3557076457214328710b4362dcc6a966dd8793`.
Short ASCII cases improve substantially in this diagnostic comparison, so the
candidate is retained. D48 still fails: only 1/26 cases passes and the worst
sample is 382.4652 ms. No compiler ran during measurement; older previews
remained open. This is not an interleaved controlled A/B test or acceptance.

| Case | Previous Unicode/ASCII p95 (ms) | Bitset p95 (ms) |
| --- | ---: | ---: |
| word | 67.40 | 40.63 |
| uppercase | 91.81 | 50.95 |
| address only | 42.12 | 26.83 |
| commands scope | 63.43 | 36.69 |
| no match | 12.33 | 9.17 |
| long near match | 377.91 | 367.56 |

Both the remaining all-scope scan and long-query exact scoring need further
work. Keep the full workload and all-scope counts; do not turn repeated-query
caching, a smaller corpus or reduced semantics into a substitute for D48.

## Long near-match stage attribution, 2026-10-10 continuation

The earlier private profiler did not exercise the acceptance workload's slow
long near match. `catalogue/profiling.rs` now retains its earlier cases and
adds that query shape against 10,000 synthetic duplicated-fragment titles
with distinct row identities. It reports finalist score ranges alongside
candidate and exact-scoring counts. This is a separate diagnostic corpus;
`xtask`'s acceptance workload, target and production search are unchanged.

The focused profiler test passes, comparing complete results against the
exhaustive reference outside every timed interval. Its log is
`target/continuation-search-stage-profile.log`. This focused invocation ran
one diagnostic test with 54 other tests filtered; it is not a passing search
suite. The preceding complete gate still failed the two NUL-highlight tests.

For the Everything near-match case, all 10,000 matched candidates receive
exact scores despite a 50-result limit. Median membership/candidate work is
21.580 ms, heap/exact scoring 303.480 ms, highlights 3.295 ms and complete
query time 328.429 ms. All 50 finalists score 1671. Commands retains the same
10,000 all-scope matches but scores only its six candidates: membership is
20.815 ms and heap/scoring 0.183 ms. These measurements identify separate
scan and pruning costs; a faster membership loop alone cannot remove the
Everything case's dominant scoring cost.

The next experiment should tighten exact-semantics candidate bounds or
otherwise reduce exact scoring, with full oracle comparison and the unchanged
acceptance benchmark before adoption. Per-field membership could avoid scoring
an impossible second field, but short addresses already reject cheaply in
this workload; that refinement is not assumed to solve its dominant cost.
No new production optimization or passing performance result is claimed.

## Query-local score equivalence experiment

The next experiment targets the repeated exact scoring shown above without
merging catalogue rows. `score_cache.rs` recognizes ASCII fields that differ
only in digits absent from the current query. Those digits remain digits and
cannot satisfy any query character, so equal signatures preserve the original
prefilter window, scoring masks, boundaries, length, exact-match eligibility
and padding. Frizbee scores an original representative, never the signature.
Independent source review checked this against the pinned implementation.

The cache exists for one query and retains at most 512 keys and 256 KiB of key
payload. Unicode and fields over 1,024 bytes use ordinary scoring. Full key
equality resolves hash collisions, and rejected fields remain distinct from
accepted zero scores. Each row retains its own identity, boosts, counts, tie
order and original highlights. This is not a saved answer reused across
queries; metadata and query changes cannot inherit old scores.

`membership/indexed.rs` separately prototypes advancing through cached
character positions for fields up to 64 scalars. It remains test-only while
memory and throughput are measured. Its diagnostic compares complete row
membership on acceptance-shaped inputs, but it does not replace the unchanged
complete-query benchmark. Validation and measured adoption results follow.

The complete search run in `target/continuation-search-prototypes-2.log`
passes 69 tests and fails only the same two NUL-highlight regressions, with
none ignored or filtered. Nine score-cache differential tests and two
catalogue regressions cover original scores, zero versus rejection, caps,
Unicode/long-field fallback, distinct identities, boosts, counts, ordering,
highlights and changes between queries. The positional prototype adds four
equivalence/boundary tests and its diagnostic. Independent review also checked
the integration and the exhaustive oracle's uncached path.

The focused stage run in `target/continuation-cache-stage-profile.log`
preserves complete exhaustive results. Near-match Everything still considers
10,000 rows, but computes two original field scores and reuses 19,998
equivalent scores. Median heap/scoring time falls from 303.480 to 13.169 ms,
with 23.092 ms in membership/candidates and 3.514 ms in original highlights;
median complete time is 40.627 ms. Field counters exclude highlighting calls.

The unchanged acceptance benchmark is
`target/continuation-score-cache-bench.json`, SHA-256
`67b269e9b78f046435b9f06956e45dcf80c7ab4a008195b0c221c140048d6d8b`.
All 1,040 sample checksums, hit counts and match counts equal the preceding
bitset report exactly. Near-match p95 improves from 367.56 to 50.85 ms and
long-match p95 from 24.34 to 19.42 ms. Word p95 is 39.11 ms; decomposed-accent
p95 is 77.39 ms. Only 1/26 cases passes, and the worst sample is 83.181 ms.
The score cache is retained for the measured long-query improvement, but D48
remains unmet. No compiler ran during measurement. This comparison is not an
interleaved controlled A/B run, and it does not attribute small differences.

The isolated positional diagnostic is recorded in
`target/continuation-index-membership-profile.log`. Its retained index costs
48,949,712 bytes (about 46.7 MiB) before allocator overhead, covers 190,000 of
200,000 fields and takes 347.267 ms to construct on this run. Common cases
regress: broad membership median rises from 7.626 to 13.815 ms, word from
29.597 to 32.276 ms and rare identity from 25.150 to 32.689 ms. Uppercase,
CJK and no-match cases improve, but that mixed result does not justify
production integration. It remains test-only; compact occurrence lists or
another approach require fresh measurement. These figures time membership
alone, not D48's complete query.

The final workspace attempt in `target/continuation-score-cache-gate-2.log`
passes dependency boundaries, formatting and workspace Clippy, then records
205 passing tests and the same two search failures. No test is ignored or
filtered. As before, failure stops the gate before store/xtask/documentation
tests and the application smoke stage. The first attempt's test-style Clippy
finding was corrected without suppression; its log is retained separately.

## Shared membership for fields differing only in digits

The positional prototype's memory cost and common-query regressions led to a
smaller alternative. For a query containing no ASCII digit, fields differing
only in ASCII digits have identical ordered membership. Replacing those digits
with `0` preserves every other byte, scalar, length and ASCII classification.
No query case pair can match a replaced digit: the existing Unicode rules
retain UTF-8 width, so a non-ASCII character cannot become an ASCII digit.
This applies to real NUL bytes and to zero/one query-character deletions too.
It does not authorize using canonical text for scoring or highlights.

`field_groups.rs` interns these canonical fields with shared string ownership,
reference counts and reusable slots. Limits are 2,048 live groups, 2 MiB of
unique key payload and 1,024 bytes per field. Each row holds two optional
four-byte IDs. Existing keys remain reusable at capacity; an uncached or
overlength field uses ordinary matching. Updates preserve unchanged field
IDs, including fallback `None`, and release changed fields. Removing the last
reference removes the canonical text immediately. Validation and generation
checks precede all mutations.

`catalogue/membership.rs` creates a fresh result table for each digit-free
zero/one-deletion query. It retains the original mask rejection and evaluates
a group only when needed; all rows still contribute independently to their
title/address union, scope counts, boosts and ordering. Original fields still
receive scores and highlights. Queries containing ASCII digits use the
preceding path. No memo survives a query or a catalogue mutation. Independent
reviews covered the proof, pool, integration and mutation regressions.

The test-only diagnostic in `membership/grouped.rs` reuses the positional
experiment's unchanged 100,000-row field shapes. It includes fresh memo
allocation in each timed scan and compares every row's membership outside
timing. `target/continuation-field-groups-profile.log` records 728 groups,
45,477 bytes of key text, 800,000 bytes of row references and 143.818 ms to
construct the groups. These byte counts exclude map/slot/Arc/free-list and
allocator overhead, source strings and query buffers; they are not a total
memory measurement.

Paired median membership time improves from 28.683 to 1.695 ms for `word`,
50.832 to 1.540 ms for decomposed accent and 23.073 to 2.010 ms for long near
match. Numeric rare identity remains essentially unchanged (23.541 versus
23.535 ms) and evaluates no groups. These figures justify integration for a
full-path experiment, not acceptance. The preliminary complete search run
passes 79 tests with the same two NUL failures; focused Clippy passes. The
production integration's gate and unchanged benchmark are recorded below
once measured. Unique-field catalogues may share little and use more fallback;
the fixed pool cap bounds retained overhead without reducing search semantics.

The production integration passes independent review and the dependency,
formatting and workspace Clippy stages. `target/continuation-grouped-membership-gate.log`
records 221 passing tests and the same two NUL failures (85/87 search tests),
with none ignored or filtered. Later gate stages remain unreached. The added
regressions cover shared title/address leases, individual field updates,
swap removal, last-reference cleanup, capacity/overlength fallback, generation
failure atomicity and fresh results after slot reuse.

The unchanged full benchmark, `target/continuation-grouped-membership-bench.json`,
has SHA-256 `ac127ac9a4f2dd8c5f83a07b0fcc8913a722dbd3da208257ab64e7cdc030be85`.
Every checksum, hit count and match count in its 1,040 samples equals the
preceding score-cache report. D48 improves from 1/26 to 6/26 passing cases:
blank, CJK, emoji, no-match, tabs scope and commands scope pass this run.
Word p95 falls from 39.11 to 6.91 ms, decomposed accent from 77.39 to
17.70 ms and long near match from 50.85 to 30.78 ms. Rare identity remains
35.18 ms; the worst sample is 36.697 ms. Corpus construction rises from
153.0114 to 276.3711 ms. The memory/build cost is retained for the substantial
measured query improvement. No compiler ran during measurement; these are
diagnostic comparisons, not three passing acceptance runs or a controlled A/B.

The isolated stage report, `target/continuation-grouped-membership-stages.log`,
also preserves complete exhaustive results. Its near-match membership/candidate
median is 1.982 ms, heap/scoring 10.211 ms and highlights 3.235 ms, totaling
15.752 ms on its separate diagnostic corpus. Full-path candidate processing
and scoring now matter more than scanning shared text.

Two next experiments have source-reviewed designs but no implementation or
passing measurement yet. A fresh group-ID score table could score original
representatives once, including Unicode: replacing unmatched ASCII digits
preserves the pinned prefilter windows, masks, gap penalties and exact bonus.
Keep the current field cap and leave highlighting unchanged while checking
that proof with score differentials. Numeric queries could share transitions
through fixed text spans and consume original digits per row, with bounded
compilation and ordinary matching fallback. Neither changes D48 or permits
reusing answers between queries.

## Reusing original scores through live field groups

The preceding membership work leaves scoring shared fields expensive: the
ASCII signature cache still copies and hashes each field, and Unicode fields
cannot use it. `score_cache.rs` now accepts the catalogue's live group IDs
for a fresh digit-free query. It scores the first original representative and
stores that result in a bounded slot table. Rejection, accepted zero and an
unseen slot remain distinct. Numeric queries and ungrouped fields retain the
preceding signature-cache path. No result survives the query.

The proof concerns the pinned frizbee implementation, not fuzzy matching in
general. Replacing ASCII digits that no query case pair can match preserves
the prefilter window, UTF-8 boundaries, matching masks, character classes,
gap penalties and exact-match bonus. The pool's 1,024-byte field cap also
keeps its fields below the matcher's long-field greedy branch. Unicode case
pairs retain scalar width and cannot turn a non-ASCII query character into
an ASCII digit. Scores are still computed from original strings; highlights
remain independent and use the originals. Existing NUL defects are neither
filtered nor corrected by this optimization.

Independent source review covers the cache, catalogue integration and score
differentials for Unicode, smartcase, real NUL bytes, field-size boundaries,
accepted zero, rejection, fresh queries and reusable group slots. The cache
borrows no IDs across mutation, and catalogue ranking holds exclusive access
throughout its lifetime. Inconsistent IDs or overlength grouped fields report
an error. The existing computed/reused counters include both reuse paths.

The same experiment removes wide division from ordinary recency boosts.
`catalogue.rs` uses the exact unsigned timestamp difference and checked
64-bit multiplication, falling back to the original 128-bit numerator on
overflow. Future timestamps retain age zero; expired or zero-weight boosts
remain zero. The existing arithmetic test compares the literal old formula
at timestamp extremes, window edges and multiplication thresholds. This is
an arithmetic optimization, not a change to ranking policy.

`membership/numeric.rs` remains a test-only experiment. For an ASCII query
containing digits, shared non-digit spans compile transitions for every
possible matched-prefix length; each row still supplies its original digits.
Two endpoints distinguish an exact subsequence from one with a query
character deleted. The supported range is 1–63 query bytes and zero or one
deletion. Fresh per-query compilation is limited to one million byte-state
steps and 512 KiB of retained span/table payload, plus bounded cache slots.
Unsupported inputs and exhausted limits use ordinary matching. Differential
tests require the compiled path to execute; separate tests cover fallback.
The paired 100,000-row diagnostic includes compilation and allocation, and
checks every row outside timing. It is not the complete D48 benchmark.

The next ranking design is reviewed but unimplemented. Rows with two live
digit-free-query field groups could enter the exact selected heap directly
when their scores are already cached, or when a strict new-group/work budget
allows computing them. All other rows would retain their conservative bounds
and the existing pending heap. Each row enters one lane, but every matching
row still contributes to all scope counts. Exact selected scores make the
existing bound cutoff valid regardless of which lane supplied them. Charge
new groups and a conservative query-length-by-field-length work estimate
before scoring, so unique fields cannot trigger unbounded eager matching.
First compare this private strategy with exhaustive results and the current
strategy, including zero budget, mixed lanes, scopes, ties and mutations.
No pair-score map is justified yet: two dense field-score reads and a maximum
already recover the exact base score without another allocation or hash.

A separate reviewed but unimplemented highlight experiment could reuse raw
traceback indices within a digit-free query's live field groups. The stronger
source argument preserves every dynamic-programming cell, matching mask,
UTF-8 boundary and deterministic tie, including the uncapped empty-trace
retry. An original representative must supply the finalized indices; unseen,
rejected and accepted-with-empty remain different states. Every reuse must
still call `highlights::ranges` on that row's original text, preserving the
NUL range error and the both-fields-rejected consistency check. Raw-index,
range and error differentials are required before integration. Exact-byte
reuse is a simpler alternative that would also support digit queries but
would miss rows whose only differences are numeric identifiers. Neither
highlight experiment is implemented in this checkpoint.

The final coordinated gate is
`target/continuation-group-score-gate-final-2.log`: dependency boundaries,
formatting and workspace Clippy pass; 235 tests pass and the same two NUL
highlight regressions fail, with 99/101 search tests passing. None is ignored
or filtered. The gate stops before later store/xtask/documentation tests and
its smoke stage. The initial test compiler was deliberately stopped after
two reference calls instantiated `&String` matcher kernels; both now pass
`&str`. Its log and the intervening formatting failure remain preserved.
The corrected optimized test build took 3m 27s. This does not establish a
controlled compilation-speed comparison against the interrupted build.

The unchanged complete benchmark is
`target/continuation-group-score-bench.json`, SHA-256
`898453853a448898339ed6179ccdca796c743ab56e47cd30e8118e034efd888f`.
All 1,040 sample checksums, hit counts and match counts exactly equal the
preceding grouped-membership report. Seven of 26 cases pass: uppercase joins
blank, CJK, emoji, no-match, tabs scope and commands scope. Long near-match
p95 falls from 30.78 to 14.81 ms, history scope from 35.55 to 14.93 ms and
composed accent from 18.09 to 14.77 ms. Rare identity remains 34.63 ms and
broad character is 14.31 ms. The worst sample is 37.6499 ms; corpus build is
270.7451 ms. No compiler or other test job ran during measurement. D48 still
fails, and three passing fresh-process runs remain required.

The isolated stage report, `target/continuation-group-score-stages.log`,
preserves complete exhaustive results. Long near-match still scores 10,000
rows, but computes only two original field scores and reuses 19,998 scores.
Its median membership/candidate time is 1.854 ms, heap/scoring 2.052 ms and
highlighting 3.413 ms, totaling 7.371 ms. Broad-query candidate work remains
4.664 ms and heap/scoring 2.572 ms. These separate diagnostic timings support
the direct-ranking and highlight experiments; they are not acceptance values.

The numeric diagnostic in `target/continuation-numeric-spans-profile.log`
passes all row comparisons. It times fresh compilation and cache allocation
over 24 cases with two warmups and five alternating paired samples. Rare
identity changes from 24.487 to 22.972 ms and repeated `111111` from 48.272
to 37.955 ms, but `12` regresses from 12.383 to 14.211 ms and the 63-byte
query from 13.942 to 46.911 ms. Digit-free cases execute the same path on both
sides and also vary; do not attribute that noise to the prototype. The rare
case uses 18,552 bytes of span/table payload and 34,944 bytes of cache slots;
these exclude allocation overhead and transient/query storage. The 63-byte
case consumes 999,871 compilation steps, compiling 565 groups and falling
back on 162. These mixed results do not justify production integration.

Read-only review identified two concrete next numeric experiments. Restore
the baseline's necessary byte-length rejection before compiling a group:
an ASCII query needs at least its length minus allowed deletions in distinct
field bytes. Preserve bad-ID and inconsistent-length errors with a validated,
memoized rejected state. For numeric-only queries, non-digit spans cannot
advance either endpoint, and the deletion closure is already represented;
omit fixed-span transitions and consume only original digit runs. Check
boundary lengths, Unicode/NUL fields, repeated digits and both budgets, and
measure compilation separately from row evaluation while retaining total
timing. These corrections are not yet implemented; scalar transition and
cache lookup costs remain hypotheses requiring attribution.

The retained production changes are built in
`target/local/atlas-app-group-scores.exe`, SHA-256
`03e314bd3a689d30f28a49e8df6f70b530960e4937c391aea83e7dbc636dafa7`.
The explicitly awaited normal-desktop `--smoke` run uses a fresh synthetic
profile, exits 0 and has empty stderr. Build and smoke logs begin
`target/continuation-group-score-app-`. It does not diagnose the original
WebView2 breakpoint or establish interactive acceptance.

## Original highlight traces and bounded ranking experiments

The preceding profile shows that repeated highlight tracing and candidate-heap
work remain expensive after score reuse. This continuation implements the
reviewed original-trace cache and two private experiments without changing the
corpus, ranking policy or D48. Source review and runtime verification are
complete; the measurements below retain the two known failures and unmet D48
target. The earlier gate, binary and timing records above remain historical
evidence, not results for these changes.

`catalogue/highlight_cache.rs` retains finalized byte indices within one
nonempty, digit-free query. A live field group supplies the identity; its
first original field supplies the trace. The existing proof preserves every
matching mask, dynamic-programming cell, UTF-8 boundary and deterministic
trace tie when unmatchable ASCII digits change. Rejected fields, accepted
fields with empty traces and unseen groups remain distinct. An accepted empty
trace still receives the existing uncapped retry through the shared
`finalized_indices` helper. No cached score or trace is computed from canonical
text, and no result survives the query or a group-slot mutation.

Every original title and address still passes through `highlights::ranges`;
two rejected fields still produce `InconsistentMatch`, and out-of-range NUL
indices still produce the existing range error. Numeric queries, fields
without IDs and fields over the pool's 1,024-byte limit use original tracing.
The cache retains at most 256 KiB of raw index payload plus the pool's bounded
2,048-slot table; a full payload budget falls back without dropping a hit.
The exhaustive query path remains uncached. Tests compare raw indices against
the literal original retry procedure, compare ranges and errors, and compare
complete queries through removals and reused slots. They cover Unicode case
pairs, equal-score trace ties, NUL, length boundaries, recovered and empty
traces, payload exhaustion and numeric fallback. Observer counters distinguish
actual matcher calls, including retries, from reused traces. Exact-byte reuse
was the simpler alternative, but would miss equivalent numeric identifiers;
caching final ranges would skip each original field's required validation.

`catalogue/hybrid.rs` is a test-only experiment to reduce heap work. It counts
every matching target before scope or limit selection. A visible row with two
live groups may enter the selected heap with an exact score when those groups
are already scored or fit the eager-work budget. Each row enters either that
path or the ordinary conservative-bound candidate heap, never both. Exact
selected scores preserve the ordinary cutoff and target tie ordering. The
default budget allows 64 newly scored groups and 1,000,000 estimated matrix
cells, charging `(query bytes + 1) * (field bytes + 1)` once per new group.
Overflow, unsupported inputs and exhausted budgets retain the ordinary path.
Tests compare all result fields with exhaustive ranking under zero, mixed and
generous budgets, scopes, limits, boosts, ties and mutations. The diagnostic
includes complete query work and fresh allocation. Unbounded eager scoring
could regress unique fields; a pair-score map adds an allocation and lookup
where two field-score reads already suffice. Neither is part of this prototype.

`membership/numeric.rs` remains test-only and now applies the reviewed
corrections. It validates the group ID, then rejects a field whose byte length
cannot hold the ASCII query minus its permitted deletion before building any
span tables. A cached rejection retains the original length check. Numeric-only
queries omit fixed-span transitions: no non-digit byte can advance either
already deletion-closed endpoint, so only the row's original digit runs matter.
The existing limits remain 1–63 ASCII query bytes, zero or one deletion,
1,000,000 compilation steps and 512 KiB of retained span/table payload, plus
bounded slots. Unsupported inputs and exhausted limits keep ordinary matching.
New differentials cover length thresholds, repeated digits, Unicode/NUL fields,
both budgets, invalid IDs and zero fixed-span work. Separate instrumented scans
attribute preparation, compilation and residual row evaluation; paired timing
samples retain their prior method and exclude those per-group timers. These
experiments still need measured evidence before production promotion.

No pending approval is waived: the frizbee source correction, interactive
window access and later slice maps remain separate gates. This section claims
neither P1-05 completion nor a fix for the original WebView2 breakpoint.

## Unimplemented grouped-count and range-ranking design

The private hybrid still visits every row. Its isolated report,
`target/continuation-hybrid-ranking-profile.log`, shows only a small broad-query
median change, from 9.591 to 9.321 ms, while composed accent regresses from
8.780 to 9.159 ms. A different derived index could avoid most row visits when
many fields share existing canonical groups. This is a source-reviewed design,
not implemented work or an acceptance result. The first next step should be an
occupancy diagnostic: count live field pairs, populated boost buckets, largest
bucket, ungrouped rows, retained storage and update work. Do not choose caps or
implement a tree before those measurements establish a useful operating range.

For a nonempty query without ASCII digits and a supported zero/one-deletion
budget, a live `(title group, address group)` pair has one exact membership
answer and one base score. Preserve title/address OR membership and maximum
field score; never add the two fields' counts or scores. Each pair stores four
target-kind counts, added once when it matches, before scope and display-limit
selection. Existing presence masks remain conservative: changing digits cannot
change an ASCII digit-free query's relevant bits, and Unicode queries or fields
already disable that rejection. Exact membership still decides the pair. Keep
a live representative `Target` and resolve its original fields through the
catalogue for scoring and bounds; canonical text must not replace the original
score or highlight input. Representative removal selects a surviving member.

Within each pair, partition members by target kind, `open`, and frequency bit
length `32 - visits.leading_zeros()`. Those values make the open and frequency
boosts constant within a bucket for every current `BoostWeights`. Kind belongs
in the bucket so a narrow scope does not descend through unrelated rows. Sort
members by descending signed `Some(last_used)`, then `None`, with `Target` as
the stable secondary order. Recency is nondecreasing in `last_used` for every
allowed weight, positive window and `now`: future timestamps receive the full
weight, expired timestamps and `None` receive zero. This remains true at signed
timestamp extremes and for zero recency weight.

A contiguous bucket range can therefore be bounded by the pair's base score,
its constant boosts, and the first member's recency. Pair that upper score with
the minimum `Target` anywhere in the range. Every member has a lower score, or
an equal score and a target no smaller than this bound. The existing descending
score/ascending target cutoff remains safe even when the maximum timestamp and
minimum target belong to different members. A heap expands only promising
ranges until individual rows can enter the ordinary selected heap. Start with
the existing conservative field-score bound and compute the original pair's
exact base lazily when needed, retaining only query-local field scores. Final
rows still use the ordinary original-text highlight and error checks. Loose
range bounds can require linear work in the worst case; this is not a proof of
five-millisecond queries.

The smallest prototype would use sorted member vectors and a flat segment tree
whose nodes store the offset of the minimum target, rather than another copied
target or string. Stable `Target` members avoid the stale-row-index hazard of
`swap_remove`; use the catalogue's existing position map when resolving a row.
Bound total members by `C`, pairs by `P`, buckets by `B`, and members per bucket
by `L`. Padded segment trees require fewer than `4C` integer nodes. Retained
storage is bounded by the member records, those nodes, `P` pair headers, `B`
bucket headers and at most `C` target-to-bucket locator entries; account for
allocated capacities and map/allocator overhead separately. Vector insertion
and tree rebuilding cost at most `O(L)` per affected bucket. This is simpler
than introducing an augmented balanced tree or a dependency. A query also caps
its range frontier at `R`, with bounded pair/group memo tables; the existing
`O(min(limit, row count))` selected results remain necessary. Hitting a query
auxiliary cap discards the private attempt and restarts the ordinary strategy,
without retaining partial counts or results.

Initially require every row to fit the derived index; otherwise use the
ordinary query path. This avoids discovering fallback rows through another
full catalogue scan, and avoids an unbounded retained fallback list. That
restriction materially limits generality: a real high-cardinality catalogue
can exhaust the existing 2,048-field pool even when the repeated synthetic
shapes fit. Such data would retain today's scan costs. A later bounded partial
design could store at most `F` fallback targets and combine their ordinary
counts/ranking with disjoint indexed rows; exceeding `F` must still fall back
for the whole query. Compare its occupancy and memory cost with improvements
to the exact scan for unique fields, such as algorithmic work reduction,
vectorization or parallel evaluation. None authorizes a dependency or worker
change. Do not substitute an easier corpus or claim the general performance
problem solved merely because repeated shapes meet D48.

Incremental maintenance is essential. `crates/xtask/src/benchmark.rs` changes
row 99,999 before every timed query, rotating query order by `pass * 7`.
`refresh_generation` rebuilds that row and appends `revision-N`; changes in the
number's width also change its canonical title length. A full rebuild after
each generation would run inside every query. Maintain only query-independent
metadata during upsert/remove and report that maintenance cost separately.
Preserve today's validation, capacity and generation checks before any
mutation; identical upserts and missing removals remain no-ops. Detach an old
indexed member before field leases can be released or group IDs reused, then
attach the replacement after its new IDs are settled. Metadata edits can move
boost buckets or timestamp positions without changing field leases. Remove
empty buckets and pairs, their counts, locators and representatives before a
slot can describe unrelated fields. The derived index does not take extra
field leases or keep deleted original text alive. A cap refusal preserves the
valid catalogue mutation and explicitly makes the ordinary query path eligible.
Failed validation or exhausted generation must leave both structures unchanged.

Required differentials include every scope and arbitrary boost/window values,
future/expired/absent/extreme timestamps, recency plateaus and zero weights,
target ties across kinds, shared title/address groups, original masks and union
counts, partial and final representative removal, same-slot replacement,
metadata moves, swap removal, caps, and failed mutations. Keep digit queries,
unsupported budgets, known NUL errors, the complete result oracle, and the
unchanged update-before-query benchmark. The simpler hybrid remains the
baseline; this larger index is justified only if measured query savings also
justify its retained memory and incremental maintenance.

## Highlight-trace checkpoint measurements

The original-trace cache is retained in production because it preserves the
complete result oracle and removes repeated work without changing ranking.
The revised numeric-span and hybrid-ranking paths remain private test-only
experiments: neither has consistent enough gains to justify promotion.
Independent source reviews found no remaining correctness issue in these
changes. No dependency, security or approval boundary changed.

`target/continuation-highlight-traces-gate.log` records dependency boundaries,
formatting and workspace Clippy passing, followed by 246 passing tests and the
same two frizbee NUL-highlight failures. Search accounts for 110 passes and
two failures; the other passes are 36 app, 30 core, 48 platform unit, four
backup and 18 storage tests. No tests were ignored or filtered in this gate.
Later workspace tests and the gate's final smoke stage were not reached.
The prior focused full search run is preserved in
`target/continuation-hybrid-traces-search-tests.log` with the same 110/112 result.

The unchanged full benchmark is
`target/continuation-highlight-traces-bench.json`, SHA-256
`e3e4723c1dc634cb2040680fd1f26d82d95c2fad229d052f998f39ebabdf48bd`.
All 3,120 serialized checksum/hit/match-count values across 1,040 samples
exactly match `target/continuation-group-score-bench.json`; comparison used
the integer text without lossy numeric conversion. Nine of 26 cases meet D48,
so the benchmark correctly exits with failure. Corpus construction takes
282.5427 ms and the worst retained query sample takes 38.4331 ms. The following
nearest-rank p95 values are milliseconds. These are successive process runs,
not a controlled claim that every timing difference comes from the change.

| Case | p95 ms |
| --- | ---: |
| blank | 2.6994 |
| broad-character | 12.5203 |
| typed-two | 17.0250 |
| typed-three | 14.7028 |
| typed-four | 14.7386 |
| word | 5.9784 |
| missing-character | 6.7155 |
| transposed-characters | 6.1518 |
| uppercase | 4.4590 |
| subsequence | 14.7092 |
| common-word | 10.2952 |
| two-words | 7.7211 |
| address-only | 4.9755 |
| address-suffix | 15.9026 |
| rare-identity | 36.2976 |
| composed-accent | 14.4972 |
| decomposed-accent | 15.4739 |
| cjk | 4.2449 |
| emoji | 4.0708 |
| no-match | 3.3642 |
| long-match | 7.0360 |
| long-near-match | 14.5201 |
| tabs-scope | 4.5935 |
| saved-scope | 4.7975 |
| history-scope | 14.0941 |
| commands-scope | 3.7537 |

The isolated diagnostic `target/continuation-highlight-traces-stages.log`
preserves exhaustive results. Near-match highlighting takes 0.190 ms versus
3.413 ms in the preceding stage report: three actual trace calls, including
the retry, supply 98 reused traces. Its complete query takes 4.204 ms versus
7.371 ms previously, with 1.972 ms membership and 2.005 ms heap/scoring work.
This does not replace the complete acceptance benchmark, whose long near-match
p95 remains 14.5201 ms. Broad-query membership and heap work still dominate.

`target/continuation-hybrid-ranking-profile.log` retains full-result equality
for 14 paired cases on 100,000 rows. The default 64-group/1,000,000-cell budget
changes broad median from 9.591 to 9.321 ms and long near-match from 9.554 to
6.873 ms, but composed accent regresses from 8.780 to 9.159 ms and long match
from 4.368 to 4.580 ms. Numeric fallback and no-match cases exhibit timing
variation despite no direct-ranked rows. The mixed evidence keeps it test-only.

`target/continuation-numeric-spans-revised-profile.log` preserves every row's
membership for all 24 cases. Baseline versus revised-prototype medians improve
from 12.856 to 8.511 ms for `0`, 11.396 to 9.927 ms for `12`, and 49.752 to
36.629 ms for `111111`. Numeric-only cases now allocate no fixed-span tables.
The length-63 case improves from 13.617 to 12.144 ms, removing the prior large
regression: 663 groups reject before compilation and five use fallback.
However, `12ab34cd56` changes from 8.459 to 8.746 ms. Separate instrumented
attribution for `12` reports 0.003 ms preparation, 0.288 ms compilation and
7.965 ms residual evaluation; those timers are excluded from paired samples.
The residual includes row access and dispatch, not just digit-state work.

A reviewed next numeric experiment would precompute the existing digit-state
transition for queries up to 15 bytes: 160 bytes for zero deletions or 5,120
bytes for one deletion, packing the two endpoints while scanning digit runs.
Longer queries keep the current path. Compare every endpoint/digit pair with
the scalar transition, retain the existing row oracle, and charge construction
in query timing. This is unimplemented and its benefit remains a hypothesis.

The new binary is `target/local/atlas-app-highlight-traces.exe`, SHA-256
`a84d7dfc9fc834332401e31505d3eda26cad0173c86e8036b35c7d5ff074f9bf`.
The build log is `target/continuation-highlight-traces-app-build.log`; the
explicitly awaited fresh synthetic normal-desktop smoke exits 0 with empty
stderr in `target/continuation-highlight-traces-app-smoke.*`. Existing windows,
binaries and uncommitted work remain preserved. This smoke check neither
diagnoses the original WebView2 breakpoint nor establishes interactive
acceptance. The frizbee patch, window access and later slice maps remain pending.

## Packed digits and index occupancy follow-up

Before adding a larger retained index, this continuation measures both a small
numeric recurrence change and the index's likely coverage. Production query
behavior remains the highlight-trace checkpoint above. Neither test-only
experiment changes the D48 corpus or grants a pending approval.

`membership/numeric/digits.rs` precomputes the existing scalar digit transition
for eligible queries of at most 15 bytes. Its zero-deletion table uses 160
bytes; its one-deletion table uses 5,120 bytes. Packed endpoints preserve the
exact recurrence and early acceptance, and longer queries use the scalar path.
The endpoint/digit oracle compares every valid endpoint pair for both budgets.
Existing numeric differential fixtures now compare scalar and packed modes.
The 24-case diagnostic compares ordinary grouped matching, revised scalar
spans and packed spans in alternating order. Every mode includes query setup
inside timing; table construction is included in packed setup. Construction
steps and table payload are reported separately from fixed-span storage.
This is a membership-only experiment, not complete-query acceptance.

`catalogue/occupancy.rs` measures six separate 100,000-row fixtures: the
unchanged acceptance-shaped diagnostic corpus, repeated fields, one-percent
and ten-percent alphabetic unique fields, entirely alphabetic unique fields,
and repeated fields with empty or overlength values. Alphabetic identifiers
avoid the digit canonicalization that could otherwise disguise uniqueness.
It reports group coverage, fallback rows, field pairs, boost buckets, largest
bucket, explicit hypothetical storage and illustrative cap sensitivity. Its
packed-record estimates exclude map/capacity/alignment/allocator overhead and
are not measurements of an implemented index. Pair and bucket kind totals,
live group text and complete ordinary/exhaustive results are checked. Revision
probes reproduce the benchmark's final-row replacement shape; the retained
group-text change flag does not claim unretained fields are unchanged.

A separate production experiment moved query-wide score-bound arithmetic out
of each field calculation. Source review confirmed equivalent saturated
arithmetic, Unicode cutoff and original-text exact bonus. Its full report,
`target/continuation-query-bounds-bench.json`, SHA-256
`91f02f0b22d22c3a8c8da555bc07a1ff35da5cd41b59fe9fc04143fc95ea27e2`,
preserves all 1,040 sample checksums, hit counts and match counts. However,
only eight cases meet D48, compared with nine in the previous report, and
several broad cases are slower. The experiment and its new reference test
were removed together; all pre-existing tests remain. These successive runs
do not prove causation, but provide no convincing reason to retain the extra
structure. The benchmark report remains evidence of the rejected experiment,
not the current production source.

### Measured outcomes

The independently reviewed packed diagnostic passes all 24 row comparisons in
`target/continuation-packed-digits-profile.log`, SHA-256
`4f2fabcb3af25f2cf4d9ddae2201130d46db7de126cd316504c52fc76aac495c`.
The table below gives median membership-only milliseconds, including fresh
query preparation, for ordinary grouped matching, scalar span compilation and
packed digit transitions. Ranking and highlights are not timed here.

| Query/case | Ordinary | Scalar spans | Packed spans |
| --- | ---: | ---: | ---: |
| rare identity | 24.307 | 22.489 | 18.998 |
| `0` | 13.353 | 8.382 | 9.857 |
| `12` | 13.805 | 12.443 | 11.300 |
| `1234` | 20.632 | 17.492 | 12.647 |
| `099983` | 30.136 | 24.930 | 17.767 |
| `catalogue-126` | 25.570 | 21.941 | 19.326 |
| `ROW-099983` | 27.413 | 25.337 | 19.955 |
| `r0w` | 28.940 | 24.977 | 23.014 |
| `111111` | 47.431 | 33.111 | 27.869 |
| `12ab34cd56` | 6.267 | 6.643 | 5.390 |
| length 63, scalar fallback | 11.463 | 12.267 | 13.358 |

Packed transitions help several multi-digit cases but regress against scalar
spans for `0`. The unsupported length-63 case follows the same scalar algorithm
in both span modes, yet its timings also differ, illustrating run variation.
Separate rare-identity attribution reports 0.021 ms packed preparation,
including 0.012 ms table construction, 1.816 ms span compilation and 18.215 ms
remaining row evaluation. Table preparation is not the main remaining cost.
These results retain the experiment for further investigation, without
promoting numeric spans into production or claiming the five-millisecond goal.

The independently reviewed occupancy diagnostic passes in
`target/continuation-range-occupancy-profile.log`, SHA-256
`c7f37587c47cf5e9339ce28ff4e66721929c1a62a9cd1d4f660a6614620fc9cc`.
Every fixture contains 100,000 rows. Empty fields are valid grouped values;
fields over 1,024 bytes and fields lacking a retained group need fallback.

| Fixture | Grouped rows | Fallback rows | Field pairs | Boost buckets | Largest bucket |
| --- | ---: | ---: | ---: | ---: | ---: |
| Existing corpus | 100,000 | 0 | 1,082 | 7,209 | 509 |
| Repeated | 100,000 | 0 | 1 | 46 | 38,926 |
| One percent unique | 100,000 | 0 | 1,001 | 1,045 | 38,537 |
| Ten percent unique | 91,023 | 8,977 | 1,024 | 1,068 | 35,033 |
| Entirely unique | 1,024 | 98,976 | 1,024 | 1,024 | 1 |
| Repeated empty/long | 90,909 | 9,091 | 2 | 85 | 30,337 |

The existing corpus would need an estimated 3,200,000 member bytes, 1,057,408
padded tree bytes, 43,280 packed pair-record bytes and 194,643 packed bucket
bytes before the explicitly excluded overhead. The fully unique case would
need 1,583,616 bytes merely for fallback targets while indexing only 1,024 rows.
An all-rows eligibility rule therefore throws away useful coverage in mixed
catalogues, whereas constructing a partial index for almost entirely unique
data buys little. The next index prototype should combine disjoint indexed
rows with a bounded fallback-target list and use the ordinary scan when its
resource envelope cannot support useful coverage. Its caps must limit memory
and update work, never search results. A fully unique catalogue still needs a
better general scan; repeated-field success must not be presented as solving it.

The revision probes preserve complete query equality and show why retained
group identity cannot be inferred from original text alone: a saturated pool
can leave a changed field without an ID. The diagnostic label was corrected
to `retained_group_text_changed` after the first gate's compilation; the
isolated occupancy run and final formatting/Clippy checks use that correction.
No retained range index has been implemented, and these occupancy estimates
do not measure its future mutation cost.

The next range-index experiment has a reviewed provisional resource envelope:
131,072 grouped members, 16,384 explicit fallback targets, 2,048 pairs and
16,384 boost buckets. A bucket may hold all 131,072 members; rejecting a large
identical bucket would reject an ideal grouping case. These limits permit
at most 4 MiB of member payload, less than 2 MiB of tree nodes, about 80 KiB
of packed pair records, 432 KiB of packed bucket headers and 256 KiB of fallback
targets. Lookup/capacity/allocation overhead remains additional and must be
bounded and measured before promotion. A frontier limit must restart the
ordinary query if reached, never omit a candidate range.

Stable targets and original metadata identify affected buckets before field
IDs are released; the existing position map resolves rows after swap removal.
Sorted member vectors and tree rebuilds give bounded but potentially expensive
linear update work for a large bucket. Compare that cost before adopting the
design; sorted blocks are the alternative if full bucket rebuilds are too slow.
On capacity failure, discard the incomplete derived index and keep ordinary
queries. Retain cheap exact grouped/fallback counters so crossing back inside
row limits can trigger a mutation-time rebuild. Pair/bucket overflow needs
bounded periodic rebuild attempts after structural mutations, rather than
permanent disablement or a whole rebuild on every query. A suggested retry
interval is `max(1, current rows / 16)` qualifying mutations; implementation
and tests must establish the exact lifecycle and latency before promotion.
Validation, no-op and generation checks precede all derived-state changes.

Final verification is `target/continuation-packed-occupancy-final-gate.log`:
dependency boundaries, formatting and workspace Clippy pass, then 250 tests
pass and the same two NUL-highlight tests fail. Search is 114/116; no test was
ignored or filtered in the gate, and later stages remain unreached. The initial
gate and post-label isolated reports are preserved separately. Only test-only
code remains from this continuation, so the production binary and its preceding
smoke evidence remain `target/local/atlas-app-highlight-traces.exe`. No new
application build or smoke acceptance is claimed. All pending approvals remain.

A smaller next numeric experiment is also source-reviewed but unimplemented:
retain ten saturating ASCII-digit counts per original field. For each queried
digit, `max(query count - field count, 0)` is a lower bound on required query
deletions; summing deficits across different digits remains necessary. Reject
when that sum exceeds the supported typo budget, then retain ordinary matching
for everything else. Do not add it to the existing distinct-byte deficit,
because those bounds overlap. This directly addresses repeated-digit queries
that the presence mask cannot reject, and works without group coverage.

ASCII prefilters consume distinct field positions, Unicode case mapping cannot
create ASCII digits, and padded NUL bytes are not digit `0`. Keep the initial
experiment within the existing zero/one-deletion path and queries up to 63
units; uncapped highlight retries remain unchanged. Ten `u8` counters per
field saturating at 255 would use 20 logical bytes per row in a separate vector
(2,000,000 bytes for 100,000 rows, excluding capacity/allocator overhead).
Check repeated and combined deficits, query and counter saturation boundaries,
Unicode/NUL fields, mutations, and complete result/error equality. Measure the
extra update/storage cost and whole-query performance before promotion. A
group-wise digit-position bitset index is a more complex alternative; these
counts should be evaluated first.

## Original digit counts and partial range-index implementation

Repeated digits expose a weakness in a presence-only mask: one missing
distinct digit does not account for five missing occurrences. The new
`digit_counts.rs` retains ten saturating byte counters per original field.
For supported queries of up to 63 Unicode scalars and zero/one deletions,
the sum of independent ASCII-digit deficits proves rejection when it exceeds
the budget. Counts saturate at 255, above every supported requirement.
The filter never adds its deficit to the existing presence-mask deficit,
never combines title and address counts, and never filters uncapped highlights.
Unicode case mapping and padded NUL bytes cannot supply ASCII digits.

`Catalogue` keeps the two counters aligned with each row through insertion,
replacement and swap removal, after validation and generation checks. Unchanged
and failed mutations retain the existing metadata. `GroupedMembership` applies
the new rejection before ordinary membership; digit-free and higher-budget
queries retain their existing paths. The exhaustive matcher remains unfiltered.
The new vector needs 20 logical bytes per row, or 2,000,000 bytes for 100,000
rows, plus vector capacity and allocator overhead. Source review and tests
cover independent deletion enumeration, pinned matcher acceptance, repeated
and combined deficits, scalar/counter limits, Unicode/NUL, scope union, result
errors and mutation atomicity.

`catalogue/range_index.rs` and its `ranking.rs`/`tests.rs` children implement
the reviewed partial range design as a test-only experiment. The ordinary
catalogue has no range index by default. The diagnostic times an explicit
bulk build after ingestion, sorts each bucket once, and then exercises real
incremental maintenance before each paired query. This avoids quietly adding
quadratic per-insert tree construction to every existing test and does not
settle a production loading strategy. Row, pair, bucket, fallback and frontier
limits remain resource limits; unavailable state or frontier exhaustion returns
to a complete ordinary query with no partial result publication.

Every eligible title/address pair contributes exact per-kind counts once.
Bucket timestamp order bounds recency, its tree supplies the minimum target,
and a global heap combines indexed ranges with fallback rows. Original-field
base scores are computed lazily and only within the current query. Stable
targets resolve through the existing catalogue map, and old membership is
detached before a group ID can be released or reused. Rebuild attempts occur
only after qualifying structural mutations, with complete post-mutation state.
Metadata-only edits still update ordering but do not spur useless unavailable
index retries. Empty buckets/pairs and oversized retained vector capacities
are released. Actual storage and update measurements are required before
production promotion; fully unique catalogues retain the ordinary scan.

Independent review corrected two diagnostic/lifecycle issues before runtime
verification: failed range attempts publish no stage counters before ordinary
fallback, and unchanged pair/class metadata does not advance rebuild retries.
Successful range work has an explicit combined membership/ranking stage.
The range diagnostic reports whole-query times, not the ordinary stage
profiler's field-score-per-row invariant; that instrumentation needs a separate
leaf/base-evaluation distinction before wider use. Source review found no
remaining result-correctness issue. Runtime outcomes are recorded below.

### Digit-filter and range-index outcomes

The full gate in `target/continuation-digit-range-gate.log` passes dependency
boundaries, formatting and workspace Clippy, then records 265 passing tests
and the same two known NUL-highlight failures. Search is 129/131; no gate test
was ignored or filtered. Later workspace and smoke stages remain unreached.
The initial focused Clippy log retains an ambiguous NUL escape in a new test;
the corrected literal and final passing run are in
`target/continuation-digit-range-clippy-final.log`. Both independent source
reviews passed. Existing tests and approval boundaries are unchanged.

The unchanged complete benchmark is `target/continuation-digit-counts-bench.json`,
SHA-256 `974ce8876c2f112e1a446bb591438810e1527c8be45f4556f81a8301e0d7bcdb`.
All 3,120 serialized checksum/hit/match-count values across 1,040 samples
exactly match the previous production report. Rare-identity p95 falls from
36.2976 to 7.3758 ms; the filter is retained. Nine of 26 cases still meet D48,
so the overall benchmark correctly fails. Corpus construction takes 280.9679
ms and the worst retained query sample is 16.7165 ms. Successive-run variation
does not establish that the digit filter caused nonnumeric timing differences.

| Case | p95 ms |
| --- | ---: |
| blank | 2.6180 |
| broad-character | 12.1526 |
| typed-two | 13.9422 |
| typed-three | 13.6945 |
| typed-four | 13.7088 |
| word | 6.0897 |
| missing-character | 5.7065 |
| transposed-characters | 5.7580 |
| uppercase | 3.6933 |
| subsequence | 14.4443 |
| common-word | 9.4953 |
| two-words | 5.8668 |
| address-only | 4.5507 |
| address-suffix | 14.2336 |
| rare-identity | 7.3758 |
| composed-accent | 13.8820 |
| decomposed-accent | 13.2030 |
| cjk | 2.9895 |
| emoji | 2.9810 |
| no-match | 2.8757 |
| long-match | 6.1850 |
| long-near-match | 13.5727 |
| tabs-scope | 3.8204 |
| saved-scope | 4.2698 |
| history-scope | 14.8523 |
| commands-scope | 3.9944 |

The isolated range experiment passes every complete-result comparison in
`target/continuation-partial-range-profile.log`, SHA-256
`a2ab38350dca307513ea420f1aa2366bd5fe500d44f4789df924cd148580ba1c`.
It measures 14 cases on each of the existing and ten-percent-unique 100,000-row
fixtures, with fresh generation-changing updates and exhaustive equality
outside timing. This is a paired diagnostic, not the 26-case D48 acceptance run.

| Fixture and case | Ordinary median ms | Range median ms |
| --- | ---: | ---: |
| Existing, broad | 8.530 | 5.159 |
| Existing, word | 3.383 | 2.192 |
| Existing, uppercase | 2.061 | 1.041 |
| Existing, composed accent | 9.058 | 5.365 |
| Existing, long near-match | 8.827 | 5.112 |
| Mixed, broad | 10.879 | 7.641 |
| Mixed, word | 9.947 | 6.451 |
| Mixed, uppercase | 4.843 | 5.955 |
| Mixed, CJK | 1.243 | 2.699 |
| Mixed, emoji | 1.069 | 2.554 |
| Mixed, no match | 2.128 | 3.205 |
| Mixed, long near-match | 2.544 | 4.046 |

Existing-corpus bulk index construction takes 45.983 ms. Member capacity is
4,285,760 bytes and tree capacity 1,081,264 bytes; pair/bucket map nodes,
structs, allocator overhead and transient query allocations are additional.
Measured per-case update medians range from 16.8 to 22.3 microseconds, with a
62.6-microsecond worst sample. Mixed-corpus construction takes 22.763 ms, with
5,085,792 member-capacity bytes, 1,279,632 tree-capacity bytes and 262,144
fallback-capacity bytes, again excluding map and other overhead. These are
the exercised final-command updates, not a worst-case bound for rebuilding a
100,000-member bucket. Additional large-bucket mutation measurements and a
production loading strategy remain required.

The partial index remains test-only: it improves several broad cases but
regresses on multiple mixed-data cases and does not establish D48. Diagnose
actual range admission/restart and fallback work before choosing a strategy
or promoting it. Fully unique catalogues still need the general scan. A later
small digit-filter experiment could test higher required multiplicities first,
without changing the deficit rule, but this checkpoint retains the measured
ascending-digit implementation.

Read-only follow-up narrows the mixed zero-match regressions. The first
revision moves the shared final-command title into fallback at the full pool,
leaving 91,022 grouped and 8,978 fallback rows, within every storage cap.
CJK/emoji queries cannot match these ASCII-only fields at budget zero, and
the long queries exceed every field length. Those cases therefore enqueue
no candidate ranges; frontier overflow cannot explain their overhead. The
current log does not report route/candidate counters, so the next diagnostic
should verify these source-derived facts at runtime.

The likely remaining cost is locality: fallback targets are sorted by target
identity, while the fixture deliberately permutes catalogue row order. Resolving
each through the position map produces scattered metadata reads. This is an
inference, not a measured attribution. A reviewed next experiment would retain
bounded fallback records containing both stable target identity and a cached
row position, sorted by row position. Explicit insertion/swap-removal hooks
must repair that derived position before any query; tests must validate both
identity and position through all mutations and failed/no-op changes. Compare
route counts, paired query times and update costs before adopting this extra
metadata. Do not discard the stable target or assume vector positions survive
`swap_remove`.

The production build is `target/local/atlas-app-digit-counts.exe`, SHA-256
`3ea8af80db81795667d32c7732bb30c2f09e6db1d9da26c360f4d221e91668da`.
Its awaited fresh synthetic normal-desktop smoke exits 0 with empty stderr;
build and smoke evidence use `target/continuation-digit-counts-app-`.
Existing binaries, windows and uncommitted work remain preserved. This is not
a diagnosis of the original WebView2 breakpoint or interactive acceptance.

### Fallback locality and large-bucket mutation follow-up

The next test-only experiment keeps each fallback row's stable target together
with its current catalogue position, ordered by position. Querying can then
read the aligned entry and metadata arrays directly. Replacement retains its
position; removal explicitly repairs the position of a moved last row before
the post-mutation index is available. This adds derived metadata rather than
changing target identity, matcher behavior, resource limits or query answers.
Focused lifecycle checks cover both swap directions, group/fallback changes,
failed and unchanged mutations, and recovery from an unavailable index.

Separate query diagnostics distinguish admission, ordinary fallback and
frontier restarts, with counts for pair checks, fallback checks, pair scoring
and selected leaves. Their counts are not interchangeable with the ordinary
per-row scoring observer. The existing paired query workload remains unchanged;
route verification runs outside its timed samples.
Counter maintenance is included in timed range queries; only reporting and
the additional verification query are outside timing.

The new `catalogue/range_index/tests/mutation_costs.rs` diagnostic measures a
single canonical pair and bucket at 1,024, 16,384, 65,536, 65,537, 100,000 and
131,072 synthetic rows. It compares indexed and ordinary catalogue mutation
costs for unchanged rows, timestamp changes, recency-order moves, digit-only
text changes and ranking-class moves. Two warmup pairs precede five retained
pairs; cloning, generation checks, complete-result comparisons and index
invariants are outside timing. It also measures exceeding and recovering the
member cap. The 65,536/65,537 boundary exposes the tree's power-of-two growth.

Production loading still needs a reviewed boundary. The current session
restore creates a catalogue and calls `sync_catalogue` with ordinary row
upserts. Activating incremental range maintenance in `Catalogue::new` would
repeatedly rebuild a growing bucket, producing quadratic ingestion work.
The prospective approach is one bulk build after successful ingestion,
retaining ordinary search if capacity prevents indexing, followed by measured
incremental maintenance. No public loading API or app restore behavior is
changed by this experiment, and first-query construction must not be hidden
outside a complete-query latency measurement.

### Locality and large-bucket results

The final full gate is `target/continuation-range-locality-final-gate.log`.
Dependency checks, formatting and workspace Clippy pass; 270 tests pass and
the same two NUL-highlight regressions fail (134/136 search tests). No gate
test is ignored or filtered. Later workspace and app-smoke stages remain
unreached. The first gate also preserved its result (269 passes and the same
two failures); independent review then requested one explicit available-state
fallback-to-fallback swap regression, which passes in the final gate.

Both isolated diagnostics pass complete-result comparisons. Their focused
selection is for measurement, not a replacement for the unfiltered gate.
The locality log is `target/continuation-range-locality-profile.log`, SHA-256
`0ed08a10491c39811d1daba90b97a51f6cedba4c126fc9c5ed80bf56cc1399f1`.
This run measures 14 cases on both 100,000-row fixtures. Representative
paired medians are below; these are not the 26-case D48 p95 acceptance test.

| Fixture and query | Ordinary median ms | Range median ms |
| --- | ---: | ---: |
| Existing, broad | 7.776 | 5.414 |
| Existing, word | 3.241 | 1.948 |
| Existing, composed accent | 8.375 | 6.410 |
| Existing, long near-match | 8.954 | 5.541 |
| Mixed, broad | 13.059 | 3.783 |
| Mixed, word | 11.040 | 4.554 |
| Mixed, uppercase | 4.371 | 3.011 |
| Mixed, composed accent | 6.757 | 7.084 |
| Mixed, CJK | 1.192 | 0.654 |
| Mixed, emoji | 1.623 | 0.912 |
| Mixed, no match | 1.910 | 1.295 |
| Mixed, long near-match | 1.636 | 0.704 |

The earlier mixed CJK range median was 2.699 ms and the mixed long near-match
median 4.046 ms. The new within-run pairs support retaining cached positions
in the experiment, while variation between processes prevents attributing
every timing difference to locality. Mixed composed-accent search still
regresses. Digit queries explicitly use ordinary search; their measured
differences do not represent a range-index improvement or regression.

Untimed reports verify that the mixed zero-match cases use the available
range index, examine 1,024 pairs and all 8,978 fallback rows, and enqueue no
candidate ranges. They neither overflow nor restart. This confirms the prior
source-derived route diagnosis. Fallback capacity grows from 262,144 to
393,216 bytes because each record now retains a row position. Other reported
member/tree capacities are unchanged; map nodes, allocator overhead and
transient query allocations remain excluded. Bulk construction takes 49.657
ms on the existing fixture and 22.612 ms on the mixed fixture.

The large-bucket log is `target/continuation-large-bucket-profile.log`, SHA-256
`bab28b6fe479faf12027c15f1790cd637a2d06ad9ad6eb861e9c77280aa42d9a`.
All six sizes and mutation families preserve complete results and invariants.
The 100,000-row bucket builds in 6.394 ms and retains 4,194,304 member-capacity
bytes plus 1,048,576 tree-capacity bytes, excluding other allocations. At
65,536 rows the tree uses 524,288 bytes; at 65,537 it doubles to 1,048,576.

| 100,000-row mutation | Ordinary median microseconds | Indexed median microseconds | Indexed worst microseconds |
| --- | ---: | ---: | ---: |
| Unchanged row | 0.300 | 0.200 | 0.300 |
| First member timestamp | 2.500 | 1,330.500 | 1,739.700 |
| Oldest to first and back | 2.350 | 1,355.000 | 1,603.000 |
| Same-pair text | 3.600 | 1,236.200 | 1,596.400 |
| Open-state class | 0.850 | 565.050 | 669.600 |
| Frequency class | 1.950 | 746.600 | 800.500 |

At 131,072 rows, the worst retained mutation sample is 2.5042 ms. Crossing
the member cap and discarding the index takes 7.102 ms; recovery bulk-builds
it in 6.127 ms. These are observed samples, not latency guarantees. The
existing full-tree rebuilds and member shifts are now measured costs rather
than assumptions. The index remains test-only: retain the locality change,
reduce avoidable mutation work, review loading and complete the unchanged
acceptance benchmark before considering production activation. The latest
production binary remains `atlas-app-digit-counts.exe`; this continuation
changes only test-only code and documentation. The original crash, matcher
patch and later-plan approval requirements remain open.

The reviewed next experiment is a narrowly guarded member update in place.
Require byte-identical title/address, unchanged target and ranking class,
and a new timestamp key that remains strictly between the existing neighbors.
Then the target at every bucket position stays unchanged, so the tree's
target-minimum indices, counts and pair identity remain valid. Update only
the member timestamp. Start with available indexes; otherwise use the current
path. Text changes, class changes and movement across a neighbor retain the
existing detach-before-lease-release and reinsert lifecycle. Comparing numeric
group IDs alone is insufficient because released IDs can be reused.

Measure this against the existing mutation path with an explicit fast-path
counter. Cover first/middle/last positions, tied timestamps and target order,
absent/extreme timestamps, neighbor crossings, within/across-class frequency
changes, fallback and unavailable indexes, and field-slot release/reuse.
This is a proposed test-only follow-up, not an implemented optimization or a
production loading/API decision.

### Guarded in-place mutation experiment

The follow-up implements that guard only in the test-only range index.
After ordinary validation, unchanged-row detection and generation checking,
an available index may accept a replacement with byte-identical fields and
unchanged target/ranking class. A grouped member must remain strictly between
its existing neighbors under the full timestamp-and-target ordering. Its
timestamp can then change without shifting any target position or rebuilding
the target-minimum tree. An eligible fallback row also needs no index mutation:
its cached target/position remain valid and queries read current row metadata.

Accepted replacements still update the catalogue row, masks and generation;
unchanged text retains its existing field leases. Declined replacements use
the original detach-before-release and reinsert path. Unavailable indexes,
changed text/classes and reordered members receive no shortcut. This preserves
the field-slot reuse rule rather than assuming an old numeric ID still names
the same field after release.

A test-only switch retains the original indexed mutation behavior for paired
measurements. Separate counters report attempts, accepted grouped/fallback
updates and declines; failed validation and no-ops never invoke the hook.
The large-bucket diagnostic compares ordinary, original-index and guarded-index
catalogues with equivalent rows and generations, while explicit route checks
verify the intended shortcut. This is still not a production index or a change
to the public loading API.

The implementation and paired diagnostic pass independent review. The full
gate in `target/continuation-in-place-gate.log` passes dependency checks,
formatting and workspace Clippy, then records 277 passing tests and the same
two NUL-highlight failures (141/143 search tests). All seven new regressions
pass; no gate test is ignored or filtered, and later gate stages remain
unreached. Active-recency tests prove actual score changes, including +9,
+9 and +60 after accepted first/middle/last updates; opposing target-order
ties correctly reject the shortcut.

The isolated paired diagnostic passes in
`target/continuation-in-place-profile.log`, SHA-256
`f1ad17b75a652f9c8c25679a6a23542ac0998a531f2dcdc933cf0bfe5b9a11f8`.
It retains six sizes, six mutation families, two warmup pairs and ten measured
updates per family. Complete-result checks, row/generation checks and route
assertions pass for ordinary, original-index and guarded-index catalogues.
These are mutation medians in microseconds, not complete-query D48 results.

| Rows, eligible first-member timestamp update | Ordinary | Original index | Guarded index | Guarded worst |
| --- | ---: | ---: | ---: | ---: |
| 1,024 | 0.500 | 7.200 | 0.600 | 0.700 |
| 16,384 | 0.600 | 122.650 | 0.800 | 1.500 |
| 65,536 | 0.900 | 739.700 | 1.450 | 2.800 |
| 65,537 | 2.650 | 1,203.200 | 5.550 | 9.800 |
| 100,000 | 1.550 | 1,380.850 | 2.350 | 10.100 |
| 131,072 | 1.900 | 1,919.250 | 7.050 | 15.900 |

Each eligible family records ten accepted grouped updates; unchanged rows
record no attempts. Reordering, text changes and class moves record ten
declines. At 100,000 rows, original/guarded medians for those unchanged paths
are respectively 1,251.800/1,260.900 microseconds (oldest-to-first),
1,127.900/1,198.600 (same-pair text), 723.650/765.150 (open class) and
781.300/740.450 (frequency class). These mixed differences do not establish
a universal speedup or a precise guard-overhead estimate. Retain the narrow
shortcut in the experiment; it removes the measured full rebuild for eligible
updates without claiming to solve every mutation cost.

Cap insertion takes 7.275 ms ordinary, 6.900 ms original-index and 7.184 ms
guarded-index in the single measured crossing. The ordinary catalogue also
grows its vectors, so the whole insertion time cannot be attributed to index
invalidation. Recovery removal takes 0.006, 9.583 and 6.609 ms respectively;
both indexed paths rebuild after returning within the cap. These single
observations remain separate from update distributions. The production binary
and 26-case benchmark are unchanged, and full D48 acceptance remains open.

### Next query-bound investigation

Read-only review identifies two position-map lookups in every range-bound
construction, even though the bucket already holds its timestamp ordering,
target-minimum tree and ranking class. The prior broad-query diagnostic admits
about 7,200 roots. A proposed next test-only experiment would use a position-free
frontier key ordered by descending score and ascending target. Actual row
indices remain required only for scored or selected rows. Carry the bucket's
class with the range, and share the existing boost calculation between ordinary
entries and retained class/timestamp metadata; do not duplicate arithmetic or
invent dummy row indices.

The first member's timestamp bounds recency monotonically and the tree's
minimum target bounds tie-breaking independently. They need not describe the
same row. The guarded update preserves those invariants. Prove old and new
bound-key equality at every populated node across weight/window extremes,
absent/tied timestamps and mutations, then compare complete results and paired
query timings with an explicit original-bound switch. Count bound constructions
and position lookups separately. This is an unmeasured proposal; loading,
unique-field performance and the full unchanged acceptance benchmark remain
required before promoting the index.

### Retained-metadata bound experiment

This experiment gives the frontier a position-free priority key: descending
score followed by ascending target identity. Ordinary ranked results still
carry real row indices, with their existing final index tie-break preserved.
Both paths share the primary key rule. A private boost helper factors the
existing open/frequency/recency arithmetic without changing weights, rounding,
overflow handling or the full-width recency fallback.

Every member of a range shares its bucket's open/frequency class. The first
member has the greatest timestamp under the existing ordering, so its recency
boost is an upper bound for any current time and permitted window. The tree
independently supplies the smallest target. Combining those optimistic values
produces the same bound as the original two-row lookup; they need not come
from the same row. Actual scored and returned rows still resolve their own
positions. No query result or computed boost survives as an answer cache.

The test-only switch retains the original entry-based bound calculation on
the same index and generation. Tests compare every populated node, prove
optimism against each member and exercise mutation, extreme weights/windows,
missing timestamps and ties. A separate literal boost-formula oracle checks
the shared extraction rather than letting two callers validate the same
mistake. Paired complete-query measurements use the established synthetic
fixtures, with bound constructions and actual bound-position lookups reported
separately. Complete-result verification stays outside timing; counter
maintenance remains inside it. The range index is still test-only.
Both comparison modes share the position-free frontier layout. Their paired
timing difference isolates the entry-based versus retained-metadata bound
construction, not every structural change from the preceding implementation.

The complete pre-patch gate (`target/continuation-metadata-bounds-gate.log`)
passes dependency checks, formatting and workspace Clippy, then records 281
passes and the existing two NUL failures (145/147 search tests). The paired
diagnostic passes in `target/continuation-metadata-bounds-profile.log`, SHA-256
`dcf4c74042554b21d1532caa09dbed4654cbef327dea2ee539b04b2822f0eb11`.
Existing-corpus broad medians fall from 4.650 to 2.304 ms, with 7,504 bounds
and 15,008 versus zero bound-position lookups. Decomposed accent falls from
9.049 to 4.027 ms; CJK regresses from 1.051 to 1.187 ms. Mixed decomposed
varies from 4.929 to 6.081 ms with zero bounds in both modes, so that difference
cannot be attributed to the changed calculation. Keep the index test-only.

The unchanged production benchmark in
`target/continuation-boost-helper-bench.json` (SHA-256
`5d2c4b29ad3b4968267da9a6d6b2898191febe3281d8beece07e5c2a67f53f8d`)
preserves all 1,040 results and their exact serialized integer checksums. Only
9/26 cases pass D48; the worst sample is 18.0096 ms. This single run does not
attribute timing changes to the arithmetic extraction. The separately rebuilt
`target/local/atlas-app-boost-helper.exe` has SHA-256
`9e6e404e933f05d9a2a0d28f6c5d07af3ac09b7fb564e205225ce5ce4042823f`.
Its fresh normal-desktop smoke exits zero with empty stderr. Build and smoke
logs use the `target/continuation-boost-helper-app-` prefix. This startup
evidence does not resolve the original WebView2 crash.

### Approved frizbee correction: focused evidence

The owner-approved three-file correction now excludes padding lanes from
ASCII scoring while retaining genuine NUL bytes. Independent source review
verifies the complete 76-file package inventory, with only the approved three
files changed. The exact pin, `std`/`safe_read`, scoring contract and greedy
fallback remain unchanged; full provenance and scope live in
`docs/p1-05-frizbee-maintenance-review.md`.

All five public-API regression tests pass, and all eight available scalar,
SSE, AVX and AVX-512 score-width variants pass literal score/index expectations
and an independent zero-penalty longest-common-subsequence oracle. The backend
harness is a separately hashed ignored source overlay with no new dependencies.
Its log is `target/continuation-frizbee-backend-tests.log`, SHA-256
`c9f12c62654944f801b4427de3bb095a4ceca6c300b0ee3a6a9c6c689c541f0c`.

The unchanged production report `target/continuation-frizbee-bench.json`,
SHA-256 `9240754dc766a6dcd4d1a83a8a75a08d33b12f0be1e9e10e21ba5d32a05fb05c`,
preserves all 1,040 sample results against the boost-helper checkpoint.
Independent comparison proves all 3,120 decimal checksum/hit/match tokens and
26 case/query identities match exactly. Only 9/26 cases meet the unchanged
D48 threshold, with a 16.6731 ms worst sample. One diagnostic run neither
completes performance acceptance nor isolates the mask's timing cost.

The initial patched gate passes 146/147 search tests, including both original
NUL differential regressions. Its single failure is a cache fixture that
expected the old invalid padding index. The reviewed test-only correction
now asserts empty highlights for a deleted NUL and independently injects
malformed title/address traces to preserve strict range validation and reuse
counts. No production highlight change or relaxed NUL assertion is involved.
All workspace tests and the unchanged normal-desktop full gate now pass; the
restricted-context smoke failure remains recorded below. The range index
remains test-only, and the
original WebView2 crash remains unresolved.

### Full test pass and restricted-context smoke failure

`target/continuation-frizbee-final-gate.log` (SHA-256
`d897444e3aafd474f20251e7739f27e7912f437cac9e6fe19896b6fd6a6a72ce`)
passes dependency checks,
formatting, workspace Clippy and all 341 workspace tests, including all 147
search tests and five public matcher regressions. No tests are ignored or
filtered, and all documentation-test stages pass. The reviewed fixture
correction preserves strict invalid-range validation; both original NUL
regressions now pass.

The gate rebuilds `target/local/atlas-app.exe` in 18.05 seconds, SHA-256
`6edfe4113ccc7b0d5995faccaf7cb23e66541af147712443eb1b17bd9adddf1c`, then fails
its restricted-context smoke run at the 30-second watchdog. A subsequent
`Chrome_WidgetWin_0` unregister error reports 1411. This is a failed gate,
not a passing application check. The unchanged complete gate subsequently
passes on the normal desktop in `target/continuation-frizbee-desktop-gate.log`,
ending with `Local gate passed.` Its 341 tests, including 147 search and five
public matcher regressions, have zero failed, ignored or filtered cases;
documentation-test stages and application smoke also pass. The log SHA-256 is
`043845edb8374f2141a6d69935494a473bc51bd216c21969523dee5db6891c5f`.
The binary hash is unchanged. No security setting or test was relaxed, and no
continuation job remains active.
The original WebView2 crash and remaining interactive acceptance stay open.

### Query-stage attribution

The independently reviewed diagnostic is now implemented in
`catalogue/range_index/tests/bounds/stages.rs` with a private module declaration.
It uses the existing query observer to report each stage from the median-total
sample plus total min/max, preserving fourteen cases, two synthetic fixtures,
same-generation pairs and complete-result checks. Both comparison modes share
the position-free layout; legacy mode isolates entry-based bound construction.
The isolated diagnostic passes in `target/continuation-range-stages-profile.log`,
SHA-256 `f78a8830eab4db02e7752533357660e1e89dcb71713f0ca4aa5c965036e67e1e`.
All 56 case/fixture/mode rows retain their expected routes and complete results.
These instrumented five-sample medians use twenty results, not D48's fifty.

With metadata bounds, existing broad queries take 2.645 ms, including 2.595 ms
in combined matching/ranking. Mixed composed-accent takes 5.590 ms, including
5.539 ms in that phase; mixed decomposed-accent takes 4.603 ms with 4.565 ms
there. Existing long near-match takes 6.265 ms, including 4.366 ms in matching/
ranking and 1.888 ms in highlights and owned results. Setup, ordering and
cleanup are small in these samples. This evidence supports separately timing
candidate collection and frontier expansion before changing either algorithm.
It does not isolate the matcher patch's cost or establish D48 acceptance.

### Candidate collection and restore attribution

The finer diagnostic separates candidate collection from frontier expansion
inside the test-only range index. Timing is opt-in and off by default. Failed,
bypassed and restarted attempts clear their measurements; only a successful
range ranking publishes them. Independent source review found no result or
observer-event change. The original overlong-query fixture incorrectly assumed
1,025 bytes exceeded the scorer's limit. Its reviewed correction uses
`Scoring::default().max_needle_len() + 1` and retains the exact error assertion.
The failed fixture run remains in `target/continuation-range-phases-build.log`.

The isolated workload passes in `target/continuation-range-phases-profile.log`,
SHA-256 `7c69853e59d4f4217679ce1f9ad31f6d39524d05c943da98c6d1ed14f65096b4`.
The unchanged fourteen cases, two fixtures and two modes retain complete
results. With metadata bounds, mixed composed-accent takes 7.741 ms total:
7.317 ms collecting candidates and 0.336 ms expanding the frontier. Mixed
decomposed-accent takes 7.065 ms total: 6.966 ms collecting candidates and
0.033 ms expanding. Existing long near-match has a different cost distribution:
7.431 ms total, 1.320 ms candidates, 3.604 ms expansion and 2.484 ms highlights
and owned results. These are stages from the same median-total sample, with
instrumentation included. Differences from earlier runs do not isolate a
regression or improvement. These twenty-result diagnostics do not replace D48.

The separately reviewed restore diagnostic feeds prepared owned rows through
ordinary upserts with the index absent, then builds it once before queries.
Both tests pass in `target/continuation-range-restore-profile.log`, SHA-256
`c8e8d3446fafe2e7321e6a5307e97484b8aa4dfe52a963c8a127ee2de9065308`.
The existing 100,000-row fixture takes 244.593 ms feeding and 39.448 ms building;
the mixed fixture takes 210.138 ms feeding and 24.501 ms building. These are
single observations of separate intervals, excluding storage I/O, fixture
creation, cloning and assertions. Their sums are not wall-clock restore latency.
Complete queries and subsequent mutations preserve results; explicit capacity
tests preserve rows and recover after crossing the limits in both directions.

Reported member/tree/fallback vector capacities total 5,367,024 bytes for the
existing fixture and 6,758,640 for mixed. The report separately lists catalogue
vector capacities. Neither is total memory: owned strings, maps, allocator
overhead, matcher storage and transient allocations remain excluded. No public
loading API or production range-index activation was introduced.

The next measured experiment should target candidate rejection for non-ASCII
queries against ASCII fields. Every non-ASCII query scalar consumes a deletion;
the remaining ASCII letters can use the existing presence mask with the remaining
budget. This must preserve whole-query case semantics, bypass Unicode fields,
avoid adding overlapping digit deficits and prove parity before promotion.
The proposal is source-reviewed but not implemented or measured. Subagents hit
an account usage limit before implementation began; no partial filter change
was applied. Production loading, independent review and full D48 acceptance
remain prerequisites for range-index promotion.

The complete normal-desktop gate passes after these diagnostic changes in
`target/continuation-range-diagnostics-gate.log`, SHA-256
`e83f60da582496bb825066c58674ad870b2c9453dde22a2dc543ed762acf91a8`.
Dependency checks, formatting, workspace Clippy, all 347 tests (153 search
unit tests plus five public matcher tests), documentation-test stages and
fresh synthetic app smoke pass. None are ignored or filtered. The corrected
overlong fixture and phase-reset tests pass. The standard app retains SHA-256
`6edfe4113ccc7b0d5995faccaf7cb23e66541af147712443eb1b17bd9adddf1c`.
The original WebView2 crash remains unresolved; no continuation job is active.

### ASCII projection experiment and narrowed scope

The reviewed filter now prepares the ASCII portion of a non-ASCII query once.
For a known ASCII field, each non-ASCII query scalar necessarily consumes one
deletion under the pinned matcher's width-preserving case rules. A field whose
presence mask cannot supply the remaining ASCII symbols is rejected before
exact membership. Unicode fields and unsupported budgets keep their existing
paths; digit-count rejection remains independent. No score, highlight, matcher
dependency or public API changes are involved.

The first implementation checked every eligible field before group-memo reuse.
Six focused tests passed, including a paired full-row diagnostic, in
`target/continuation-projection-tests.log`, SHA-256
`59f66cfc6d9fd0779b4d8ccad241913f0c6db1f9e09678af925637172b746253`.
Mixed composed-accent membership medians changed from 7.460 to 2.328 ms and
decomposed-accent from 7.296 to 2.903 ms. The existing fixture regressed from
1.533 to 2.288 ms and 1.517 to 2.253 ms respectively. These paired scans include
query preparation and fresh memo allocation but exclude ranking and highlights;
they are not D48 acceptance or complete range-candidate timings.

Repeating the stronger mask before cheap group-cache hits is avoidable work.
The independently reviewed revision applies projection only when the field has
no group ID or the query has no active group memo. Cached fields retain the
original mask and checked membership path, including malformed-ID errors.
Regressions explicitly check both cached true and false values, uncached pruning,
public matcher parity and unsupported budgets. The revised scope still needs
its own executable measurements before claiming it removes the regression.

The first full benchmark did not execute: Windows Application Control blocked
the rebuilt `xtask.exe` with error 4551 in
`target/continuation-projection-bench.stderr.log`. The unchanged normal-desktop
retry was also blocked in `target/continuation-projection-bench-desktop.stderr.log`.
A direct PowerShell attempt separately failed to launch with a redirection-
encoding error; its shell exit code does not establish a benchmark pass.
The empty JSON files contain no samples. No Windows setting was changed and no
new D48 result is claimed from these attempts.

The narrowed implementation subsequently passes its isolated paired diagnostic:
`target/continuation-projection-narrowed-profile.log`, SHA-256
`170e56f72c3c9061f5bd5279599d5a5fab718cc0696f3e10a676d585257ad89e`.
Existing composed/decomposed medians are 1.960/2.460 ms without projection and
1.809/2.272 ms with it; mixed medians are 7.176/7.255 versus 2.735/2.517 ms.
Independent review verifies all 56 rows and complete row membership equality.
These mode medians are not medians of pairwise differences or full-query costs.

The unchanged production benchmark now executes in
`target/continuation-projection-narrowed-bench.json`, SHA-256
`0f7949b22c96494a412f67a67432e89ff8f969304a96fe8d740509d3084a23bf`.
Independent raw-integer comparison confirms all 3,120 checksum/hit/match tokens
across 1,040 samples match the frizbee baseline exactly. Recomputed p95 verdicts
give 10/26 passes, maximum p95 13.9768 ms and worst sample 17.7388 ms. D48 still
fails. Historical full-benchmark timing differences do not isolate a speedup.
All 160 search unit tests pass in the combined gate, but Windows blocks the
separate public matcher-test executable with error 4551; no whole-gate pass
or WebView2 crash resolution is claimed.

### Next bounded proposal — 2026-10-11

Evaluate the existing test-only group-bound cache before adding another search
algorithm. Production `Catalogue::bound_candidates` still computes both field
score bounds for every visible matching row. The preserved
`target/continuation-highlight-traces-stages.log` attributes the broad query's
median sample to 4.632 ms in membership/candidates, 2.686 ms in heap/scoring and
0.067 ms in highlights, with 100,000 candidates but only four original field
scores computed. This older diagnostic supports removing repeated candidate
work; it does not isolate bound arithmetic or measure the current narrowed
projection. The latest complete production report remains 10/26 under D48.

`candidate.rs::GroupBoundCache` and
`catalogue/range_index/tests/bound_reuse.rs` already implement a private,
query-local comparison. They reuse identical conservative field bounds through
existing group IDs while leaving row counts, boosts, candidate order, exact
scoring and highlights unchanged. They do not activate the range index.
The preserved `target/continuation-socket-close-commit-gate.log` records all four
bound-reuse tests passing, but no standalone timing output was found among the
target-root logs reviewed. Cheap bound arithmetic competes with allocation,
slot lookup and branching, so reuse counts alone cannot establish a speedup.

The concrete next action belongs to the integration chat's serialized queue:

```powershell
cargo test -p syrinx-search --profile local --offline --locked catalogue::range_index::tests::bound_reuse -- --nocapture --test-threads=1
```

Preserve the existing diagnostic first: fourteen queries, two fixtures, two
modes, complete production/exhaustive equality and identical candidate bounds.
Its twenty-result, five-sample timings remain diagnostic. If they support the
cache, the smallest follow-up implementation assignment is `candidate.rs` and
`catalogue.rs`, with `bound_reuse.rs` retaining the uncached oracle and adding
the same paired comparison at fifty results. Keep existing tests for digit
bypass, Unicode/NUL, long fields, malformed/recycled group IDs, extreme boosts,
scopes, zero limits and mutations. No promotion is authorized by this proposal.

Before retaining a production change, require independent review, focused tests,
the full gate and the unchanged 26-case complete-query benchmark with exact
integer checksum/count parity. Retain every failed or regressed result. D48
completion still needs its prescribed three fresh-process acceptance runs; a
small measured improvement does not complete P1-05. No benchmark, build or source
implementation ran during this read-only diagnosis, and historical records above
remain intact.

### Bound-reuse diagnostic result — 2026-10-11

The integration chat ran the isolated comparison in
`target/continuation-bound-reuse-browser-first.log`, SHA-256
`360dbe46cbb324d837d431e3e847fd80cafa4c5d5f1179f2f0f506bf770401d7`.
All four focused tests passed in 19.24 seconds; 160 other search tests and five
public matcher tests were filtered out. All 56 expected case/fixture/mode rows
are present. Every pair preserves total field-bound requests, original-mode
reuse is zero, and the test's candidate-bound and complete-result assertions
passed. The complete 28-pair table, including min/max, median differences and
work counts, is `target/continuation-bound-reuse-browser-first-analysis.md`.

Keep the cache test-only. Existing/mixed broad medians improve from
8.518/13.374 ms to 7.858/12.213 ms, and word improves from 3.928/12.700 ms to
3.495/11.695 ms. However, existing composed-accent rises from 10.712 to
11.924 ms despite replacing 197,820 bound calculations with reuse. Existing
long-near-match remains essentially unchanged at 12.601 versus 12.650 ms
despite 19,927 reuses. Mixed typed-two also rises slightly, from 10.951 to
11.016 ms. Reduced work does not consistently reduce complete query time.

Controls further limit causal interpretation: existing/mixed no-match medians
fall from 1.945/1.874 to 1.631/1.591 ms with zero bound requests, while mixed
uppercase rises from 4.417 to 5.215 ms with zero requests. These differences
cannot be attributed to saved bound calculations; timing variation and cache
setup/path overhead remain possible contributors. Do not average these controls
or regressions away to claim a speedup.

The timed path includes query setup, complete scope counts, boosts, exact
ranking, highlights and owned results, but uses twenty returned hits. Each
printed work count belongs to its mode's independently selected median-total
sample. Only min/median/max are retained in the log, so paired differences,
variance, p95 and statistical significance cannot be reconstructed. Under the
predeclared criteria this is mixed, insufficient evidence for production
promotion, not a correctness rejection or D48 acceptance. No production source
changed; the unchanged full benchmark remains 10/26.

### Cached-first membership experiment — 2026-10-11

The next narrow experiment removes repeated presence-mask work before an
already-known membership result. Production `GroupedMembership` checks the
field mask before consulting its query-local group memo. A populated memo is
already an exact true/false answer for digit-free queries and equivalent field
groups; rechecking the conservative mask cannot improve that answer. This
targets per-row membership work identified in the preserved candidate profiles,
separately from the inconclusive bound-cache experiment above.

The test-only alternate entry point preserves digit-count rejection first. It
uses only populated, in-bounds memo slots; cached false continues to the other
field, while cached true accepts the row. Missing, cold and malformed IDs follow
the original mask-first path, preserving cases where a mask rejects before ID
validation. Numeric queries and unsupported budgets retain their existing
behavior. No memo survives a query or a group mutation. Independent guard review
supports this equivalence; runtime validation is pending.

Only test-only code is added in `catalogue/membership.rs`, its new
`membership/cached_first_tests.rs`, the strategy wiring in `catalogue.rs`, and
`catalogue/range_index/tests/membership_order.rs` with its private declaration.
Existing production method bodies are unchanged. The full-query adapter changes
only the membership call and retains original counts, bounds, boosts, exact
scoring and highlights. Focused regressions cover both fields, masks and invalid
IDs, Unicode/NUL, digit variants, unsupported paths, scopes, limits, boosts and
mutation. The original production and exhaustive results remain oracles.

The diagnostic uses the unchanged two 100,000-row fixtures and fourteen queries
with fifty returned hits, two warmups and five retained alternating same-generation
pairs. It prints all 140 individual pairs in nanoseconds plus 56 mode summaries.
Timing includes fresh memo preparation/cleanup and complete owned results;
fixture revisions, equality assertions and returned-result destruction are
outside timing. Warm cached fields may save work, but cold or mask-rejected
fields may incur an extra lookup. Neither a speedup nor production promotion is
claimed. The integration chat exclusively owns serialized execution.

Final independent source review found no actionable issue in the frozen
experiment. Six focused helper regressions and two full-query tests await
execution. Scoped formatting and whitespace checks pass; no Cargo command or
performance measurement ran in this implementation chat. Run the helper filter
`catalogue::membership::cached_first_tests` first, then
`catalogue::range_index::tests::membership_order`, with the optimized local
profile, locked/offline dependencies, captured output and one test thread.

### Cached-first paired results — 2026-10-11

The integration chat ran validation with no competing Cargo or native run.
All six helper regressions passed in
`target/continuation-cached-first-correctness.log` (SHA-256
`0ca02d1498fdfe842d758e00a210d47fc5cc115564e21e3a967d1bfc6a42fcc2`).
Both full-query tests passed in 16.57 seconds in
`target/continuation-cached-first-membership-order.log` (SHA-256
`4a5cb52d8b536d6baad90c009889758a4fb850df13f9a555fb0a78ef0a1685a2`).
The commands filtered out the remaining search tests and all five public matcher
tests; these are focused passes, not a new full gate.

Independent analysis verifies 140 unique raw pairs, all five pass indices and
the alternating order for each of 28 case/fixture combinations. All 56 summaries
match their raw extrema and medians within printed rounding. The test assertions
confirm complete production/exhaustive equality, including counts, scores,
ordering, original strings, highlights and generation. The complete comparison
is `target/continuation-cached-first-paired-analysis.md`; it includes paired
median differences and separates the two execution orders.

Keep the unconditional cached-first experiment test-only. Existing-corpus
typed-two, typed-three, word, uppercase, CJK and emoji improve in all five
pairs, so this is not simply a case of the second mode always winning. However,
existing long-near-match loses all five pairs, with a paired median increase
of 0.918 ms; mixed no-match also loses all five, increasing 0.466 ms. Both
regressions persist in each execution order. The latter is a zero-result query,
not proof of zero memo work: this experiment does not instrument cache hits.

Separate mode medians would conceal some outcomes. Existing long-match appears
to improve from 7.667 to 7.017 ms, yet only two pairs improve and its paired
median increases 0.520 ms. Mixed typed-three similarly improves its mode median
from 10.978 to 10.474 ms while its paired median increases 0.112 ms. Numeric
rare-identity bypasses memo reuse but still changes by paired medians of
+0.034/+0.048 ms for existing/mixed fixtures; timing and alternate-path overhead
cannot be separated from that control alone.

For original-first/cached-first-first order, existing-fixture improvement counts
are 31/42 and 20/28; mixed-fixture counts are 24/42 and 17/28. These pooled counts
do not erase per-case losses. Five pairs cannot establish D48 p95, significance
or repeatability across processes. The evidence rejects unconditional production
adoption at this checkpoint. Any narrower proposal must first explain, from
source, how it avoids added cold/mask-rejected lookup cost while retaining the
same guards and errors; do not select query categories merely because they won
this run. No production source or acceptance criterion changed.

### Unicode-selected membership follow-up — 2026-10-11

The integration chat's subsequent unchanged-source full gate passed all 400
tests and application smoke in `target/continuation-recovery-watchdog-full-gate.log`.
That validates the prior test-only experiment's integration, not its performance.

The next authorized test-only comparison selects cached-first membership once
per query only when the query is non-ASCII. This guard has a source reason:
`AsciiPresence::new` produces no presence mask for such queries, and grouped
fields with an active memo bypass the separate projection filter. Their original
presence-mask gate therefore cannot reject. ASCII queries retain the old path,
avoiding an added memo probe before a potentially useful rejection. Ungrouped
fields still use the original projection and matching path on a cache miss.
Independent review confirmed these guards; this is not a query-string exception
chosen to hide a benchmark failure.

The existing original and unconditional cached-first variants remain intact.
New tests in `catalogue/range_index/tests/membership_order.rs` compare all three
modes through the complete query path. Mode selection is timed; all modes use
the existing strategy and helper. Production methods, helper guards, dependencies
and public APIs remain unchanged. The selected mode must preserve complete results
under every scope, limits, boosts, supported and fallback budgets, and mutations.

The new diagnostic keeps the two fixtures, fourteen queries and fifty-result
limit. After two warmup triplets, it retains six same-generation triplets,
cycling all six permutations of the three modes. Each mode occupies each timing
position twice, and each pair appears in either order three times. It prints
168 raw triplets with order and generation, plus 84 summaries. Six-sample medians
average the central two values; they remain diagnostic, not D48 p95. ASCII
selected-versus-original comparisons are controls: both take the old membership
path, so their timing differences cannot establish a shortcut benefit. The
integration chat owns all execution; no new timing outcome is claimed here.

Final independent source review passed the frozen two-test addition. Scoped
formatting and whitespace checks passed, with no direct generic frizbee calls
added. The selected mode is algorithmically equivalent to original on ASCII
queries and to unconditional cached-first on Unicode queries; differences within
those equivalent comparisons measure selection overhead and variation, not a new
matching algorithm. Runtime validation remains pending. The exact isolated command
is:

```powershell
cargo test -p syrinx-search --profile local --offline --locked catalogue::range_index::tests::membership_order::unicode_selected -- --nocapture --test-threads=1
```

### Unicode-selected triplet results — 2026-10-11

Both focused tests passed in 21.33 seconds in
`target/continuation-unicode-selected-triplets.log`, SHA-256
`bc2d639a67d386a619834ad2184759010123601219024f75f0ac86e36eaa46c9`.
The run filtered out 172 other search tests and all five public matcher tests;
it is not a new full gate. Two independent analyses and the coordinating review
verified 168 unique triplets, all six permutations for every case/fixture,
balanced positions and pairwise order, and all 84 summaries against raw timings.
The passing tests retain complete production/exhaustive result equality.
`target/continuation-unicode-selected-triplets-analysis.md` contains all 28
comparisons, paired differences, equivalent-mode comparisons and order splits.

Unicode-selected beats original in 43 of 48 Unicode samples. Each of the six
execution orders improves seven or eight of its eight Unicode comparisons;
the median paired improvement by order ranges from 0.689 to 1.013 ms. Existing
CJK and emoji, and all four mixed-fixture Unicode cases, improve in all six
samples. Their paired median changes are respectively -0.369, -0.807, -1.184
(mixed composed), -1.487 (mixed decomposed), -1.085 and -0.985 ms.

Existing decomposed-accent improves four of six samples, with a paired median
change of -1.506 ms. Existing composed-accent remains inconclusive: three of six
improve, paired median change is -0.082 ms, and separate mode medians increase
from 10.651 to 10.761 ms. The selected-original order split reverses sign for
that case. Do not describe every Unicode workload as improved.

The algorithm-equivalent controls are approximately centered in aggregate:
Unicode selected versus unconditional cached-first improves 25/48 samples with
pooled paired median -0.042 ms; ASCII selected versus original improves 63/120
with median -0.048 ms. However, their individual differences span
-3.359 to +5.385 ms and -7.793 to +4.152 ms respectively. Those outliers limit
claims about small individual changes. Control differences are timing variation
or selection/path overhead, not a membership shortcut gain.

Keep source test-only pending review. Unlike unconditional adoption, the guarded
candidate now has a source-supported boundary and a Unicode benefit that survives
balanced execution order. Recommend retaining it for a reviewed comparison over
the complete unchanged 26-case workload, including all ASCII controls and both
accent cases, rather than selecting only winning cases. The independent challenge
also emphasizes that the existing accent results need confirmation. This is a
recommendation for the integration owner, not production promotion: six samples
in one process establish neither D48 p95 nor repeatability nor zero ASCII
overhead. The production benchmark remains 10/26, and no production code changed.

### Complete-workload comparison prepared — 2026-10-11

The authorized diagnostic now reuses the actual benchmark workload through the
private source module `crates/xtask/src/benchmark/workload.rs`. Its corpus
construction, insertion permutation, case texts/scopes, policy, reference time,
generation refresh and constants were mechanically extracted from `benchmark.rs`.
Both consumers import the same definitions; no copy of the fixture, new dependency
or public search API is introduced. A shared private `case_index` preserves the
existing rotation expression. Before/after source review found the moved function
bodies identical, with the original benchmark loop changing only that helper call.
Its validation, checksums, timing boundaries, JSON output and acceptance logic
remain unchanged.

The new `syrinx-search` test-only `catalogue/acceptance_comparison.rs` compares
actual production `Catalogue::query` with the Unicode-selected test strategy.
It retains all 100,000 rows, 26 cases, fifty results, five warmup passes and forty
measured passes. Rotation resets after warmups exactly as in the benchmark.
Each case pair receives one revision, so both modes see the same generation;
pass parity gives each mode twenty first runs per case. All 1,040 measured pairs
are retained and printed after measurement, including integer nanoseconds,
generation, order, all scope counts and hit counts. Complete `SearchResults`
equality checks original text, scores, ranking, counts and highlights outside
timing. The first warmup also compares every case with the exhaustive oracle.

This is a paired diagnostic, not a replacement for the unchanged acceptance
command or its three fresh-process requirement. It intentionally retains timing
failures as data rather than failing its correctness test on a slow case. The
preserved raw samples permit per-case nearest-rank p95, maximum and paired/order
analysis afterward. No production search path has changed, and no runtime result
is claimed for this new source. Root execution commands are:

```powershell
cargo test -p xtask --profile local --offline --locked benchmark::tests -- --test-threads=1
cargo test -p syrinx-search --profile local --offline --locked --features acceptance-diagnostics catalogue::acceptance_comparison::unicode_selected_unchanged_acceptance_workload_comparison -- --nocapture --test-threads=1
```

Independent source review passed the extraction and complete comparison. Scoped
formatting and whitespace checks passed; compilation and runtime validation
remain pending exclusively with the integration chat. The frozen experiment
adds no direct generic frizbee calls and leaves the earlier diagnostics intact.

### Complete-workload comparison result — 2026-10-11

All seven existing benchmark regressions passed in
`target/continuation-shared-workload-tests.log`, SHA-256
`13e27c29faa8e7c6a152e6b3529ccb5441cbabb43a87186aa7bb10f7084285bf`.
The complete comparison passed its correctness test in 19.96 seconds in
`target/continuation-unicode-full-comparison.log`, SHA-256
`ae4b10d9c6476566c4ad90d8d4276e4b95194dec6cfc2fe83ab1bfc3ee064b66`.
It filtered out 174 other search tests and five public matcher tests. This is
focused correctness evidence, not a new full gate or a performance pass.

Two independent analyses and the coordinating review verified all 1,040 unique
case/pass pairs, forty samples and twenty first runs per mode for every case,
the unchanged case rotation, and contiguous generations 100131 through 101170.
Scope count bounds and limited hit counts are valid. The passing test establishes
complete result equality on every pair and exhaustive equality during the first
warmup. All 26 p95/max/order comparisons are preserved in
`target/continuation-unicode-full-comparison-analysis.md`.

Do not promote this candidate. With nearest-rank p95 taken from sample 38 of 40
and the unchanged strict 5 ms limit, the paired diagnostic yields 8/26 passing
baseline cases and 5/26 candidate cases. Highest p95 is 18.0562 versus 17.5281 ms;
worst samples are 19.8957 versus 19.6607 ms. The candidate improves nine case p95s
and eight paired case medians, which does not outweigh the remaining failures.
These diagnostic verdicts do not replace the historical production benchmark's
10/26 record or prove a production regression between runs.

CJK improves in 37/40 pairs, with p95 4.6193 to 3.5488 ms; emoji improves in 36/40,
with p95 4.1675 to 3.0485 ms. Both benefits persist in either execution order.
Composed-accent improves 28/40 pairs but its p95 barely changes, 15.6680 to
15.6125 ms. Its paired median is -0.071 ms when baseline runs first versus
-1.608 ms when candidate runs first. Decomposed-accent improves only 22/40 pairs
and its p95 worsens from 15.1061 to 16.3135 ms. General Unicode tail improvement
is therefore not established.

ASCII semantic controls improve only 380/880 pairs, with positive paired median
changes in eighteen of twenty-two cases. Address-only, tabs-scope and uppercase
lose their baseline passing verdicts: respectively 4.9883 to 5.0747, 4.9235 to
5.4703, and 4.8970 to 5.2451 ms. The candidate uses a separate compiled test
adapter, so these differences cannot be attributed solely to Unicode selection.
Adapter/code-generation effects and timing variation remain unresolved. Preserve
this negative result rather than promoting the gate or restricting evaluation
to CJK and emoji. Any future implementation proposal must address that limitation
before another production decision. No production search code changed; D48 and
its three-process acceptance requirement remain unmet.

### Complete-query partition design — 2026-10-12

Repeated per-row membership, bounds and heap work dominate the existing stage
evidence. A cache-order change cannot remove that linear pass. The continuation
therefore evaluates a structural change behind the same public API. The machine
currently has competing builds; fresh acceptance timing remains pending. The new
`complete_workload_stage_profile` diagnostic covers all 26 actual workload cases,
including mutations, and reports setup, membership/candidates, ranking, ordering
and highlights separately. Busy-machine durations are attribution, not D48 proof.

Three alternatives are considered. An incremental prefix membership cache could
save most scans during consecutive typing, at roughly 12.5 KB per 100,000-row
cached prefix plus invalidation metadata. Mutations must invalidate or repair
those snapshots. However, deletion budgets change at four
characters, arbitrary edits need fallback, and the benchmark rotates unrelated
queries with mutations. It cannot promise a cold complete-query improvement.
An aggregate index over equivalent field pairs could replace many row visits with
bucket counts and rank bounds. Its scan reduction could approach the ratio of
rows to occupied buckets, but approaches one when most field pairs are unique.
Even a single four-byte row-to-bucket mapping adds 0.4 MB at 100,000 rows, before
bucket bounds and membership metadata. Mutations must update affected buckets;
the existing range-index experiments already explore that direction.
Their promotion needs a separate review of fallback occupancy and update costs.

Choose four independent partitions of the complete bounded search. Each counts
every matching row in its partition and computes its exact top N using the
existing membership, bounds, score cache and tie ordering. The global top N must
belong to the union of those local top Ns: a discarded row already has N better
rows in its own partition. Summed disjoint scope counts remain exact. Only the
global finalists need the existing original-text highlight path. Unlike the old
matcher-only parallel probe, this divides candidate scanning and heap work too.

The estimate is up to fourfold reduction in the scan/ranking portion, not total
latency; thread creation, coordination and serial highlights limit it. Eligible
group scores are shared through one query-local table of standard-library
`OnceLock` cells: one worker computes each original representative and others
reuse that exact score, including rejection and accepted zero scores. Numeric
queries keep their existing local fallback caches. Four partitions retain
O(rows) transient candidates, four bounded query caches, at most 2,048 shared
score cells and four times N finalists, plus three native thread stacks.
No strings, index or query answers are persistently duplicated.
Small catalogues and unsupported typo budgets retain the serial path. Mutation
complexity is unchanged; the existing ranking scratch may retain capacity for
four times N finalists. Query completion joins all workers before any mutation
can proceed. Native thread creation failure falls back to computing that
partition on the caller; worker panics propagate.

Retain this change only with exact full-result comparisons, mutation/Unicode/tie
regressions and the full gate. No budget, corpus, feature, dependency or public
API changes are proposed. Three quiet fresh-process benchmarks are still the
acceptance condition; if none can run, leave them explicitly for integration.

### Partition diagnostic and score-sharing correction — 2026-10-12

The first candidate kept independent score caches. All five new regressions and
all 1,040 complete-result pairs passed, with exhaustive equality in the first
warmup. The five-pass stage profile covers all 26 cases and 130 samples. Raw
logs and checked tables are `target/d48-partition-regressions-20261012.log`,
`target/d48-stage-profile-direct-20261012.log`,
`target/d48-exactness-pairs-20261012.log`,
`target/d48-profile-summary-20261012.md` and
`target/d48-pair-summary-20261012.md`. Competing builds were active throughout:
none of these times establishes D48 or reproduces the historical 10/26 result.

The serial stage medians support dividing the row scan: broad-character spends
5.634 ms in membership/candidates and 2.615 ms in ranking, with 0.101 ms in
highlights. Long-near-match instead spends 2.588, 7.263 and 3.579 ms respectively;
serial highlight work remains a material limit after partitioning.

Independent caches improve most cases but regress long-match in 37/40 pairs:
its paired median increases 0.808 ms and p95 increases from 7.718 to 8.496 ms.
The diagnostic threshold counts are 8/26 serial and 17/26 partitioned, not valid
acceptance verdicts. Preserve that regression. Local top N selection can score
the same expensive equivalence group in every partition, which the serial path
scores once. The final design therefore shares those exact query-local scores
through `OnceLock`, retaining field guards and disabling reuse for numeric
queries. Concurrent regressions verify one computation, rejection/zero-score
parity with both typo budgets and fresh cells after recycled group IDs.
The correction passes both shared-score regressions, all five partition
regressions and a fresh 1,040-pair exactness comparison, including exhaustive
first-warmup equality. The logs are
`target/d48-shared-score-regressions-20261012.log`,
`target/d48-shared-partition-regressions-20261012.log` and
`target/d48-shared-exactness-pairs-20261012.log`; all exit zero.
`target/d48-shared-pair-summary-20261012.md` verifies every case/pass pair,
balanced execution order and generations 100131 through 101170.

The long-match regression is not reproduced: the corrected candidate improves
38/40 pairs against its serial control, with paired median -1.059 ms and p95
7.571 versus 6.242 ms. All 25 nonblank cases have negative paired median changes.
The blank control follows the same serial algorithm and changes by +0.040 ms.
Long-near-match remains the slowest candidate case at p95 8.077 ms, maximum
8.397 ms. Diagnostic threshold counts are 9/26 serial and 17/26 candidate.
These busy-machine data neither satisfy nor invalidate the required three quiet
production runs. No claim that D48 is met, or impossible, follows from them.

### Full gate and acceptance handoff — 2026-10-12

The implementation must remain reviewable while the timing environment is busy.
`cargo xtask check` passes dependency boundaries, formatting, workspace Clippy
with warnings denied, all 445 workspace tests (including 184 search unit tests
and five public matcher regressions), documentation tests and application smoke.
The log is `target/d48-gate-20261012-jobs2.log`, SHA-256
`6db10855bbd73f0ddde97b2b32c00c519508246bc6b998788642edc2ed77a8df`.
The existing wry dependency warning remains visible; no suppression or vendor
change was added. The first queued gate was stopped before executing checks to
restart with two Cargo build jobs in this process only. Its cancelled log,
`target/d48-gate-20261012.log`, is preserved. No other session was stopped.

The unchanged baseline from `7dbf579` was built before the production edits and
preserved as `target/d48-baseline-20261012/xtask.exe`, SHA-256
`13b28a7f627eb2c9e2c54d0508f32a8352a15d8397f3c8b412f150411d8666d4`.
Its dependency check executes successfully. The corrected production benchmark
executable is also built; `target/d48-final-benchmark-build-20261012.log` exits
zero. Neither build establishes timing acceptance. No Application Control block
was encountered, and no Windows setting, pin, feature, lockfile or shared product
source outside `syrinx-search` was changed.

The first 30-minute short-poll window found no build-free interval; see
`target/d48-quiet-window-20261012.log`. The final post-gate follow-up ran from
11:17 to 11:24 EDT with no build-free interval; its last check still found eleven
Cargo/compiler processes. `target/d48-acceptance-window-20261012.log` records
those checks and ending this session's waiting poll after review. Other sessions'
processes were left alone. Baseline reproduction and three fresh-process
production acceptance runs remain for Claude when the machine is quiet.
Use the preserved baseline executable with `bench` for three baseline runs, then
the unchanged `cargo xtask bench` command for three candidate runs, with a fresh
report path each time and no competing builds. Stop if baseline results differ
materially from the historical 10/26. Retain every p95, maximum and failed report.
P1-05 remains partial; D48 is unchanged. The diagnostic gains justify quiet
remeasurement, not a budget change or a claim that the design cannot meet it.

### Review corrections: diagnostics out of the gate — 2026-10-12

Review found three problems with the partition change, and this section records
how each was corrected.

**The timing diagnostics ran in every gate.** `acceptance_comparison.rs` held
three ordinary tests: `partitioned_unchanged_acceptance_workload_comparison`,
`complete_workload_stage_profile` and the earlier
`unicode_selected_unchanged_acceptance_workload_comparison`. Each builds the
100,000-row workload and runs over a thousand complete queries, so every
`cargo xtask check` paid for more than 2,000 queries and printed timings taken
on a busy machine, which mean nothing. The module now compiles only with the
new opt-in `acceptance-diagnostics` feature of `syrinx-search`. The gate builds
default features, so it no longer runs them. `#[ignore]` was rejected because
the repository forbids it; moving them into `cargo xtask bench` was rejected
because they compare test-only strategies through private catalogue functions
that a separate binary cannot reach. Their correctness assertions remain in
the gate on small data: the existing partition regressions compare the serial,
partitioned and exhaustive paths, the membership-order tests cover that
strategy, and the new
`public_partitioned_query_matches_exhaustive_for_acceptance_shapes` runs the
acceptance query shapes through the public `query` at the 16,384-row
threshold against the exhaustive oracle. Run a diagnostic explicitly with:

```powershell
cargo test -p syrinx-search --profile local --offline --locked --features acceptance-diagnostics catalogue::acceptance_comparison::partitioned_unchanged_acceptance_workload_comparison -- --nocapture --test-threads=1
cargo test -p syrinx-search --profile local --offline --locked --features acceptance-diagnostics catalogue::acceptance_comparison::complete_workload_stage_profile -- --nocapture --test-threads=1
```

Because the gate's Clippy step does not enable the feature, check the module
with `cargo clippy -p syrinx-search --all-targets --features
acceptance-diagnostics --profile local --offline --locked -- -D warnings` after
changing it. That check passed for this correction.

**A refused worker thread was silently discarded.** When the operating system
refused to start a partition worker, the code searched that partition on the
caller, which keeps results complete, but it dropped the `io::Error`. The
fallback stays, and the refusal is now counted: `Catalogue::worker_fallbacks()`
returns a `WorkerFallbacks` value with the number of refused workers and the
last error kind. The new regression
`refused_workers_fall_back_to_complete_results_and_are_recorded` uses a
test-only strategy whose workers are always refused, checks exhaustive parity
and checks that three refusals are recorded for nineteen rows.

**The quiet-machine result needs the machine's shape.** Each partitioned query
starts three native threads beside the caller, so its timing depends on how
many logical processors are free. The quiet-machine acceptance record must
state the logical processor count, for example from
`$env:NUMBER_OF_PROCESSORS`, beside the three baseline and three candidate
reports. An optional seventh run with Atlas open and a few ordinary pages
loaded shows how the four partitions behave when other work competes for
cores. Record it as a separate under-load result; it does not replace the
quiet acceptance runs, and D48 is judged on the quiet ones.
