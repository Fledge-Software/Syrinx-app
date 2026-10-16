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
   `v0.2.0`); the first release takes the workspace version, `0.1.0`.
2. **Release** (Windows). On a GitHub-hosted Windows runner it installs Rust
   1.97.1 and commits the new version into `Cargo.toml` and `Cargo.lock` with
   `set-version.sh`. That commit sits on top of the `main` commit being
   released and is reachable only through its tag, so `main` is never pushed.
   It then runs `cargo xtask check` (the full gate in `docs/dev/LOCAL-DEV.md`,
   including the smoke run) and `cargo xtask package` (P1-37's setup program
   and portable zip).
   A failing gate stops everything: nothing is tagged or published.
3. **Screenshots** (`capture-screenshots.ps1`). The freshly built
   `syrinx.exe` is launched four times, on brand-new empty profiles under
   `--test-data-dir` and only `https://example.com/`, a reserved example
   domain: Home, a page, the command palette and the theme sample window. A
   capture that comes out blank is dropped with a warning, and the website
   keeps its previous images.
4. **Publish.** `release-notes.sh` lists `main`'s first-parent commits since
   the previous release, which are the reviewed slice merges. The workflow
   pushes an annotated tag `v0.N.0` and creates a GitHub pre-release with the
   setup program (`Syrinx-<version>-<commit>-x64-setup.exe`), the zip and
   `SHA256SUMS.txt`. The setup program is the download the website offers:
   people run it to install Syrinx and run a newer one to update (owner,
   2026-10-14). The zip stays for P1-39's updates and the owner-run scripts.
5. **Website.** If this repository is public, the workflow checks out
   `Fledge-Software/syrinx-public-site`. It runs that repository's
   `scripts/publish-release.mjs` to write the version, download link,
   checksum, notes and screenshots, builds the site to prove it still builds,
   and pushes the site's `main`. Vercel deploys that push to syrinxapp.dev
   and docs.syrinxapp.dev. While this repository is private, this step is
   skipped with a warning, because visitors could not download the zip.

To release outside the schedule, run the workflow from the Actions tab
("Run workflow"). Tick **force** to release even when `main` has not
changed.

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

- **It does not sign anything.** Syrinx has no code-signing certificate
  (D38), so Windows SmartScreen warns before the first run, and the website
  says so. The P1-38 update packages and their signing key (DW-30) are a
  separate piece of work; these zips are not update packages.
- **It does not install, register or update Syrinx anywhere.** The zip is
  P1-37's portable package; installing it remains a step the person
  downloading it takes.
- **It never uses the owner's data.** Screenshots come from empty synthetic
  profiles, and release notes come from commit subjects, which already follow
  the "no personal data" rule in `AGENTS.md`.

## One-time setup by the owner

These steps need the owner's GitHub account, so no agent does them.

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
4. **Run it once by hand** from the Actions tab, so the first release is
   watched rather than discovered. The Windows gate takes a long time on a
   clean runner (allow over an hour). If the smoke run or a screenshot fails
   only on the runner, that is a new trap: record it in
   `docs/dev/trapstoknow.md`.

## Pre-publication checklist

Making the repository public publishes its whole history, not just the
current files. Rewriting history is the owner's decision (`AGENTS.md`), so
these are listed rather than changed. Checked on 2026-10-13:

- **Commit author email.** About 230 commits carry the owner's personal
  Gmail address as author. GitHub shows it on every commit once public. To
  hide it for future commits, turn on "Keep my email addresses private" in
  GitHub's email settings and use the `noreply` address in `git config`.
  Existing commits keep it unless history is rewritten.
- **Windows user name in paths.** Some handoff notes, [`docs/next-session-prompt.md`](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/next-session-prompt.md)
  and two tracked gate logs under `.claude/worktrees/` contain
  `C:\Users\<name>\Documents\GitHub\…` paths that reveal the Windows account
  name. The logs can simply be deleted from the tree; the history keeps them.
- **Editor settings.** `.idea/` is tracked. It holds no secrets, but it is
  usually left out of public repositories.
- **Third-party agent skills.** `.claude/skills/` contains skills copied from
  elsewhere (for example `ui-ux-pro-max`). Check that each one's licence
  allows redistribution, or stop tracking them before going public.
- **Secrets.** No keys, tokens, `.env` or certificate files are tracked, and
  the P1-38 production key does not exist yet (DW-30). GitHub's secret
  scanning runs automatically once the repository is public.
