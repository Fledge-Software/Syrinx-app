# Backups that remain usable after interruption

P1-04 protects the saved destinations and session records introduced in
P1-03. Copying a file is not enough: an incomplete backup must never displace
a valid one, and a failed restore must never make Atlas initialize an empty
database. This review records those boundaries and their verified implementation.

Status on 2026-10-10: implementation verified on `codex/p1-04-backups`, branched from
`2e3d261`. The separate mobile-sync outline was subsequently committed as
`8cc0e50`; preserve that work. P1-03's product commit is `c4a6631`, already
merged with the owner's approval. The owner approved the SQLite maintenance
patch and scoped upstream-comment exception described in
`docs/p1-04-sqlite-maintenance-review.md`. That prerequisite is applied and
passed its prerequisite gate with 78 tests. The complete P1-04 implementation
now passes the full local gate with 138 tests and independent source reviews.
Lifecycle ownership, daily snapshots, rotation, confirmed restoration,
interruption resume and the pure deletion-log contract are implemented.
The owner explicitly approved the P1-04 commit and local merge on 2026-10-10.
Plan 1's ledger records the resulting commit and merge completion.

## Approved behavior and scope

The owner chose confirmation before restoration. Atlas offers the newest
verified eligible backup with its date, and preserves damaged files locally.
Opening a store or discovering corruption does not authorize replacement.
P1-04 supplies the headless result and explicit restore operation; the later
application interface presents the confirmation.
Bind confirmation to the offered backup and current primary identities.
If the chosen candidate becomes invalid, offer a new choice rather than
silently substituting an older backup under the previous confirmation.

The owner also chose a local deletion record to prevent cleared history
from returning through an older backup. That record contains only cleared
date ranges and data categories, never URLs or page contents. It lives
outside the restored database. P1-27 will connect actual history clearing
and its other storage participants; P1-04 establishes the restore boundary
without inventing a history table or collecting browsing data.

D13 already requires idle daily `VACUUM INTO` snapshots and seven retained
copies. D40 excludes the expiring page-text cache and index from these
durable-state backups. The chosen recovery behavior does not change the
seven-day page-text limit or authorize access to the owner's daily data.

## Existing code that constrains the implementation

`crates/atlas-store/src/engine.rs` owns one serialized writer and two read
connections. `Store::open` consumes a shared `DirectoryLease`; every SQLite
connection retains that lease through close. `schema.rs` refuses foreign,
future and altered schemas before persistent configuration, and `records.rs`
checks core snapshot invariants. Preserve these boundaries and redacted errors.

`crates/atlas-platform/src/storage.rs` retains ancestor and main-database
handles without delete sharing. That prevents a checked path being replaced
while SQLite uses it. Sidecar inspection handles are released so SQLite can
perform its normal WAL cleanup. A recovery operation cannot rename the main
file while its ordinary lease or any SQLite connection remains alive.

The current `WindowsDirectoryLease::prepare` creates `atlas.db` if absent.
Before adding replacement, every normal open must detect unfinished recovery
before reaching that creation path. Otherwise a crash between preserving
the damaged file and installing its replacement could silently create a
blank database. An application-wide single-instance feature is later work;
exclusive ownership of this storage lifecycle is a P1-04 prerequisite.

## Implemented backup sequence

1. The host supplies time and idle status to a blocking maintenance request
   on the existing writer queue. There is no polling or timer thread in the
   store. Measure the interval from the last successfully published snapshot;
   a failed attempt must remain due. Test clock reversal and restart behavior.
2. The platform reserves a fresh guarded bundle with an opaque identifier and
   a fixed, empty database filename. SQL receives the checked destination as
   a bound parameter. It never overwrites a prior backup.
3. Run `VACUUM main INTO ?1` outside a transaction on the serialized writer.
   Ordinary writes cannot interleave on that connection. Cancellation, if
   exposed, must wait for SQLite to finish before disposing of output and
   must not interrupt an unrelated write.
4. Open the candidate read-only and require full `integrity_check`,
   `foreign_key_check`, the supported Atlas schema and core record validation.
   Reject unexpected sidecars. Do not treat `query_only` as read-only access
   or use `immutable=1` to avoid proper locking. Close validation connections
   before publishing the candidate.
5. Publish a checked completion record last, carrying only its format,
   creation time, source revision and deletion-record generation. Missing or
   malformed completion records do not identify eligible snapshots. Bound
   parsing and keep all filesystem names and SQL diagnostics out of errors.
6. Only after a new valid snapshot is published, prune the oldest recognized
   valid copies beyond seven through checked filesystem identities. Report
   pruning failures and temporarily retain extra copies. Incomplete output,
   foreign entries and preserved damaged files never count as valid backups
   or become arbitrary recursive-deletion targets.

SQLite documents that successful `VACUUM INTO` flushes output when the source
uses `synchronous=NORMAL` or `FULL`; P1-03 uses NORMAL. Interrupted output can
still be unusable. That flush does not establish durability of later Atlas
metadata writes or filesystem operations. Publication needs its own ordered
writes and failure tests. See [SQLite's VACUUM contract](https://www.sqlite.org/lang_vacuum.html).

## Startup, clean shutdown and recovery

Use a small external lifecycle record, held under exclusive storage-session
ownership, to identify an unclean exit. Mark a session running before opening
SQLite, and finalize it as clean only after every connection closes and the
writer joins successfully. This avoids a false clean result if a database
flag commits but closing that database then fails. It needs no schema migration
solely for the shutdown flag and does not change the model's save revision.
A missing or malformed lifecycle record provides no evidence of a clean exit;
validate an existing database conservatively. Failed startup and fallback drop
must not mark the session clean merely because connections eventually close.

After an unclean exit, run `quick_check` before admitting writes, together
with the existing Atlas schema and record checks. A foreign or future schema
is a refusal, not a reason to replace data with an older backup. Operational
failures such as permissions, a busy database or disk exhaustion must also
remain distinct from corruption. `quick_check` does not cover every integrity
constraint; the stronger backup validation remains necessary. See
[SQLite's PRAGMA contracts](https://www.sqlite.org/pragma.html#pragma_quick_check).

Shutdown drains writes and maintenance and reports all failures. Only a
successful orderly close may leave clean-shutdown state. One exclusive
directory gate excludes independently prepared leases, including aliases;
the three connections share the same lease. A separate session claim must
also reject two stores opened through the same shared lease. Review that
additive capability in `atlas-store/src/location.rs`; retaining path identities
alone does not establish exclusive session ownership.

### Session capability

Implemented and runtime-verified on 2026-10-10. The contract preserves
`Store::open(Arc<dyn DirectoryLease>)` and the lease's existing path and
revalidation methods. A provided method keeps existing lease implementations
compatible:

```rust
fn session_control(&self) -> Option<&dyn SessionControl> {
    None
}

pub trait SessionControl: Send + Sync {
    fn begin(&self) -> Result<Box<dyn StorageSession>, LocationError>;
}

pub trait StorageSession: Send + Sync {
    fn previous_shutdown(&self) -> ShutdownState;
    fn complete(self: Box<Self>) -> Result<(), LocationError>;
}

pub enum ShutdownState {
    Clean,
    Unclean,
}
```

`None` supplies no clean-shutdown proof: the store conservatively checks an
existing database on every open. It is suitable for existing synthetic
leases, not evidence that an arbitrary lease implementation excludes other
users of the same database. The production Windows lease supplies the
capability and retains one exclusive filesystem gate; all three connections
share that lease. `begin` claims an active flag in the Windows control object,
so a second store using the same `Arc` receives a structural busy error.
Independently prepared leases are excluded by the filesystem gate. No global
session registry or shared-to-exclusive gate upgrade is required.

Order the session operations as follows:

1. `Store::open` retains a strong lease and calls `begin` before spawning work
   that can open SQLite. The token reserves session ownership, reads the
   previous status, and durably marks the new session dirty before returning.
   SQLite opening can itself perform journal recovery, so there is no separate
   dirty-marking call for the caller to forget. Failure returns before SQLite
   opens and releases the active claim. A foreign-database refusal may leave
   the external status dirty; it does not authorize altering that database.
2. The caller retains the token through startup and the entire store lifetime;
   the writer receives only the previous-shutdown value. Do not give the
   writer sole ownership of the token: a worker panic must not release the
   session claim while read connections remain alive.
3. Explicit shutdown closes readers, drains and joins the writer, and checks
   every close result before consuming the token with `complete`. Completion
   durably marks clean and then releases ownership. A failed startup, worker
   failure, close error or fallback `Drop` releases ownership without marking
   clean. Token destruction performs no fallible filesystem operation.
4. Keep the strong lease until token completion or destruction, including all
   failure paths. The owner's later restore confirmation can then transition
   a uniquely owned lease into maintenance without a live connection left
   behind. Unfinished recovery intent must still be checked before normal
   `prepare` can create a missing primary; the session token does not replace
   that earlier gate.

This prerequisite is now implemented: the platform owns the
exclusive gate, external status and token; the store owns ordering and error
propagation. First test overlap refusal, startup failure, writer failure,
close failure and post-close finalization on synthetic locations. Do not add
general path operations to this capability or claim backups, rotation or
restoration complete from these tests. The approved maintenance patch has
now passed source verification and the full gate; subsequent damaged-input
tests must use that verified build and disposable synthetic stores.

A failed final marker flush is reported and poisons reuse of that control.
It cannot prove that cached clean bytes did not reach disk, so a later reopen
may see either state. The clean write occurs only after successful database
connection cleanup; failed startup, worker cleanup and fallback destruction
never attempt that write.

Recovery is a separately authorized, exclusive maintenance operation:

1. Close every connection and release all running model state. Retain checked
   directory identities and exclude competing normal opens for the operation.
2. Copy the chosen eligible backup into a fresh staging bundle. Never move
   the only valid backup into the primary location. Validate the copy and
   apply every applicable deletion record before it can become live. Validate
   and flush the staged result again after replay, and record its applied
   deletion generation before moving any primary file.
3. Persist a structural recovery intent before moving primary files. Use
   opaque bundle identities and fixed names, not unchecked paths from disk.
4. Preserve the old database and its remaining WAL, SHM and journal files
   together in a fresh local quarantine bundle. Reacquire the necessary
   file access only after ordinary no-delete leases are gone, and compare
   identities before each move. Do not overwrite an unexpected destination.
5. Install the staged database and reacquire its ordinary guard. No sidecar
   from the old database may accompany it. Complete the intent only after
   the installed database and privacy state have passed the startup contract.
6. If interrupted, normal startup refuses to create or open a new primary
   until the intent can be reconciled. Ambiguous states stay blocked with a
   structural recovery result; they do not trigger guessed file deletion.

Windows replacement calls have documented partial-failure states. The design
therefore uses explicit recoverable stages rather than assuming that one
rename makes the complete operation atomic. Preserved damaged files are not
part of automatic seven-backup rotation. See [ReplaceFileW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew)
and [SQLite's WAL lifecycle](https://www.sqlite.org/wal.html#the_wal_file).

## Deletion-record boundary

The deletion journal and its generation must never be rolled backward by
restoring `atlas.db`. Durably record a requested clear, apply it to every
required storage participant, durably record completion, and only then
acknowledge success. An interrupted clear remains pending; startup and
recovery must finish the required work before exposing affected records.
Once initialized, the journal is required for recovery even if an old backup
claims generation zero. A missing journal, torn record, sequence gap or
unavailable required generation prevents recovery instead of silently
accepting older data. Initialization must itself distinguish a genuinely new
store from missing previously established control state.

A restored candidate must prove that its applicable clear operations have
been applied. An unsupported category or schema cannot be ignored. P1-04 can
test this contract with structural synthetic records and rejection paths;
P1-27 must add end-to-end tests using synthetic history, the full-text index
and the WebView2 clearing result before history clearing ships. Page text is
excluded from these snapshots regardless of the journal. P1-04 implements
the pure codec and replay contract in `atlas-store/src/deletion.rs` and
refuses recovery with any pending or nonzero deletion state. It does not
persist the journal or claim to clear history. P1-27 must connect the durable
external journal/generation barrier and actual clearing/replay participants
before history clearing ships.

## Validation result

The full local gate passed on 2026-10-10 with 138 tests and no ignored or
filtered tests. Failure coverage includes snapshot validation/flush/uncertain
publication, corruption after publication, pruning, lifecycle startup/close
failures, malformed markers and intent, and all six known recovery
interruption checkpoints. Each known interruption completes through a renewed
offer for the original source. Unknown identities remain refused. Native
integration verifies seven retained valid copies, restart scheduling,
corrupt-newest fallback, preservation of incomplete output and long paths.
Detailed commands and counts are in `docs/LOCAL-DEV.md`.

## Evidence required before calling P1-04 complete

Use disposable synthetic directories and reserved domains exclusively:

- Verify the approved SQLite source/version and unchanged dependency graph,
  including the Windows long-path and journal-suffix integration cases.
- Test due/idle scheduling, failed-attempt retry, clock reversal, restart,
  clean and unclean startup, the first startup without a lifecycle record,
  and refusal of overlapping sessions without falsely marking either clean.
- Interrupt every backup publication stage; the previous good snapshot must
  remain eligible. Eight successful backups should leave seven valid copies.
  Corrupt newest output must fall back safely; pruning failures must surface.
- Reject foreign/future schemas, malformed markers, unexpected sidecars,
  reparse points, hardlinks and identities changed between checking and use.
- Interrupt each restore stage, including absence of the primary after
  quarantine. Reopening must not create a blank database or attach stale WAL.
  Confirm that the original backup and preserved damaged files survive.
- Reject restore without explicit confirmation or with live store ownership.
  Prove deletion-record state survives replacement and cannot be bypassed by
  an old backup or an interrupted clear.
- Run `cargo xtask check` on this machine and obtain independent store,
  native-lifecycle and documentation review before requesting the slice's
  commit and merge. The owner's P1-03 approval does not cover P1-04.
