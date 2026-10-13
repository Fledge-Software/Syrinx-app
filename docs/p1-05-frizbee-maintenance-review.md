# P1-05: approved frizbee padding correction

Date: 2026-10-10. Status: **owner-approved and applied; independent review and unchanged full normal-desktop gate pass; restricted-context smoke failure retained**.

The owner explicitly approved this exact three-file correction and retaining
unchanged upstream comments and lint annotations in `vendor/frizbee-0.13.0`.
The narrow exception is recorded in `AGENTS.md`. Approval changes neither the
matcher contract nor the independent-review and validation requirements below.

Atlas must accept valid text without manufacturing highlight positions. The
new ASCII-filter differential tests exposed an existing frizbee 0.13.0 defect:
the query `\0` with one permitted typo gives a nonempty field without a NUL a
score of 16 and a highlight at its byte length, outside the field. Both the
filtered and exhaustive catalogue reproduce it. Keep their assertions and
Atlas's range checks; the filter is not the cause.

The observed evidence is the synthetic-only local artifact
`target/p1-05-mask-diagnostic.log`. For example, `"needle"` returns index 6,
and `"東京😀"` returns index 10. The original review inspected the baseline
without changing matcher source, dependencies or the lockfile or running a build.

## Cause and behavioral scope

The published package's `src/smith_waterman/algo/ascii.rs` loads the final
SIMD chunk with `BytesVec::load_partial`. SIMD processes several byte positions
at once; missing positions at the end are filled with zero. Lines 111–115
compare those positions to the query without masking out the missing bytes.
A NUL query therefore earns a real match score on padding. The padded score
enters the matrix maximum, and `alignment_iter.rs` finds that winning position
and returns its out-of-range index. The scalar backend also zero-fills partial
loads, so this is not specific to the owner's processor.

The Unicode scorer already has `valid_haystack_lanes` and applies its mask to
scalar starts. ASCII queries select the ASCII scorer even when a field contains
Unicode, explaining the second example. The correction belongs in ASCII
scoring, not in UTF-8 range conversion or the catalogue's candidate filter.

This affects scores and potentially ranking as well as highlights. Discarding
invalid indices would conceal the inflated score. Membership is a separate
prefilter decision: deleting the one-character NUL query fits a one-typo
budget, so a field without NUL remains a valid zero-score match with no
highlights. The approved patch leaves the prefilter and those memberships
unchanged. Genuine NUL bytes remain matchable. Longer queries containing NUL
can also gain false scores from padding, so the regression must cover them.
The existing over-1024-byte greedy path is outside this patch.

## Exact baseline and approved patch

Use the already audited published package, not an unreviewed upstream branch.
The archive `target/dependency-audit/frizbee-0.13.0/source.crate` was rehashed
for this review and matches the registry identity in `docs/dependency-audit.md`:

```text
Package: frizbee 0.13.0
Archive SHA-256: 19435f276a44d90570cb403d22fbe4b20c80eabc134f718d6ec0ae888eb47edc
Packaged upstream commit: d608923f526c57c7ac90b79c0cb1b8619b449179
Licence: MIT
Features: default-features=false, std + safe_read
```

Primary source references are the published
[ASCII scorer](https://docs.rs/crate/frizbee/0.13.0/source/src/smith_waterman/algo/ascii.rs),
[Unicode scorer](https://docs.rs/crate/frizbee/0.13.0/source/src/smith_waterman/algo/unicode.rs),
[traceback](https://docs.rs/crate/frizbee/0.13.0/source/src/smith_waterman/alignment_iter.rs)
and [matcher](https://docs.rs/crate/frizbee/0.13.0/source/src/matcher/algo.rs).
The cached, previously audited bytes were inspected directly in this review.

The patch moves the existing lane-mask helper into the shared algorithm
module, reuses it in ASCII, masks both match identities and stored scores,
and otherwise preserves the algorithm. Masking stored scores ensures a
padded cell cannot win or enter traceback, including with unusual scoring
settings. A chunk mask is computed once per chunk, outside the query-row loop.
The owner approved the following exact diff; approval alone is not evidence
that it has been applied, compiled or validated:

```diff
--- a/src/smith_waterman/algo/mod.rs
+++ b/src/smith_waterman/algo/mod.rs
@@
 pub(crate) const MAX_HAYSTACK_LEN: usize = 1024;

+#[inline(always)]
+unsafe fn valid_haystack_lanes<B: Backend>(haystack_len: usize, start: usize) -> B::Mask {
+    unsafe {
+        let valid_lanes = haystack_len.saturating_sub(start).min(B::LANES);
+        let mut bytes = [0u8; 64];
+        for byte in bytes.iter_mut().take(valid_lanes) {
+            *byte = u8::MAX;
+        }
+        let valid_bytes = B::Bytes::load_partial(bytes.as_ptr(), 0, B::LANES);
+        valid_bytes.gt(B::Bytes::splat(0))
+    }
+}
+
 impl<B: Backend> Kernel for SmithWaterman<B> {
--- a/src/smith_waterman/algo/unicode.rs
+++ b/src/smith_waterman/algo/unicode.rs
@@
-use super::MAX_HAYSTACK_LEN;
+use super::{MAX_HAYSTACK_LEN, valid_haystack_lanes};
@@
-#[inline(always)]
-unsafe fn valid_haystack_lanes<B: Backend>(haystack_len: usize, start: usize) -> B::Mask {
-    unsafe {
-        let valid_lanes = haystack_len.saturating_sub(start).min(B::LANES);
-        let mut bytes = [0u8; 64];
-        for byte in bytes.iter_mut().take(valid_lanes) {
-            *byte = u8::MAX;
-        }
-        let valid_bytes = B::Bytes::load_partial(bytes.as_ptr(), 0, B::LANES);
-        valid_bytes.gt(B::Bytes::splat(0))
-    }
-}
--- a/src/smith_waterman/algo/ascii.rs
+++ b/src/smith_waterman/algo/ascii.rs
@@
-use super::MAX_HAYSTACK_LEN;
+use super::{MAX_HAYSTACK_LEN, valid_haystack_lanes};
@@
                 (col_idx + 1, haystack_chunk)
             }) {
+                // Exclude zero-filled padding from matches and matrix scores.
+                let valid_mask =
+                    valid_haystack_lanes::<B>(haystack.len(), (col_idx - 1) * B::LANES);
+                let valid_score_mask = B::widen_mask(valid_mask);
+
                 // Bonus for matching a capital letter after a lowercase letter
@@
-                    let exact_case_match_mask = needle_char.eq(haystack_chunk);
-                    let flipped_case_match_mask = flipped_case_needle_char.eq(haystack_chunk);
+                    let exact_case_match_mask = needle_char.eq(haystack_chunk).and(valid_mask);
+                    let flipped_case_match_mask =
+                        flipped_case_needle_char.eq(haystack_chunk).and(valid_mask);
@@
                         gap_open_penalty,
                         gap_extend_penalty,
-                    );
+                    )
+                    .and(valid_score_mask);

                     score_matrix.set(row_idx, col_idx, row_scores);
```

The three baseline SHA-256 values are:

| Path below `src/smith_waterman/algo/` | SHA-256 |
| --- | --- |
| `ascii.rs` | `e8005d38ce5ad71b58d23c52d6dead6b6b0c3f986c0aa7c45e703b5ab8c934ce` |
| `mod.rs` | `40b14e6d48c422b7221f9013a33f6bd958df27d6c38ab6d1c29f6432500446ea` |
| `unicode.rs` | `53ba2cb8e7fe28a91419d85442e8e2ff11cd135be1f6b2c23ddb6482c11e96b3` |

## Approved scope and required provenance

The approval covers only this correction to the audited frizbee 0.13.0
package, with its pin, features, safety checks, active dependency graph and
licence unchanged. Apply it through a local `vendor/frizbee-0.13.0` override.
Record the complete package inventory, unchanged-file hashes, the exact patch
and modified hashes in a provenance file beside it. Review the lockfile so
only frizbee's registry source/checksum changes; do not shift package versions.

`AGENTS.md` now records the separate narrow exception for this source patch
and for retaining existing upstream comments and lint annotations unchanged
inside `vendor/frizbee-0.13.0`. The earlier SQLite/wry exceptions and D12/D37
did not supply this authorization; the owner's explicit frizbee approval does.
Atlas-authored changes must add no lint suppression or deferred-work markers.
No new licence exception is needed. Approval does not authorize a commit,
merge, push or upstream message; an upstream-ready patch can be prepared
locally and submitted only with explicit permission.

The alternative is waiting for an upstream release containing a reviewed fix.
Rejecting NUL input, dropping accepted matches or weakening the failing tests
would change the agreed input contract or hide the defect and is not proposed.

## Validation before acceptance

- Keep all four ASCII-filter differential tests, including every ASCII byte,
  and require the optimized and exhaustive results both to succeed and match.
- Add exact NUL regressions for no-NUL fields with typo budgets zero and one,
  real NUL at the beginning/middle/end, repeated NUL and mixed queries such as
  `a\0` and `\0a`. Check membership, score and indices through list, single-match
  and indexed APIs; the one-typo/no-NUL case must remain present with score zero.
- Cover lengths around 8/16/32/64-byte chunk boundaries and 1023/1024/1025,
  empty fields, Unicode fields under ASCII queries, and Unicode queries that
  contain NUL. Test supported scalar and SIMD backends against real-byte
  expected results, including zero-penalty scoring. Backend parity alone can
  miss a defect shared by every backend.
- Reuse existing upstream test infrastructure only after reviewing its test
  dependencies, or place public-API regressions in Atlas's existing test crate.
  Do not bring upstream benchmark dependencies into Atlas merely to run them.
- Run the full unchanged Atlas gate and benchmark. Verify non-NUL benchmark
  membership/counts/checksums against the recorded results and report timing;
  the added mask is not evidence that the outstanding five-millisecond goal
  is met. Any changed non-NUL score needs investigation rather than a new
  expected value without explanation.

The approval record makes no corrected-output, test-pass or performance claim.
Implement the bounded patch, complete independent source review and the central
validation gate, then record the exact provenance and runtime evidence. Keep
any unresolved failure visible; approval itself does not close P1-05.

## Applied correction and validation evidence

The patch is now applied through the local override. Independent review
compared every packaged path and file against the checksum-verified archive:
all 76 files remain present, 73 are byte-identical and exactly the three
approved algorithm files differ. The adjacent patch exactly reproduces those
differences. `vendor/frizbee-0.13.0.provenance.md` records the archive identity,
complete baseline/final inventory and hashes; the patch SHA-256 is
`7324609c1a7dabbb50b7a40ca61212b92883606b86fa89927d33c67f104218d3`.
The exact version, MIT licence, `std`/`safe_read` features and greedy fallback
remain unchanged. No upstream submission, commit or merge has occurred.

Root's edit record identifies the lockfile change as removal of only
frizbee's registry source/checksum lines; every subsequent Cargo invocation
uses `--locked`, and the gate's dependency check passes. No immediate pre-patch
lockfile snapshot was retained, so this is not an independent whole-lock
before/after comparison. The large preceding uncommitted dependency graph
is preserved; do not mistake its diff from HEAD for changes from this patch.

All five public-API regression tests pass in
`target/continuation-frizbee-public-tests.log`, SHA-256
`24c48b7e6e584272bebad97a04be872c0773a3776845169f85fcccc618cd4dee`.
They assert membership, literal scores and real-byte indices across single,
list and indexed APIs, zero/one deletion, genuine/absent/repeated NUL,
Unicode, zero-score matches and chunk/1024-byte boundaries. The existing
ASCII-filter differential regressions also pass in the initial full run.

All eight scalar/SSE/AVX/AVX-512 score-width variants were available and pass
literal scores, indices and an independent zero-penalty longest-common-
subsequence oracle. Evidence: `target/continuation-frizbee-backend-tests.log`,
SHA-256 `c9f12c62654944f801b4427de3bb095a4ceca6c300b0ee3a6a9c6c689c541f0c`.
This dependency-free harness uses a separate ignored source overlay under
`target/frizbee-padding-validation`. Independent review verified all 51 source
files against its manifest. Its only transformations disable 96 upstream
test-module/helper gates and append the private validation module; vendor
sources and Atlas's gate tests remain intact. It exercises the available
Windows x86-64 backends rather than claiming cross-platform verification or
a run of upstream's entire development test suite.

The unchanged production benchmark preserves all 1,040 sample results.
Independent comparison checks all 3,120 serialized checksum/hit/match integer
tokens exactly against the preceding boost-helper report, avoiding floating-
point conversion of checksums. Case/query identities are also unchanged.
Only 9/26 cases pass D48; the worst sample is 16.6731 ms. Report:
`target/continuation-frizbee-bench.json`, SHA-256
`9240754dc766a6dcd4d1a83a8a75a08d33b12f0be1e9e10e21ba5d32a05fb05c`.
This is correctness and diagnostic timing evidence, not performance acceptance
or a controlled estimate of the mask's isolated cost.

The initial full gate reaches 146/147 search tests: the remaining fixture
incorrectly required the old padding defect to return `InvalidMatchRange`.
The independently reviewed correction now requires successful empty highlights
for a deleted NUL and separately injects an invalid cached index in each
field, preserving explicit original-text range validation and reuse counters.
No production highlight code changed and neither original NUL differential
regression was weakened. The repeat gate passes all tests but fails its restricted-context smoke, as
recorded below; the unchanged normal-desktop gate subsequently passes. P1-05 performance and the original
WebView2 crash remain open.

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
