# Notes' synthetic vault workloads

#36 asks for the small, medium and large synthetic vaults to be chosen and
recorded before anything is optimised, so that every measurement of Notes (the
plugin's scan and memory, the host's file service and watcher, #41's warm
reopen, D48's search timing) uses the same vaults and can be compared. #98
lists the choice as paper work that can start now. This file records the
choice of scales and the rules Syrinx-app's host tests follow; it holds no
measurements.

**Every vault is generated. None is ever a real vault**, and no agent opens the
owner's vault or its `.obsidian` folder (`AGENTS.md`, "Test data"; #98,
"Watch").

## The three workloads

The scales are the ones the Notes plugin's generator already produces
(`synthetic-vault` in `Fledge-Software/Syrinx-Notes-Plugin`), so the plugin's
figures and the host's describe the same vaults.

| Workload | Notes | Folders | Paragraphs per note | Wikilinks per note | What it is for |
|---|---:|---:|---:|---:|---|
| small | 50 | 5 | 3 | 2 | Unit and contract tests; every host test on a real filesystem |
| medium | 2,000 | 40 | 4 | 4 | An everyday vault: scan progress, watch bursts, reindexing (P3-03) |
| large | 20,000 | 200 | 4 | 6 | Upper bound: memory, #41's warm reopen, D48's search timing |

One further case stands beside the three: **a one-mebibyte note**, built by
joining generated bodies, for the editor's language data (#37) and the search
feed's 1 MiB body cap.

The measured figures for these vaults (scan time, catalogue memory per note,
the note shape the generator writes) are the Notes repository's, in
[`docs/workloads.md` at `82ccf81`](https://github.com/Fledge-Software/Syrinx-Notes-Plugin/blob/82ccf81081386e9d2ddd115654b8e4a00b45ea6a/docs/workloads.md).
They are not copied here, so they cannot drift apart.

## Host-side rules

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

## Where Syrinx-app gets the generator

Syrinx-app's host tests need the generator, which lives in the Notes
repository. **On 2026-11-10 the owner decided to vendor `synthetic-vault`,
pinned to a fixed commit, and rejected a published crate**
([#98, comment](https://github.com/Fledge-Software/Syrinx-app/issues/98#issuecomment-6767766166)).
The copy goes under `vendor/` with a provenance note recording the commit and
its hash, as `vendor/` already does for reviewed outside code. Reasons:

- the gate stays offline: a Git dependency would make `cargo xtask check`
  fetch code, which decision 12 of 2026-10-24 rejected for npm for the same
  reason;
- publishing a crate would make a test helper a public, versioned interface
  with its own release process;
- a second generator written in Syrinx-app would duplicate the rule and drift
  from the plugin's.

The cost is that a change to the generator needs a re-vendor in Syrinx-app.
The vendored copy arrives with the first host slice that needs generated
vaults.
