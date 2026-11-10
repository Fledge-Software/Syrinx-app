# Notes' synthetic vault workloads

#36 asks for the small, medium and large synthetic vaults to be chosen and
recorded before anything is optimised, so that every measurement of Notes (the
plugin's scan and memory, the host's file service and watcher, #41's warm
reopen, D48's search timing) uses the same vaults and can be compared. #98
lists the choice as paper work that can start now. This file records it for
Syrinx-app's host tests.

**Every vault is generated. None is ever a real vault**, and no agent opens the
owner's vault or its `.obsidian` folder (`AGENTS.md`, "Test data"; #98,
"Watch").

## The three workloads

The scales are the ones the Notes plugin's generator already produces
(`synthetic-vault` in `Fledge-Software/Syrinx-Notes-Plugin`, measured in that
repository's `docs/workloads.md` on 2026-10-24), so the plugin's native figures
and the host's figures describe the same vaults.

| Workload | Notes | Folders | Paragraphs per note | Wikilinks per note | What it is for |
|---|---:|---:|---:|---:|---|
| small | 50 | 5 | 3 | 2 | Unit and contract tests; every host test on a real filesystem |
| medium | 2,000 | 40 | 4 | 4 | An everyday vault: scan progress, watch bursts, reindexing (P3-03) |
| large | 20,000 | 200 | 4 | 6 | Upper bound: memory, #41's warm reopen, D48's search timing |

Each generated note has four frontmatter properties (a word, a number, a
two-item list and a date), three headings, the paragraphs above, the
workload's number of wikilinks and one web link. The plugin measured the medium vault at about 1.3 MiB of files and a
4.6 MiB catalogue, and the large at about 14.2 MiB and 51.8 MiB (about 2.7 KB
of catalogue per note), which is the figure #135's per-plugin memory limit
needs (#98, comment on Notes 1, gap 4).

One further case stands beside the three: **a one-mebibyte note**, built by
joining generated bodies, for the editor's language data (decorations and
outline per pause, #37) and for the 1 MiB body cap of the search feed.

## Generation rules

- **A fresh seed on every run, printed with the result**, so a failure can be
  reproduced and no fixed file set becomes a stand-in for real data.
- **Words from a fixed synthetic vocabulary**, names and titles made from it,
  and links only to reserved domains (`example.com`, `.example`, `.test`,
  `.invalid`).
- **Written into a fresh folder in the test's own local temporary
  directory**, never in a synced folder (D13's reasoning applies to vaults in
  tests too), and deleted after a passing run.
- **The same generator for every layer.** #36: "The same generator serves unit
  tests, contract tests and the host's real-filesystem tests." The host's
  adversarial cases (case collisions, reserved names, junctions and links,
  `:` streams, 8.3 names, hard links and placeholders, #98 row 17) are
  separate fixtures added on top of the small vault, not new workloads.

## How each measurement uses them

- **The plugin's scan and memory** (P3-02): all three, natively now; inside
  WebAssembly once the test host can feed a component a vault (⚠️ parse speed
  in WebAssembly is unmeasured; `docs/dev/notes-parser-audit.md`).
- **The host's file service and watcher** (P3-01, P3-03): small for
  correctness, medium for bursts of watch events and a full rescan after an
  overflow.
- **#41's warm reopen and memory**: the large vault.
- **D48 with plugin rows** (#36, "What done looks like"): the large vault's
  20,000 note titles and paths added to the existing 100,000-row tier-1
  benchmark, with the criterion unchanged (95% of complete searches within
  5 ms, worst sample reported).

## Open

Syrinx-app's host tests need a path to the generator, which lives in the Notes
repository. A Git dependency would make the offline gate fetch code, so the
choices are a vendored copy at a pinned commit (as `vendor/` holds other
reviewed code) or a published crate; #98 or the owner decides (#98, comment
on Notes 1, gap 10). Until then this file is the specification both sides
follow.
