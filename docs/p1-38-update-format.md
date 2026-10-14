# P1-38: the signed update package format

Status: implemented locally on branch `claude/p1-38-update-verify`, awaiting
independent review and the owner's approval. This note is the reviewed
format and key-handling design that the P1-38 row of
`docs/plans/Syrinx v1 — Plan.md` asks for before the verifier is built.

## Why this exists

An update replaces the executable that holds the owner's browsing session,
history and signed-in profile. If Syrinx installed whatever bytes a download
produced, anyone who could alter those bytes in transit, on the update host or
on disk before installation could run their own code as the owner. Windows
code signing does not help here: Syrinx is unsigned for now (D38), so nothing
on the machine vouches for a new executable. Syrinx must therefore check the
update itself, before anything is installed, against a key it already trusts.

P1-38 is that check and nothing else. It has no download, network call,
installer, restart or user interface; those belong to P1-39, which needs the
owner's approval of the update host and network policy first (AGENTS.md, "Ask
before proceeding": no network call Syrinx makes on its own). The verifier
lives in `crates/syrinx-update`. Until P1-39b no other crate depended on it;
since P1-39b only `syrinx-app` may, which `cargo xtask check-deps` enforces.

## What a package is

A package is three kinds of thing, however P1-39 eventually transports them
(a directory, an archive it unpacks into a staging directory, or separate
downloads):

1. `manifest`: a small text file listing the version and every file.
2. `manifest.sig`: a detached signature over the manifest's exact bytes.
3. The payload files the manifest lists.

Only the manifest is signed. Each payload file is bound to the signature
through the SHA-256 hash and exact size the manifest records for it, so one
signature covers the whole package without signing large files directly.

### The manifest

The manifest is UTF-8 text with LF line endings, in exactly this shape:

```text
syrinx-update-manifest 1
version 1.4.0
minimum-version 1.2.0
file 3f0c…(64 lowercase hex digits) 1048576 syrinx.exe
file 9a1b…(64 lowercase hex digits) 2048 resources/editor/index.html
```

- The first line names the format and its version. A later incompatible
  format changes the number, and this verifier rejects it rather than
  guessing.
- `version` is the version the package installs: `MAJOR.MINOR.PATCH`, each a
  decimal number from 0 to 4,294,967,295 without leading zeros. There are
  no pre-release or build suffixes; a test build gets its own patch number.
- `minimum-version` is the oldest installed version allowed to apply this
  package, so a release that needs an earlier migration can insist on it.
  It must not be greater than `version`.
- Each `file` line gives the file's SHA-256 hash in lowercase hex, its size
  in bytes in decimal without leading zeros, and its path in the package.

**Canonical form.** There is exactly one valid byte sequence for any given
content, and the verifier refuses everything else: single spaces between
fields, no blank lines, no trailing spaces, no carriage returns, no byte-order
mark, a final newline, the fields in the order above, and `file` lines sorted
by path in ascending byte order. After parsing, the verifier writes the
manifest back out and requires the result to equal the input byte for byte.
A canonical form means the signature, the hashes and the parser all agree on
what the manifest says; a lenient parser would let two readers of the same
signed bytes see different packages. This also avoids the CRLF trap in
`docs/trapstoknow.md`: a manifest that passed through a CRLF conversion is
rejected instead of silently hashing differently.

**Paths.** A payload path must be safe to join onto a staging directory on
Windows without escaping it or naming something other than a plain file:

- relative, with `/` separating components, never `\`;
- each component made only of ASCII letters, digits, `.`, `_` and `-`, which
  excludes drive letters, `:` (so alternate data streams like `file:stream`),
  wildcards and control characters;
- no empty component (so no leading `/`, trailing `/` or `//`), no `.` or `..`
  component, and no component ending in `.`, which Windows would strip;
- no Windows device name such as `CON`, `NUL`, `COM1` or `LPT9`, with or
  without an extension, in any letter case;
- at most 64 bytes per component, 8 components and 240 bytes in total.

Two paths that differ only in letter case are duplicates, because the
Windows file system treats them as one file; an exact duplicate is also
refused. The sorted order makes duplicates adjacent and the check exact.
For the same reason a path may not also be, ignoring letter case, a
directory of another listed path: `lib` and `LIB/x.dll`, or `a` and `a/b`,
are refused as a collision, because one name cannot be both a file and a
directory on disk.

**Bounds.** The verifier never trusts a size it has not bounded. A manifest
is at most 2 MiB (room for the most files with the longest paths), a package
lists between 1 and 4,096 files, a file is at most 1 GiB and the listed files
total at most 4 GiB. Reading a payload file stops one byte past its declared
size, so an oversized or endless stream cannot exhaust memory or time.

### The signature

`manifest.sig` is exactly 76 bytes:

| Bytes | Content |
| --- | --- |
| 0–3 | the ASCII magic `AUS1` (Syrinx update signature, format 1) |
| 4–11 | the signing key's identifier |
| 12–75 | the ECDSA P-256 signature over SHA-256 of the manifest bytes, as `r` then `s`, each 32 bytes big-endian |

ECDSA P-256 with SHA-256 is a standard elliptic-curve signature that Windows
implements in its built-in cryptography library (CNG, "Cryptography API: Next
Generation"). The 64-byte `r || s` layout is the one CNG produces and accepts.

A key's identifier is the first 8 bytes of SHA-256 over its 64-byte public
point (`X || Y`). The identifier only selects which pinned key to try; it is
not trusted. A forged identifier either names no pinned key, which is
refused, or names a key whose verification then fails.

The verifier checks the signature over the raw manifest bytes *before*
parsing them, so the parser only ever sees text the release key signed. The
parser is still strict and bounded, because a parser bug should not become a
vulnerability the moment a key is misused.

ECDSA signatures are malleable: from a valid `(r, s)` anyone can compute a
second valid `(r, n − s)`. That does not matter here, because Syrinx never
uses a signature as an identifier; it only asks whether the manifest was
signed.

## Order of checks

`syrinx_update::verify_manifest` and `VerifiedManifest::verify_payload` run:

1. Bound the manifest's size and require the signature to be 76 bytes with
   the `AUS1` magic.
2. Find the pinned key named by the signature; refuse an unknown identifier.
3. Verify the signature over SHA-256 of the manifest bytes with CNG.
4. Parse the manifest strictly and require its canonical form.
5. **Rollback protection:** refuse a package whose version is lower than *or
   equal to* the installed version, and one whose `minimum-version` is newer
   than the installed version. Refusing equal versions stops an attacker
   replaying today's package to force a reinstall over a repaired or
   reconfigured installation.
6. For the payload: refuse any entry present but not listed (in exact
   letter case, apart from the directories that hold listed files), then
   read each listed file, refusing a missing file, a wrong size or a wrong
   hash.

Every refusal is a typed `UpdateError`; the verifier does not panic on any
input, and its tests feed it truncated, flipped, oversized, duplicated and
traversing inputs. The verifier itself writes nothing to disk; P1-39b's
install module beside it does, and is described below.

## Installing exactly the verified bytes

Verification holds only for the bytes it read. If P1-39 verified a staging
folder and then copied it with a second open, a process running as the same
user could swap `syrinx.exe` in between, and the copy would install bytes
nobody checked. The API therefore makes the safe path the easy one:
`VerifiedManifest::verify_payload` takes the payload by value and returns a
`VerifiedPayload` holding, for each listed file, the very handle that was
hashed, rewound to its start. P1-39 installs by reading those handles and
must never reopen a payload file by name.

The `Payload` trait states the rules a source must keep: `names()` returns
every entry, files and directories, in its stored letter case; links,
junctions and other reparse points are refused; and `open()` returns the
plain file itself, read-only, refusing writers. `DirectoryPayload` is the
staging-directory implementation and enforces them on Windows:

- every handle is opened with read-only sharing, so while the
  `VerifiedPayload` lives no other handle can write, rename or delete a
  verified file, and opening fails if a writer already holds one;
- every directory in the staging tree is opened the same way and held for
  the payload's lifetime, so a verified directory cannot be renamed and
  replaced by a junction between listing and reading;
- entries are opened with `FILE_FLAG_OPEN_REPARSE_POINT` and both the
  listing and each opened handle refuse the reparse-point attribute, so a
  symbolic link or junction (for example a `resources` junction pointing
  outside the staging folder) is refused rather than followed;
- names are compared with the manifest in exact case, so `Syrinx.exe` on disk
  does not stand in for a listed `syrinx.exe`.

The tests build staging folders of synthetic files under the temporary
directory, check that the held handles return the listed bytes and block a
writer and a rename, and refuse a wrong-case name, an extra directory and a
junction created with `mklink /J`. P1-39 should still stage into a folder
only Syrinx can write, as a second layer.

Three limits remain, and P1-39 must design around them:

- **Held directory handles do not stop new entries.** They prevent renaming
  or deleting a verified directory, but another process can still create a
  new file inside it after the listing. P1-39 must therefore never copy,
  list or enumerate the staging folder again after verification; it
  installs only from the `VerifiedFile` handles.
- **Nothing above the staging root is pinned.** `DirectoryPayload::open`
  resolves the root by path, and each later open joins names onto that
  path, so a junction in a folder above the root could redirect it before
  the root handle is taken. P1-39 must place the staging root inside a
  folder only Syrinx can write, or open entries relative to the held
  directory handle instead of by path.
- **Only `DirectoryPayload` enforces the `Payload` rules.** Any other
  implementation is trusted to keep them, including returning the same
  bytes when its handle is rewound and read again. A new source needs its
  own review and tests before P1-39 relies on it.

Releasing a CNG handle cannot report a failure from `Drop`, so
`syrinx-update` writes such a failure to standard error. It only happens if a
handle Syrinx created is already invalid, which would be a bug. P1-39 should
route these messages to Syrinx's own logging once it connects the verifier.

What rollback protection does not cover: an attacker who can serve old but
still newer packages can hold Syrinx at, say, 1.2 when 1.5 exists (a "freeze"
attack). Defending against that needs a signed, expiring statement of the
latest version, which only makes sense alongside the update host. P1-39a
adds it; see "P1-39a: the latest-version statement" below.

## Keys

**The verification key is pinned in the executable.** `syrinx-update` holds a
compiled-in set of trusted public keys (`PRODUCTION_KEYS`) and nothing else:
it never fetches a key, accepts one from the package or reads one from the
registry or a settings file. A key delivered alongside an update would let
whoever controls the delivery choose the key.

**The production key does not exist yet.** `PRODUCTION_KEYS` is an empty
placeholder marked `TODO(DW-30)` in `crates/syrinx-update/src/keys.rs`, and
the register row DW-30 in the plan tracks it. Because the set is empty,
every package fails with an unknown-key error: the verifier fails closed
until the owner creates a key. No real key, public or private, is in the
repository.

**The private key never touches the repository or test runs.** The owner
generates the release key pair on a machine or hardware token they control,
outside this repository and outside any agent session, and keeps the private
half there. Only the 64-byte public point is copied into `keys.rs`, and that
copy is checked independently: its identifier is recomputed and compared
with one the owner records separately from the repository. The tests never
read a key from disk; each one asks CNG for a fresh in-memory key pair that
disappears when the test ends.

**Rotation.** The pinned set can hold more than one key, which is how a key
is replaced without stranding installed copies:

1. The owner creates the new key B.
2. A release signed with the old key A ships an executable whose pinned set
   is {A, B}.
3. Later releases are signed with B.
4. Once no supported installation predates step 2, a release drops A from the
   set.

If key A is lost, installations that only know A can no longer be updated
automatically; the owner reinstalls by hand. If key A is stolen, the
attacker can sign updates for every installation that trusts A until a
release removes it, so step 2 should happen promptly and the stolen key's
identifier should be refused by every later executable. A Windows
code-signing certificate (D38's later step) would add a second, independent
check, not replace this one.

## Crypto approach and dependencies

Hashing and signature verification call Windows CNG (`bcrypt.dll`) through
the `windows =0.62.2` crate already pinned for `syrinx-platform`. The only
change is enabling its `Win32_Security_Cryptography` feature (which brings
`Win32_Security` and `Win32_Foundation`) for `syrinx-update`. Cargo features
are not recorded in `Cargo.lock`, and the feature adds no package: the
resolved graph is unchanged. The audit is in `docs/dependency-audit.md`
under P1-38.

The main alternative was a Rust implementation such as the RustCrypto `p256`
and `sha2` crates or `ring`. They would add a dozen or more new packages to
audit and, for `ring`, assembly and a build script, to do what the operating
system already provides and Microsoft already maintains. CNG ties the
verifier to Windows, which v1 already is (D2).

Because only `syrinx-platform` may use `windows` directly, the dependency
check now names `syrinx-update` as the one other crate allowed the `windows`
bindings (not WebView2 or wry), and refuses any workspace crate that depends
on `syrinx-update` until P1-39 is approved. It also lists the exact
`windows` features the verifier may enable, `Win32_Foundation` and
`Win32_Security_Cryptography`, with default features off, so adding a
networking namespace such as `Win32_Networking_WinHttp` fails
`cargo xtask check-deps`.

## P1-39a: the latest-version statement

The owner approved P1-39's update delivery on 2026-10-13
(`docs/decisions.md`; the design is
`docs/proposals/p1-39-update-delivery.md`). P1-39a is its first slice. It
adds no network call, install step or interface; it adds the signed file
that tells Syrinx what the newest release is, the code that checks it, and
the release tool that writes it.

### Why a statement is needed

Rollback protection refuses a package that is not newer than the installed
one, but it cannot tell Syrinx that something newer exists. Someone who
controls the host or the connection could keep serving last month's genuine
package, or nothing at all, and Syrinx would believe it was current (the
freeze attack described above). The statement closes that gap: the owner
signs "the newest version is X, its manifest hashes to H, and this expires
on D", and Syrinx refuses to act on a statement that has expired. A frozen
host can then hold Syrinx back for at most the statement's lifetime, which
the owner capped at 60 days.

### The format

`latest` is UTF-8 text with LF line endings, at most 4 KiB, in exactly this
shape:

```text
syrinx-update-latest 1
serial 12
version 1.4.0
manifest-sha256 9a1b…(64 lowercase hex digits)
issued 2026-11-02T10:00:00Z
expires 2027-01-01T10:00:00Z
```

- The first line names the format and its version, and differs from the
  manifest's first line, so neither file parses as the other.
- `serial` is a decimal number from 0 to 2^64 − 1 without leading zeros. The
  owner raises it with every statement they sign; Syrinx keeps the highest
  one it has accepted (P1-39b stores it) and refuses a lower one.
- `version` is the newest released version, in the manifest's
  `MAJOR.MINOR.PATCH` form.
- `manifest-sha256` is SHA-256 of that version's exact `manifest` bytes, in
  lowercase hex.
- `issued` and `expires` are UTC instants spelled exactly
  `YYYY-MM-DDTHH:MM:SSZ`: four-digit year from 1970 to 9999, a real calendar
  date, upper-case `T` and `Z`, no fractions and no offsets. `expires` must
  be later than `issued` and at most 60 days (5,184,000 seconds) after it.

The canonical-form rule is the manifest's: single spaces, no blank lines, no
carriage returns, no byte-order mark, one final newline, the fields in this
order, and nothing after `expires`. After parsing, the verifier writes the
statement back out and requires the result to equal the input byte for
byte.

`latest.sig` uses the 76-byte layout of `manifest.sig` with a different
magic, `ALS1` (Syrinx latest statement, format 1), in place of `AUS1`. The
signature is over SHA-256 of the statement's exact bytes, with the same
ECDSA P-256 key set, exactly as P1-38 signs a manifest. In v1 one key signs
both; a separate statement-only key was deferred (proposal, "The release
key").

The magic is not what keeps the two kinds of file apart. It sits outside
the signed digest, so anyone can relabel an `AUS1` signature as `ALS1`; it
only catches a misplaced file early, with a clear error. The separation
rests on the signed bytes themselves: a statement's first line must be
exactly `syrinx-update-latest 1` and a manifest's exactly
`syrinx-update-manifest 1`, each parser requires its own header, and both
require the byte-exact canonical reparse. A manifest signature relabelled
as a statement signature therefore verifies over bytes the statement
parser refuses, and the reverse. Any future change must keep both header
checks exact and the canonical reparse in place: loosening either would
let one signed file be read as the other.

### How a statement is accepted

`syrinx_update::verify_statement(statement, signature, keys, now,
previous)` runs, in order:

1. Bound the statement to 4 KiB and require a 76-byte signature with the
   `ALS1` magic.
2. Find the pinned key the signature names, refusing an unknown identifier,
   and verify the signature with CNG. Any pinned key is accepted, which is
   how the backup key keeps installed copies updatable if the primary is
   lost (DW-30).
3. Only then parse the statement strictly and require its canonical form,
   including the 60-day cap. A statement signed with a longer lifetime is
   still refused.
4. **Freshness:** refuse it if `expires` is at or before `now`, the local
   clock. Expiry is exclusive: at the stated second it has already expired.
   Also refuse it if `issued` is more than one day (86,400 seconds) after
   `now`: a slow clock still accepts a fresh statement, but a statement
   newer than the local clock allows is refused. This does not stop an old
   statement on a clock set back into that statement's window. With no
   `previous` record (a fresh install, or lost state), a genuine old
   statement then passes the freshness check; only the serial rule below
   stops it, and P1-38's rollback check still refuses a manifest that is
   not newer than the installed version. Freeze resistance therefore
   assumes a roughly correct clock and a kept serial record. A clock
   outside 1970 to 9999 is an error, not a guess.
5. **Replay:** given the last accepted statement (`AcceptedStatement`: its
   serial and the SHA-256 of its bytes), refuse a lower serial, and refuse
   an equal serial whose bytes differ.

The result, `VerifiedStatement`, then checks the manifest it names through
`VerifiedStatement::verify_manifest`: the manifest's SHA-256 must equal
`manifest-sha256` (checked before the manifest's own signature), P1-38's
`verify_manifest` must accept it with its rollback and minimum-version
rules, and the manifest's `version` must equal the statement's. So a
statement cannot be paired with a manifest for another version, and a
genuine manifest cannot be paired with a statement that names a different
one.

`verify_statement` does not decide whether the statement names the
installed version ("Syrinx is up to date"); P1-39b compares
`statement().version()` with the installed version before downloading
anything.

### Writing a release: `cargo xtask package` and `cargo xtask sign-update`

`cargo xtask package` now also writes P1-38's `manifest` into the package
folder, beside `syrinx-package.json`. The package's `Syrinx-*.zip` is built
at the same time, before any signing, so it holds the unsigned `manifest`
but no `manifest.sig`, `latest` or `latest.sig`. It lists every program file the
install script copies (`syrinx.exe`, `syrinx-package.json` and the five
scripts), with `minimum-version 0.0.0` because no release yet needs an
earlier migration. The manifest is unsigned. Writing it needs the
verifier's own parser, and `cargo xtask check-deps` still forbids any crate,
xtask included, from linking `syrinx-update` until P1-39b. So xtask runs a
small release tool inside the update crate,
`crates/syrinx-update/src/bin/syrinx-update-tool.rs`, through
`cargo run --package syrinx-update --bin syrinx-update-tool`. The tool
refuses to replace an existing `manifest`.

`cargo xtask sign-update --package <folder> --storage <software|smart-card>
--key <name> --serial <n> --days <1-60>` signs that folder's `manifest` and
a fresh statement with the owner's stored key, and writes `manifest.sig`,
`latest.sig` and then `latest` beside it, each through a temporary file
renamed into place, so a failure part-way never leaves a new `latest` next
to an old signature. It opens the key by its name in
Windows' software key storage or smart-card key storage through NCrypt
(Windows' key-storage interface) and asks Windows to sign; it reads only the
public half, never the private key. The statement is issued at the current
time and expires `--days` later. The serial must be higher than one already
in the folder's `latest`. Before writing, the tool verifies both signatures
as an installed copy would, against `PRODUCTION_KEYS`, so it refuses a key
that is not pinned. Because `PRODUCTION_KEYS` is empty until DW-30, the
command refuses every key today; that is intended, as it fails closed until
the owner's keys exist.

The key-storage signer and `sign_release` sit behind the update crate's
non-default `release-tool` feature, which only the release tool requires,
so Syrinx itself never links code that opens Windows key storage. The gate
builds and tests that feature (`--features syrinx-update/release-tool`), so
the tool stays covered by Clippy and the tests. The signing logic is tested
only with in-memory CNG keys created fresh by each test. No test opens Windows key storage, and the
repository holds no key.

### What remains for P1-39b to P1-39d

Both P1-39b items below are done; see "P1-39b" further on.

- P1-39b stores the accepted serial, stages a package and installs it.
  The package folder `cargo xtask package` writes holds `manifest`,
  `manifest.sig`, `latest` and `latest.sig` next to the payload files, and
  `DirectoryPayload` refuses unlisted entries. P1-39b's "Install update from
  file…" must therefore read the payload from a folder holding only the
  listed files, as the host's `files/` layout does.
- P1-39b must also move the gate's `release-tool` coverage out of the
  workspace steps. Today `check_workspace` (`crates/xtask/src/main.rs`)
  passes `--features syrinx-update/release-tool` to the workspace Clippy
  and test steps so the signing tool stays covered; that is harmless while
  no crate depends on `syrinx-update`. Once `syrinx-app` does, Cargo builds
  one copy of `syrinx-update` with the feature on for the whole workspace,
  so the gate would lint and test `syrinx-app` with the key-storage signer
  linked in, unlike the shipped build and the smoke run, and nothing would
  enforce that Syrinx never links it. In the slice that lifts the verifier
  quarantine for `syrinx-app`, make the workspace steps featureless again,
  add separate `cargo clippy -p syrinx-update --all-targets --features
  release-tool -- -D warnings` and `cargo test -p syrinx-update --features
  release-tool` steps, and add a `check-deps` rule in
  `crates/xtask/src/dependency_policy.rs`, with a test, refusing any
  dependency on `syrinx-update` that enables `release-tool`.
- P1-39c fetches `latest`, `latest.sig` and the package from
  `https://syrinxapp.dev/updates/v1/`.
- P1-39d is the owner's key ceremony, the pinning of both public keys
  (DW-30) and the first real `sign-update`.

## P1-39b: staging, "Install update from file…", the helper and rollback

P1-39b is the second slice of the approved delivery design
(`docs/proposals/p1-39-update-delivery.md`). It makes a verified package
installable without any network: the owner picks a package folder, Syrinx
checks and stages it beside the installed program, and a small helper swaps
it in when Syrinx closes, keeping the previous version and putting it back if
the new one does not start. Without this slice the verifier protects
nothing, because no update passes through it, and every fix during the
owner's default-browser week (P1-40) would mean rerunning
`install-syrinx.ps1` by hand. It adds no network call, no WinHTTP and no
key: `PRODUCTION_KEYS` is still empty (DW-30), so today every real install
from a file is refused as signed by an unknown key, which is the intended
fail-closed behaviour until P1-39d.

### Where the code lives

The staging, swap and recovery code sits in `crates/syrinx-update/src/install/`
beside the verifier, and the helper is the crate's second executable,
`src/bin/syrinx-apply.rs`. Keeping them in the verifier's crate means the
helper needs no new crate and no dependency (a new crate depending on
`syrinx-update` would also break `check-deps` change 1, which allows only
`syrinx-app`), and the recovery tests can sign synthetic packages with the
in-memory CNG keys that exist only in that crate's test build. The crate
still requests only the `windows` features `Win32_Foundation` and
`Win32_Security_Cryptography`; everything else is `std`. `syrinx-app` adds
the Customize section, the palette command and the handover
(`src/updates.rs`, `src/ui/updates.rs`), `syrinx-platform` adds a folder
picker and a free-space query (`file_dialog::choose_folder`,
`disk::free_bytes`), and `syrinx-store` adds the pre-update snapshot.

### The folders

`UpdateLayout` names every folder an update touches, and only those. In an
installed copy the program folder is `%LOCALAPPDATA%\Programs\Syrinx` and
the data folder is `%LOCALAPPDATA%\Syrinx\data` (P1-37):

```text
Programs\Syrinx\                 the installed version; renamed, never written into
Programs\Syrinx.new\             the verified copy waiting to replace it
Programs\Syrinx.previous\        the version an update replaced, kept for rollback
data\Updates\incoming\<version>\ the package's listed files, copied for verification
data\Updates\apply\              syrinx-apply.exe, manifest and manifest.sig
data\Updates\pending             written last: version, from-version, manifest hash
data\Updates\state               the accepted statement serial, its hash and expiry,
                                 and the last result as a fixed category
data\Updates\outcome             what the helper did, shown once by the next start
data\Updates\started             written by a relaunched Syrinx after a clean start
data\Updates\apply.lock          held by whichever process changes update files
data\Updates\apply.log           the helper's standard error: categories, no paths
```

Only an `installed` build running from a folder named `Syrinx` has a layout
(`UpdateLayout::for_running_program`); a development build, the gate and the
smoke run never install anything, so nothing beside `target\local` can be
renamed. The data folder may not sit inside a folder an update renames, nor
the program folders inside the data folder. `Updates` is a sibling of the
store's `State` folder, so P1-03's storage lease, which holds the data
folder as an ancestor of `State`, keeps the folder above `Updates` from
being swapped for a junction while Syrinx runs (P1-38's limit (b)). Syrinx
creates `Updates`, `incoming` and `apply` itself and refuses any of them that
is a link. A folder Syrinx would remove (`Syrinx.new`, `Syrinx.previous`) is
removed only when it is empty or holds P1-37's `syrinx-package.json`, the
same recognition rule the install scripts use; anything else stops the step
and is left alone.

### Staging: "Install update from file…"

The command is in Customize's new Updates section ("Install from file…")
and in the palette ("Install update from file…"). The owner picks the
package folder that `cargo xtask package` and `cargo xtask sign-update`
wrote. On its own thread, `stage_from_folder`:

1. takes the update lock, and reads `state`; an unreadable `state` refuses
   staging rather than forgetting the accepted serial;
2. verifies `latest` and `latest.sig` with `verify_statement` against the
   pinned keys, the clock and the stored serial; a statement naming the
   running version records its serial and reports "up to date";
3. verifies `manifest` and `manifest.sig` through the statement, so the
   manifest's hash and version must be the stated ones, then P1-38's
   rollback and minimum-version rules;
4. refuses a package that does not list `syrinx.exe`, `syrinx-package.json`
   and `syrinx-apply.exe`, and one the disks lack room for: twice the
   package plus 256 MiB, on both the data and program volumes;
5. only now writes: it removes an earlier staged copy (`pending` first),
   copies each listed file, and nothing else, from the chosen folder into a
   fresh `incoming\<version>` (refusing one longer or shorter than listed),
   and runs `verify_payload` on a `DirectoryPayload` of that folder;
6. copies from the verified handles, never by name, into `Syrinx.new`
   (`syrinx-package.json` first, so an interrupted copy is still
   recognisable) and the helper into `Updates\apply`, with the exact
   manifest and signature bytes that were checked, flushing each file;
7. removes `incoming`, stores the accepted serial in `state`, and writes
   `pending` last.

Because only listed files are copied into `incoming`, the extra files in a
package folder (`manifest`, the signatures and the statement) never reach
the payload check, which is how the P1-39a note's "a folder holding only
the listed files" is met without a separate `files/` layout. The serial is
stored only once a statement is acted on (up to date, or staged), before
`pending`: a crash between the two only means staging repeats with the same
statement, which an equal serial with identical bytes allows.

Any refusal removes what staging wrote and leaves the installed version
untouched. The owner sees one plain sentence (for example "This update isn't
signed by a key this Syrinx trusts, so nothing was installed."), and the
diagnostic line names only the fixed category and the verifier's reason.

### Restart now, Later, and the pre-update snapshot

Once staged, Customize shows "Syrinx 1.4.0 is ready" with **Restart now**
and **Later**. Nothing is replaced until one of them leads to a clean exit:

- **Restart now** asks the session worker for D13's extra snapshot
  (`SessionCommand::SnapshotBeforeUpdate`, `Store::backup_before_update`),
  then closes exactly as an ordinary quit does.
- **Later** arms the install for the next quit; that close first takes the
  same snapshot.

The snapshot is an ordinary validated `VACUUM INTO` copy taken outside the
daily schedule, and D13's rotation still keeps seven, so the oldest copy may
rotate out. If the snapshot fails, nothing installs: Restart now stays open
and says why; a quit closes without installing and the update stays staged.
After a successful snapshot, `main` waits for the window, the store and the
page host to close and P1-34's single-instance claim to be released, then
`prepare_handoff` takes the update lock, rechecks `Updates\apply\syrinx-apply.exe`
against the kept, signed manifest and holds it open so nothing can replace it
before Windows starts it, and starts the helper. Syrinx keeps the lock until
its process ends.

### The helper's protocol

`syrinx-apply.exe --programs <folder> --data <folder> --mode <restart|quit>
[--relaunch-data-dir <folder>]` trusts only `KeySet::production()`:

1. It waits up to 60 s for the update lock, which Syrinx holds until its
   process has ended; this is how it learns the old process is gone without
   any Windows process API, which the verifier's feature list does not allow.
2. It reads `pending`, requires the kept manifest's SHA-256 to match it,
   reverifies the manifest (rollback rule against the from-version) and
   runs `verify_payload` on `Syrinx.new`. A failure removes the staged copy,
   records `verification_failed` and leaves the installed version alone.
3. It removes an older `Syrinx.previous` (only if Syrinx made it), renames
   `Syrinx` to `Syrinx.previous` and `Syrinx.new` to `Syrinx`, retrying for
   up to 30 s while a just-closed program still holds the folder. If the
   second rename fails, it renames the first back and records `apply_failed`.
4. **Quit (Later):** it records `installed` and leaves an outcome, so the
   next start says "Syrinx was updated to 1.4.0." It starts nothing.
5. **Restart now:** it starts `Programs\Syrinx\syrinx.exe --after-update`
   and waits up to 60 s for `Updates\started` to name the new version. The
   relaunched Syrinx skips the launch activation (`cx.activate(true)`), so it
   never forces itself to the front, and writes `started` once its session
   has loaded and the window has drawn it. On success the helper records
   `installed`.

**The rollback trigger** is the new version not writing `started` within
60 s, exiting first, or failing to launch at all. The helper then stops it,
removes `pending` (so a crash part-way never offers the failed version
again), renames the failed `Syrinx` to `Syrinx.new` and `Syrinx.previous`
back to `Syrinx`, records `apply_rolled_back` with an outcome, releases the
lock and starts the previous version, which tells the owner "The update to
1.4.0 didn't start; you are on 1.3.0." The failed copy is removed then, or
by the next start if Windows still holds it.

### What a start does

Before the window opens, `recover` takes the lock (and does nothing if the
helper still holds it), removes any `incoming`, reads and removes the
outcome, and settles `pending`: a `pending` naming the running version means
the swap finished but its records were not cleared, so it is cleared and
recorded as `installed`; a `pending` staged by another version, or with no
`Syrinx.new`, is discarded; and a `Syrinx.new` with no `pending` is an
unfinished copy and is removed. A complete staged copy is kept and shown as
ready again.

### Recovery tests

`crates/syrinx-update/src/install/tests.rs` and `tests_apply.rs` build
synthetic programs, packages and data folders in fresh folders under the
workspace's `target\syrinx-update-tests`, sign them with in-memory CNG keys,
and remove only their own folder. Fault hooks exist only in the test build
(`install/fault.rs`): each named step can stop staging or the helper as a
crash would, and the test then runs the next start and checks that a working
program folder remains. They cover: a successful stage; production keys
refusing every package; up to date; a bad statement or manifest signature,
tampered manifest bytes, a wrong hash, a truncated, oversized or missing
file, an unknown key, an expired statement, a lower serial, a reused serial
with other bytes, a missing statement, a package lacking the helper, a full
disk and an unreadable `state`, each leaving the installed version untouched
and `incoming` gone; staging stopped at each of its six steps; the helper's
lock wait; a `Syrinx.previous` Syrinx did not make; reverification refusing
a changed file, an added file and a changed kept manifest; installing on
quit; Restart now with a clean start; rollback for a hung start, an
immediate exit and a failed launch; the helper stopped before the swap,
after it, after launching and part-way through a rollback; and the handoff
refusing a replaced helper while blocking writes to the checked one. A
further test proves Windows still starts an executable held open the way
the handoff holds the helper.

One case cannot be automated, as the proposal says: a stop between the two
renames leaves no `Programs\Syrinx` until the owner renames
`Syrinx.previous` back (P1-37's checklist). The test for it performs that
rename and shows the next start then finds the update still staged.

### `check-deps` and the gate

`cargo xtask check-deps` now allows `syrinx-app`, and only `syrinx-app`, to
depend on `syrinx-update` (change 1; P1-39c adds `syrinx-update-net` in its
own slice), and refuses any dependency on `syrinx-update`, of any kind, that
enables `release-tool`. As the P1-39a note required, the gate's workspace
Clippy and test steps are featureless again, so the app is linted and tested
as it ships, and two separate steps cover the release tool:
`cargo clippy --package syrinx-update --features release-tool --all-targets`
and `cargo test --package syrinx-update --features release-tool`. Tests in
both `dependency_policy.rs` and `main.rs` pin these rules.

### The package

`cargo xtask package` now also builds `syrinx-apply.exe` (without the
release tool's feature) into the same separate target directory and adds it
to the fixed layout, so the update manifest lists it and
`install-syrinx.ps1` installs it; `packaging/syrinx-registration.ps1` was
regenerated for the new file list. The only lockfile change is
`syrinx-app`'s new edge to the workspace's own `syrinx-update`.

### Limits

- **Malware running as the owner** can already write to the program and data
  folders; the handle discipline narrows the windows but cannot close them
  (the proposal's "honest limit"). The helper drops its verified handles
  before renaming, because Windows cannot rename a folder whose files are
  open.
- **Rollback covers Restart now only.** With Later, the helper does not
  start Syrinx, so nothing watches the next start; if it fails, the owner
  renames `Syrinx.previous` back by hand. This follows the proposal's step 4;
  whether the next start should also be watched is a question for the owner.
- **The foreground behaviour of a relaunched Syrinx** (question 6) is
  implemented as the proposal specifies and was checked only with a
  development build started with `--after-update` from an agent's shell
  while another application was in front. Its window still appeared above
  that application, so a screenshot cannot tell whether it took activation;
  this does not settle question 6. A real update needs the owner's keys
  (P1-39d), and the owner's check then is the evidence.

## What P1-39 needs from the owner

- The update host, how and when Syrinx contacts it, and what it may send.
- Install and restart behaviour, and proof that a failed download, invalid
  signature or interrupted install leaves the working version.
- The release key pair, generated by the owner outside the repository, and
  its public point and separately recorded identifier for `PRODUCTION_KEYS`
  (DW-30).

The owner answered the first two and the statement question on 2026-10-13
(`docs/decisions.md`); the keys remain theirs to create in P1-39d.
