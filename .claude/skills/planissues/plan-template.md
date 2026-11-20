# Plan template

Copy everything below the line into the new plan file, named
`<Subject> — Plan.md`. Delete a section only when it truly has nothing in
it, and say so in one line rather than leaving it empty. The shape follows
a well-run implementation plan, so a reader who knows one knows them all.

Writing rules, from AGENTS.md: say **why before what**; define terms on first
use; name real files and documents; record every decision **with the
alternatives that lost**; write full sentences. Mark anything assumed rather
than verified with ⚠️.

---

```markdown
---
title: "<Subject> — Plan"
status: draft
created: <YYYY-MM-DD>
subject: <one line: the product and the area this plan covers>
relates_to: "<path to the active document(s) this sits beside, or to sibling plans from a split>"
issues_repo: <owner/repo where this plan's issues are filed>
branch: <the branch this plan was written on>
---

# <Subject>

> **Status: draft.** Not a source of truth until it is marked `active`.
> Where it disagrees with an active document, the active document wins, and
> the disagreement belongs under Open threads.

## Why this exists

<The problem, for whom, and what happens if nothing is done. Two or three
paragraphs. This goes stale slowest and is hardest to reconstruct later.>

## The idea, as first described

<The braindump, close to verbatim, dated. Kept so that anyone can see how
far the plan moved from where it started, and why.>

## What we found

<Research that shaped the plan. Lead with the single finding that shapes
everything, if there is one ("it was built once and dropped", "the vendor
does not offer X", "the table already exists"). Each finding names its
source: `path:line`, an issue link, a URL. Longer notes live in `research/`
next to this file.>

## Is it realistic?

<The honest verdict: feasible or not, with what. Effort in phases or weeks,
build cost and running cost per month, and the risks that could kill it.
Include the realism check's strongest objections and how each was answered,
or that it wasn't.>

## Decisions taken

| # | Decision | Chosen | Rejected, and why | Date |
|---|---|---|---|---|
| D1 | <the question> | <the answer> | <each alternative and the reason it lost> | <YYYY-MM-DD> |

## What exists today

<What is already built, with paths, tables, routes and issue numbers. What is
half-built. What a reader might find by grepping and be misled by.>

## The design

<How it works: the parts, the data, the flows, the edges (empty, error,
permissions, offline, many tenants). Scale each part to its complexity.>

## Sequencing

<Phases in order. Each phase has: why it comes here, what it delivers, and
its **exit criteria** (what "done" means, checkably). Phases become
milestones.>

### Phase 1 — <name>

- **Why now:** <…>
- **Delivers:** <…>
- **Exit criteria:** <…>

## What is deliberately not built

<Everything consciously left out, and why, so nobody builds it by
accident.>

## ⚠️ Still unproven

<Assumptions the plan rests on that nobody has verified, and what it would
take to verify each one.>

## Open threads

<Everything undecided. Each line: the question, what it blocks, and who
decides. Cleared as it is answered. At the readiness gate every entry becomes
a decision, a `decision` issue, or out of scope.>

## Issue map

<Written when the user approves the issue breakdown, with the Issue column
empty; each number is filled in the moment that issue is created. This is
the ledger that lets a cut-off run finish publishing without asking again,
and stops it from creating the same issue twice.>

| Plan section | Title | Labels | Blocked by | Epic | Milestone | Issue |
|---|---|---|---|---|---|---|

## Iteration log

<Newest first. One dated entry per round: what changed, why, and what the
research said. When a decision changes, the old answer and the reason are
recorded here, and the plan body holds only the current one.>

- **<YYYY-MM-DD>** — <…>

## Provenance

<Sources, research notes, the documents this plan read, and the conversation
it came from.>
```
