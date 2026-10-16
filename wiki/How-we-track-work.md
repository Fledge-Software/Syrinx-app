# How we track work

Until 2026-10-16 Syrinx's outstanding work lived in Markdown: plan files with
slice tables, "Open threads" and a "Deferred work" register, plus hand-off
notes between sessions. Keeping the same task in several files meant they
drifted apart, and a new contributor had to read thousands of lines to learn
what was next. On 2026-10-16 the owner moved all of it to GitHub issues.
This page says how that works.

## Issues are the only place for planned work

**Every piece of planned or outstanding work is a
[GitHub issue](https://github.com/Fledge-Software/Syrinx-app/issues), and
nowhere else** (owner, 2026-10-16). That covers features, slices of a plan,
bugs, deferred work and questions waiting on the owner. A plan, a note in
`docs/` or a code comment may explain *why* something is built the way it
is, but it never holds a to-do list. If you find work that needs doing, open
an issue for it.

## Epics and sub-issues

A large piece of work is an **epic**: an issue titled `Epic: …` that explains
what the whole thing is, why it comes where it does, the decisions that
shape it and what "done" looks like. Each piece of the epic is a
**sub-issue** attached to it with GitHub's sub-issue feature, so the epic
shows its progress. For example,
[#25 Epic: Plugin runtime — WASM host and plugin template](https://github.com/Fledge-Software/Syrinx-app/issues/25)
has #26 to #31 as sub-issues.

Syrinx uses **no milestones**. The order of work is stated in each epic's
body and in "Blocked by #n" lines, which is enough for one owner and avoids
a second structure that has to be kept in step.

## Labels, kept simple on purpose

The label set is deliberately small:

- **GitHub's defaults**: `bug`, `enhancement`, `documentation`, `question`,
  `duplicate`, `invalid`, `wontfix`, `good first issue` and `help wanted`.
- **Area labels**: `plugin-runtime`, `fledge`, `notes` and `forge`, one per
  plugin epic.
- **`blocked`**: the issue cannot start yet, and its body says what it waits
  for (another issue, the owner, or Plan 1's exit).

An issue that needs **an owner's decision** before the work can start is
labelled `question`. Its body sets out the options, what each costs and a
recommendation, so the owner can answer in one reply.

In the owner's words (2026-10-16): "the label set may grow (sizes,
priorities, a decision label) when feature work needs it, but not before."
Do not add a label without the owner's agreement.

## Deferred-work markers in code

A `TODO`, `FIXME` or `HACK` comment in Syrinx-authored code is allowed only
when it names the issue that will remove it, as in
`TODO(#123): restore zoom per site`. Each such issue also has a row in
[`docs/dev/deferred-work.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/dev/deferred-work.md),
and `cargo xtask check` fails on a marker with no issue number or with an
issue missing from that register. A marker never excuses a failing test, a
silenced lint or a swallowed error. Upstream comments inside `vendor/` are
exempt, because those files are kept as their authors wrote them.

## From issue to `main`

1. **One branch per change.** Each issue (or slice of one) is built on its
   own branch, usually in its own git worktree, so unfinished work never
   sits on `main`.
2. **Pass the gate.** `cargo xtask check` must pass on the branch (see
   [Getting started](Getting-started)). After changing anything the owner
   sees, run the app and look at the window as well.
3. **Independent review.** Someone other than the author, a person or a
   separate agent session, reviews the change against the issue and the
   rules in `AGENTS.md`.
4. **The owner merges.** Agents push only the branch their session was
   assigned and open a pull request only when the owner asks. Nobody merges
   their own work; fixes to a pull request go on that pull request's branch.

## How issues are written

Issues follow the "Writing voice" section of
[`AGENTS.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/AGENTS.md)
and the format in
[`.claude/skills/planissues/issue-format.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/.claude/skills/planissues/issue-format.md).
In short, an issue is written to educate someone who has never seen the
codebase:

- say why before what;
- define the terms and name the real files, decisions and issues;
- state what breaks if the work is skipped, and what "done" looks like;
- show the reasoning, including what was rejected;
- write prose, not fragments, and never a bare "see #12";
- mark an unverified assumption with ⚠️.
