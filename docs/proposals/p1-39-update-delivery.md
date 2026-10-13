---
status: approved by the owner, 2026-10-13
slice: P1-39 (plan 1, Phase 5), split into P1-39a to P1-39d below
decisions: D38, D39, D9, D13; the 2026-10-12 foreground rule; DW-30
date: 2026-10-13
---

# P1-39: how Syrinx gets its updates

## Why this proposal exists

P1-38 built the check that decides whether an update may be trusted: a
signed manifest, a pinned public key and a payload whose every byte is
hashed (`docs/p1-38-update-format.md`, code in `crates/syrinx-update`).
Nothing calls it yet. P1-39's row in the plan's Issue map
(`docs/plans/Syrinx v1 — Plan.md`) says what is missing: "Delivery needs
explicit network and restart behavior. Connect the verifier to the
owner-approved host/check/install policy; prove failed download, invalid
signature and interrupted install preserve the working version." It is
blocked on "owner approval of host, network calls and install/restart
behavior". This document asks for that approval. It writes no code.

Three rules make the approval necessary rather than a formality. `AGENTS.md`
forbids "any network call Syrinx makes on its own" without the owner's yes,
and today Syrinx makes none: `cargo xtask check-deps`
(`crates/xtask/src/dependency_policy.rs`) refuses every network crate and
every Windows networking namespace in Syrinx's crates, citing D9. Keys never
go into the repository, and the release key does not exist yet (DW-30).
And the owner decides where Syrinx is published, while the repository is
private and the licence undecided.

**What breaks without P1-39.** P1-40, the owner's week with Syrinx as the
default browser, is blocked on it. Without it every fix during that week
means rebuilding with `cargo xtask package` and rerunning
`install-syrinx.ps1` by hand (`docs/p1-37-installer-design.md`, "Upgrade"),
and P1-38's verifier protects nothing because no update passes through it.

## Terms used here

- **Package**: P1-38's three parts: `manifest` (version, minimum version,
  and each file's path, size and SHA-256), `manifest.sig` (a 76-byte
  ECDSA P-256 signature over the manifest) and the payload files.
- **Release key**: the owner's private signing key. Its public half is
  pinned in `PRODUCTION_KEYS` (`crates/syrinx-update/src/keys.rs`), which
  is empty today, so every package is refused (it fails closed).
- **Latest statement**: a new, small signed file saying "the newest
  version is X, its manifest hashes to H, and this statement expires on D".
- **Freeze attack**: someone who controls the download path keeps serving
  an old but genuine update, so Syrinx believes it is current while a fix
  exists. P1-38's rollback check cannot see this, because the old package
  is validly signed (`docs/p1-38-update-format.md`, last paragraph before
  "Keys").
- **Staging**: downloading and verifying a package in Syrinx's own folder,
  away from the running program, before anything is replaced.

## What any design must satisfy

1. Syrinx makes no network call unless the owner has just asked for one.
2. A failed or tampered download, a bad signature, a full disk, a power cut
   or a crash at any step leaves a working Syrinx and the owner's data
   untouched (P1-39's row).
3. Install only bytes the verifier read, through the handles it returns,
   never by reopening a file by name (P1-38, "Installing exactly the
   verified bytes"), and design around P1-38's three stated limits.
4. No key, token or secret in the repository or in Syrinx.
5. Syrinx never takes the foreground from another application (owner
   decision, 2026-10-12, `docs/decisions.md`).
6. Nothing changes Windows settings, the registry or SmartScreen. Syrinx
   stays unsigned (D38), and the per-user layout of P1-37 stays as it is.
7. Tests use synthetic packages and in-process keys only.

## When Syrinx checks: only when the owner asks

**Recommended: a "Check for updates" button in Settings and the same
command in the palette, and nothing else.** One press makes one sequence
of HTTPS requests to one fixed host and then stops. Settings shows the day
of the last check and, with no network call, a quiet line such as "Last
checked 47 days ago" once that exceeds a threshold. No popup, timer or
wake-up is added.

Alternatives considered and rejected:

- **Check at every start-up.** This is a network call Syrinx makes on its
  own, on every launch. The host's access log would also hold a record of
  each time the owner started their browser, from their IP address: the
  kind of detailed timeline D39 keeps out of Syrinx's own analytics.
- **A periodic background check** (daily, say). It has the same timeline
  problem, and it adds a timer to an app whose idle cost D37 measures in
  fractions of a percent.
- **Background download through BITS**, Windows' Background Intelligent
  Transfer Service. It keeps transferring after Syrinx exits and resumes
  after reboot. That is a system service acting for Syrinx beyond its
  lifetime, which is the opposite of point 1.
- **Downloading through a WebView2 page.** That would put Syrinx's own
  traffic through the browser engine that holds the owner's cookies, and
  it touches a page web view's behaviour, which `AGENTS.md` reserves.

A later automatic check would be its own decision for the owner, not a
setting switched on by default.

## Where updates come from

Two hosts were considered. Both serve files over HTTPS, and neither is
trusted for integrity: the signature decides, so a compromised host can
withhold updates but cannot install code.

**GitHub Releases of `Fledge-Software/Syrinx-app`.** Releases are free and
already where the code lives. But the repository is private (owner,
2026-10-13), so downloading a release asset needs an authenticated request.
Syrinx would have to carry a GitHub token, and any token that can read
release assets of a private repository can also read its source. That
breaks "keys never go into the repository" in spirit, because the token
would ship inside every copy. Public releases would need a public
repository, which waits on the licence. GitHub also answers a download with
a redirect to a second host, which Syrinx would have to follow.

**Static files on `syrinxapp.dev`, which the owner holds** (decision
2026-10-12). Syrinx would fetch fixed paths from one host, follow no
redirects, send no credentials and accept nothing but HTTPS. The whole
`.dev` domain is on browsers' HTTPS-only preload list, so plain HTTP is
not even an option. Which static host serves the files is the owner's
choice, and only the owner sets it up. The cost is that the binaries become
publicly downloadable from the owner's domain. Publishing builds is a
publication decision even with no licence chosen, so this proposal asks
rather than assumes (question 3).

**Also recommended: "Install update from file…".** The owner picks a
package folder they built or copied, and Syrinx runs the same verification,
staging and install with no network at all. It works before any host
exists, and it remains the fallback if the host is down.

**Recommendation:** `https://syrinxapp.dev/updates/v1/`, compiled into
the executable as one constant. No setting can redirect it, for the same
reason P1-37 compiled in the data folder. The layout:

```text
updates/v1/latest                    the latest statement
updates/v1/latest.sig                its signature
updates/v1/1.4.0/manifest            P1-38's manifest
updates/v1/1.4.0/manifest.sig
updates/v1/1.4.0/files/syrinx.exe    one file per manifest entry
```

Files are served one by one rather than as a zip, so Syrinx needs no
archive parser, which would be new attack surface and probably a new
dependency. Each download URL is built from the fixed base, the version and
a manifest path that has already passed P1-38's path rules. No URL is ever
taken from a downloaded file.

**What the host learns.** An HTTPS request reveals the owner's IP address,
the time and the paths requested. Syrinx sends no cookies, no query
string, no identifier and no version number. The required agent string is
the constant `Syrinx-Update`. Settings states the host's name next to the
button.

**How the request is made.** Recommended: WinHTTP, Windows' built-in HTTP
client, through the `windows =0.62.2` crate already pinned. TLS then uses
Windows' own certificate validation, and the lockfile gains no package.
The rejected alternative is `reqwest` with `rustls`. Those packages are
already in `Cargo.lock`, because GPUI pulls in `gpui-pre-reqwest`, `hyper`,
`hyper-rustls`, `rustls` and `tokio`, so the cost is not lockfile growth.
It is audit surface and an async runtime inside a Syrinx crate. Today
`check-deps` keeps all of that out of Syrinx's own crates, and using it
would make a TLS stack and `tokio` part of Syrinx's reviewed code for one
feature.

The dependency check (`crates/xtask/src/dependency_policy.rs`) needs three
narrow changes, not one:

1. **Lift the verifier quarantine.** Today any crate depending on
   `syrinx-update` is refused with "nothing may use the update verifier
   before P1-39 is approved". P1-39b needs `syrinx-app` to depend on it,
   and P1-39c needs `syrinx-update-net` to as well. Only those two would be
   allowed.
2. **Let `syrinx-update-net` use `windows`.** Add it to
   `may_use_platform_library`, which allows only `syrinx-platform` and the
   verifier today. Give it its own exact feature allowlist, like the
   verifier's `Win32_Foundation` and `Win32_Security_Cryptography`.
3. **Make the one D9 exception.** Allow `Win32_Networking_WinHttp` in that
   crate alone, with no other networking namespace or network crate, and
   no Syrinx crate other than `syrinx-app` allowed to depend on it.

Whether WinHTTP's automatic proxy
detection behaves like the browser's on this machine is unverified, and
P1-39c must test it.

## The latest statement, against freeze and rollback

P1-38 refuses a package older than or equal to the installed one. It cannot
tell Syrinx that a newer one exists. The statement does:

```text
syrinx-update-latest 1
serial 12
version 1.4.0
manifest-sha256 9a1b…(64 lowercase hex digits)
issued 2026-11-02T10:00:00Z
expires 2027-01-01T10:00:00Z
```

It follows the manifest's rules: canonical text that is reparsed and must
match byte for byte, a hard size limit (4 KiB), and a signature checked
before parsing. The signature uses a distinct magic (`ALS1`), and the first
line differs from a manifest's, so a manifest signature can never pass as
a statement signature or the other way round.

Syrinx accepts a statement only when all of these hold:

1. Its signature verifies against a pinned key.
2. Its `serial` is at least the highest serial Syrinx has accepted before.
   An equal serial must have identical bytes. This stops an old statement
   being replayed.
3. `expires` is in the future, and `issued` is no more than a day ahead of
   the local clock.
4. `expires` is at most 60 days after `issued` (question 4).
5. The manifest it then downloads hashes to `manifest-sha256`.

If the statement has expired, Syrinx tells the owner "Couldn't confirm the
latest version: the update information has expired" and offers nothing.
Browsing is never affected. An attacker can therefore freeze Syrinx for at
most the expiry window, and only by also controlling the host or the
connection. The owner pays for this by re-signing the statement at every
release and at least every few weeks between releases. That is one short
command with the key (see "The release key").

## Download bounds and staging

All update files live in `%LOCALAPPDATA%\Syrinx\data\Updates\`, inside the
installed data folder the owner chose on 2026-10-12
(`docs/p1-37-installer-design.md`, line 142). That folder is local, not
roaming (D13). The 2026-10-12 naming entry in `docs/decisions.md` (line 62)
still names `%LOCALAPPDATA%\Atlas\data`. P1-37's later decision and the
`DATA_FOLDER` constant in `crates/syrinx-app/src/main.rs` both use
`Syrinx\data`, and this proposal follows them. The bounds:

| What | Limit |
| --- | --- |
| Statement, its signature | 4 KiB; exactly 76 bytes |
| Manifest | 2 MiB (P1-38's limit) |
| Each payload file | its manifest size exactly. Reading stops one byte past it |
| Whole package | P1-38's 4 GiB ceiling. Free space of twice the package plus 256 MiB is checked first |
| Stalls | 30 s to connect, 60 s with no bytes received, then abandon |
| Scheme, host, redirects | HTTPS to the one host. Any redirect is refused |

The steps:

1. Fetch and accept the statement. Stop if it names the installed version
   ("Syrinx is up to date").
2. Ask the owner: "Syrinx 1.4.0 is available (38 MB). Download and verify
   it?" Nothing larger than the statement is fetched without that yes.
3. Download `manifest` and `manifest.sig` and run P1-38's
   `verify_manifest`, which applies its rollback and minimum-version rules.
4. Download each listed file into `Updates\incoming\1.4.0\`, under its
   checked path, then run `verify_payload` on a `DirectoryPayload` of that
   folder.
5. While the verified handles are held, copy from them (never by name)
   into `%LOCALAPPDATA%\Programs\Syrinx.new\`, the sibling folder that
   P1-37's upgrade already uses. Then flush to disk, delete `incoming`,
   and only then write a small `Updates\pending` record holding the version
   and manifest hash. A copy without that record is unfinished and is
   deleted on the next start.

**P1-38's limits, answered.** (a) *Held directory handles don't stop new
entries*: after verification nothing lists or copies the staging folder
again; step 5 reads only the verified handles. (b) *Nothing above the
staging root is pinned*: the root sits inside the data folder, which P1-03's
storage lease already guards against synced folders and redirection
(`docs/storage-path-preflight.md`). P1-39b must confirm that the lease
covers the `Updates` subfolder, or else open entries relative to held
directory handles. (c) *Only `DirectoryPayload` enforces the rules*: P1-39
adds no other `Payload` source, and downloads land as plain files that
`DirectoryPayload` reads.

**The honest limit.** Any program running as the owner can already write
to `%LOCALAPPDATA%\Programs\Syrinx` and to the data folder. The update path
defends against the network, the host and corrupted downloads, not against
malware already running as the owner. The handle discipline above narrows
the window but does not close it, and no per-user design can.

## Installing on restart, and keeping the working version

After step 5 Settings shows "Syrinx 1.4.0 is ready" with **Restart now**
and **Later**. Nothing is replaced until one of them leads to a clean exit:

- **Restart now** saves the session as an ordinary quit does, then exits.
- **Later** applies the update the next time the owner quits Syrinx, and
  does not relaunch it; the next start runs 1.4.0.

A running program cannot rename the folder it runs from, so a small helper
does the swap: `syrinx-apply.exe`, listed in the manifest like any file,
and copied from its verified handle into `Updates\apply\`. Before Syrinx
exits it takes one extra D13 snapshot, so the database can be recovered if
the new version's migration goes wrong. Then:

1. The helper waits for the old process to end and for P1-34's
   single-instance mutex to be free (`docs/p1-34-35-instance-runtime.md`).
2. It reverifies `Syrinx.new` with P1-38's verifier against the copied,
   signed manifest. If that fails, it deletes `Syrinx.new` and leaves the
   current version alone.
3. It renames `Syrinx` to `Syrinx.previous`, replacing the older one, and
   then `Syrinx.new` to `Syrinx`. If the second rename fails, it renames
   the first back. This is the same order as `install-syrinx.ps1`.
4. On Restart now, it starts `Programs\Syrinx\syrinx.exe` and waits up to
   60 s for the new Syrinx to record that it started cleanly (session
   restored, first frame drawn). If that never happens, it puts
   `Syrinx.previous` back, starts the old version, and the old version
   shows "The update to 1.4.0 didn't start; you are on 1.3.0".

Every step keeps a working copy on disk. One case cannot be fully automated:
a power cut between the two renames leaves `Programs\Syrinx` missing, so the
Start menu shortcut finds nothing. At the next launch the helper's record
lets Syrinx finish the swap, but the launch itself needs the program. The
owner's checklist will therefore say what P1-37's step 11 already says:
rename `Syrinx.previous` back. Mending this automatically would need a
run-once registry entry, which this proposal does not ask for.

An older Syrinx may refuse a database that a newer one has migrated
(`docs/p1-37-installer-design.md`, "Upgrade"). The pre-update snapshot is
the route back, through P1-04's existing restore offer. Rolling back to
`Syrinx.previous` remains a manual owner action, because P1-38 refuses to
install an older version by design.

**Foreground.** The relaunched Syrinx does not force itself to the front.
It skips the launch activation (`cx.activate(true)` in
`crates/syrinx-app/src/main.rs`) when started by the helper. We expect Windows to show
it normally if the owner is still in Syrinx's context and to leave it
behind if they have moved to another app. That behaviour is unverified, and
P1-39b must check it on this machine (question 6).

## The release key, at a high level

Only the owner does this, on their own machine, outside this repository
and outside any agent session. No agent sees, handles, generates or is told
where the private key is.

1. The owner creates two P-256 keys: a **primary**, kept on a hardware
   security key that supports P-256 through Windows' smart-card key storage,
   or else as a non-exportable key in Windows' software key storage behind
   a strong password; and a **backup**, created the same way and stored
   offline somewhere separate.
2. The owner exports the two 64-byte public points and records each key's
   identifier (P1-38: the first 8 bytes of SHA-256 over the point) somewhere
   outside the repository.
3. An agent pins both public points in `PRODUCTION_KEYS` (closing DW-30), and
   a reviewer recomputes the identifiers and checks them against the
   owner's separate record.
4. To release, the owner runs a reviewed signing command,
   `cargo xtask sign-update`, which names the key by its storage name and
   asks Windows to sign. The command never reads key bytes. It signs the
   manifest and then a fresh statement with the next serial.

Pinning the backup from day one means a lost primary does not strand
installed copies. P1-38's rotation procedure still applies if a key is
stolen. One key signs both packages and statements in v1. A separate,
less-guarded statement key would let the owner re-sign statements without
the primary, and a stolen statement key could only freeze, not install.
That separation is rejected for now because it needs key roles that
P1-38's key set does not have, and a second ceremony; it can come later.

## What is logged

- **`Updates\state`**, a small local file holding the highest accepted
  serial and its expiry, the day of the last check, the last result as a
  fixed category (`up_to_date`, `available`, `staged`, `statement_expired`,
  `network_failed`, `verification_failed`, `apply_rolled_back`, and so
  on) and any staged version. It is not part of D13's backups and is kept
  on uninstall like the rest of the data folder.
- **Diagnostics** use the same fixed categories plus P1-38's `UpdateError`
  variant names, in Syrinx's existing diagnostic output. Today that is
  standard error. P1-38 asks that its CNG release failures be routed there
  too. The host name and version numbers may appear. Headers, IP addresses
  and the user's paths do not.
- **No usage counters.** AN-01's version 1 schema
  (`docs/proposals/an-01-02-local-analytics.md`) has no update fields.
  Adding any would be a schema change for the owner.

## Testing with synthetic packages only

Every test generates its packages and keys at run time. Keys are in-memory
CNG keys, as in P1-38. Installs go into temporary folders standing in for
`Programs` and the data folder, passed in as parameters, so no test can
touch the owner's real install, data or WebView2 profile.

- **Download logic** runs against an in-process fake transport. The tests
  cover truncated, oversized and endless bodies, stalls, a redirect, a
  wrong hash, a bad signature, an unknown key, an expired statement, a
  lower serial, an equal serial with different bytes, a statement whose
  manifest hash does not match, and a full disk. Each must end with the
  installed version untouched and `incoming` cleaned up.
- **The WinHTTP adapter** runs against loopback servers. It must refuse
  `http://`, refuse an untrusted certificate (using P1-14's
  standard-library Python fixture with a fresh self-signed certificate,
  never an installed one), refuse a redirect and stop at the size bounds. A
  real TLS download succeeds only in the owner's own check against the real
  host.
- **Apply and recovery** stop the helper at every step, by fault hooks that
  exist only in test copies (as in the wry patch rule in `AGENTS.md`), and
  assert that a following start leaves a working program folder. Cases: a
  failed reverification, a new version that never reports a clean start,
  and an unfinished `Syrinx.new` with no `pending` record.
- **Policy**: `check-deps` proves that only `syrinx-update-net` has the
  WinHTTP feature, and a test proves that the base URL is a constant with
  no override outside `cfg(test)`.

Native fixtures are serialised with the gate, per the "Do not build an
example while its executable is running" trap.

## Proposed slices

These replace the single P1-39 row, as an amendment that renumbers nothing.

| ID | Delivers | Network | Blocked by | Then blocks |
| --- | --- | --- | --- | --- |
| P1-39a | The statement format and its verifier in `syrinx-update`. `cargo xtask package` emits P1-38's `manifest`; today it writes only `syrinx-package.json` (`crates/xtask/src/packaging/manifest.rs`). `cargo xtask sign-update` is tested with in-memory keys | None | P1-38, this approval | P1-39b |
| P1-39b | Staging, "Install update from file…", `syrinx-apply.exe`, the pre-update snapshot, restart and rollback, with the recovery tests. `check-deps` lifts the verifier quarantine for `syrinx-app` only (change 1) | None | P1-39a | P1-39c |
| P1-39c | `syrinx-update-net` (WinHTTP). Its `check-deps` changes: it may depend on the verifier (change 1), it may use `windows` with its own feature allowlist (change 2), and it alone may use `Win32_Networking_WinHttp` (change 3). Also the Settings button and palette command, and the confirmation and ready messages | Owner-pressed check only | P1-39b, owner's host approval | P1-39d |
| P1-39d | Owner gates: the key ceremony and pinning (DW-30), host set-up, and the first real update run by the owner. Recorded in a P1-39 note | Owner's own | P1-39c | P1-40, AN-01 |

P1-39a and P1-39b deliver real value even if the owner declines a host:
signed, verified updates from a file, with rollback. AN-01 stays after
P1-39, as the owner approved on 2026-10-12. If the owner prefers, P1-40 can
wait only for P1-39b, with P1-39c following.

## Questions for the owner

Each is a yes or no. The recommendation is in brackets.

1. Should Syrinx check for updates only when you press "Check for updates"
   (in Settings or the palette), never at start-up or in the background?
   *(Recommended: yes.)*
2. Should updates come from fixed HTTPS files on `syrinxapp.dev`, compiled
   into Syrinx, rather than GitHub Releases of the private repository?
   *(Recommended: yes.)*
3. Are you willing to publish signed Syrinx builds at that address before
   the licence is chosen? If not, P1-39c waits and you update from files.
   *(Recommended: yes, once P1-39b is done; your call.)*
4. Should Syrinx require a signed latest-version statement that expires
   after at most 60 days, which you re-sign at each release and between
   releases? *(Recommended: yes.)*
5. Should an update be downloaded only after you confirm its version and
   size, and installed only through Restart now or your next quit?
   *(Recommended: yes.)*
6. Should Syrinx, after relaunching for an update, not force itself to the
   front if you have moved to another app? *(Recommended: yes.)*
7. Will you create two release keys (a primary on a hardware key, a backup
   stored offline), pin both, and keep them away from every agent session?
   *(Recommended: yes.)*
8. Do you approve these three dependency-check changes? (a) `syrinx-app`
   and `syrinx-update-net` may depend on the update verifier; (b)
   `syrinx-update-net` may use the `windows` crate, with its own exact
   feature list; (c) that crate alone may use WinHTTP, the only exception
   to D9's no-network rule. *(Recommended: yes.)*
9. Do you approve splitting P1-39 into P1-39a to P1-39d as above, with no
   other renumbering? *(Recommended: yes.)*
