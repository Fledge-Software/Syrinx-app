# P1-03 storage-path preflight

Date: 2026-10-10. Status: implemented, local gate and independent review passed;
committed and merged as `c4a6631` with the owner's approval.
The source review used no owner database or provider listing. Runtime tests
query registered-root metadata through the guard, keep it out of output and
open only newly created synthetic databases. Results are recorded below.

D13 places the durable SQLite database in the local app-data folder and
forbids synced locations. P1-03 must check the location before SQLite can
create or modify a database. D40 keeps the later expiring page-text cache
outside durable-state backups; this document does not add that cache or
P1-04's snapshot implementation.

## Boundary and dependency gate

`syrinx-platform` implements Windows filesystem discovery and validation.
`syrinx-store` defines a small owned directory-guard contract and accepts
its implementation from the application. The dependency points from platform
to store, never from store to platform. Introducing the filesystem-only
platform crate is a prerequisite of P1-03, not the start of browser or UI work.

The `DirectoryLease` contract supplies the fixed database path and a fallible
revalidation operation. The store retains the guard until every writer and
reader connection has closed; connection ownership must make early guard
release impossible. Raw, unchecked production path opens should not bypass
this boundary. Tests can supply a disposable root through the same validator
or inject failures into its OS-query boundary, rather than a skip-check flag.

The pinned `windows = "=0.62.2"` source contains the required APIs. Its selected
explicit namespace features are:

| Feature | Purpose |
| --- | --- |
| `Storage_Provider` | Enumerate registered sync roots; transitively enables `Storage` and `Foundation`. |
| `Win32_Foundation` | Handles and Windows error values. |
| `Win32_Security` | Types required by `CreateFileW` and `CreateDirectoryW`, even with null security attributes. |
| `Win32_Storage_FileSystem` | Open directories, inspect file identity and attributes, resolve paths and classify drives. |
| `Win32_System_Com` | Free the known-folder path allocation. |
| `Win32_System_WinRT` | Initialize and release the Windows Runtime apartment used for enumeration. |
| `Win32_System_WindowsProgramming` | Named drive-type constants, including `DRIVE_FIXED`. |
| `Win32_UI_Shell` | Discover `FOLDERID_LocalAppData`. |

The complete active graph and exact package versions are reviewed in
`docs/dependency-audit.md`; default features, including `std`, are disabled
for `windows`. No `CloudFilters`, WebView2 or GPUI feature is needed. The owner
approved Unicode-3.0 for `unicode-ident 1.0.26` and Zlib for SQLite's
`foldhash 0.2.0` dependency on 2026-10-10. These are narrow package allowances,
not a licence selection for Syrinx.

## Discovery and refusal rules

Use `SHGetKnownFolderPath(FOLDERID_LocalAppData, 0, ...)` for the current user,
then append Syrinx's fixed child names. Environment variables, the working
directory and a folder name containing `OneDrive` are not location proofs.
Do not request known-folder creation during discovery. Manage the returned
allocation according to the API's `CoTaskMemFree` requirement.
[Microsoft known-folder API](https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shgetknownfolderpath)

Reject relative paths, drive-relative paths, parent traversal, arbitrary
device namespaces, alternate data streams and UNC paths before opening them.
Use `GetDriveTypeW` with an explicit root and trailing separator, accepting
only `DRIVE_FIXED`; remote, removable, unknown and missing-root results fail
closed. A drive letter alone does not prove locality. Normalized local paths
returned with the `\\?\` prefix are not automatically unsafe.
[Microsoft drive classification](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdrivetypew)

Enumerate `StorageProviderSyncRootManager::GetCurrentSyncRoots`, then obtain
each entry's `Path` and its folder path. Microsoft documents this collection
as including registered legacy roots. Perform WinRT work on an initialized
thread, balance every successful `RoInitialize`, including `S_FALSE`, with
`RoUninitialize`, and release WinRT objects before that balance completes.
Return owned path/identity results to the guard; do not retain account or
provider descriptions or put them in diagnostics.
[Sync-root enumeration](https://learn.microsoft.com/en-us/uwp/api/windows.storage.provider.storageprovidersyncrootmanager.getcurrentsyncroots?view=winrt-26100),
[root path](https://learn.microsoft.com/en-us/uwp/api/windows.storage.provider.storageprovidersyncrootinfo.path?view=winrt-26100),
[WinRT initialization](https://learn.microsoft.com/en-us/windows/win32/api/roapi/nf-roapi-roinitialize)

Failure to enumerate, extract a path or establish a relevant root's identity
is an unknown result and refuses the open. An empty successful enumeration
means no registered roots were returned, not that no software can synchronize
the directory. Unsupported APIs do not silently fall back to name matching.

Compare real directory identities against the candidate's ancestor identities
where possible. `FILE_ID_INFO` combines a volume serial number with a file ID;
this avoids short-name, case and path-alias mistakes. If path containment is
used as a preliminary check, compare normalized components, not string
prefixes or ad hoc Unicode lowercasing. A potentially relevant registered root
that cannot be resolved must not be treated as unrelated. Query only metadata,
not the files inside any registered root.
[Microsoft file identity](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_id_info)

`CfGetSyncRootInfoByHandle` is not the negative proof in this design. Its
documentation says a path outside a root fails but does not specify that
function's exact no-root HRESULT. Treating every failure as "not synced"
would also accept permission and API failures.
[Cloud Files query contract](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfgetsyncrootinfobyhandle)

## Directory and database lifetimes

Walk existing ancestors from the root toward the destination. Open each with
`OPEN_EXISTING`, `FILE_READ_ATTRIBUTES`,
`FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT`, and
`FILE_SHARE_READ | FILE_SHARE_WRITE`, deliberately excluding
`FILE_SHARE_DELETE`. Reject reparse points, non-directory ancestors,
delete-pending entries and failed metadata queries. Checking only the final
component misses a redirecting ancestor. Retain the validated directory
handles while the database is open: omitting delete sharing prevents ordinary
delete/rename opens against those directories. It does not lock their entire
contents against writes.
[Microsoft open and sharing contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)

Resolve the handle's final path and volume, and query `FileAttributeTagInfo`,
`FileStandardInfo` and `FileIdInfo` with `GetFileInformationByHandleEx`.
Unsupported identity information or normalization failures refuse the open.
Create a missing Syrinx-owned directory one component at a time only after its
parent passes, then immediately validate and retain the new handle. Check
again before SQLite opens its first connection; all later connections reuse
the retained proof. A normalized path alone is insufficient because
normalization follows redirects.
[Handle information](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex),
[final path](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfinalpathnamebyhandlew)

Inspect an existing `syrinx.db` and its `-wal`, `-shm` and `-journal` companions
before SQLite opens anything. Refuse a reparse point, directory,
delete-pending file or `NumberOfLinks` other than one. Holding an additional
non-delete-shared main-database handle can preserve its identity; its
compatibility with the selected SQLite build must be tested. For a new
database, create it without truncating or replacing an existing file, inspect
and retain its identity, then let SQLite initialize it.
[Microsoft standard file information](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_standard_info)

Do not retain non-delete-shared handles for WAL, SHM or journal files for the
whole connection lifetime. SQLite ordinarily removes WAL and SHM at the last
connection close, and journal deletion is part of its transaction modes.
Release sidecar inspection handles before SQLite needs to manage them; keep
the directory lease alive until all SQLite connections are actually closed.
Never manually delete a WAL to recover from a guard error.
[SQLite WAL lifecycle](https://sqlite.org/wal.html)

## Honest limits

This is a conservative check for local filesystem placement, registered sync
roots and detectable redirects or hardlinks. It cannot prove that an
unregistered third-party program does not copy the directory. Enumeration is
a point-in-time result; another process could register synchronization later.
Do not describe this as a complete guarantee against synchronization.

Directory handles prevent normal ancestor replacement through delete/rename,
but they do not establish an adversarial filesystem sandbox. A same-user
process can change metadata, add hardlinks or replace sidecars after a check.
A custom SQLite virtual filesystem or broader access-control design would be
separate work and is not implied by P1-03. Revalidation narrows races but does
not remove this limitation. Errors should identify the failing operation and
safe error category/code, without logging private paths or root names.

## Verification before calling P1-03 complete

All runtime cases below use newly created synthetic paths and disposable
databases. They must not discover or open the owner's daily database.

1. Unit-test fixed/remote/removable/unknown drive outcomes, malformed paths,
   enumeration failure, missing root paths and identity-query failure. Refusal
   must occur before database creation or modification.
2. Test root equality and ancestor containment with component boundaries,
   case variants, extended paths and synthetic identity aliases. A similarly
   prefixed sibling is not an ancestor. Inject registered-root results; do
   not register a real provider just to make a test pass.
3. On this machine, exercise successful WinRT enumeration and the normal
   guarded-open path using a disposable directory, while keeping returned
   root paths/providers out of output.
4. Use disposable junctions and, where existing permissions permit, symbolic
   links at the destination and at an ancestor. Both must be refused without
   creating a database at their target. Do not change Windows settings to
   enable a test; report any unavailable test case.
5. Test synthetic hardlinks for each database/sidecar filename and injected
   delete-pending or inaccessible metadata. Verify the original bytes stay
   unchanged after refusal.
6. Prove directory rename is blocked while the guard is retained and works
   after all connections and guard handles close. Verify failure and shutdown
   paths release handles in the right order without leaking them.
7. Exercise writer plus reader connections, WAL writes, checkpoint and clean
   close with the guard present. Normal SQLite sidecar cleanup must work;
   reopening must retain the expected database identity and contents.
8. Inject a changed identity between validation and open, plus sidecar-check
   failures, and require an explicit error. Document the remaining race
   window instead of claiming that this test proves an adversarial guarantee.

## Runtime findings

The first full gate passed formatting, dependency boundaries and Clippy.
All 16 native guard tests and six of seven integration tests passed. The
long-path integration test exposed a real SQLite open failure (code 14):
passing a normal DOS path beyond 260 UTF-16 units was insufficient on this
machine. The separate store suite passed all 13 tests.

The correction keeps ordinary DOS spelling when the main file and longest
`-journal` sidecar fit the native legacy limit, including the terminating
NUL. Longer paths use validated extended DOS spelling. All three connections
select SQLite's built-in locking `win32-longpath` VFS, its file-access layer,
which also supports paths exceeding the default VFS's 1040-byte buffer.
Neither a custom VFS nor an unlocked variant is used. Source inspection
confirmed that the MSVC build does not add the prefix automatically.
[SQLite's standard Windows VFS implementations](https://www.sqlite.org/vfs.html#standard_windows_vfses).

The long-path spelling retains SQLite 3.53.2's known Windows locking
performance caveat, detailed in `docs/dependency-audit.md`. Registry or
manifest changes and short-name aliases were rejected: they cannot guarantee
support under the owner's unchanged Windows settings.

The final `cargo xtask check` passed all 77 tests, formatting, Clippy,
dependency boundaries and console launch. The guard contributes 18 unit
tests and nine native/SQLite integration tests. These exercise real WinRT
enumeration, main/sidecar hardlinks, destination/ancestor junctions, retained
directory names, WAL cleanup, reopened snapshots and three long-path cases:
the sidecar-only 252-unit boundary, 350 UTF-16 units, and 1100 units beyond
the default SQLite VFS capacity. Failure-policy tests use injected drive,
root and identity data. Independent reviews found no remaining actionable
issues after the corrections.

This is not exhaustive filesystem or performance testing. Separate physical
symbolic links, mounted-drive aliases and adversarial race schedules were
not exercised; junction tests and structural reparse/identity checks cover
the relevant refusal mechanism. The known limitations above remain. No
production database, account or browser profile was opened, and no Windows
settings were changed.
