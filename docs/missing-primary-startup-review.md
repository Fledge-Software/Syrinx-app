# Guarded startup when the primary database is missing

Validation update: integration completed formatting, all 60 focused platform
storage tests, all 23 worker tests and the normal-desktop full gate with 436
tests plus synthetic application smoke. The authoritative gate log is
`target/continuation-atomic-startup-desktop-gate.log` (SHA-256
`5654ffa30e3f5af2b8711410ea862db2d5dcb8c42b474450b294048f49ccad79`).
This supersedes the pending-validation statements in the historical handoffs
below. The first platform failures and restricted smoke timeout remain
preserved; the correction passed both rename assertions unchanged. Current
visual recovery acceptance and the original WebView2 crash remain unresolved.

Status: the owner explicitly approved this exact API and conservative fresh
initialization policy on 2026-10-11, then separately approved exactly the three
Windows feature flags listed below. Implementation is frozen after independent
source review; integration-owned validation has not run. The approval covers retained
startup ownership, matching recovery confirmation, platform preparation and
minimal app wiring/tests and only that precise feature amendment. It grants no
other Store/public-trait, dependency, vendor, security, commit or merge change.

Syrinx must not silently create an empty workspace when an established primary
database disappears. The implemented failed-start recovery flow cannot detect
that case: `WindowsDirectoryLease::prepare` in
`crates/syrinx-platform/src/storage.rs` creates an absent `syrinx.db`, after which
normal Store startup can succeed. D46 still requires explicit confirmation
before installing a backup. Tests and inspection must use disposable synthetic
state, never the owner's daily database or browser profile.

## Why existing APIs are insufficient

Calling `offer_recovery` before every open does not solve the ownership problem.
Its `NoValidBackup` result describes both fresh storage and missing or damaged
storage without a usable backup. Archive enumeration deliberately skips invalid
and incomplete bundles; an empty candidate list is not evidence of freshness.
Its `HealthyPrimary` result releases all guards before a later `prepare` can
create a now-missing file. Read-only inspection can also refuse a healthy
database needing ordinary SQLite journal recovery.

The noncreating `PreparedDirectory` is private. Public archive/control access
requires a lease whose preparation already creates the primary. Application
filesystem predicates cannot supply an equivalent guarded handoff.

## Approved API and flow

Add one platform acquisition entry point, leaving `Store::open`,
`DirectoryLease`, backup validation and explicit `RecoveryOffer::confirm`
unchanged:

```rust
pub enum StartupStorage {
    Ready(WindowsDirectoryLease),
    Recovery(RecoveryOffer),
}

pub fn prepare_startup(
    directory: &Path,
    deletions: &DeletionLog,
) -> Result<StartupStorage, RecoveryError>;
```

Retain the same checked directory identities and exclusive lifecycle ownership
through inspection and the returned lease or offer. Extract private helpers
from existing preparation/recovery code rather than duplicate their rules.

1. A pending recovery intent uses the existing resumable offer for its exact
   source. Malformed intent remains a failure.
2. An existing primary returns its retained lease for normal Store startup,
   without a preliminary read-only health check. Preserve the implemented
   failed-start recovery path and ordinary I/O/error discrimination.
3. A missing primary with a verified eligible backup returns the actual offer.
   The session worker retains it and the existing UI requests explicit matching
   confirmation. Cancellation leaves the primary absent and preserves files.
4. A missing primary with uncertain prior state or unusable backups fails
   closed. `NoValidBackup` never permits fallback to blank creation.
5. Automatic fresh initialization requires this invocation to create the final
   State directory, exclusively create and retain its lifecycle control, and
   verify that no other entries exist before exclusive primary creation.
   Creating only an ancestor, accepting `AlreadyExists`, or finding an empty
   lifecycle record does not meet that criterion.

The fresh criterion needs private creation-provenance changes:
`storage.rs::create_directory` currently accepts `AlreadyExists`, and
`storage/native.rs::open_control` uses `OPEN_ALWAYS`. An existing empty State
directory, including one left by interrupted first initialization, remains
ambiguous and would be refused under this proposal.

This is a bounded missing-primary policy. Removing the entire former State
tree erases its evidence; newly creating that tree cannot prove that it never
existed. Detecting whole-tree loss would need a durable witness outside State
and a separately approved initialization protocol. No such witness is proposed
here, and the sibling browser profile must not become an informal substitute.

## Approval boundary and ownership

The owner approved this additive platform API and the conservative fresh
criterion as one narrow follow-up slice, in answer to integration's specific
approval question. General development authorization alone did not approve
them. `AGENTS.md` requires explicit confirmation for
“Changing public APIs other code depends on”; the preceding implementation
assignment expressly limited changes to application files.

The assigned work keeps platform preparation, lifecycle/native creation and
private recovery helpers together, with minimal app session wiring and their
synthetic tests. Keep Store/public traits, all other dependency settings, vendor patches,
security settings and D46's confirmation policy unchanged. Integration owns
formatting, the full gate and all native/UI execution.

## Synthetic acceptance

- A freshly created final State directory initializes once; existing empty,
  malformed-control and interrupted-initialization directories do not.
- Missing primary plus a valid backup yields a dated offer before creation;
  invalid/incomplete backups and surviving sidecars never establish freshness.
- Cancel, disconnect and close preserve absence and all existing files.
  Matching confirmation restores once; stale confirmation cannot substitute
  another source.
- Pending/malformed recovery intent, unsupported deletion state, ordinary I/O,
  locking and cleanup errors preserve their existing refusals.
- Competing directory/control creation and primary appearance/disappearance
  cannot turn another invocation's state into this invocation's fresh state.
- Existing healthy storage, including ordinary journal recovery, follows normal
  Store startup; the failed-start recovery regressions continue passing.

## Approved amendment: retain the directory created by this invocation

Integration's implementation review identified a gap in the proposed creation
provenance. `fs::create_dir` followed by `GuardedEntry::directory` can retain a
replacement directory if another process removes and replaces the newly
created child between those calls. The retained ancestor handles protect the
ancestors, not that unguarded child. A creation boolean plus the identity read
after reopening therefore does not meet the approved fresh-state criterion.
The owner subsequently approved the typed-binding feature amendment needed
for atomic creation. Final source review and integration validation are still
required before this slice can be accepted.

The owner explicitly approved enabling exactly these additional features on the existing pinned
`windows = 0.62.2` dependency in `crates/syrinx-platform/Cargo.toml`:

```toml
"Wdk_Foundation", "Wdk_Storage_FileSystem", "Win32_System_IO"
```

The cached package's feature declarations enable only their Wdk/Win32 parent
modules. Its typed `NtCreateFile` binding additionally requires the already
enabled `Win32_Security` and `Win32_Storage_FileSystem` features. No package,
version, lockfile, Syrinx public API or operating-system minimum change is
proposed. The owner's answer was “Approve these three feature flags.” That
answer authorizes this manifest amendment only; it does not broaden the
surrounding implementation or security policy.

The private wrapper accepts a retained parent `File` and a checked single
child component, builds `UNICODE_STRING` with checked UTF-16 byte lengths, and
uses that parent handle in `OBJECT_ATTRIBUTES`. It requests `FILE_CREATE` and
`FILE_DIRECTORY_FILE`, synchronous operation with `SYNCHRONIZE`, attribute
and directory-list access, and read/write sharing without delete sharing. Collisions fail;
the wrapper never reopens a competing entry as fresh. It adopts the returned
handle into `File` before subsequent completion checks, then inspects that
same handle through `GuardedEntry::from_directory_file`. `OBJ_DONT_REPARSE`
refuses reparse traversal; a disposable junction regression awaits execution.
Microsoft
documents this user-mode operation and its returned handle in
[NtCreateFile](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile).

The wrapper maps failed native status through the already available
`Win32::Foundation::RtlNtStatusToDosError`, then `HRESULT::from_win32`, into
the existing structural `LocationError::Os` for `CreateDirectory`. Unexpected
completion or identity failure remains `LocationError::IdentityChanged`;
no path is embedded and no two-call creator fallback is used.
The fresh primary also uses strict `CREATE_NEW`, without the legacy
`create_database` helper's adoption of a racing existing file. Existing lease
preparation keeps its current semantics.

This recommendation avoids handwritten foreign-function declarations that
bypass the feature boundary. `CreateDirectory2W` is another atomic operation,
but Microsoft lists Windows 11 24H2 as its minimum; choosing it would narrow
Syrinx's Windows 11 support decision. See
[CreateDirectory2W requirements](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createdirectory2w).
The typed binding is the smaller reviewed extension. The independent source
reviewers agree that the current feature set does not supply the needed
provenance guarantee.

Test atomic creation while retaining the exact handle,
competing creation and replacement refusal, existing reparse refusal, strict
primary collision refusal, ordinary error mapping and unchanged recovery
ownership. Integration alone runs these native tests and the full gate.

## Frozen handoff for integration validation

The existing-primary lease handoff, retained recovery offers, pending-intent
continuation and application confirmation wiring are implemented. Atomic
creation retains the exact new final directory through exclusive lifecycle
control and primary creation. Fresh routing precedes established-primary
adoption, so a competing database already present at acquisition is refused.
Strict `CREATE_NEW` also rejects a database appearing after the inventory check.
Unknown entries, sidecars, archives and interrupted state do not establish
freshness. Existing empty State directories remain refused under the approved
policy; whole-tree loss remains the previously disclosed limitation.

Twenty platform and five worker regressions were added. The worker fixture now
reserves an owned parent while leaving its final State child absent, matching
normal application startup. Existing assertions remain. Reviews corrected the
fresh-path competing-primary ordering and a test that read the exclusively
held lifecycle file too early. That test now verifies backup bytes while the
offer is held, then lifecycle bytes after worker release. The journal-sibling
test proves normal Store routing without a read-only preflight; it does not
claim a genuine SQLite crash-journal replay.

The new public enum retains exactly its approved shape. To avoid carrying a
large private continuation inline, `RecoveryOffer` boxes that private field.
Integration explicitly extended file ownership to `storage/recovery/resume.rs`
for the single constructor adaptation; confirmation still consumes the same
continuation and releases the same guards. No lint suppression or public API
change was used. Independent final source review passed the atomic creation,
ownership, routing, boxing and regressions. No compilation, formatting, tests
or native execution occurred in this chat. `git diff --check` passed. Only
the exact approved platform feature flags changed dependency configuration;
package versions, the workspace manifest and lockfile are unchanged.

The exact files changed for this assignment are:

- `crates/syrinx-platform/src/lib.rs`
- `crates/syrinx-platform/Cargo.toml` (only the three approved features)
- `crates/syrinx-platform/src/storage.rs`
- `crates/syrinx-platform/src/storage/native.rs`
- `crates/syrinx-platform/src/storage/backup.rs` (retained archive directory opener)
- `crates/syrinx-platform/src/storage/session.rs`
- `crates/syrinx-platform/src/storage/recovery.rs`
- `crates/syrinx-platform/src/storage/recovery/resume.rs` (private boxed constructor)
- `crates/syrinx-platform/src/storage/recovery/tests.rs`
- `crates/syrinx-platform/src/storage/startup.rs` (new)
- `crates/syrinx-platform/src/storage/startup/tests.rs` (new)
- `crates/syrinx-app/src/session.rs`
- `crates/syrinx-app/src/session/recovery.rs`
- `crates/syrinx-app/src/session/worker_tests.rs`
- `docs/missing-primary-startup-review.md`
- `docs/parallel-browser-usability-review-2026-10-10.md`
- `docs/trapstoknow.md` (Windows metadata-only sharing exemption)

Integration alone should format and validate this frozen source:

```powershell
cargo fmt --all
cargo test -p syrinx-platform --profile local --offline --locked storage::
cargo test -p syrinx-app --profile local --offline --locked session::worker_tests
cargo xtask check
```

The prior 411-test gate validates the palette-refocus checkpoint only. It does
not validate any implementation in this handoff.

### Native validation found ineffective name retention

Integration's first focused platform run failed two unchanged rename-refusal
assertions: atomic final-directory retention and existing-primary retention.
The preserved `target/continuation-atomic-startup-platform-tests.log` records
55 passed, 2 failed and 8 filtered tests; its SHA-256 is
`bc2f91c5e9fe3e9519803333c3a027415a310e145677ed758270bcd0f6a23b09`.
This chat read and hashed that log but performed no execution.

Both handles requested only metadata access. Windows excludes such opens from
the relevant delete-sharing check, so omitting delete sharing did not retain
their names. Microsoft's
[existing-file access algorithm](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fsa/82b364ce-6d7b-422f-8d88-4db32eea809a)
describes that distinction. Retained directory opens now request
`FILE_LIST_DIRECTORY`; retained existing primary opens request `FILE_READ_DATA`.
The correction also covers legacy lease preparation, its creation-collision
fallback, ancestors and backup/recovery directories. Inspection and named
identity revalidation remain metadata-only so they can inspect an exclusively
held lifecycle control or maintenance handle. No access-control list or Windows
setting changes.

Three additional regressions check both existing-directory opener routes
without incidental child locks, an existing legacy primary with control
revalidation, and the legacy creation-collision fallback. Each verifies refusal
while retained and a successful rename after release. Both originally failing
assertions remain intact. Independent source review passed the correction and
guard-release ordering. The source is frozen for integration's formatting,
complete storage tests, worker tests and gate; no passing runtime result is yet
claimed for this correction.
