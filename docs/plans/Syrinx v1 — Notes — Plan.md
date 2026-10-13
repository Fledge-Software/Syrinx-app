---
title: "Atlas v1 — Notes — Plan (plan 3)"
status: active
created: 2026-10-09
subject: Atlas v1 plan 3 — the Notes plugin (spec phase 4)
relates_to: "docs/plans/Atlas v1 — Plan.md (plan 1, and the v1-wide decisions D1–D38); docs/plans/Atlas v1 — Plugins and Fledge — Plan.md; docs/research/04-forge-and-notes.md"
issues_repo: not yet created
branch: claude/plan-atlas-v1
---

# Atlas v1 — Notes

> **Status: active, approved by the owner on 2026-10-10.** Implementation is
> queued behind plans 1 and 2. Work uses plan 1's D41 local branch,
> independent-review and owner-approved-merge workflow; commits still need
> an owner request. The owner also approved this plan's 16-slice breakdown
> on 2026-10-10; its predecessor and acceptance gates remain in force.

## Why this exists

The owner wants notes in the same window and the same search as the browser
and code (`docs/spec.md`, "Notes"): roughly Obsidian crossed with Notion,
with no graph view (S4).

The important promise is that leaving Atlas does not strand the notes.
Markdown files remain the source of truth; SQLite and the full-text index
are rebuildable catalogues. A successful v1 lets the owner write, follow
links, find context and edit a simple property table without damaging an
existing Obsidian-compatible folder. Matching all of Obsidian or Notion is
not the v1 bar (D5).

## Scope (thin v1, D5)

- **A vault is a folder of `.md` files**, Obsidian-compatible (D27):
  wikilinks, Obsidian's property types, a subset of its `.base` format.
- **Rust indexer:** pulldown-cmark and serde-saphyr into SQLite tables for
  notes, headings, links and properties, feeding both search tiers; every
  change Atlas makes is a byte-range patch.
- **The editor:** CodeMirror 6 with live preview in a local web view (D28),
  with Atlas's shortcut routing on that web view.
- **Backlinks, rename with link rewrite, the outline panel,** outside-edit
  handling (debounced watcher, conflict banner when dirty).
- **One table view** over a folder or tag: frontmatter columns, one sort,
  AND filters, inline edits written back losslessly.
- **Existing local Markdown images and ordinary attachment links** display
  or open through the supported local-resource path (N4). This does not add
  attachment importing or Obsidian's special embed syntax.

The original outline and the owner's additions below define this scope.
The remaining sections turn it into ordered work and acceptance criteria;
technical checks still needed before implementation remain explicit.

## Decisions taken for this plan

| ID | Question | Owner's decision | Alternative and reason | Date |
|---|---|---|---|---|
| N1 | How many vaults are active, and how does one start? | One active vault at a time. A folder picker opens an existing Obsidian-compatible folder or creates an empty vault. | Several simultaneously indexed vaults add cross-vault links and search ambiguity without a stated need. No existing notes need to be supplied to an agent. | 2026-10-10 |
| N2 | When are edits saved, and what happens to unsaved work? | Notes autosaves after a short typing pause; Ctrl S saves immediately. Keep local recovery drafts, and do not overwrite the disk version when an outside edit conflicts with a dirty buffer. Forge remains explicit-save with Ctrl S. | Explicit-save only for Notes adds work to everyday note taking; automatically choosing either side of a conflict risks losing text. Recovery drafts preserve unsaved work without replacing the source file. | 2026-10-10 |
| N3 | How much of Bases belongs in v1? | The table subset is enough: frontmatter columns, one sort and AND filters. Unsupported constructs are clearly labelled, left unchanged, and their original source is available to read. | Silently simplifying an unsupported view can change its meaning; implementing all Bases features would expand the thin v1. | 2026-10-10 |
| N4 | Which attachments belong in Notes v1? | Display existing local Markdown images and ordinary attachment links. Defer paste/drop attachment importing and special Obsidian embeds. | An attachment importer and embed compatibility introduce file-management and rendering work beyond the thin first version; existing ordinary Markdown content should still be usable. | 2026-10-10 |

## Prerequisites and boundaries

Implementation starts only after plan 1 and plan 2 meet their exit criteria
and this plan's local slice breakdown is approved. Plan 1 supplies the generic
tab model, local storage, both search tiers, themes, shortcut router and overlay
behaviour. Plan 2 supplies the built-in plugin host and its command, search,
view and lifecycle contracts. An unfinished prerequisite is a blocker, not
permission to build another feature out of order.

Product analytics follows plan 1's D39: local aggregate feature/command
counts, timings and error categories, with explicit report export and
suggestions the owner reviews. There is no automatic upload or adaptive UI.
Account/device identifiers, URLs, queries, paths, text and detailed timelines
remain excluded, including notes, attachment content and recovery drafts.
This plan adds no separate analytics integration or personal-data prompts.

- **D27 stays settled:** plain markdown, wikilinks, Obsidian's property types
  and a subset of `.base` files. Unsupported syntax is preserved on disk.
- **D28 stays settled:** CodeMirror owns the live text, selection and undo;
  Rust owns disk access and saves only when the two buffer checksums agree.
  The shared editor bridge is designed for reuse by Forge in plan 4.
- **D32 stays settled:** English and Latin-script input are the input-test
  scope. Unicode preservation and offset conversion still need tests;
  excluding an IME matrix does not permit corrupting existing text.
- `plugin-notes` reaches Atlas through `atlas-plugin-api` and `Host`, with
  no direct dependency on core, store, search, GPUI or wry. Storage and
  indexing services stay behind the host; `atlas-ui` renders the shell and
  `atlas-platform` owns WebView2 plumbing.
  Required host capabilities are reviewed before implementation, because
  changing the public plugin contract needs the owner's approval.
- Atlas's database and recovery metadata stay in local app data (D13).
  Choosing a vault does not move SQLite beside the markdown files. A vault
  folder may have its own external sync tool; Atlas supplies no sync client.
- Tests use generated vaults only. The owner may assess their own notes in
  the app; agents do not inspect or copy them into fixtures or reports.

## Architecture and data integrity

### Vault and index

A vault service identifies files by their canonical vault-relative paths
and maintains records for notes, headings, links and typed properties.
Paths shown in note links use forward slashes. Windows case collisions,
reserved file names and paths that escape the selected folder are rejected
before a write. Junctions and symbolic links must not silently expand the
indexed or writable area beyond the selected vault.

An initial scan builds the catalogue incrementally and reports progress.
Subsequent scans reuse unchanged files using modification metadata and
content hashes. A debounced watcher schedules reconciliation of current
disk contents; it does not treat every native event as an exact operation.
An interrupted scan can resume or rebuild without changing any note.
Explicit re-scan is available when the watcher reports an error or misses
a change. Switching the active vault (N1) removes the old vault's search
entries before publishing the new catalogue.

Titles and paths feed tier 1; note bodies feed the tier 2 store selected by
plan 1. This plan does not choose tantivy in advance of that bake-off.
Index updates carry document revisions so an older background parse cannot
replace newer content. Search results open the corresponding Notes view
through the generic tab and plugin host contracts.

The candidate parsers remain `pulldown-cmark` and `serde-saphyr` from the
outline. They supply interpreted values and source locations; they do not
justify parsing and serialising the entire file on save. Dependency versions
are chosen at implementation after maintenance, security and licence checks.

### Shared editor and save protocol

Notes autosaves after a short pause in typing and lets Ctrl S request an
immediate save (N2). The shared bridge keeps save timing as a caller policy,
because Forge uses explicit saving. A conflict or integrity error suspends
autosaving until it is resolved; retry timers cannot bypass that state.

The trusted local editor uses a versioned message protocol containing a
document identity, base revision and change set. CodeMirror applies typing
locally without waiting for Rust. Rust validates and applies each change,
acknowledges its revision and provides derived results asynchronously.
Messages from a previous document or an older editor instance are rejected.
The host also validates the bundled editor origin, message kind and
document access before accepting a message.

CodeMirror positions count UTF-16 units and a line break as one position;
Rust parsers use UTF-8 byte offsets. The bridge explicitly converts these
coordinates and hashes the same logical text on both sides, without Unicode
normalisation. Preserve the source encoding marker and original byte ranges,
including a mapping for each line ending. CodeMirror's default normalises
line endings, and choosing one `lineSeparator` is insufficient to preserve
arbitrary mixed endings. The raw disk revision is tracked separately:
two matching buffers are not proof that another program left disk unchanged.

A save captures one acknowledged revision, checks the editor and Rust
checksums, verifies the expected disk revision and stages the bytes for
replacement. It clears the dirty state only for the revision actually
written. A change typed during saving remains dirty. Failed writes retain
the buffer and report a recoverable error; they never pretend to succeed.
Checking a fingerprint and then replacing a file is not one conditional
operation: another program can write between the two. Phase 2 must prove
a preservation/recovery strategy for that race before enabling ordinary
saves. Windows replacement can also fail after changing file names; handle
each documented outcome rather than treating every failure as no change.
Checked flushing, replacement and recovery-record ordering need interruption
tests before claiming crash-safe saves. `ReplaceFileW` does not support its
write-through flag, so that flag cannot supply the durability guarantee.

Unsaved text also has a local recovery draft, separate from the note and
from the rebuildable search index (N2). Drafts identify the document and
last known disk revision. After a restart, recovery compares that revision
with disk and offers restoration or a separate copy; it never silently
replaces a newer file. Drafts stay in local app data and never enter logs,
telemetry, fixtures or repository files. A draft is cleared only after its
revision is durably saved or explicitly discarded; an older completed save
must not clear a draft containing newer edits.

Formatting and property changes to an open note travel through the same
document transaction path, so the editor cannot later save an obsolete
copy over them. An acknowledgement does not create a second undo event.
Restoring synchronisation after a mismatch must preserve the live buffer;
an automatic reset that loses edits is not an acceptable recovery method.

CodeMirror live-preview decorations reveal syntax near the cursor without
rewriting the markdown. Rust remains authoritative for saved links,
properties, search and outline data. A display parser may disagree at an
edge case; the plan must test and expose that limitation rather than allow
its result to drive file rewrites. Unknown markdown remains editable source.

The editor web view is local and trusted, separate from page web views.
Only it receives editor inter-process messages. Note text is never treated
as executable HTML or script, and previewing a note does not fetch remote
resources automatically. Following a web link is an explicit user action
that opens a normal page tab through the host.

Existing local images use ordinary Markdown image syntax and a bounded
resource resolver (N4). Resolve their paths against the note and vault,
using the same containment checks as the file service; never interpolate
note-supplied markup as executable UI. Missing, inaccessible or unsupported
images show an honest placeholder while their source remains intact.
Ordinary attachment links use an explicit click and the host's supported
file-opening route, with a clear error if unavailable. Neither action adds
an importer, a new document editor or automatic execution of attachments.

### External edits and multi-file operations

An unchanged open buffer reloads when another program changes its file.
A dirty buffer instead shows a conflict and pauses saves; neither side is
silently chosen (N2). The resolution UI offers comparison, keeping
the disk version or saving the editor version as a separate copy. Replacing
either version requires a deliberate action. If a file disappears, the
editor retains unsaved text and offers saving it elsewhere inside the vault.

Renaming a note first resolves its incoming links, previews the affected
paths and records the revisions used to prepare edits. It rewrites only
the destination portions of actual links, preserving aliases, headings,
comments, quoting and unrelated text. Links in code blocks are not notes.
A file changed since preview invalidates that part of the operation.

A rename and its link edits span several files and are not one atomic
filesystem operation. The implementation therefore needs a recoverable
operation record and a tested interruption policy. Undo applies only when
the affected files still have the expected post-operation revisions;
later outside edits must not be overwritten by rollback. Dirty open notes
participate through their document state or block the rename with a clear
explanation, never through an independent disk patch.

### Properties and the table subset

The seven property kinds are text, list, number, checkbox, date, date and
time, and tags. One property name has a vault-wide type; `tags` is special.
Frontmatter is YAML at the beginning of a note. Unknown keys, comments,
ordering and unedited values survive every Atlas change.

The first table supports a folder or tag source, selected property columns,
one sort and property/operator/value filters joined by AND. Its saved form
has one `table` view in a `.base` file. There is no `.base` source/from key:
represent the folder/tag restriction as filters, and include
`file.ext == "md"` so Obsidian does not add attachment rows to Atlas's note
table. Combine global and view-level conditions with AND. `order` specifies
columns, not row sorting; the exact saved sort representation needs the
compatibility check below. The query implementation parses the supported
expressions into Rust values rather than executing arbitrary JavaScript.

Typed edits are byte-range patches against a validated revision. Complex
YAML constructs are never normalised merely to make a property editable.
If Atlas cannot safely locate or replace a value, it leaves the bytes intact
and explains that the value needs source editing. Malformed frontmatter
does not make the whole note unreadable or erase its existing index entry
without an error state.

A `.base` file containing unsupported filters, formulas or views is clearly
labelled and left unchanged, with the original source available to read
(N3). Atlas does not present a partial interactive table as though it had
applied every condition: dropping a filter can show the wrong rows.

## Sequencing

Each phase has a usable result and a clear predecessor. Readiness and the
local slice breakdown are approved; the issue map below preserves the
implementation gates. These are phases, not published issues.

### Phase 1 — Open and index a synthetic vault

**Why first:** editing against an unreliable catalogue creates broken links
and stale search before the editor has a chance to be useful.

Deliver vault selection, the bounded file service, the Rust indexer, note
navigation through the plugin host, both search contributions and scan
progress. Establish the fixture corpus and dependency checks here.

**Exit:** a generated vault can be opened, searched, closed and reindexed;
duplicate paths, inaccessible files, unsupported encodings and malformed
frontmatter produce contained errors. Unfinished Markdown remains editable
text, not a document-wide parse failure. Cancelling a scan leaves files
untouched, and rebuilding reproduces the same notes, headings and links.

### Phase 2 — Edit and save without losing text

**Why now:** links, table edits and future Forge work all depend on the
document bridge handling ordinary typing and failure correctly.

Deliver the local CodeMirror view, live preview, source editing, outline,
shortcut routing, revision acknowledgements, autosave and checked saves.
Add the watcher and clean/dirty external-edit handling before any bulk rewrite.
Add recovery drafts and restart recovery under N2 in this same phase.
Display existing local Markdown images and ordinary attachment links (N4).

**Exit:** opening and saving unchanged fixtures preserves their bytes;
typing, paste, undo, redo and Ctrl S survive delayed or duplicate messages.
Checksum disagreement, disk conflicts, denied writes and interrupted saves
retain recoverable text. Palette and sidebar overlays return focus correctly
without consuming editor shortcuts or introducing duplicate undo steps.
Recovery drafts survive a forced interruption, and restored text cannot
silently overwrite a file changed while Atlas was closed.
An external write deliberately raced between fingerprint validation and
replacement remains recoverable and produces a conflict. Replacement
partial failures and failed flushing do not clear unsaved drafts.
Image/link fixtures cover relative paths, missing files and escaping paths;
special embeds stay unchanged source and no remote resource loads on preview.

### Phase 3 — Navigate links and rename safely

**Why now:** backlinks are useful only when the resolver agrees with the
links the owner actually opens, and rename builds on reliable saves.

Deliver wikilink completion and navigation, linked backlinks, heading
navigation, rename preview and guarded link rewrites. Define the supported
markdown-link forms in the compatibility corpus. Unlinked mentions are a
separate search result; they never become automatic rewrite targets.

**Exit:** reference fixtures cover aliases, headings, missing destinations,
duplicate filenames and case-only renames. Rename and guarded undo touch
only intended ranges. Injected failures at each write produce either the
completed operation or an explained, recoverable partial operation.

### Phase 4 — Edit properties through one table

**Why now:** a property table writes many of the same files as the editor,
so it must share the proven revision and conflict handling.

Deliver typed property display, the table subset, `.base` persistence,
filtering, sorting and inline property edits. Unsupported features follow
N3: label them, preserve them and expose the original source for reading.

**Exit:** Atlas-created tables open correctly in a disposable Obsidian test
vault, and the supported Obsidian-created subset opens correctly in Atlas.
Folder/tag fixtures containing notes, attachments and a `.base` file produce
the same note rows in both apps; global and view filters both take effect.
Edited values change only intended bytes. An already-open note immediately
reflects a table change through its document state and can undo it without
restoring stale content over newer disk edits.

### Phase 5 — Compatibility and daily-use acceptance

**Why last:** correctness tests cannot prove the interaction is pleasant or
that the selected dependencies remain fast in the real shell.

Deliver a compatibility matrix, measured performance report and keyboard
and accessibility pass in the five Atlas palettes. Review the resulting
Notes interface with the owner against `design/README.md`; that document
has no detailed Notes board, so this plan does not invent an approved one.

**Exit:** all preceding gates pass; the owner approves the supported Notes
workflow and known limits. Any personal-vault check is performed by the
owner in the app, with only their pass/fail report recorded. Plan 4 starts
only after this acceptance and the owner's go-ahead.

## Verification and performance

Run the repository gate in `docs/LOCAL-DEV.md` once it exists; until then,
use `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and
`cargo test`. Editor protocol tests also run in the local web-view harness.
Every visible change is inspected in the Windows app, using D38's profile.

The generated corpus covers LF, CRLF, mixed endings, UTF-8 markers,
accented Latin text, supplementary Unicode characters, empty files, absent
final newlines, large files, fenced code, unknown syntax, YAML comments,
quoted scalars, lists, duplicate keys, anchors and malformed frontmatter.
Assertions compare the actual bytes outside edited ranges, not merely the
parsed result. Unsupported encodings must fail safely without rewriting.

Failure tests exercise replace-style external saves, delayed watcher events,
rename during editing, external deletion, permissions changes, stale parse
results, checksum mismatch, an outside write after fingerprint validation,
replacement partial failures and interruption between multi-file writes.
No note contents or personal paths appear in logs or panic diagnostics.

Before performance work, record a reproducible generated corpus, machine,
runtime and build settings. Measure initial index, warm reopen, incremental
update, note open, keystroke-to-paint, save acknowledgement, table filtering,
renderer memory and idle CPU. Reuse plan 1's tier 1 ranking target of 5 ms
at 100,000 catalogue rows. Notes-specific size and latency budgets remain
an implementation measurement task: select and record representative small,
medium and large synthetic vaults before optimizing. The owner may later
refine that workload without providing personal notes. Missing personal
vault measurements do not block completing this plan, and no timing here
is presented as an achieved result.

## Not in v1

Formulas, relations, rollups, kanban and calendar views, block references,
special Obsidian embeds, paste/drop attachment importing, any WYSIWYG mode
and a graph view (`docs/research/04-forge-and-notes.md`, "Notes"). Ordinary
local Markdown images and attachment links are included by N4. No IME
testing (D32).

## Open threads

- **Measurement workload:** choose and record synthetic vault scales before
  optimization. Outside-edit handling is required regardless of whether
  the owner currently edits a vault in another app.
- **Property type metadata:** Obsidian's seven value types are documented,
  but a stable external contract for its vault-wide type configuration has
  not been verified. Decide storage and interoperability from a generated
  test vault before building, without modifying the owner's `.obsidian`
  configuration. Any additional product choice returns to the owner.
- **Search:** inherit plan 1's tier 2 bake-off result; assess with synthetic
  notes first, then an owner-operated check if desired.

## Evidence checked on 2026-10-10

Research note 04 explains the original alternatives; it is not a substitute
for these current checks. No new dependency is approved by being listed.

| Claim | Current evidence and remaining limit |
|---|---|
| CodeMirror transactions and undo can support the bridge. | The [reference manual](https://codemirror.net/docs/ref/) documents transaction annotations, undo-history control and UTF-16 positions. The [split-view example](https://codemirror.net/examples/split/) explicitly forwards changes and keeps one undo history. Atlas's Rust bridge still needs fault-injection tests. |
| Mixed line endings need preservation outside CodeMirror's default text model. | The [state implementation](https://raw.githubusercontent.com/codemirror/state/main/src/state.ts) splits lines and joins them with the chosen separator. Phase 2 must prove original-byte/EOL mapping and unchanged-range preservation; matching logical-text hashes alone is insufficient. |
| The markdown parser exposes relevant syntax and locations. | `pulldown-cmark` documents [wikilink and metadata options](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.Options.html) and [offset iteration](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.Parser.html). Event ranges alone do not prove every link destination can be rewritten safely. |
| Unfinished Markdown is not generally invalid Markdown. | [CommonMark's character model](https://spec.commonmark.org/0.31.2/#characters-and-lines) accepts every Unicode character sequence as a document. Distinguish frontmatter/encoding/I/O failures from ordinary incomplete markup. |
| YAML parsing does not guarantee a lossless file round trip. | [`serde-saphyr`](https://docs.rs/crate/serde-saphyr/latest) documents limitations, including uncaptured freestanding comments. Its [span API](https://docs.rs/serde-saphyr/latest/serde_saphyr/struct.Span.html) supplies byte offsets for string inputs, not reader inputs. Keep original text and patch validated ranges. |
| Property kinds and `.base` structure are documented. | Obsidian documents [properties](https://obsidian.md/help/properties), [Bases syntax](https://obsidian.md/help/bases/syntax) and [view sorting](https://obsidian.md/help/bases/views). Bases initially include every vault file; folder/tag restrictions belong in filters alongside the note-file restriction. ⚠️ The exact serialized table-sort key and property-type metadata need a disposable-vault compatibility check. |
| Basic internal-link forms are documented. | Obsidian documents [wikilinks, aliases, paths and headings](https://obsidian.md/help/links); its [public API declarations](https://github.com/obsidianmd/obsidian-api/blob/master/obsidian.d.ts) describe best-match resolution. ⚠️ They do not establish a complete duplicate-name tie-break algorithm; do not claim exact compatibility before testing it. |
| Watchers cannot be treated as a perfect history of file operations. | [`notify`](https://docs.rs/notify/latest/notify/) documents differing save patterns and missed-event cases. Reconcile disk state and verify revisions before writing. |
| Safe replacement needs more than a pre-write fingerprint. | Microsoft's [ReplaceFileW contract](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew) has partial-failure outcomes and an unsupported write-through flag. Rust's [file synchronization](https://doc.rust-lang.org/std/fs/struct.File.html#method.sync_all) reports flush errors that dropping a file can hide. Phase 2 must prove concurrent-write preservation and recovery ordering on Windows. |

## Issue map

The owner approved these 16 local slices on 2026-10-10, exactly as
recorded in [the approved Plans 2–4 breakdown](<Atlas v1 — Plans 2–4 slice proposals.md>).
The boundaries, deliverables and explicit blockers below are unchanged from
that approval. Every slice also depends on the preceding slice in this plan;
finish each phase exit before its next phase. The breakdown's common gates
apply to every row. All rows are queued, not implemented.

**Entry gate:** Plans 1 and 2 pass their exits, including Plan 2 owner acceptance.
This slice map is approved. Missing Host capabilities still need review and
any required public-contract approval before implementation.

D41 supplies one local branch and independent review per slice, with owner
approval before every merge. Commit only on request. No remote, push, public
issue or upstream message is authorized. Concrete dependency and public-API
approvals remain separate; the map does not waive predecessor or operator gates.

### Phase 1 — open and index a synthetic vault

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P3-01 | A vault must have a clear boundary before reading or writing files. Add the one-vault existing/new picker and bounded host file service, canonical relative identities, path containment and cancellation. | Generated fixtures cover Windows case collisions, reserved names, junction/symlink escapes, inaccessible files and unsupported encodings. Switching removes the old vault's contributions; cancellation leaves files untouched. Blocked by the Notes entry gate and required capability/dependency reviews. |
| P3-02 | Rebuildable metadata must preserve the Markdown source. Add Rust indexing of notes, headings, links and properties, scan progress and malformed/unsupported-file states; record synthetic small/medium/large vault workloads. | Rebuilding reproduces catalogue records from files. Unfinished Markdown stays editable; malformed frontmatter is reported without rewriting source or silently erasing a previously valid entry. Parser candidates receive exact audits before adoption. |
| P3-03 | Notes need shared navigation and search before editing adds complexity. Connect stable note targets, incremental/revisioned updates, explicit rescan and both search contributions through Host. | A generated vault can be opened, searched, closed and reindexed. Stale parse/search results cannot replace a newer revision; cancelled/superseded scans cannot restore the old vault. Phase 1 closes with both search tiers verified. |

### Phase 2 — edit and save without losing text

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P3-04 | Native and editor copies must agree on which document and revision they describe. Build the trusted bundled CodeMirror surface and versioned change/acknowledgement bridge, origin/type/access checks, UTF-16/UTF-8 conversion and canonical checksums. | Synthetic protocol tests cover delayed, duplicate, stale and previous-editor messages, mixed line endings, encoding markers and supplementary Unicode. Typing stays local; synchronization failure retains the live buffer. Blocked by Phase 1 and editor dependency/Host capability approval where required. |
| P3-05 | Editor views must not change the source merely to display it. Add live preview, source editing, outline navigation and Atlas shortcut/focus routing through the shared bridge. | Typing, paste, undo/redo and outline navigation preserve text and do not add duplicate undo events. Palette/sidebar overlays restore editor focus; unknown syntax remains editable source. Inspect the actual local editor view. No unsafe disk-save path is exposed while later safety slices remain open. |
| P3-06 | A matching buffer checksum alone does not protect against outside disk changes. Implement staged saves for one acknowledged revision, source-byte/line-ending preservation, checked flushing, expected disk revision and recoverable replacement failures. | Unchanged files round-trip byte-for-byte. Tests race an outside write between validation and replacement and interrupt each durability step; both versions remain recoverable. Denied writes, partial replacement failures and edits during saving retain dirty state. No write-through guarantee is attributed to unsupported `ReplaceFileW` flags. |
| P3-07 | Watcher events are hints, and unsaved work must survive a crash. Add debounced reconciliation, clean reload, dirty conflict state, recovery drafts and restart reconciliation against current disk. | External replace/delete/rename and delayed/missed events are covered. Conflict pauses saving; disk/editor/separate-copy choices are deliberate. Restart cannot silently overwrite changed files, and an older completed save cannot clear a newer draft. Drafts remain in local app data outside logs and fixtures. |
| P3-08 | Ordinary save controls must use the proven safety path. Enable Notes autosave after a short pause and Ctrl S, error/retry presentation and document transactions shared with non-editor edits. | Checksum disagreement, conflicts and write failures keep recoverable text and prevent unsafe retry. Autosave and explicit save clear only the durably saved revision. Forge's later explicit-save policy remains a caller choice on the same service. |
| P3-09 | Existing images and links must be usable without turning note text into executable content. Add bounded local Markdown image resolution and explicit ordinary attachment opening. | Relative/missing/escaping paths and unsupported images produce honest states; no preview loads remote resources. Note text is not executable HTML/script. Local editor focus and save/recovery regressions pass together, closing Phase 2. Paste/drop importing and special embeds remain excluded. |

### Phase 3 — links and guarded rename

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P3-10 | Link navigation and backlinks must agree before any rewrite uses them. Add wikilink completion, heading navigation, linked backlinks and a documented supported Markdown-link corpus. | Fixtures cover aliases, headings, missing/ambiguous destinations, duplicate filenames, case variants and links inside code blocks. Unlinked mentions remain separate search results. Blocked by the complete save/recovery/editor exit. |
| P3-11 | Renaming several files is not an atomic operation. Add previewed rename and lossless destination-range rewrites through the shared document service, with revision checks and a recoverable operation record. | Inject failure between each write; report completed or recoverable partial work. Guarded undo refuses later outside changes, and dirty open buffers participate or block clearly. Case-only renames, aliases, headings and unrelated bytes survive. Phase 3 closes with recovery evidence. |

### Phase 4 — typed properties and one table

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P3-12 | Typed properties need verified interoperability before storage conventions become permanent. Verify the seven property kinds and vault-wide type metadata using a disposable generated Obsidian vault; implement bounded interpretation and safe source ranges. | Record the supported metadata contract and any unresolved product choice for the owner. Unknown keys, YAML comments/order and complex unsupported values stay intact; malformed content remains source-readable. Blocked by Phase 3 and authorized application access if required for the compatibility check. |
| P3-13 | A saved table must apply every supported filter without pretending unsupported views are equivalent. Implement one folder/tag table, selected columns, one sort, AND filters and `.base` round trips. | Generated cross-application fixtures verify note-only rows, `file.ext == "md"`, global/view filter combination and the actual sort representation. Unsupported filters/formulas/views are labelled, unchanged and source-readable. No partial interactive result silently drops a condition. |
| P3-14 | Table edits must not compete with the editor's live copy. Route property changes through the shared revision, save/conflict and undo service as validated byte-range patches. | Only intended bytes change; an open note immediately reflects a table edit and can undo it without restoring stale disk content. Unsafe YAML replacements fail visibly rather than normalizing the file. Phase 4 closes with supported Obsidian-to-Atlas and Atlas-to-Obsidian table checks. |

### Phase 5 — compatibility and acceptance

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P3-15 | Compatibility and performance claims need reproducible limits. Complete the synthetic compatibility matrix and measure index/reopen/update, note open, input paint, save acknowledgement, table filtering, renderer memory and idle cost. | Use the workloads recorded before optimization; keep D48 intact. Report failures and supported/unsupported forms explicitly. All interruption and byte-preservation tests plus the repository/editor gates pass. Blocked by Phase 4. |
| P3-16 | The owner must accept the actual Notes workflow before Forge builds on it. Complete keyboard/accessibility and five-palette review, then owner acceptance of Notes and its known limits. | Inspect the implemented UI against the design authority without inventing a detailed approved Notes board. Any personal-vault assessment is owner-operated with only pass/fail recorded. Notes acceptance and the owner's go-ahead are explicit blockers for Forge. |

This map adds no graph, WYSIWYG mode, advanced Bases formulas/relations/views,
special embeds, attachment importing or sync client.

## Iteration log

- **2026-10-10:** Expanded the outline at the owner's request into the vault,
  editor, link, property-table and acceptance phases. Kept the established
  thin-v1 scope and D27/D28/D32 unchanged. Owner questions, interoperability
  gaps and unmeasured performance remain explicit; no implementation or
  personal-data inspection was performed.
- **2026-10-10:** The owner approved one active vault and a picker to open
  an existing Obsidian-compatible folder or create an empty one (N1).
- **2026-10-10:** The owner approved Notes autosave after a short pause,
  immediate Ctrl S, local recovery drafts and preserving both sides of
  dirty outside-edit conflicts (N2); Forge keeps explicit saves. The owner
  also accepted the table subset and preserving labelled unsupported
  `.base` constructs with their original source available to read (N3).
- **2026-10-10:** The owner included existing local Markdown images and
  ordinary attachment links while deferring paste/drop importing and
  special Obsidian embeds (N4). Vault-scale measurement remains technical
  implementation work, not an unanswered scope question.
- **2026-10-10:** The owner-authorized readiness review added explicit
  mixed-ending byte preservation, concurrent-write and Windows replacement
  failure gates, and the `.base` Markdown-file filter needed for matching
  rows in Obsidian. Clarified malformed-frontmatter versus incomplete
  Markdown handling. Official sources still leave table-sort serialization,
  property-type metadata and duplicate-link resolution to the compatibility
  checks. No owner decision or implementation order changed.
- **2026-10-10:** The owner approved all four plans and plan 1's 40-slice
  breakdown, with the local workflow in D41. Marked Notes active and queued
  behind plans 1 and 2; its own breakdown still returns for approval.
  Commits remain on request. D39 permits local aggregate analytics and
  explicit report export with owner-reviewed suggestions; note content and
  recovery drafts remain excluded.
- **2026-10-10:** The owner subsequently approved the exact 16-slice map
  and dependencies now recorded above. Predecessor exits, operator acceptance
  and separate public-API/dependency/commit/merge permissions remain in force.
