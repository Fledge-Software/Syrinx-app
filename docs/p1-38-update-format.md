# P1-38: the signed update package format

Status: implemented locally on branch `claude/p1-38-update-verify`, awaiting
independent review and the owner's approval. This note is the reviewed
format and key-handling design that the P1-38 row of
`docs/plans/Atlas v1 — Plan.md` asks for before the verifier is built.

## Why this exists

An update replaces the executable that holds the owner's browsing session,
history and signed-in profile. If Atlas installed whatever bytes a download
produced, anyone who could alter those bytes in transit, on the update host or
on disk before installation could run their own code as the owner. Windows
code signing does not help here: Atlas is unsigned for now (D38), so nothing
on the machine vouches for a new executable. Atlas must therefore check the
update itself, before anything is installed, against a key it already trusts.

P1-38 is that check and nothing else. It has no download, network call,
installer, restart or user interface; those belong to P1-39, which needs the
owner's approval of the update host and network policy first (AGENTS.md, "Ask
before proceeding": no network call Atlas makes on its own). The verifier
lives in `crates/atlas-update` and no other crate depends on it yet;
`cargo xtask check-deps` enforces that.

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
atlas-update-manifest 1
version 1.4.0
minimum-version 1.2.0
file 3f0c…(64 lowercase hex digits) 1048576 atlas.exe
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

**Bounds.** The verifier never trusts a size it has not bounded. A manifest
is at most 2 MiB (room for the most files with the longest paths), a package
lists between 1 and 4,096 files, a file is at most 1 GiB and the listed files
total at most 4 GiB. Reading a payload file stops one byte past its declared
size, so an oversized or endless stream cannot exhaust memory or time.

### The signature

`manifest.sig` is exactly 76 bytes:

| Bytes | Content |
| --- | --- |
| 0–3 | the ASCII magic `AUS1` (Atlas update signature, format 1) |
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
second valid `(r, n − s)`. That does not matter here, because Atlas never
uses a signature as an identifier; it only asks whether the manifest was
signed.

## Order of checks

`atlas_update::verify_manifest` and `VerifiedManifest::verify_payload` run:

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
6. For the payload: refuse any file present but not listed, then read each
   listed file, refusing a missing file, a wrong size or a wrong hash.

Every refusal is a typed `UpdateError`; the verifier does not panic on any
input, and its tests feed it truncated, flipped, oversized, duplicated and
traversing inputs. Nothing in `atlas-update` writes to disk.

What rollback protection does not cover: an attacker who can serve old but
still newer packages can hold Atlas at, say, 1.2 when 1.5 exists (a "freeze"
attack). Defending against that needs a signed, expiring statement of the
latest version, which only makes sense alongside the update host. It is
listed for P1-39.

## Keys

**The verification key is pinned in the executable.** `atlas-update` holds a
compiled-in set of trusted public keys (`PRODUCTION_KEYS`) and nothing else:
it never fetches a key, accepts one from the package or reads one from the
registry or a settings file. A key delivered alongside an update would let
whoever controls the delivery choose the key.

**The production key does not exist yet.** `PRODUCTION_KEYS` is an empty
placeholder marked `TODO(DW-30)` in `crates/atlas-update/src/keys.rs`, and
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
the `windows =0.62.2` crate already pinned for `atlas-platform`. The only
change is enabling its `Win32_Security_Cryptography` feature (which brings
`Win32_Security` and `Win32_Foundation`) for `atlas-update`. Cargo features
are not recorded in `Cargo.lock`, and the feature adds no package: the
resolved graph is unchanged. The audit is in `docs/dependency-audit.md`
under P1-38.

The main alternative was a Rust implementation such as the RustCrypto `p256`
and `sha2` crates or `ring`. They would add a dozen or more new packages to
audit and, for `ring`, assembly and a build script, to do what the operating
system already provides and Microsoft already maintains. CNG ties the
verifier to Windows, which v1 already is (D2).

Because only `atlas-platform` may use `windows` directly, the dependency
check now names `atlas-update` as the one other crate allowed the `windows`
bindings (not WebView2 or wry), and refuses any workspace crate that depends
on `atlas-update` until P1-39 is approved.

## What P1-39 needs from the owner

- The update host, how and when Atlas contacts it, and what it may send.
- Install and restart behaviour, and proof that a failed download, invalid
  signature or interrupted install leaves the working version.
- The release key pair, generated by the owner outside the repository, and
  its public point and separately recorded identifier for `PRODUCTION_KEYS`
  (DW-30).
- Whether to add a signed, expiring latest-version statement against freeze
  attacks.
