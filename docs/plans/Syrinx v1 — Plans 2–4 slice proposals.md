# Approved local slices for Plans 2–4

**Status: owner-approved, 2026-10-10.** The owner explicitly approved these
12 Plan 2, 16 Plan 3 and 21 Plan 4 slice boundaries and dependencies. The
respective plans now carry these exact rows in their issue maps. Implementation
remains queued behind every predecessor and acceptance gate; this approval
authorizes no dependency addition, public API change, commit or merge.
The separate permission for computer-use testing covers a fresh synthetic
Syrinx window, not general integration-app access or personal data.

The breakdown records the approved concrete increments without
reopening settled product choices. Large phases hide the points where a
document can be lost, a plugin result can arrive late, or a child process can
outlive its view. Each approved slice below identifies why it exists, what it
delivers, and the evidence that closes it.

Plan 1's "Sequencing" section explicitly permits local detailed planning for
Plans 2–4 before implementation. Their implementation still proceeds in order.
D49 brings the integrated browser preview forward; it does not bring these
plugins forward or waive P1-05 search performance, the GPUI idle gate, browser
acceptance or the remaining Plan 1 work. The current implementation and
outstanding evidence are recorded in [the implementation ledger](../implementation-status.md).

## Authority and common gates

The approved sources remain [Plan 2](<Syrinx v1 — Plugins and Fledge — Plan.md>),
[Plan 3](<Syrinx v1 — Notes — Plan.md>) and
[Plan 4](<Syrinx v1 — Forge — Plan.md>), beneath `docs/decisions.md`,
`docs/spec.md` and `design/README.md` in the repository's authority order.
These approved IDs identify local slices, not published issues. The plans and
their settled decisions are unchanged. The issue maps reproduce the approved
boundaries below; later changes to those boundaries require owner review.

- **Order:** every row depends on the previous row in its plan as well as
  the explicit blockers shown. Finish each phase's exit before its next phase.
  A dependency cannot be bypassed by supplying a second store, editor bridge,
  command table or search path. A blocked next slice returns to the owner.
- **Workflow:** each slice gets one local branch,
  independent review and the owner's approval before merge. Commit only on
  request. Nothing here authorizes a remote, push, upstream message or publication.
- **Verification:** each implemented slice must pass `cargo xtask check`
  under `docs/LOCAL-DEV.md`, including relevant core/search tests. Additional
  JavaScript and native-harness checks belong to slices that introduce those
  surfaces. Inspect the real Windows window after visible changes, with the
  required access permission. A source review or passing smoke check is not
  interactive acceptance. Failures remain visible; no test weakening or lint
  suppression is part of this approval.
- **Dependencies and contracts:** before adding a dependency, audit the exact
  version, resolved features, maintenance, security record and licence. Named
  candidates in the approved plans are not pins. Review required Host operations
  before implementing consumers; obtain required approval before changing a
  public contract or a pinned dependency. No GPL exception is proposed.
- **Data and execution:** use generated fixtures, disposable stores and
  synthetic projects/vaults. Agents do not inspect personal browser profiles,
  business records or vaults. No real account sign-ins, automatic service calls,
  software installation, Windows security changes or Fledge production access
  follows from these tables. Approved product actions remain explicit user
  actions when exercised, not permission for an agent to mutate this repository.
- **Measurements:** fix synthetic workloads, repeat counts and any unset
  acceptance budgets before optimization. Reuse D48's complete 100,000-row,
  5 ms p95 tier 1 requirement with the worst sample reported. Research timing
  proposals and simulated service delays are not achieved Syrinx measurements.
- **Scope:** none of these plans adds a separate analytics collector. D39's
  schema/control and slice approval remains separate. All page-web-view privacy
  and identity rules continue to apply; trusted local editor/terminal surfaces
  do not relax them for ordinary pages.

## Plan 2 — plugin runtime and Fledge demonstration

**Entry gate:** Plan 1 must deliver its generic model, command registry,
palette, storage, both search tiers, themes, shell and local gate, and pass its
GPUI idle and browser exits. This slice map is approved; the three preview
placeholder rows do not satisfy the predecessor entry gate.

The main ownership boundary is `crates/syrinx-plugin-api`: it depends on no
other Syrinx crate. Plugins depend only on that crate for Syrinx services.
`syrinx-core` hosts lifecycle and registration, `syrinx-ui` renders owned view
descriptions, `syrinx-app` wires adapters, and `syrinx-platform` owns native
surfaces. These are planned locations, not claims that the crates exist today.

### Phase 1 — contract and a test plugin

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P2-01 | A reviewed boundary prevents later plugins from depending on private application objects. Define the versioned manifest, owned serializable commands/results, errors, tab/view references, forms/lists, Host requests and bounded index batches in `syrinx-plugin-api`; review the object-safe asynchronous contract before consumers depend on it. | Serialization and compatibility tests reject unsupported versions and malformed identities. No GPUI, WebView2, SQLite or borrowed core state crosses the boundary. Blocked by the Plan 2 entry gate and any required concrete API approval. |
| P2-02 | Registration mistakes must fail before commands become visible. Add explicit built-in registration, duplicate-ID rejection, command/view ownership checks and dependency-boundary checks in `syrinx-core` and `xtask`. | A synthetic plugin uses the same contract as future built-ins. Duplicate plugins/commands, foreign command ownership and unknown views fail deterministically; no early activation occurs during manifest discovery. |
| P2-03 | A slow or failed plugin must leave browsing usable. Implement lazy activation, bounded request queues, request identity, cancellation, observed completion, timeout/uncertain-outcome states and lifecycle cleanup. | Synthetic lifecycle tests cover late completion, close/reopen, unavailable service and uncertain writes without automatic resubmission. Fix workload and queue/timeout bounds before judging behavior. No blanket panic swallowing or native-crash-isolation claim is introduced. |
| P2-04 | Plugin settings must use the shared store without exposing another plugin's namespace. Add Host-scoped settings/storage services and a synthetic service adapter through application wiring. | Tests reject cross-plugin access, prove persistence and error propagation, and exercise the test plugin through activation, commands, view requests and close/reopen. The complete Phase 1 gate passes; this trusted-native boundary is not described as a security sandbox. |

### Phase 2 — a plugin command through the real palette

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P2-05 | The shared palette must render commands without plugin-specific UI paths. Add Plugin actions scope and declarative form/list rendering, Rust-owned field state, validation, pending/error presentation and generic outcomes, using the test plugin. | Invalid fields retain input and focus the failing field. Repeated Enter cannot duplicate a pending submission; cancellation and late responses cannot update another command/view. Blocked by the complete Phase 1 exit. |
| P2-06 | Opening or closing a plugin must preserve one tab identity and the live page's focus. Wire permanent plugin rows, owned view activation, close/reopen and palette focus routing through the generic model. | Over a synthetic live page, Enter, Ctrl Enter, Tab, Shift Tab and Esc work; focus returns correctly. No duplicate Open tabs row or retained closed-view subscription exists. Inspect keyboard focus and readability in all five palettes. Phase 2 closes only with native interactive evidence. |

### Phase 3 — the Fledge demonstration end to end

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P2-07 | A deterministic stand-in proves service behavior without a real account. Add the `fledge-client` trait and session-only implementation with create/list/detail operations, opaque IDs, explicit amount/currency and submission idempotency. | Contract tests cover duplicate submission IDs, missing records, invalid input, cancellation and simulated service/write failures. Only Name is required; optional Company, Source and value follow the approved demo rules and values are labelled demo USD. Blocked by Phase 2; no REST/OAuth client is added. |
| P2-08 | Fledge must use the same Host boundary as every plugin. Add the application adapter and `plugin-fledge` command/list/detail contribution, with clear "New demo lead" and demonstration labels. | The path is plugin → Host → application adapter → `FledgeClient` → stand-in. The plugin imports no `fledge-client`, core, store or UI crate. Enter creates once; Ctrl Enter also opens the created record through the normal lifecycle. |
| P2-09 | Closing a palette must not lose unfinished typing or resubmit completed work. Complete session-only draft and submission-state behavior across Esc, reopen, success, explicit discard and restart. | Tests prove retained editable drafts, distinct pending/completed outcomes, failures that preserve input, and no duplicate write after reopen. Restart clears drafts and demo records. The owner can exercise the labelled demonstration in the normal app; Phase 3 has no production-service prerequisite. |

### Phase 4 — search, lifecycle and acceptance

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P2-10 | Both search tiers must receive plugin changes without lost updates. Connect registered commands and bounded revisioned index feeds, stable targets and per-consumer completion/cursor tracking to Plan 1's chosen stores. | Interrupt delivery, index commit and cursor checkpoint independently; replay remains idempotent with no missing or duplicate synthetic records. Tier 1 completion cannot masquerade as a durable tier 2 commit. Stale query responses remain rejected. Blocked by Phase 3. |
| P2-11 | Session-only demo data must not survive in a durable index after its source disappears. Finish demo namespace teardown, deletion/replay and view/subscription cleanup across close and restart. | A fresh session has no earlier demo records in either search tier, even after interruption/replay. Repeated open/close leaves no active subscriptions or leaked work. Do not depend on the empty new stand-in enumerating old records. |
| P2-12 | Functional success does not establish responsive daily use. Measure activation, composed palette ranking and idle cost, then complete keyboard, lifecycle and owner acceptance. | The gate and prior exits pass; D48 still passes with plugin contributions and idle work does not busy-poll. The report separates application cost from simulated service latency. Owner acceptance closes Plan 2 before Notes implementation starts. |

Real Fledge integration remains outside this map until Stage C, an authorized
current client contract and separate real-client approval. The map adds no
mail/calendar/document integration, marketplace or third-party sandbox.

## Plan 3 — Notes and the shared document service

**Entry gate:** Plans 1 and 2 pass their exits, including Plan 2 owner acceptance.
This slice map is approved. Missing Host capabilities still need review and
any required public-contract approval before implementation.

`plugin-notes` describes behavior through `syrinx-plugin-api`; host services own
files, indexing and document safety. The shared document/CodeMirror bridge is
designed for Forge reuse. The exact host-service module placement is an
implementation review, not permission to let the plugin import core or storage.

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
| P3-05 | Editor views must not change the source merely to display it. Add live preview, source editing, outline navigation and Syrinx shortcut/focus routing through the shared bridge. | Typing, paste, undo/redo and outline navigation preserve text and do not add duplicate undo events. Palette/sidebar overlays restore editor focus; unknown syntax remains editable source. Inspect the actual local editor view. No unsafe disk-save path is exposed while later safety slices remain open. |
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
| P3-14 | Table edits must not compete with the editor's live copy. Route property changes through the shared revision, save/conflict and undo service as validated byte-range patches. | Only intended bytes change; an open note immediately reflects a table edit and can undo it without restoring stale disk content. Unsafe YAML replacements fail visibly rather than normalizing the file. Phase 4 closes with supported Obsidian-to-Syrinx and Syrinx-to-Obsidian table checks. |

### Phase 5 — compatibility and acceptance

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P3-15 | Compatibility and performance claims need reproducible limits. Complete the synthetic compatibility matrix and measure index/reopen/update, note open, input paint, save acknowledgement, table filtering, renderer memory and idle cost. | Use the workloads recorded before optimization; keep D48 intact. Report failures and supported/unsupported forms explicitly. All interruption and byte-preservation tests plus the repository/editor gates pass. Blocked by Phase 4. |
| P3-16 | The owner must accept the actual Notes workflow before Forge builds on it. Complete keyboard/accessibility and five-palette review, then owner acceptance of Notes and its known limits. | Inspect the implemented UI against the design authority without inventing a detailed approved Notes board. Any personal-vault assessment is owner-operated with only pass/fail recorded. Notes acceptance and the owner's go-ahead are explicit blockers for Forge. |

This map adds no graph, WYSIWYG mode, advanced Bases formulas/relations/views,
special embeds, attachment importing or sync client.

## Plan 4 — Forge

**Entry gate:** Plans 1–3 pass their exits, the owner accepts Notes and gives
the go-ahead for Forge. This slice map is approved. Forge reuses the Host,
shortcut table, search and shared document service. Reconcile the Forge layout
with the live canvas using authorized access before UI implementation; no new
visual direction is chosen here.

`plugin-forge` contributes owned descriptions through Host; Rust services own
files, parsers, Git, language servers and processes. `syrinx-ui` renders native
chrome and `syrinx-platform` manages the trusted local editor/terminal surfaces.
The eight initially required highlighting languages remain CSS, JavaScript,
HTML, TypeScript, Svelte, Rust, GDScript and SQL.

### Phase 1 — project, editor and highlighting

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P4-01 | Project and document identity must be distinct before more than one file opens. Add one-root selection, lazy tree, bounded file opening, generic Forge lifecycle and shared document reuse. | Reopening a file does not create another buffer. Missing/binary/unsupported files fail safely. Project switch and whole-Forge close collect Save/Discard/Cancel choices before mutation; failed saves or Cancel keep work open. Blocked by the Forge entry gate. |
| P4-02 | Forge needs its agreed keyboard and navigation behavior before richer tools. Wire explicit Ctrl S/recovery, dirty indicators, code-location history, focused-file Ctrl W and the shared command table. | Ctrl K/L/T remain Syrinx actions; Ctrl W closes only the active editor file. Sidebar default/peek/pinning, address and back/forward behavior match the approved design, with reload hidden. Synthetic round trips, outside edits and bridge failures preserve work; inspect the layout and focused keys with the owner. |
| P4-03 | Highlighting cannot block typing or paint stale revisions. Audit and bundle the required grammars/queries, then implement incremental Rust Tree-sitter parsing and revisioned visible-region highlighting. | All eight required languages and mixed Svelte/HTML/script/style fixtures pass. Multiline/off-screen edits invalidate correctly; missing/failed grammar leaves plain text usable. Record large-file workload and limits before optimization. No runtime grammar download or GPL query import occurs. Phase 1 closes with the owner-reviewed editor layout and complete document-safety regressions. |

### Phase 2 — bounded interactive terminal

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P4-04 | A terminal needs proven transport and process ownership before output starts flowing. Audit the Windows pseudoterminal and xterm.js dependencies, installed shell and actual local WebView2 bridge; implement bounded input/output and resize/start/exit states. | Measure both bridge directions on synthetic data. Missing shell/startup failure is visible; no install or WSL addition occurs. Origin/message checks isolate the trusted terminal surface. Blocked by Phase 1 and exact dependency/Host reviews. |
| P4-05 | Renderer acknowledgements must bound every output queue. Implement flow control through xterm.js write completion, UTF-8 chunk handling and responsive input/control paths. | Flood tests preserve output, accept Syrinx keys and Ctrl C, and resume reads after renderer capacity returns. Resize and split Unicode sequences work. Record actual queue/throughput bounds rather than inheriting the old Tauri claim. |
| P4-06 | An ignored interrupt must not strand a task or its children. Implement explicit Stop and tracked process-tree teardown, integrating running-task Stop/Cancel with existing dirty-document close preparation. | A child that ignores Ctrl C and a renderer with blocked output both stop through the explicit path. Cancel changes no document/process state; chosen saves finish before any discard/stop. Repeated close/reopen leaves no PTY processes, readers or subscriptions. Phase 2 closes with native flood/focus/cleanup evidence. |

### Phase 3 — deliberate Git reads and writes

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P4-07 | Background repository inspection is not automatically safe or format-compatible. Audit an exact in-process `GitRead` implementation, resolved advisories/licences and configuration-triggered execution; verify supported formats. | Compare generated traditional-ref, reftable, linked-worktree and object-format fixtures against installed Git. Unsupported repositories are distinguished from empty/clean ones. A conflict with D29 returns to the owner; command-line reads do not silently replace the required in-process path. Blocked by Phase 2. |
| P4-08 | Git views must remain current while typing continues. Add cancellable status, diff, log, selected-file history and branch reads, with external-change reconciliation. | Stale snapshots cannot show a false clean state or another file's diff. Missing Git/unsupported format/error states are clear; no background fetch or arbitrary external diff/textconv/filter execution occurs. |
| P4-09 | Selected-file writes must preserve hooks and literal paths. Add staged/unstaged views, file staging/unstaging and explicit commit through installed Git, serializing mutations and revalidating selections. | Disposable-repository tests include `[draft].txt`, renames, untracked files, changed snapshots and failed hooks. Exact selected paths are literal, and failures preserve work. No hook bypass, force flag, hunk staging or agent commit authorization is implied. |
| P4-10 | Branch and network operations need an explicit conflict path. Add branch create/switch and user-triggered fetch/pull/push/merge with visible progress and editable conflict handoff. | Synthetic repositories and local remotes cover failed operations, conflicts and external changes without discarded work or automatic retries. No automatic fetch on open, history rewrite or three-way merge UI is added. Phase 3 closes with approved actions matching Git's results. |

### Phase 4 — installed language servers

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P4-11 | Starting an installed server may execute project code or fetch dependencies. Record installed versions/capabilities and audit launch/project configuration, then add bounded Rust JSON-RPC transport and lifecycle for child-process stdio and configured Godot loopback. | Synthetic projects prove no unapproved implicit execution/download. Missing/crashed/wrong-project states are visible; cancellation and Stop clean up owned children without terminating an owner-launched Godot. No server installation, Godot launch or SQL database connection is introduced. Blocked by Phase 3. |
| P4-12 | Server positions and edits must refer to the right document revision. Implement negotiated full/incremental synchronization, position encoding, request identity and stale-result rejection. | A synthetic server covers initialization, malformed/bounded messages, out-of-order responses, exit and both synchronization modes. UTF-16 defaults and negotiated alternatives convert correctly; no server is assumed to support a feature from its name alone. |
| P4-13 | Read-only language features should be useful without blocking ordinary editing. Add supported completion, diagnostics, hover, definition, references and inlay hints through the shared UI/document path. | Every feature has success, unsupported-capability and stale-response cases; installed-server checks use the named-language matrix and synthetic projects. Missing servers leave normal editing usable. Server output/text cannot become executable local UI. |
| P4-14 | Rename and formatting can change several files and cannot bypass safe saves. Apply versioned and unversioned workspace edits through validated base snapshots, shared document transactions and recoverable multi-file operations. | Dirty open files, stale/unversioned replies, position encodings and partial failures retain recoverable text. Reject unsupported resource operations before mutation and advertise only implemented handling. Server commands use supported authorized dispatch only. Phase 4 closes with installed-server and protocol evidence for the complete approved feature set. |

### Phase 5 — tasks, splits and shared context

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P4-15 | Opening a project must not execute its task discovery code. Add declarative task descriptions and safe discovery for Cargo, npm-family scripts, just/make and installed Python entry points, preserving the project's package manager. | Synthetic discovery fixtures cover duplicate names/origins, missing tools and unsafe executable declarations. Cargo discovery is offline/locked with rustup auto-install disabled for the command; `make -qp` is not assumed passive. No special uv/Poetry/tox/nox adapter or downloader is added. Blocked by Phase 4. |
| P4-16 | A selected task must run against saved source and remain stoppable. Integrate explicit task launch, progress/exit/restart with the proven terminal/process service and save-all preparation. | Any dirty-document save failure blocks launch, including a file open in another group. Synthetic providers cover run/failure/cancel, and Stop cleans children under blocked output. The user sees what will run; discovery never implies execution. |
| P4-17 | Split views must share one logical buffer and undo history. Add editor groups, per-view selections/scroll positions and coordinated shared transactions. | Two views stay synchronized through edit, undo/redo, save, outside change and close. Ctrl W closes the focused file view according to the approved policy without closing all Forge. Saves and recovery still operate on document identity, not a second copy per pane. |
| P4-18 | The context rail must use existing sources rather than creating another index. Connect revisioned outline, related Notes and selected-file Git history through Host/search, with navigation and sidebar-width behavior. | Navigation cannot show stale file context. Pinning the Syrinx sidebar folds the rail as the design specifies; missing context has honest empty/error states. Phase 5 closes with tasks, splits and rail exercised together. |

### Phase 6 — workflow, performance and closure

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P4-19 | One successful edit does not prove a complete development workflow. Run generated project workflows across editing, recovery, terminal/tasks, Git and supported language-server actions, including repeated open/close and interruptions. | Repository and JavaScript/native gates pass; no unowned child processes, lost buffers or stale search contributions remain. Cover keyboard focus and all dirty-file/running-task cancellation branches. Blocked by Phase 5. |
| P4-20 | Performance must include the rest of Syrinx, not only an isolated editor. Measure cold/warm opening, input paint, highlighting, large files, scrolling, completion, terminal floods and incremental memory/idle cost using fixed workloads/budgets. | Repeat with background browser tabs and report failures and limits. The research's frame/10 MB/million-line/100 MB proposals are not assumed passed. If Rust highlighting misses its budget, investigate and seek approval before adding a second parser location. D48 remains unchanged. |
| P4-21 | Final acceptance requires the actual Windows workflow and approved visual design. Complete five-palette, scale-factor, keyboard/accessibility and owner workflow review, then update the implementation ledger with evidence and known limits. | All phase exits, cleanup checks and measured budgets pass, and the owner accepts the workflow. No unresolved data-integrity defect, skipped gate or unverified visible behavior is labelled complete. This acceptance does not itself authorize a repository commit, merge, release or publication. |

This map adds no debugger, semantic-token implementation, minimap, native
editor, third-party marketplace, remote development, interactive rebase or
three-way merge editor. Existing terminal access does not expand those scopes.

## What approval settles and preserves

The approved map contains **12 Plan 2 slices, 16 Plan 3 slices and 21 Plan 4
slices**, retaining the plans' phase order and acceptance obligations.
This approval satisfies their separate slice-map gate. It does not declare
predecessor work complete or supply future operator, public-API, dependency,
integration-access, commit or merge permissions. The separately approved
frizbee correction and fresh synthetic Syrinx window testing have their own
narrow scopes; they do not waive these plans' remaining gates.

The next implementation remains P1-05 and the authorized D49 preview work.
The unresolved WebView2 breakpoint and failing search gates are not displaced
by this planning document. Once predecessor exits permit a later plan to begin,
its first row can be prepared with concrete dependency/contract choices for
review under the existing rules.
