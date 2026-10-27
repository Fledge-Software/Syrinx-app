# Weekly preview releases

Every Thursday at 10:00 New York time, a GitHub Actions workflow builds Syrinx,
runs the gate and, if it passes, publishes the result twice: as a GitHub
pre-release on this repository and on the website, syrinxapp.dev. The owner
asked for this on 2026-10-13 (`docs/decisions.md`) so that people following
the project get a predictable build without the owner remembering to make one.
This note explains what the workflow does, what it deliberately does not do,
and the steps only the owner can take.

## What happens on a Thursday

The workflow is `.github/workflows/weekly-release.yml`; each step's logic is a
short script in `.github/scripts/` so it can be read and tested on its own.

1. **Plan** (`plan-release.sh`, Linux). GitHub's scheduler only knows UTC, so
   the workflow is scheduled at both 14:00 and 15:00 UTC. The plan keeps the
   one that is 10:00 in New York on that day: 14:00 during daylight saving,
   15:00 otherwise. It then compares `main` with the commit the latest
   `v*` tag was built from. No new commits means no release that week. The
   version is the next minor step after the latest tag (`v0.3.0` follows
   `v0.2.0`); the first release takes the workspace version, `0.1.0`. A
   quiet week still runs the **re-sign** job described under "Signed
   updates" below.
2. **Release** (Windows). On a GitHub-hosted Windows runner it installs Rust
   1.97.1 and commits the new version into `Cargo.toml` and `Cargo.lock` with
   `set-version.sh`. That commit sits on top of the `main` commit being
   released and is reachable only through its tag, so `main` is never pushed.
   It then runs `cargo xtask check` (the full gate in `docs/dev/LOCAL-DEV.md`,
   including the smoke run) and `cargo xtask package` (P1-37's setup program
   and portable zip).
   A failing gate stops everything: nothing is tagged or published.
   Next the release job **signs the update** (P1-39d, issue #55; see
   "Signed updates" below), when the release key secret is set and the
   source pins a key. A failed signature also stops everything.
3. **Screenshots** (`capture-screenshots.ps1`). The freshly built
   `syrinx.exe` is launched four times, on brand-new empty profiles under
   `--test-data-dir` and only `https://example.com/`, a reserved example
   domain: Home, a page, the command palette and the theme sample window. A
   capture that comes out blank is dropped with a warning, and the website
   keeps its previous images.
4. **Publish.** `release-notes.sh` lists `main`'s first-parent commits since
   the previous release, which are the reviewed slice merges, then an
   "Engines" section: the GPUI, gpui-component, wry and webview2-com versions
   from `Cargo.lock` (marked when they changed since the previous release)
   and the WebView2 runtime installed on the runner. The workflow
   pushes an annotated tag `v0.N.0` and creates a GitHub pre-release with the
   setup program (`Syrinx-<version>-<commit>-x64-setup.exe`), the zip and
   `SHA256SUMS.txt`. The setup program is the download the website offers:
   people run it to install Syrinx and run a newer one to update (owner,
   2026-10-14). The zip stays for P1-39's updates and the owner-run scripts.
   A signed release also attaches `manifest`, `manifest.sig`, `latest` and
   `latest.sig`, and uploads the host's `updates/v1/` files as the
   workflow artifact `update-site`.
5. **Website.** Because this repository is public, the workflow then checks
   out `Fledge-Software/syrinx-public-site`. It runs that repository's
   `scripts/publish-release.mjs` to write the version, download link,
   checksum, notes and screenshots, builds the site to prove it still builds,
   and pushes the site's `main`. Vercel deploys that push to syrinxapp.dev
   and docs.syrinxapp.dev. The workflow checks the visibility on every run:
   on a private repository this step is skipped with a warning, because
   visitors could not download the zip.

To release outside the schedule, run the workflow from the Actions tab
("Run workflow"). Tick **force** to release even when `main` has not
changed.

## Signed updates

**Why.** An installed Syrinx installs an update only if it is signed by a
key compiled into it (`PRODUCTION_KEYS` in
`crates/syrinx-update/src/keys.rs`). That signature is what protects the
owner if the website or Vercel is broken into, because HTTPS alone only
stops someone on the network. On 2026-10-14 the owner chose to keep the
primary release key as an encrypted GitHub Actions secret, so this workflow
signs every release and re-signs the latest statement every week, and the
owner keeps no key on a device (`docs/decisions.md`). The owner approves
this workflow change before it is pushed (`AGENTS.md`, "Environment and the
owner's machine").

**How the key travels.** The secret `SYRINX_RELEASE_KEY` holds the key as
base64 PKCS#8 text, stored in the GitHub Environment `release-signing`,
which the owner limits to `main`; the release and re-sign jobs name that
environment, so no other branch or workflow can read the key.
`signing-ready.sh` first asks the source being released which keys it pins
(`cargo xtask pinned-release-keys`, whose failure fails the run). With none
pinned, the run warns and publishes an unsigned release, as every release
did before issue #55. With keys pinned, a missing secret fails the run
rather than quietly shipping an unsigned update. Then one step imports the key into the runner's
software key storage under a per-run name, not exportable, with the secret
in that step's environment only (`cargo xtask import-release-key`).
`sign-update.sh release` then signs with `cargo xtask sign-update` (serial
= Unix time, 30-day statement) and writes the host files with `cargo xtask
host-layout`, which first checks everything as an installed Syrinx would.
An always-run step deletes the key (`cargo xtask delete-release-key`),
whether signing passed, failed or was cancelled. The key never appears in
a command line, a file, a log or an artifact. The full design is in
`docs/update-delivery.md`, "How the workflow signs".

**The weekly re-sign.** A statement expires 30 days after signing, so an
attacker who freezes the update host can hide a fix for at most that long.
In a quiet week the `resign` job downloads the latest release's four
signed assets (`manifest`, `manifest.sig`, `latest`, `latest.sig`). It
**verifies them before any key exists on the runner**: `cargo xtask
verify-release` checks the manifest signature, the old statement's
signature, that the statement names this manifest, and that both are for
the tag's own version, against the keys pinned in that release's own tag,
setting aside only the old statement's expiry. An older release's validly
signed files, swapped in, are therefore refused rather than re-signed as
the latest. Only then is the key imported, and `cargo xtask resign-statement`
signs nothing but a new statement, with a higher serial, for that verified
manifest; it never re-signs a manifest, and it verifies again before
writing. The job then replaces `latest` and, after it, `latest.sig` on the
release. Its `update-site` artifact holds just those two files. The
workflow's scripts come from `main`, and the tools and pinned keys from the
tag, checked out only when the tag has signed assets: a release made before
signing began (any tag older than this change) has none, so its quiet week
is skipped with a notice instead of failing on tools the tag lacks.

**Where the files go.** The `update-site` artifact holds exactly what
belongs under `https://syrinxapp.dev/updates/v1/`. It is public, signed
data, not a secret: anyone may read it, and its safety rests on the
signatures, which only the release key can make. Like the website step,
copying it to the host waits until the repository is public; until then
the owner decides where, if anywhere, the files are published (issue #55).

## Keeping main releasable

`.github/workflows/gate.yml` runs the same `cargo xtask check` on every push
to `main` and every pull request, with a formatting check on Linux that
reports within a minute or two. It exists because run 8 of the release
failed on a rustfmt slip that had merged unnoticed. Before running a release
by hand, check that the Gate workflow is green on the commit it will build.
Both workflows run the gate through `.github/scripts/run-gate.sh`, which puts
the end of the log in the run summary. The one-at-a-time rerun and the
debugger stack run only when a test process actually crashed (a Windows
`STATUS_` code in the log), not on a formatting or Clippy failure.

## What it does not do

- **It does not code-sign anything.** Syrinx has no code-signing
  certificate (D38), so Windows SmartScreen warns before the first run, and
  the website says so. The update signature above is a different thing: it
  is checked only by an installed Syrinx, never by Windows.
- **It does not install, register or update Syrinx anywhere.** The zip is
  P1-37's portable package; installing it remains a step the person
  downloading it takes.
- **It never uses the owner's data.** Screenshots come from empty synthetic
  profiles, and release notes come from commit subjects, which already follow
  the "no personal data" rule in `AGENTS.md`.

## One-time setup by the owner

These steps need the owner's GitHub account, so no agent does them. All
four are done: the repository is public, and the website step, which needs
the `SITE_REPO_TOKEN` secret, has published releases since 2026-10-14. What
remains is renewing that token before it expires (step 2).

1. **Review the pre-publication checklist below**, then make this repository
   public: Settings → General → Danger zone → Change visibility.
2. **Create a token that can update the website repository.** In GitHub,
   Settings → Developer settings → Fine-grained personal access tokens →
   Generate new token. Set the resource owner to `Fledge-Software`, choose
   "Only select repositories" with `syrinx-public-site`, and grant
   **Contents: Read and write** and nothing else. The longest expiry is a
   year; put a reminder in your calendar to renew it.
3. **Store it as a secret.** In this repository, Settings → Secrets and
   variables → Actions → New repository secret, named `SITE_REPO_TOKEN`.
4. **Create the release keys** (issue #55) with the commands in
   `docs/owner-checklist.md`, item 1, outside any agent session. Until an
   agent has pinned both public keys, releases stay unsigned.
5. **Run it once by hand** from the Actions tab, so the first release is
   watched rather than discovered. The Windows gate takes a long time on a
   clean runner (allow over an hour). If the smoke run or a screenshot fails
   only on the runner, that is a new trap: record it in
   `docs/dev/trapstoknow.md`.

## Pre-publication checklist

The repository is public, so its whole history is published, not just the
current files. Rewriting history is the owner's decision (`AGENTS.md`), so
these were listed rather than changed. Checked on 2026-10-13, before the
repository was made public. Since then almost every commit carries a
`noreply` author address (Plan 1's ledger at af3de53 records the owner's
history rewrite of 2026-10-13), and `.idea/` and the logs under
`.claude/worktrees/` are no longer tracked; the third-party skills under
`.claude/skills/` still are.

- **Commit author email.** About 230 commits carried the owner's personal
  Gmail address as author, which GitHub would show on every public commit. To
  hide it for future commits, turn on "Keep my email addresses private" in
  GitHub's email settings and use the `noreply` address in `git config`.
  Existing commits keep it unless history is rewritten.
- **Windows user name in paths.** Some handoff notes, [`docs/next-session-prompt.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/next-session-prompt.md)
  and two tracked gate logs under `.claude/worktrees/` contained
  `C:\Users\<name>\Documents\GitHub\…` paths that reveal the Windows account
  name. The logs could simply be deleted from the tree; the history keeps them.
- **Editor settings.** `.idea/` was tracked. It held no secrets, but it is
  usually left out of public repositories.
- **Third-party agent skills.** `.claude/skills/` contains skills copied from
  elsewhere (for example `ui-ux-pro-max`). Check that each one's licence
  allows redistribution, or stop tracking them before going public.
- **Secrets.** No keys, tokens, `.env` or certificate files are tracked, and
  the P1-38 production keys do not exist yet (issue #55); when they do, the
  primary lives only in the `SYRINX_RELEASE_KEY` secret and the backup only
  in the owner's password manager. GitHub's secret scanning runs
  automatically on a public repository.
