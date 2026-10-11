---
title: "Atlas v1 — Forge — Plan (plan 4)"
status: active
created: 2026-10-09
subject: Atlas v1 plan 4 — the Forge code plugin (spec phase 5)
relates_to: "docs/plans/Atlas v1 — Plan.md (plan 1 and the v1-wide decisions); docs/plans/Atlas v1 — Plugins and Fledge — Plan.md; docs/plans/Atlas v1 — Notes — Plan.md; docs/research/04-forge-and-notes.md"
issues_repo: not yet created
branch: claude/plan-atlas-v1
---

# Atlas v1 — Forge

> **Status: active, approved by the owner on 2026-10-10.** Implementation is
> queued behind plans 1–3. Work uses plan 1's D41 local branch,
> independent-review and owner-approved-merge workflow; commits still need
> an owner request. The owner also approved this plan's 21-slice breakdown
> on 2026-10-10; its predecessor and acceptance gates remain in force.

## Why this exists

The owner uses PyCharm for every language and wants that development workflow
inside Atlas (`docs/spec.md`, "Forge"). Forge supplies the project tree,
editor, terminal, basic Git and installed language servers without making
Atlas start like an IDE. Its code and services load when Forge opens; normal
browsing remains the default. PyCharm is the direction, not the v1 bar (D5).

Skipping the file-safety and process-lifecycle work would make an attractive
editor that can lose changes or leave background work running after it closes.
Done means a synthetic project can be edited, run, inspected and committed
through the agreed workflow, with measured performance and recoverable failures.

## Authority and prerequisites

`docs/decisions.md`, `docs/spec.md` and `design/README.md` retain their order
of authority. Plan 1's D28–D32 settle the editor, Git transport, server
installation policy, run control and input-method scope. The older research
note supplies reasoning, not current pins or a promise of its benchmark numbers.

Implementation starts after plans 1–3 pass their exit criteria and the owner
approves this plan's local slice breakdown. It consumes plan 2's plugin/Host
contract and plan 3's shared local editor, versioned document bridge and safe-save path.
If a required host operation is missing, resolve the contract before adding
a second private route into Atlas. No `Plugin` API change happens implicitly.

Product analytics follows plan 1's D39: local aggregate feature/command
counts, timings and error categories, with explicit report export and
suggestions the owner reviews. There is no automatic upload or adaptive UI.
Account/device identifiers, URLs, queries, paths, text and detailed timelines
remain excluded, including source, terminal/server contents and recovery
drafts. This plan adds no separate analytics integration.

The live design canvas remains the visual authority. This planning pass used
the local design documentation; access to integration apps was not authorized.
⚠️ Reconcile the Forge layout against the live canvas with permission before
UI implementation. No new visual direction is decided by this plan.

## Scope (thin v1)

1. Open a project folder, browse its tree, edit files and navigate between
   code locations. CodeMirror 6 owns live typing, selection and undo; Rust
   owns files, parsing and the authority to save (D28).
2. Run a Windows shell through a pseudoterminal, with an xterm.js view and
   bounded output queues. A pseudoterminal connects a process to the visible
   terminal while preserving interactive input, colour and window resizing.
3. Inspect Git status, differences, history and branches. D29 requires the
   Git command line for writes/network operations and fast in-process reads
   behind `GitRead`. The owner approved file staging/unstaging, diff/history,
   commit, branch create/switch, fetch/pull/push and merge with conflict handoff.
4. Connect installed language servers through a Rust Language Server Protocol
   (LSP) client. The approved set is completion, diagnostics, hover, go to
   definition, find references, rename, formatting and inline type hints,
   available when the installed server supports them.
5. Discover project tasks for Cargo, npm scripts, Python, just and make (D31),
   run a selected task in the terminal, add split editor groups, and show the
   file outline, related notes and file history in the context rail.

The Forge tab hides Atlas's sidebar by default; hover still peeks and pinning
narrows the editor. Back/forward navigate code locations, reload is hidden,
and the address shows the project/file. These are existing design decisions.

On 2026-10-10 the owner confirmed explicit Ctrl S saving in Forge, saving
before a task runs, and local recovery drafts for both editors. Outside edits
must never be overwritten silently. Git uses file-level staging in v1;
hunk/line staging and a visual merge editor are deferred. Conflicts can be
resolved in the terminal or another tool without adding a three-way UI.

The owner also approved one active project folder with split editor groups.
Switching projects replaces that root after the agreed close/save checks;
it does not create a second Atlas workspace. With an editor focused, Ctrl W
closes its active file. Ctrl K always opens Atlas's palette. Closing Forge
itself uses its sidebar close button or the keyboard-accessible "Close Forge"
palette command, both reaching the same lifecycle handler.

The owner confirmed Save/Discard/Cancel for dirty files and a separate
Stop/Cancel choice for running tasks when closing Forge or switching project.
Cancel leaves Forge and its work open. Python needs standard installed entry
points plus the terminal; special uv, Poetry, tox and nox task adapters are
outside this v1 plan. These decisions and the LSP feature set were confirmed
on 2026-10-10.

### Key ownership

Use plan 1's single command table in `atlas-core`, with the focused surface
as context. The owner-approved editor exception does not change browser-tab
shortcuts. No independent JavaScript keymap may silently shadow Atlas commands.

| Key/action | Page focused | Forge editor focused | Forge terminal focused |
|---|---|---|---|
| Ctrl K | Open Atlas palette. | Open Atlas palette, never commit. | Open Atlas palette. |
| Ctrl L | Focus Atlas address/palette. | Focus Atlas address/palette. | Focus Atlas address/palette. |
| Ctrl T | Open a new browser tab. | Open a new browser tab. | Open a new browser tab. |
| Ctrl W | Close the active browser tab. | Close the active editor file, applying dirty-file checks. | Leave terminal input to its shell; it must not close all of Forge. |
| Ctrl Alt S | Toggle pinned sidebar. | Toggle pinned sidebar. | Toggle pinned sidebar. |
| Ctrl S | Existing page behavior. | Save the active document. | Preserve terminal input; file saving remains an editor command. |
| Ctrl C | Existing page behavior. | Copy the selection. | Send interrupt to the running terminal program. |
| Escape | Close the active Atlas overlay, otherwise page behavior. | Close the active overlay/completion, otherwise editor behavior. | Close an Atlas overlay when open, otherwise send terminal input. |
| Close Forge command | Available in the palette when Forge is open. | Close the whole Forge view through its lifecycle handler. | The same whole-view close handler checks running tasks. |

Back/forward in the Forge chrome and Alt Left/Right navigate code locations,
as `design/README.md` already specifies. Git commit remains a named palette
command and visible action; no replacement shortcut is invented. Focus returns
to the previously active surface when the Atlas palette closes.

## Not in v1

The debugger (D31), downloading/updating language servers (D30), interactive
rebase, a three-way merge editor, a minimap, semantic tokens and a native editor
remain out. No remote-development environment, container manager or IDE plugin
marketplace is added. Unsupported workflows remain available in the terminal.

## Boundaries and document safety

`crates/plugin-forge` depends on `atlas-plugin-api` for its Atlas-facing
contract. It contributes owned, serialisable descriptions and commands through
the Host; it does not import core, store, search or GPUI. `atlas-ui` renders
Forge's native chrome; `atlas-platform` alone creates and operates the local
WebView2 surfaces, with `atlas-app` wiring the views and services. Rust host
services handle filesystem access, indexing and child processes through the
same contract a later plugin would use. Follow plan 2 for service ownership.

Reuse plan 3's document mechanism rather than inventing a Forge save protocol.
One document identity carries a revision, saved-disk fingerprint and Rust
mirror. Local changes carry ordered revisions; stale replies never replace
newer text. A save waits for a matching revision and checksum in both copies,
then checks whether the disk changed since it was read. Hash the same canonical
text on both sides and preserve the agreed file encoding and line endings.
CRLF handling is already a known trap in `docs/trapstoknow.md`.

A clean file can reload after an outside edit; a dirty file keeps its buffer
and exposes a conflict. Permission errors, full disks, missing files and
failed replacement keep the buffer dirty with an actionable error. A bridge
resynchronisation must preserve unacknowledged edits rather than reload disk.
Unsupported encodings and binary files must not be silently converted on save.
Keep local recovery drafts separate from project files and reconcile them
with the current disk fingerprint on recovery. Clear only the exact revision
that was durably saved or explicitly discarded; an older save completion
cannot clear a newer draft. Before running a task, save every dirty document
in the active project, including files outside the current editor group.
If any save fails or conflicts, do not launch against stale disk contents.
Recovery drafts are private local data, never logs or fixtures.

Prepare a whole-Forge close or project switch before mutating lifecycle state:
collect all dirty-file and running-task choices before discarding any buffer
or stopping any process. Cancel during this preparation leaves the work
untouched. Perform chosen saves before any discard/stop; a failed save keeps
Forge open. Dirty documents offer Save/Discard/Cancel, and live tasks separately
offer Stop/Cancel. Cancelling either step abandons the entire close or switch.

Only bundled local editor and terminal assets receive inter-process communication (IPC),
the channel between JavaScript and Rust. Validate origin, message type,
document identity and revision before dispatching. Project HTML, a terminal
link or language-server text cannot become executable local UI content.
Page web views retain all of Atlas's existing no-injection rules.

## Ordered implementation

Each numbered phase depends on the previous exit. A phase will contain several
small reviewable slices when the owner approves its later breakdown; it is not
a request for one large pull request. Required fixes land before moving on.

### Phase 1 — Open, edit and highlight a project

Register Forge through the plugin host and reuse the generic tab model. Add a
lazy project tree, file opening, dirty indicators, save and code-location
history. Separate project identity from document identity so reopening a file
does not create independent copies. Keep one active project root with several
editor groups, as approved above.

Reuse plan 3's Rust document service and add incremental Tree-sitter parsing.
Return only the highlighting needed for the visible region plus a margin, labelled with
the document revision. Typing must never await Rust parsing. Text edits,
syntax changes and query context all participate in invalidation; a guessed
fixed payload size is not a correctness rule. Plain text remains usable when
a grammar is absent or parsing fails.

Language detection and grammar/query selection are data, not a separate editor
implementation for each language. On 2026-10-10 the owner named CSS,
JavaScript, HTML, TypeScript, Svelte, Rust, GDScript and SQL. These form the
initial acceptance set, not an exhaustive ceiling; the research's roughly 20
grammars are candidates. Verify mixed Svelte/HTML/script/style regions and
each selected grammar before pinning it. Compile grammars in after licence,
maintenance and security checks, with permissive or whole-file MPL queries,
no GPL queries and no runtime grammar downloader. Record the installed-server
matrix when verifying the implementation; no new product choice is needed.

**Exit:** synthetic files round-trip through save, undo/redo and reopening;
outside edits and bridge failures cannot overwrite unacknowledged changes.
Keyboard navigation works in the tree/editor. Highlighting handles multiline
edits and off-screen changes; large-file measurements identify any degraded
mode honestly. The owner reviews the implemented layout, and the key-ownership
table passes focus-sensitive tests, including Ctrl W in a split editor.

### Phase 2 — An interactive terminal that remains responsive

Use a Rust-owned pseudoterminal service, with portable-pty/ConPTY and xterm.js
as candidates to verify before adoption. Select an installed Windows shell;
do not install one or add WSL support as an incidental dependency. Handle
resize, normal exit, interrupt, startup failure and closing a running terminal.

Coalesce output and apply backpressure: stop reading when the renderer queue
is full and resume after it drains. Keep input/control messages responsive
without silently dropping output. xterm.js documents that its non-blocking
`write()` returns before parsing completes: use its completion acknowledgments
to bound every queue across the transport, not merely the Rust send queue.
Its write buffer can overflow; its throughput figures are not Atlas benchmarks
([flow-control guide](https://xtermjs.org/docs/guides/flowcontrol/)).

⚠️ The old research's Tauri raw-ArrayBuffer transport and roughly 50 MB/s
figure do not establish wry performance. wry 0.57's JavaScript-to-Rust IPC
handler takes a string request ([API](https://docs.rs/wry/0.57.0/wry/struct.WebViewBuilder.html#method.with_ipc_handler));
that signature does not establish the outbound transport or either direction's
throughput. Choose and measure both directions of the actual local WebView2
transport; no remote-debug port, external server or disabled web security is
a workaround.

Ctrl C requests an interrupt; a program may ignore it. The explicit Stop
action must independently clean up the tracked task and its child processes,
including when output is blocked. portable-pty has a Windows ConPTY backend
([source](https://raw.githubusercontent.com/wezterm/wezterm/main/pty/src/win/conpty.rs)),
but process-tree ownership and teardown require tests on this machine.

**Exit:** a synthetic output flood still accepts Ctrl C and Atlas shortcuts;
resizing and split UTF-8 sequences render correctly. Closing/reopening Forge
does not leak its PTY processes, reader threads or renderer queues. Test Stop
with a child that ignores interrupt and with a full output queue. Terminal
contents are transient UI data, never Atlas diagnostic logs or test fixtures.

### Phase 3 — Inspect and change Git state deliberately

Implement `GitRead` against an audited in-process reader and test status,
diff, log and file history on disposable local repositories. Reads must
distinguish an unsupported repository from an empty or clean one. Refresh
after both Atlas writes and external Git changes, with cancellable background
work so status scanning does not block typing.

For the approved actions, run the installed Git for Windows with
structured arguments, a known working directory and visible progress/errors.
Serialize mutations per repository; pass selected paths after the option
separator and use literal pathspecs. `--` stops option parsing but does not
stop Git interpreting wildcard characters in a selected filename; use
`git --literal-pathspecs ... -- <path>` or equivalent literal handling
([Git documentation](https://git-scm.com/docs/git#Documentation/git.txt---literal-pathspecs)).
Preserve hooks and credential helpers (D29); never silently retry with hooks
disabled, force flags or discarded local changes. Network actions require
the user's explicit action, never an automatic fetch on project open.

Pull and merge use the same failure/conflict path; never select force or
history-rewriting behavior as an automatic recovery. Show staged and unstaged
content distinctly. Refresh/revalidate the selected
snapshot before acting so a changed file is not committed under a stale diff.
Conflicts are visible and editable as text; the excluded three-way merge UI
does not imply that conflicts can be hidden. Synthetic tests use local
repositories/remotes, invented identities and hooks, never real credentials.

⚠️ Do not inherit note 04's proposed git2 dependency without review: libgit2
uses GPLv2 with a linking exception, outside the currently listed allowed
licences ([licence](https://raw.githubusercontent.com/libgit2/libgit2/main/COPYING)).
gix is a permissively licensed candidate ([manifest](https://raw.githubusercontent.com/GitoxideLabs/gitoxide/main/gix/Cargo.toml)),
and its [changelog](https://raw.githubusercontent.com/GitoxideLabs/gitoxide/main/gix/CHANGELOG.md)
shows active maintenance, not proof that a selected release meets Atlas's needs.
Its [maintainer status](https://github.com/GitoxideLabs/gitoxide/blob/main/crate-status.md)
leaves reftable backend support unchecked. Verify the exact release against
traditional refs, reftable, linked worktrees and the supported object formats.
The [security advisory list](https://github.com/GitoxideLabs/gitoxide/security/advisories)
includes GHSA-pmpw-wwcm-r8rc, a `.mailmap` parsing CPU denial of service published
2026-09-25. Reading repositories does not avoid every vulnerability. Before
pinning, check patched versions across the resolved dependency/feature graph;
this review has not verified that advisory's patched range. Also check whether
external diff, textconv or filter configuration could execute commands during
background inspection, and enforce the existing execution/network permissions.
If D29's in-process requirement cannot be met, bring a concrete alternative
to the owner; do not quietly substitute command-line reads everywhere.

**Exit:** supported actions match Git's result on clean, dirty, renamed,
untracked and conflicted files; failed hooks preserve the visible work.
Selected-path tests include a literal Windows filename such as `[draft].txt`.
Missing Git and unsupported formats show honest states. Only approved actions
appear in the UI, and external changes cannot produce a false clean state.

### Phase 4 — Installed language servers

Launch only installed, configured servers and keep their process ownership,
JSON-RPC transport and lifecycle in Rust (D30). Start/stop with the applicable
project/language lifetime, handle cancellation and timeouts, and surface a
missing/crashed server while ordinary editing continues. Bound messages and
result lists; do not stream unbounded server output into Atlas logs.

Installed does not mean passive. For example, rust-analyzer enables build
scripts, procedural macros and checks on save by default, and its default
configuration does not skip dependency fetching
([configuration](https://rust-analyzer.github.io/book/configuration.html)).
Audit each server's launch configuration and project configuration handling
before enabling it. Synthetic tests must establish that opening/saving a
project causes no implicit downloads or command execution outside the owner's
authorization. If a required feature needs broader behavior, explain the
specific tradeoff to the owner before enabling it; do not silently weaken the
existing permissions or claim that installation alone enforces them.

GDScript needs an additional transport: the installed Godot editor supplies
LSP while running the same project, normally on port 6005
([Godot docs](https://docs.godotengine.org/en/stable/tutorials/editor/external_editor.html#lsp-dap-support)).
Its protocol uses TCP ([source](https://raw.githubusercontent.com/godotengine/godot/master/modules/gdscript/language_server/gdscript_language_protocol.cpp));
support a configured loopback connection as well as child-process stdio.
Show disconnected/wrong-project states, and close the connection without
terminating an owner-launched Godot instance. This adds neither a debugger nor
automatic Godot launch/install. Verify the installed version and configured
connection during implementation; SQL editing does not authorize a database connection.

Negotiate full-document or incremental synchronization from the server's
advertised capabilities; do not send incremental updates unconditionally.
Godot's current source advertises full-document synchronization, and its
[capabilities](https://raw.githubusercontent.com/godotengine/godot/master/modules/gdscript/language_server/godot_lsp.h)
do not promise formatting or inline hints. Verify the installed version.
Synchronize document revisions and translate positions explicitly between
Rust offsets, CodeMirror and the server's negotiated encoding. LSP positions
can use different encodings; UTF-16 is the required default
([LSP position specification](https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.17/types/position.md)).
English/Latin input testing under D32 does not justify corrupting Unicode
offsets. Reject or remap stale results; validate server edits through the
document service and its existing save/conflict rules.

Deliver the approved first feature set without promising unsupported server
capabilities. Use configured feature ownership if multiple installed servers
are needed for an accepted language. A server-provided command is not a free
pass to execute arbitrary project commands; dispatch only supported operations.
Rename and formatting edits use the same versioned document transactions as
typing, including files already dirty in another group. Multi-file changes
validate their target revisions and remain recoverable on partial failure;
an LSP response cannot bypass project scope or safe-save checks.

LSP workspace edits may contain unversioned `changes` rather than versioned
`documentChanges`. Retain and validate the operation's base snapshots before
applying unversioned edits; do not assume the server supplies every revision.
Advertise only implemented resource operations and failure handling, and
reject unsupported operations before mutation. Recoverable partial disk saves
are not a guarantee of transactional atomicity
([WorkspaceEdit specification](https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.17/types/workspaceEdit.md)).

**Exit:** a synthetic protocol server tests initialization, document updates,
out-of-order replies, cancellation, exit and malformed data. Installed-server
smoke tests cover the owner's named languages on synthetic projects. Every
approved feature has success, unsupported-capability and stale-response tests;
rename/formatting cannot overwrite a newer buffer or outside file revision.
Exercise both synchronization modes, unversioned edits and multi-file failures;
verify the installed-server execution/network configuration before sign-off.

### Phase 5 — Run tasks, split editors and connect context

Read project declarations into a common task description: runner, arguments,
working directory, display name and source location. Keep task discovery
separate from execution; show what the selected task will run and use the
terminal service for output, exit status and stop/restart. Missing tools are
reported, not downloaded. Preserve the chosen project package manager.

Cargo targets can be obtained from `cargo metadata --format-version 1`;
use `--no-deps` when workspace target data is sufficient, with offline/locked
discovery, and surface unavailable metadata rather than changing a lockfile
or fetching packages implicitly
([Cargo command](https://doc.rust-lang.org/stable/cargo/commands/cargo-metadata.html)).
Cargo flags cannot prevent rustup from installing a missing toolchain before
Cargo starts. Resolve an already installed toolchain or set command-scoped
`RUSTUP_AUTO_INSTALL=0`; do not change the machine's global environment
([rustup environment variables](https://rust-lang.github.io/rustup/environment-variables.html)).
Read npm-family scripts as project data. For just and make, verify the selected
tool's discovery mechanism before executing it: `make -qp` is not a guarantee
of no execution because Makefile expansion can invoke shell commands
([GNU make](https://www.gnu.org/software/make/manual/html_node/Shell-Function.html)).
Opening a project alone must not execute its task-discovery code.

`[project.scripts]` in `pyproject.toml` defines console entry-point object
references, not arbitrary task commands. Treat them as entry points requiring
the appropriate installed project/environment; do not run the value as a shell
string ([PyPA specification](https://packaging.python.org/en/latest/specifications/pyproject-toml/#entry-points)).
The approved Python scope is these installed entry points plus ordinary
terminal commands. No special uv, Poetry, tox or nox adapter is required, and
no unnamed "common" task table is advertised as supported.

Add split groups after a single document works reliably. CodeMirror views do
not synchronize themselves; forward transactions with one logical undo history
([official example](https://codemirror.net/examples/split/)). Different panes
may have distinct selections/scroll positions while sharing text and dirty
state. The context rail consumes Rust outline results, Notes links and Git
history through the shared host/index; it does not build a second notes index.

**Exit:** every supported task provider has synthetic discovery/run/failure
fixtures; duplicate names retain their origin and cancellation cleans up.
Running waits until all dirty project documents are saved, and stops on any
save failure; editing an imported file in another group is part of the test.
Two views of one file stay in sync through edit/undo/save/outside change.
The rail follows navigation without showing stale file context, and sidebar
pinning folds it according to the existing design.

### Phase 6 — Daily workflow, performance and closure

Run the workspace gate from `docs/LOCAL-DEV.md` with D38's release-style
profile, plus the editor's JavaScript checks. Exercise the complete approved
workflow on this Windows machine and inspect the real window at the supported
scale factors and all five palettes. Use English/Latin keyboard tests (D32),
including accented text, punctuation, selection, undo and clipboard operations.

Measure cold/warm Forge opening, input-to-paint, highlighting, large-file open,
scrolling, completion after server reply, terminal flood responsiveness and
incremental memory/idle CPU. Include repeated open/close and background
browser tabs so a feature cannot pass by making the rest of Atlas stall.

⚠️ Research targets remain proposals: p95 input/highlight within roughly one
60 Hz frame, a 10 MB file open within 300 ms, responsive million-line scrolling
and a 100 MB terminal flood that still accepts interrupt. They are not measured
capabilities. Document the synthetic fixture, sample count and acceptance budget
before optimizing; report failures, hardware/runtime versions and measurement
limits. If the Rust highlight round trip fails, investigate first and ask
before adopting web-tree-sitter as a second parser location under D28.

**Exit:** core document/search tests and the gate pass, processes/resources
are released, the owner completes the accepted workflow, and measured results
and remaining limits are recorded locally. No skipped gate, silent loss of
edits or unresolved data-integrity defect is an acceptable performance trade.

## Implementation checks still open

The product questions above are answered. The following are verification work
within the approved scope, not another round of product decisions.

| Check | Evidence required before calling its phase complete |
|---|---|
| Installed tooling | Record installed shell/server versions, executable paths and Godot loopback settings. Verify synchronization/capabilities and launch configuration on synthetic projects, including project-code execution and network behavior, without inspecting personal projects or downloading tools. |
| Editor and terminal bridge | Measure both directions of the actual wry/WebView2 transport, bounded queues, focus routing and Stop cleanup under blocked output; Tauri measurements are insufficient evidence. |
| Git reader | Check the exact release's licence, maintenance, advisory fixes, dependency/features and repository-format support against Git before selecting it. A conflict with D29 goes to the owner, not a silent workaround. |
| Performance corpus | Fix the generated file/project sizes, cold/warm conditions and repeat count before measuring the proposed budgets; document results and limitations on this machine. |
| Live design | Compare with the authoritative canvas when access is authorized; the local snapshot alone cannot prove current visual agreement. |

## Evidence and issue map

The linked primary sources were checked on 2026-10-10 for the particular
claims beside them. Dependency candidates and their exact versions still need
maintenance, security and licence checks immediately before implementation;
this plan adds no dependency. Actual WebView2 editor/terminal throughput,
the selected Git reader's format coverage and the live-canvas comparison
remain explicitly unverified.

### Verification and iteration log

| Date | Review | Result and remaining gate |
|---|---|---|
| 2026-10-10 | Expanded the outline against the owner-approved scope and shared plans 2–3. | Recorded the language set, save/recovery and close policy, Git breadth, project model, keys and LSP features. Implementation remains after plans 1–3. |
| 2026-10-10 | Readiness review against the linked primary sources. | Corrected literal Git paths, rustup discovery, LSP synchronization/unversioned edits, server execution/network assumptions and terminal acknowledgments/Stop cleanup. Documented gix's reference-backend and security gates without selecting a dependency or changing D29. |
| 2026-10-10 | The owner approved all four plans and plan 1's 40-slice breakdown. | Marked Forge active and queued behind plans 1–3. D41 permits local branches, independent review and owner-approved merges; this plan's slice breakdown still needs approval and commits remain on request. D39 permits local aggregate analytics and explicit report export with owner-reviewed suggestions; project and recovery content remain excluded. |
| 2026-10-10 | The owner subsequently approved Forge's exact 21-slice map and dependencies. | The issue map below now records those approved boundaries. Plans 1–3 exits, Notes acceptance and the owner's go-ahead for Forge still precede implementation. Concrete dependency/public-API and commit/merge permissions remain separate. |

These checks establish the source-backed plan corrections, not runtime test
results. The selected Git reader, language grammars, installed servers and
Windows editor/terminal integration still require their phase-specific tests.

### Issue map

The owner approved these 21 local slices on 2026-10-10, exactly as
recorded in [the approved Plans 2–4 breakdown](<Atlas v1 — Plans 2–4 slice proposals.md>).
The boundaries, deliverables and explicit blockers below are unchanged from
that approval. Every slice also depends on the preceding slice in this plan;
finish each phase exit before its next phase. The breakdown's common gates
apply to every row. All rows are queued, not implemented.

**Entry gate:** Plans 1–3 pass their exits, the owner accepts Notes and gives
the go-ahead for Forge. This slice map is approved. Forge reuses the Host,
shortcut table, search and shared document service. Reconcile the Forge layout
with the live canvas using authorized access before UI implementation; no new
visual direction is chosen here.

D41 supplies one local branch and independent review per slice, with owner
approval before every merge. Commit only on request. No remote, push, public
issue or upstream message is authorized. Concrete dependency and public-API
approvals remain separate; the map does not waive predecessor or operator gates.

#### Phase 1 — project, editor and highlighting

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P4-01 | Project and document identity must be distinct before more than one file opens. Add one-root selection, lazy tree, bounded file opening, generic Forge lifecycle and shared document reuse. | Reopening a file does not create another buffer. Missing/binary/unsupported files fail safely. Project switch and whole-Forge close collect Save/Discard/Cancel choices before mutation; failed saves or Cancel keep work open. Blocked by the Forge entry gate. |
| P4-02 | Forge needs its agreed keyboard and navigation behavior before richer tools. Wire explicit Ctrl S/recovery, dirty indicators, code-location history, focused-file Ctrl W and the shared command table. | Ctrl K/L/T remain Atlas actions; Ctrl W closes only the active editor file. Sidebar default/peek/pinning, address and back/forward behavior match the approved design, with reload hidden. Synthetic round trips, outside edits and bridge failures preserve work; inspect the layout and focused keys with the owner. |
| P4-03 | Highlighting cannot block typing or paint stale revisions. Audit and bundle the required grammars/queries, then implement incremental Rust Tree-sitter parsing and revisioned visible-region highlighting. | All eight required languages and mixed Svelte/HTML/script/style fixtures pass. Multiline/off-screen edits invalidate correctly; missing/failed grammar leaves plain text usable. Record large-file workload and limits before optimization. No runtime grammar download or GPL query import occurs. Phase 1 closes with the owner-reviewed editor layout and complete document-safety regressions. |

#### Phase 2 — bounded interactive terminal

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P4-04 | A terminal needs proven transport and process ownership before output starts flowing. Audit the Windows pseudoterminal and xterm.js dependencies, installed shell and actual local WebView2 bridge; implement bounded input/output and resize/start/exit states. | Measure both bridge directions on synthetic data. Missing shell/startup failure is visible; no install or WSL addition occurs. Origin/message checks isolate the trusted terminal surface. Blocked by Phase 1 and exact dependency/Host reviews. |
| P4-05 | Renderer acknowledgements must bound every output queue. Implement flow control through xterm.js write completion, UTF-8 chunk handling and responsive input/control paths. | Flood tests preserve output, accept Atlas keys and Ctrl C, and resume reads after renderer capacity returns. Resize and split Unicode sequences work. Record actual queue/throughput bounds rather than inheriting the old Tauri claim. |
| P4-06 | An ignored interrupt must not strand a task or its children. Implement explicit Stop and tracked process-tree teardown, integrating running-task Stop/Cancel with existing dirty-document close preparation. | A child that ignores Ctrl C and a renderer with blocked output both stop through the explicit path. Cancel changes no document/process state; chosen saves finish before any discard/stop. Repeated close/reopen leaves no PTY processes, readers or subscriptions. Phase 2 closes with native flood/focus/cleanup evidence. |

#### Phase 3 — deliberate Git reads and writes

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P4-07 | Background repository inspection is not automatically safe or format-compatible. Audit an exact in-process `GitRead` implementation, resolved advisories/licences and configuration-triggered execution; verify supported formats. | Compare generated traditional-ref, reftable, linked-worktree and object-format fixtures against installed Git. Unsupported repositories are distinguished from empty/clean ones. A conflict with D29 returns to the owner; command-line reads do not silently replace the required in-process path. Blocked by Phase 2. |
| P4-08 | Git views must remain current while typing continues. Add cancellable status, diff, log, selected-file history and branch reads, with external-change reconciliation. | Stale snapshots cannot show a false clean state or another file's diff. Missing Git/unsupported format/error states are clear; no background fetch or arbitrary external diff/textconv/filter execution occurs. |
| P4-09 | Selected-file writes must preserve hooks and literal paths. Add staged/unstaged views, file staging/unstaging and explicit commit through installed Git, serializing mutations and revalidating selections. | Disposable-repository tests include `[draft].txt`, renames, untracked files, changed snapshots and failed hooks. Exact selected paths are literal, and failures preserve work. No hook bypass, force flag, hunk staging or agent commit authorization is implied. |
| P4-10 | Branch and network operations need an explicit conflict path. Add branch create/switch and user-triggered fetch/pull/push/merge with visible progress and editable conflict handoff. | Synthetic repositories and local remotes cover failed operations, conflicts and external changes without discarded work or automatic retries. No automatic fetch on open, history rewrite or three-way merge UI is added. Phase 3 closes with approved actions matching Git's results. |

#### Phase 4 — installed language servers

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P4-11 | Starting an installed server may execute project code or fetch dependencies. Record installed versions/capabilities and audit launch/project configuration, then add bounded Rust JSON-RPC transport and lifecycle for child-process stdio and configured Godot loopback. | Synthetic projects prove no unapproved implicit execution/download. Missing/crashed/wrong-project states are visible; cancellation and Stop clean up owned children without terminating an owner-launched Godot. No server installation, Godot launch or SQL database connection is introduced. Blocked by Phase 3. |
| P4-12 | Server positions and edits must refer to the right document revision. Implement negotiated full/incremental synchronization, position encoding, request identity and stale-result rejection. | A synthetic server covers initialization, malformed/bounded messages, out-of-order responses, exit and both synchronization modes. UTF-16 defaults and negotiated alternatives convert correctly; no server is assumed to support a feature from its name alone. |
| P4-13 | Read-only language features should be useful without blocking ordinary editing. Add supported completion, diagnostics, hover, definition, references and inlay hints through the shared UI/document path. | Every feature has success, unsupported-capability and stale-response cases; installed-server checks use the named-language matrix and synthetic projects. Missing servers leave normal editing usable. Server output/text cannot become executable local UI. |
| P4-14 | Rename and formatting can change several files and cannot bypass safe saves. Apply versioned and unversioned workspace edits through validated base snapshots, shared document transactions and recoverable multi-file operations. | Dirty open files, stale/unversioned replies, position encodings and partial failures retain recoverable text. Reject unsupported resource operations before mutation and advertise only implemented handling. Server commands use supported authorized dispatch only. Phase 4 closes with installed-server and protocol evidence for the complete approved feature set. |

#### Phase 5 — tasks, splits and shared context

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P4-15 | Opening a project must not execute its task discovery code. Add declarative task descriptions and safe discovery for Cargo, npm-family scripts, just/make and installed Python entry points, preserving the project's package manager. | Synthetic discovery fixtures cover duplicate names/origins, missing tools and unsafe executable declarations. Cargo discovery is offline/locked with rustup auto-install disabled for the command; `make -qp` is not assumed passive. No special uv/Poetry/tox/nox adapter or downloader is added. Blocked by Phase 4. |
| P4-16 | A selected task must run against saved source and remain stoppable. Integrate explicit task launch, progress/exit/restart with the proven terminal/process service and save-all preparation. | Any dirty-document save failure blocks launch, including a file open in another group. Synthetic providers cover run/failure/cancel, and Stop cleans children under blocked output. The user sees what will run; discovery never implies execution. |
| P4-17 | Split views must share one logical buffer and undo history. Add editor groups, per-view selections/scroll positions and coordinated shared transactions. | Two views stay synchronized through edit, undo/redo, save, outside change and close. Ctrl W closes the focused file view according to the approved policy without closing all Forge. Saves and recovery still operate on document identity, not a second copy per pane. |
| P4-18 | The context rail must use existing sources rather than creating another index. Connect revisioned outline, related Notes and selected-file Git history through Host/search, with navigation and sidebar-width behavior. | Navigation cannot show stale file context. Pinning the Atlas sidebar folds the rail as the design specifies; missing context has honest empty/error states. Phase 5 closes with tasks, splits and rail exercised together. |

#### Phase 6 — workflow, performance and closure

| Approved ID | Why and deliverable | Done evidence and explicit blockers |
| --- | --- | --- |
| P4-19 | One successful edit does not prove a complete development workflow. Run generated project workflows across editing, recovery, terminal/tasks, Git and supported language-server actions, including repeated open/close and interruptions. | Repository and JavaScript/native gates pass; no unowned child processes, lost buffers or stale search contributions remain. Cover keyboard focus and all dirty-file/running-task cancellation branches. Blocked by Phase 5. |
| P4-20 | Performance must include the rest of Atlas, not only an isolated editor. Measure cold/warm opening, input paint, highlighting, large files, scrolling, completion, terminal floods and incremental memory/idle cost using fixed workloads/budgets. | Repeat with background browser tabs and report failures and limits. The research's frame/10 MB/million-line/100 MB proposals are not assumed passed. If Rust highlighting misses its budget, investigate and seek approval before adding a second parser location. D48 remains unchanged. |
| P4-21 | Final acceptance requires the actual Windows workflow and approved visual design. Complete five-palette, scale-factor, keyboard/accessibility and owner workflow review, then update the implementation ledger with evidence and known limits. | All phase exits, cleanup checks and measured budgets pass, and the owner accepts the workflow. No unresolved data-integrity defect, skipped gate or unverified visible behavior is labelled complete. This acceptance does not itself authorize a repository commit, merge, release or publication. |

This map adds no debugger, semantic-token implementation, minimap, native
editor, third-party marketplace, remote development, interactive rebase or
three-way merge editor. Existing terminal access does not expand those scopes.
