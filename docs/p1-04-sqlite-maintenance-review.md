# P1-04: SQLite maintenance proposal

Date: 2026-10-10. Status: **owner approved; applied and local gate passed**.

P1-04 must inspect damaged databases and validate backups before offering
recovery. The SQLite 3.53.2 source bundled by the original registry dependency has
upstream fixes in those paths. This proposal moves that bundled C library to
3.53.4 while keeping the audited Rust wrapper, features and dependency graph.
It is a prerequisite maintenance proposal within P1-04, not a completed
backup implementation or a new implementation slice.

The owner explicitly approved this pinned patch and its vendor-comment
exception on 2026-10-10. The applied source inventory and hashes are in
`vendor/libsqlite3-sys-0.38.2.provenance.md`; the validation record is below.
The P1-03 77-test gate covers its earlier 3.53.2 build.
See `docs/dependency-audit.md` for that historical audit and
`docs/handoff-p1-03-2026-10-10.md` for the completed storage slice.

## Why the current source needs attention

The [SQLite maintenance timeline](https://sqlite.org/src/timeline?from=version-3.53.0&to=version-3.53.4&to2=branch-3.53&y=ci)
records these specific fixes after 3.53.2:

| Fixed in | Check-in | Defect and trigger relevant to review |
| --- | --- | --- |
| 3.53.3 | `aa756c8038` | A super-journal name could overrun a page-sized buffer when the file-access layer permits longer paths. Journal processing and a sufficiently long embedded name are required. |
| 3.53.3 | `9a79ca31ff` | Corrupt page-size handling with read-only WAL/shared-memory files. |
| 3.53.3 | `6826c17021` | Missing detection of index cells extending beyond their page. |
| 3.53.3 | `1a888ea60b` | Improved corrupt freelist detection and integrity-check heap assertions. |
| 3.53.3 | `9471fa2c9c` | A crafted hot journal could direct super-journal cleanup at an unrelated file. |
| 3.53.4 | `bf70dadc2d` | Crash-damaged super-journal records with zeroed name/checksum could prevent required rollback. |
| 3.53.3 | `246f46614f` | Extended DOS paths were mistaken for network paths when choosing shared-memory locks. |

These are conditional defects, not evidence of damage to an Syrinx database.
Syrinx's ordinary single-database WAL transactions do not normally create
multi-database super-journals. P1-04 nevertheless handles damaged input by
design, and a read-only connection does not eliminate every affected path.

The cached 3.53.2 `sqlite3/sqlite3.c` confirms the first issue in
`pager_playback`: it passes `Pager.pTmpSpace` to `readSuperJournal` using the
VFS pathname limit. A VFS is SQLite's file-access implementation. Syrinx uses
the standard locking `win32-longpath` VFS, whose limit is 65,534 bytes; the
ordinary database page size is 4,096 bytes. This establishes a relevant
buffer-size relationship, not proof that Syrinx has exercised the full
trigger. The P1-03 filesystem guard checks file identities and locations,
not journal contents. Keep long-path support; changing the VFS to evade
this review would break an already tested requirement.

## Exact base and replacement

Keep these workspace declarations and their current features:

```toml
rusqlite = { version = "=0.40.2", default-features = false, features = ["bundled", "cache"] }
rusqlite_migration = { version = "=2.6.0", default-features = false }
```

The base is the already audited published **libsqlite3-sys 0.38.2** package.
Its registry archive SHA-256, recorded in `Cargo.lock`, is:

```text
f1d20bef17f513b9b3004532233187769cd072d790971f4e4da0e346eb6401e8
```

Its packaged `.cargo_vcs_info.json` identifies upstream base commit
`e88f112bef7899234a497baed5cc3c3d553deeb8`, path `libsqlite3-sys`.
Use the published package as the baseline, because its normalized manifest
is the version actually resolved and audited by Syrinx.

Replace exactly these four files with their complete contents from
[rusqlite commit `901f9946efdaaa289e6b1c5bd56dc67f4b651e51`](https://github.com/rusqlite/rusqlite/commit/901f9946efdaaa289e6b1c5bd56dc67f4b651e51):

| Path inside libsqlite3-sys | SHA-256 of cached 0.38.2 baseline bytes |
| --- | --- |
| `sqlite3/sqlite3.c` | `0a409f1633283fa31a9126b11fbfd64a1991c5d30defad07e5745d4667f5e23d` |
| `sqlite3/sqlite3.h` | `9e69a1353a4288450b0d5239ede11fc7f1f4c8e5eb07491fc8317eacb5b7de7e` |
| `sqlite3/bindgen_bundled_version.rs` | `5b5a37a74dc728fb0bcd041867eb577c03c95e2fe46285262c589a3b1cd4bbac` |
| `sqlite3/bindgen_bundled_version_ext.rs` | `445cc206e04a87131d4690742517ee7dcc3a430319bac847ce5e9586f98ee301` |

The C file combines SQLite's implementation into one compilation unit,
called an amalgamation. The header and generated Rust bindings must identify
the same version. The upstream commit also changes `upgrade.sh`; do not
import or run that script. Take the complete four files, not just the
3.53.3-to-3.53.4 patch shown by that commit, so the intervening 3.53.3 fixes
are included.

The [official 3.53.4 release record](https://www.sqlite.org/releaselog/3_53_4.html)
supplies these independent identity values:

```text
SQLITE_VERSION: 3.53.4
SQLITE_SOURCE_ID:
2026-07-24 19:02:57 bf7c7f30031888f4e796e429ab3978879485813aaca6f641c7b33e4e09459bcc
sqlite3.c SHA3-256:
67f423e9ebbbdc473cbc4772c872ee6b89f31fde4ed0279a5c25d5f65c043a16
```

The baseline hashes above were measured locally without modifying the cache.
At proposal time, the replacement C hash was an upstream published value
and individual replacement SHA-256 values were unverified. After approval,
the exact-revision files were staged locally, their version/source identities
and the C SHA3-256 verified, and all replacement SHA-256 values recorded in
`vendor/libsqlite3-sys-0.38.2.provenance.md`. No mismatch was found.

## Why 3.53.4 rather than 3.54.0

On the review date, published [rusqlite](https://docs.rs/crate/rusqlite/latest)
and [libsqlite3-sys](https://docs.rs/crate/libsqlite3-sys/latest) remain
0.40.2 and 0.38.2, respectively; their bundled SQLite is 3.53.2.
[SQLite 3.54.0](https://www.sqlite.org/changes.html) was released on October 9.
The recommended 3.53.4 maintenance release includes the identified fixes and
already has matching bindings imported by rusqlite upstream. Choosing it
keeps this change focused on the existing SQLite release line. Moving to
3.54.0 would introduce a larger set of API, query-planner and Windows backend
changes without a corresponding published Rust binding update established
by this review.

A direct Cargo Git override to the import commit is not the proposed route.
Its [manifest](https://raw.githubusercontent.com/rusqlite/rusqlite/901f9946efdaaa289e6b1c5bd56dc67f4b651e51/libsqlite3-sys/Cargo.toml)
still declares 0.38.1, while Syrinx's published rusqlite 0.40.2 requires
libsqlite3-sys `^0.38.2` on Windows. Retain the audited 0.38.2 manifest and
wrapper rather than changing rusqlite or mixing in upstream master changes.
Waiting for a published package containing the fixes is the alternative if
the owner declines local vendoring; later slices must not skip past P1-04.

## Licence, security and maintenance review

The Rust wrapper and its generated bindings remain under the package's MIT
licence. SQLite's deliverable C source and header are
[public domain](https://www.sqlite.org/copyright.html), as in the already
approved bundled choice; they must not be relabelled MIT. Preserve the base
package's licence notices and record this distinction in the vendor
provenance file. This proposal introduces no new licence category. The
existing narrow Unicode-3.0 and Zlib permissions remain scoped to their
previously approved packages and versions.

The upstream import and SQLite maintenance releases demonstrate active
maintenance. The [RustSec libsqlite3-sys index](https://rustsec.org/packages/libsqlite3-sys.html)
currently lists [RUSTSEC-2022-0090](https://rustsec.org/advisories/RUSTSEC-2022-0090.html),
whose patched range starts at 0.25.1. Both the existing wrapper and this
proposed replacement are beyond that issue's affected SQLite version. That
registry advisory does not describe all later SQLite defects; the upstream
source review above is necessary in addition to it.

The package's audited `build.rs` remains unchanged: it selects bundled
bindings and compiles the C source through the existing `cc` dependency.
No bindgen run, system SQLite installation, build-time download, new Cargo
feature or environment override is part of this proposal. Compatibility of
the replacement bindings with this exact wrapper was then checked by the
local gate recorded below; the source review alone did not establish it.

## Proposed repository and lockfile change

After approval, create a deliberately trimmed copy of the published package
under `vendor/libsqlite3-sys-0.38.2/`. The four replacements above are the
semantic source change, not the entire vendor directory. Copy these
additional files unchanged from the audited 0.38.2 package:

| Purpose | Unchanged baseline files |
| --- | --- |
| Package metadata and notices | `Cargo.toml`, `Cargo.toml.orig`, `README.md`, `LICENSE` |
| Rust wrapper and build | `build.rs`, `src/lib.rs`, `src/error.rs` |
| Small supporting headers and bindings | `wrapper.h`, `wrapper_ext.h`, `bindgen-bindings/bindgen_3.34.1.rs`, `bindgen-bindings/bindgen_3.34.1_ext.rs`, `sqlite3/sqlite3ext.h` |
| Existing build-script watched input | `sqlite3/wasm32-wasi-vfs.c` |

This gives 17 upstream files in the proposed vendor directory, plus a
separate Syrinx provenance document. The WASI source is not compiled on
Windows; keeping it avoids inventing a missing path for the unchanged build
script's unconditional change-watcher declaration. The small alternate
bindings and headers preserve the published wrapper layout without enabling
their optional features.

Explicitly omit the entire dormant `sqlcipher/` subtree, the package's
standalone `Cargo.lock`, `Upgrade.md`, `.gitignore`, and Cargo-cache metadata
such as `.cargo-ok` and `.cargo_vcs_info.json`. Record these omissions and the
original archive/VCS identity in provenance. Do not copy the second large
SQLCipher amalgamation merely to reproduce inactive package contents. The
trimmed package supports only Syrinx's audited Windows bundled feature set;
the unchanged manifest still declares optional features, but this proposal
does not claim to support or authorize enabling SQLCipher or other paths.
Inventory every copied/omitted file rather than silently treating this as a
complete unmodified registry package.

Add a root-manifest `[patch.crates-io]` entry pointing `libsqlite3-sys` to
that directory, retaining package version 0.38.2. Explicitly exclude the
vendor directory from workspace membership. Cargo otherwise includes local
path dependencies automatically; the dependency must not become an Syrinx
product crate. These are the documented
[patch](https://doc.rust-lang.org/cargo/reference/overriding-dependencies.html#the-patch-section)
and [workspace exclusion](https://doc.rust-lang.org/cargo/reference/workspaces.html#the-members-and-exclude-fields)
mechanisms.

Use a `.gitattributes` rule restricted to that vendor directory to preserve
upstream bytes (`-text`), because this checkout otherwise normalizes text
line endings. Store the source identities, four baseline/replacement hashes,
licence notices and removal condition in a short provenance document outside
the unchanged upstream source. Do not reformat generated bindings or C code.

Resolve only the changed libsqlite3-sys source, offline after staging. The
expected lockfile difference is that libsqlite3-sys 0.38.2 becomes the local
path package instead of a registry package; its registry source/checksum
fields disappear. All existing package/version pairs and active dependency
edges must remain unchanged. Any broader resolution change requires review
before proceeding. No GPUI, wry, WebView2, Windows binding or migration pin
changes are included.

### Narrow upstream-source policy exception

`AGENTS.md` prohibits `TODO` and `FIXME` markers in the codebase. The existing
cached SQLite source contains both, and an unmodified upstream import can
retain them. Approval must explicitly allow those existing upstream comments
inside this one vendor directory. Syrinx-authored files remain subject to the
rule, and new Syrinx work is recorded in the plan rather than vendor comments.

`crates/xtask/src/main.rs` currently contains no marker-scanning step, so no
gate check needs removal. Keep the existing product formatting, Clippy,
tests and dependency-policy checks intact. The workspace exclusion preserves
the dependency's status as outside code; it does not permit lowering Syrinx
lint levels or hiding test failures. Any future marker scan should apply the
same precise vendor boundary, not a repository-wide exception.

## Verification required before P1-04 proceeds

1. Verify the original package archive identity and the complete copied-file
   inventory. Confirm the 13 retained baseline files are unchanged, the four
   replacements match the pinned import, and all package omissions match the
   list above. Record replacement hashes and verify the C source against
   SQLite's published SHA3-256.
2. Confirm the lockfile/source change and active Windows feature graph are
   exactly the intended patch. Recheck build environment overrides listed in
   `docs/dependency-audit.md`. Confirm workspace metadata excludes the vendor
   crate and still includes every Syrinx crate.
3. Add a meaningful runtime version/source-ID assertion using the linked
   SQLite engine, so selecting an unpatched registry or system build cannot
   silently pass. Review the relevant fixed C paths and retain the existing
   long-path tests, including the sidecar boundary and path exceeding the
   ordinary VFS buffer. No Windows setting or security change is permitted.
4. Run `cargo xtask check` on this machine with D38's optimized local profile.
   Independently review the vendor diff, manifest, lockfile and runtime
   evidence. The existing 77 passing tests are a baseline to preserve, not
   the complete P1-04 acceptance suite.
5. Implement and test P1-04 against disposable synthetic databases only:
   completed and interrupted backup creation, validation failures, crash
   restart, durable promotion/rotation, and refused recovery without damage
   to the source. Do not run crafted native-corruption reproducers in the
   unpatched engine as a prerequisite. Use bounded malformed inputs against
   the fixed build, and preserve failures rather than ignoring tests.

`VACUUM INTO` already supports the chosen backup method. With source
`synchronous=NORMAL` or `FULL`, successful completion flushes the output;
interruption or power loss before completion can leave an unusable output.
A later application-controlled rename still needs its own durability
design. These are [SQLite's documented guarantees](https://www.sqlite.org/lang_vacuum.html),
not results of a new Syrinx backup test.

Cancellation is cooperative and can race with successful completion. Use a
dedicated operation connection/interrupt handle, wait for the result, and
close it before cleanup or promotion. The
[interrupt contract](https://www.sqlite.org/c3ref/interrupt.html) does not
provide a fixed deadline. Candidate validation needs full `integrity_check`,
`foreign_key_check`, exact Syrinx schema/version checks and snapshot
invariants: [quick_check alone omits index correspondence and UNIQUE checks](https://www.sqlite.org/pragma.html#pragma_quick_check).
Reject unexpected candidate sidecars. Preserve the live database and its
WAL together; [the WAL can contain committed state](https://www.sqlite.org/wal.html#the_wal_file).
Read-only validation is not equivalent to `query_only`, and
[immutable mode](https://www.sqlite.org/uri.html#uriimmutable) must not be used
as a shortcut to suppress locking or damaged-input handling.

## Applied verification record

On 2026-10-10, the complete `cargo xtask check` passed with the patched
engine: dependency boundaries, formatting, Clippy with warnings denied,
all 78 tests, documentation-test selection and the console application
launch. This preserves the P1-03 suite and adds
`crates/syrinx-store/tests/sqlite_engine.rs`, which queries the linked C
library's actual version and source ID. Both match the approved 3.53.4
source. The existing long-path and sidecar-boundary integration tests passed.

The import checks verified all 17 files, the 13 retained archive inputs,
the four replacement identities and hashes, the C source's published
SHA3-256, unchanged Rust package versions/features and dependency edges,
and the scoped Git byte-preservation rule. The only lockfile change replaces
libsqlite3-sys's registry source with its local path. No system override,
upstream script or new feature was used.

These checks verify the maintenance prerequisite. They do not establish
that P1-04's backup creation, rotation, lifecycle marker, deletion journal
or recovery protocol is implemented. Those remain the next work, described
in `docs/p1-04-recovery-preflight.md`.

Independent reviews found no actionable issue in the source inventory,
checksum verification, wrapper/build compatibility or linked-engine test.
The focused C review confirmed the bounded journal-name allocation and
cleanup, damaged-page checks and corrected extended-DOS-path classification.
This does not claim an exhaustive audit of SQLite or coverage of every
possible damaged input.

## Approval and exit

The owner approved this bounded source patch, local vendoring and its narrow
upstream-comment exception before compiled source changes began. This is
not permission to commit, merge, push, change Windows
settings or use an integration app. The usual independent review and owner
commit/merge approvals still apply.

Once an audited published libsqlite3-sys release contains these fixes and
fits the approved wrapper, propose replacing the path patch with that exact
release. Remove the vendor source and its scoped exceptions through the
owner-approved maintenance change after equivalent tests pass. Until then,
the provenance document must identify the actual bundled engine as 3.53.4
even though the wrapper package remains 0.38.2. Do not claim that 3.53.4
eliminates every possible damaged-input defect or quietly change the source
behind the documented hashes.
