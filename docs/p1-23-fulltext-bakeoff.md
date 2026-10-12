# P1-23 full-text bake-off

Page-text recall needs a store whose deletion and recovery behavior fits D40, not just a fast index. This benchmark isolates FTS5 from product code and tests only synthetic data. Tantivy remains outside the workspace pending owner review of `docs/p1-23-fulltext-audit.md`; its resolved lru dependency has an applicable RustSec advisory. P1-24 is not started.

## Method and scope

The new `crates/fulltext-bench` executable uses the existing rusqlite pin and approved SQLite 3.53.4 patch. No product crate depends on it. The workspace wildcard already includes the crate; no root manifest or dependency-policy change is required. Cargo.lock adds only the local package and its edge to existing rusqlite. A temporary resolver change to syn was removed before final validation.

The plan explicitly gives 100,000 rows for tier 1 but does not specify a separate numeric tier 2 corpus scale in its Search or P1-23 sections. This experiment uses 100,000 documents as an explicit conservative assumption, not a newly settled product requirement. Seed `71830312407843` (`0x41544c415323`) drives the generator. URLs are under `shelfN.example.test`, titles are fictional observatory folios, and bodies use invented combinations of fourteen terms plus unique identifiers. Every tenth body is a near-duplicate of its neighbor, except its identifier; every hundredth page has 4,096 word selections instead of 128. Bodies total 131,539,344 UTF-8 bytes. French/Spanish accents, Cyrillic and CJK tokens occur throughout. This high-overlap corpus deliberately stresses common terms and is not a representative sample of natural language or the owner's browsing.

The generator materializes the corpus before the bulk timer. Bulk indexing includes one transaction, commit, FTS5 optimization and a successful WAL truncation checkpoint. Adds and updates each process 1,000 documents as a single batch with the same maintenance. Update timing includes generating replacement documents; add timing likewise includes generation. Those generation costs are absent from bulk time, so compare engines within an operation, not bulk versus incremental throughput. Explicit deletion writes synthetic integer tombstones and deletes IDs 1000–1999 in one transaction. These are benchmark-local tombstones, not the production D47 content-free deletion journal.

Captured timestamps use a fixed synthetic clock (day 10), distributed over days 0–9. Expiry removes timestamps at or before day 3: exactly seven days old is expired. The sweep includes physical active-table deletion, secure-delete-enabled FTS5 segment updates, optimization and WAL truncation. SQLite `secure_delete=ON` also applies. This establishes managed logical retention in this disposable cache; it is not forensic erasure. Database files may retain allocated free space after deletion, which is why on-disk size is measured rather than inferred from document count.

Query SQL prepares and executes `ORDER BY rank LIMIT 20`, consumes each returned identifier and a bounded 16-token snippet, and includes both in timing. FTS5 uses `unicode61 remove_diacritics 2` and prefix indexes for lengths 2 and 3. This tokenizer supports the measured word/prefix/phrase workload; the trigram alternative addresses substring search and is not compared here. Multilingual cases check literal tokens and accent folding, not language-specific segmentation, stemming or typo tolerance. A later Tantivy comparison must match these semantics or explicitly report differences. Each query has one separately reported first sample and 100 subsequent samples; nearest-rank p50/p95/max use sorted indices 49/94/99. These are warm repeated-query distributions, not cold-cache latency. Fresh processes do not flush Windows' filesystem cache. Full-text queries run before expiry, so they measure the complete stress corpus, including rows that the later sweep removes. Production must enforce expiry before exposing a search reader.

Recovery starts the same executable as a child against the disposable cache after committed deletion and expiry. The child begins a transaction, removes retained row 5 and inserts large interrupted-write sentinels at expired ID 3, explicitly deleted ID 1005 and 64 new IDs, with an eight-page cache. The setup has a 60-second timeout, and the parent requires a WAL larger than its header after killing the child. Once those writes return, it signals the parent, which forcibly kills that exact child without committing. Reopen runs SQLite and FTS5 integrity checks, verifies the expected retained count, checks age/tombstone violations and queries deleted, expired, retained and interrupted sentinels. A further transaction tries stale input through the harness's replay barrier, then verifies again after a second reopen. This is one controlled kill point, not exhaustive power-loss or crash-at-every-instruction coverage. No backup contains this cache; restoring real Atlas backups is outside this benchmark and remains later integration work.

Run environments use Windows build 10.0.26300.0, 24 reported logical processors, AMD64 Family 26 Model 36 Stepping 0, Rust 1.97.1 and Cargo 1.97.1. Other agents' build processes were observed during this session. Results therefore include possible contention and cannot certify quiet-machine latency. No machine or security settings were changed.

## Exact commands

Run from this branch's worktree in PowerShell. Each destination must not exist; the executable refuses reuse. No runtime network requests occur.

```powershell
cargo test -p fulltext-bench --profile local --offline --locked
cargo clippy -p fulltext-bench --all-targets --profile local --offline --locked -- -D warnings
cargo build -p fulltext-bench --profile local --offline --locked
1..3 | ForEach-Object {
    & target/local/fulltext-bench.exe "target/p1-23-final-$_" 100000 > "target/p1-23-final-$_.csv" 2>&1
    if ($LASTEXITCODE -ne 0) { throw "Run $_ failed" }
}
cargo xtask check > target/p1-23-gate.log 2>&1
```

The benchmark launches recovery children with `fulltext-bench.exe crash-writer <run-directory>/synthetic-cache.db`. Only the parent-created child is killed. Reproduction on the same checkout needs new directory names to preserve earlier evidence.

## Status and recommendation

All three final FTS5 processes completed successfully. Each expired 40,000 documents, retained exactly 60,000, passed SQLite/FTS5 integrity checks and rejected expired/deleted text after a forced kill, reopen and attempted stale replay. Every crash had 11,746,152 bytes of WAL data. The full workspace gate passed with exit code 0; its validation record is below.

Recommend keeping FTS5 as the measured control and withholding a production engine choice. Common-term queries on this high-overlap corpus take hundreds of milliseconds, while unique/absent queries have sub-millisecond p95. Expiry with full maintenance takes about 35 seconds. FTS5 has demonstrated the tested retention behavior, but these results do not establish acceptable interactive performance for broad queries or representative natural-language relevance. The research note's 10 ms tier-2 suggestion is not a newly approved product threshold; broad queries clearly exceed it in this experiment.

Tantivy is unmeasured and awaits owner review of its audited graph and lru advisory. Recommend auditing a corrected candidate before approval; its inspected integer-key cache does not show the advisory's stated trigger, but that scoped observation does not itself approve the dependency. Reject choosing Tantivy from third-party speed claims, silently patching lru, or declaring FTS5 the winner just because it is already installed. A quieter rerun with a broader synthetic vocabulary should accompany the eventual same-corpus two-engine comparison before P1-24. No product choice or later slice is implemented here.

## Failed exploratory attempt

The original 100,000-document attempt (`target/p1-23-fts5-run-1.csv`) completed query and retention measurements but was manually stopped during its fault setup. That setup deleted every retained row under an eight-page cache and spent several minutes preparing the crash point. The parent correctly reported failure before the ready signal; no recovery pass was claimed. The final harness narrows the uncommitted mutation to the retained/expired/deleted sentinels and 64 new documents, adds a bounded wait and confirms spilled WAL frames. It does not change the timed normal indexing, query, update, delete or expiry operations. Three fresh full runs use this final scenario; the abandoned attempt is excluded from their comparison.

A 2,000-document exploratory verification of the corrected mutation completed with 1,200 retained documents and successful recovery/replay checks. It is not one of the required full-size runs. Its log remains `target/p1-23-recovery-smoke.csv`.


## Measured operation costs

All times are milliseconds. Each mutation includes the maintenance described above; reopen timing includes integrity and content verification, not just opening a handle.

| Operation | Run 1 | Run 2 | Run 3 |
| --- | ---: | ---: | ---: |
| bulk | 13682.812 | 13036.951 | 13904.987 |
| add_1000 | 2170.998 | 2187.851 | 2194.942 |
| update_1000 | 2758.416 | 2697.085 | 2747.181 |
| delete_1000 | 586.623 | 550.188 | 602.965 |
| expiry_and_maintenance | 34706.023 | 36367.572 | 35209.572 |
| interrupted_writer_until_kill | 211.537 | 215.172 | 214.945 |
| reopen_and_verify | 5787.134 | 7690.149 | 7132.749 |
| stale_replay_and_verify | 6401.669 | 7241.242 | 6832.319 |

All runs measured 256,835,584 bytes on disk after bulk indexing and after expiry, including database and sidecars. This footprint includes stored titles, URLs, bodies, timestamps and the benchmark tombstone table, not just inverted-index terms; a Tantivy comparison must retain equivalent fields for snippet retrieval. No VACUUM/repacking is included; physical allocation is reusable but not returned to the filesystem by this policy.

## Query latency by run

Each cell is p50 / p95 / maximum in milliseconds for 100 warm samples. First samples and returned-row totals are retained in the raw output below.

| Query | Run 1 | Run 2 | Run 3 |
| --- | --- | --- | --- |
| lantern | 282.332 / 302.241 / 308.575 | 272.539 / 292.158 / 361.402 | 274.889 / 292.966 / 330.596 |
| orb* | 272.615 / 291.483 / 334.143 | 269.790 / 288.234 / 299.986 | 269.884 / 296.353 / 366.456 |
| "copper meadow" | 364.959 / 381.586 / 406.822 | 360.964 / 376.152 / 387.007 | 366.233 / 398.175 / 417.862 |
| café | 278.817 / 295.044 / 304.816 | 273.295 / 293.258 / 315.078 | 274.228 / 298.727 / 311.559 |
| naive | 281.715 / 377.193 / 771.570 | 275.801 / 291.578 / 303.058 | 281.837 / 306.085 / 320.034 |
| naïve | 278.367 / 297.184 / 333.800 | 274.690 / 291.065 / 303.436 | 281.824 / 296.650 / 339.212 |
| 星砂 | 283.693 / 403.887 / 615.515 | 277.229 / 295.142 / 301.043 | 285.190 / 340.423 / 398.433 |
| сад | 277.276 / 292.029 / 297.609 | 279.495 / 297.100 / 309.198 | 282.293 / 309.789 / 362.378 |
| lantern AND violet | 363.097 / 385.483 / 399.556 | 362.395 / 421.733 / 472.621 | 388.900 / 870.545 / 1214.486 |
| copper OR orchard | 353.595 / 375.760 / 406.354 | 359.155 / 419.904 / 681.181 | 423.984 / 1074.881 / 1468.989 |
| sigil99999 | 0.092 / 0.264 / 0.681 | 0.094 / 0.176 / 0.285 | 0.092 / 0.204 / 0.421 |
| absentquartzword | 0.039 / 0.114 / 0.426 | 0.038 / 0.104 / 0.264 | 0.038 / 0.067 / 0.074 |

## Raw final process output

The lines below are copied verbatim from each final process log. Per-sample arrays were not retained; the raw outputs contain operation durations, quantile summaries, first samples, row totals and correctness results.


### Run 1

```text
engine,sqlite,3.53.4
seed,71830312407843
count,100000
corpus_body_bytes,131539344
operation,bulk,13682.812
disk_after_bulk,256835584
query,lantern,282.332,302.241,308.575,first=287.638,rows=2020
query,orb*,272.615,291.483,334.143,first=317.785,rows=2020
query,"copper meadow",364.959,381.586,406.822,first=361.036,rows=2020
query,café,278.817,295.044,304.816,first=288.055,rows=2020
query,naive,281.715,377.193,771.570,first=310.188,rows=2020
query,naïve,278.367,297.184,333.800,first=285.636,rows=2020
query,星砂,283.693,403.887,615.515,first=272.572,rows=2020
query,сад,277.276,292.029,297.609,first=279.171,rows=2020
query,lantern AND violet,363.097,385.483,399.556,first=354.402,rows=2020
query,copper OR orchard,353.595,375.760,406.354,first=383.091,rows=2020
query,sigil99999,0.092,0.264,0.681,first=0.498,rows=101
query,absentquartzword,0.039,0.114,0.426,first=0.100,rows=0
operation,add_1000,2170.998
operation,update_1000,2758.416
operation,delete_1000,586.623
operation,expiry_and_maintenance,34706.023
expired_count,40000
disk_after_expiry,256835584
crash_wal_bytes,11746152
operation,interrupted_writer_until_kill,211.537
operation,reopen_and_verify,5787.134
operation,stale_replay_and_verify,6401.669
recovery_and_replay,PASS
final_count,60000
```

### Run 2

```text
engine,sqlite,3.53.4
seed,71830312407843
count,100000
corpus_body_bytes,131539344
operation,bulk,13036.951
disk_after_bulk,256835584
query,lantern,272.539,292.158,361.402,first=234.964,rows=2020
query,orb*,269.790,288.234,299.986,first=266.724,rows=2020
query,"copper meadow",360.964,376.152,387.007,first=357.313,rows=2020
query,café,273.295,293.258,315.078,first=267.029,rows=2020
query,naive,275.801,291.578,303.058,first=279.596,rows=2020
query,naïve,274.690,291.065,303.436,first=253.741,rows=2020
query,星砂,277.229,295.142,301.043,first=292.867,rows=2020
query,сад,279.495,297.100,309.198,first=284.832,rows=2020
query,lantern AND violet,362.395,421.733,472.621,first=353.199,rows=2020
query,copper OR orchard,359.155,419.904,681.181,first=372.162,rows=2020
query,sigil99999,0.094,0.176,0.285,first=0.613,rows=101
query,absentquartzword,0.038,0.104,0.264,first=0.240,rows=0
operation,add_1000,2187.851
operation,update_1000,2697.085
operation,delete_1000,550.188
operation,expiry_and_maintenance,36367.572
expired_count,40000
disk_after_expiry,256835584
crash_wal_bytes,11746152
operation,interrupted_writer_until_kill,215.172
operation,reopen_and_verify,7690.149
operation,stale_replay_and_verify,7241.242
recovery_and_replay,PASS
final_count,60000
```

### Run 3

```text
engine,sqlite,3.53.4
seed,71830312407843
count,100000
corpus_body_bytes,131539344
operation,bulk,13904.987
disk_after_bulk,256835584
query,lantern,274.889,292.966,330.596,first=297.419,rows=2020
query,orb*,269.884,296.353,366.456,first=281.856,rows=2020
query,"copper meadow",366.233,398.175,417.862,first=373.596,rows=2020
query,café,274.228,298.727,311.559,first=300.941,rows=2020
query,naive,281.837,306.085,320.034,first=282.416,rows=2020
query,naïve,281.824,296.650,339.212,first=278.806,rows=2020
query,星砂,285.190,340.423,398.433,first=314.036,rows=2020
query,сад,282.293,309.789,362.378,first=297.552,rows=2020
query,lantern AND violet,388.900,870.545,1214.486,first=355.501,rows=2020
query,copper OR orchard,423.984,1074.881,1468.989,first=694.698,rows=2020
query,sigil99999,0.092,0.204,0.421,first=0.441,rows=101
query,absentquartzword,0.038,0.067,0.074,first=0.099,rows=0
operation,add_1000,2194.942
operation,update_1000,2747.181
operation,delete_1000,602.965
operation,expiry_and_maintenance,35209.572
expired_count,40000
disk_after_expiry,256835584
crash_wal_bytes,11746152
operation,interrupted_writer_until_kill,214.945
operation,reopen_and_verify,7132.749
operation,stale_replay_and_verify,6832.319
recovery_and_replay,PASS
final_count,60000
```

## Measured source identities

SHA-256 values for the harness used by all three final runs:

| File | SHA-256 |
| --- | --- |
| crates/fulltext-bench/src/corpus.rs | 10307361C09D72145C8E98F892CA3D4233C5D552AFBA248AE4592BB43080915B |
| crates/fulltext-bench/src/main.rs | 98737B21E817056BB531331223E5042E0B8D82E0BD9CF2FC105285406B3F78E7 |
| crates/fulltext-bench/src/recovery.rs | CC042E20EF3F3A29849B0C160010DA4D000047F12FF67ACD0213FB2DF176D1D1 |
| crates/fulltext-bench/src/store.rs | C8BB22C94F4FECD61D69D668108B125A46FBA877236B1BCAF856D6D833FE29A1 |


Source and audit-lock hashes describe the measured working-tree bytes. Git line-ending conversion can change byte hashes on another checkout; preserve the recorded line endings when checking these identities.

## Validation and review boundary

`cargo xtask check` passed with exit code 0: dependency boundaries, formatting, workspace Clippy, 438 tests (zero failures and zero ignored), documentation-test execution, and the fresh-profile `atlas-app --smoke` run. The log is `target/p1-23-gate.log` and ends with `Local gate passed.` The pre-existing vendored-wry `platform_webview_version` unused-function warning remains visible; no suppression or vendor change was made.

The focused benchmark tests and Clippy logs are `target/p1-23-tests.log` and `target/p1-23-clippy.log`. Three final fresh-process logs are `target/p1-23-final-1.csv` through `target/p1-23-final-3.csv`; their complete output is embedded above. The reverse workspace dependency tree shows only `fulltext-bench` itself, confirming no product crate depends on it.

Shared tracked changes are limited to the seven-line local-package entry in `Cargo.lock` and the P1-23 ledger split/update in `docs/implementation-status.md`. Neither the workspace manifest nor dependency policy needed a change. Product crates and vendor files are untouched. The unapplied Tantivy lockfile diff under the benchmark's `audit/` directory is review evidence, not a workspace dependency change.

Tantivy approval, the production engine decision and independent review before any owner-approved merge remain outstanding. This branch does not begin P1-24 and is not merged or pushed.
