---
title: "Atlas v1 — Readiness and implementation breakdown"
status: approved
created: 2026-10-10
subject: Approved readiness review and 40-slice local breakdown, with a proposed analytics amendment
relates_to: "Atlas v1 — Plan.md; Atlas v1 — Plugins and Fledge — Plan.md; Atlas v1 — Notes — Plan.md; Atlas v1 — Forge — Plan.md"
---

# Readiness before implementation

The owner approved all four plans, the 40-slice plan 1 breakdown and the
local implementation/review workflow on 2026-10-10 (D41). They also kept
WebView2, chose local counters with explicit export and reviewed model
suggestions (D39), and excluded page text from the seven daily backups (D40).
The plans are active; implementation is not complete. The owner subsequently
approved D43–D45 and a revised map with the same 40 IDs/order: inline saved
pages and folders, reversible sidebar width dragging, and separate Favorites
for supported Atlas destinations. Those decisions are requirements for the
named slices below, not permission to start later phases early.

The approval retains the plans' explicit evidence gates. A gate means
the relevant feature cannot be built on an unproved assumption; it does not
mean every later feature must be prototyped before the scaffold. The first
implementation was plan 1's scaffold. The owner-requested documentation
baseline is `d79fc29`. P1-01 passed its gate and independent review and was
merged with owner approval as `ed19d6d` into `claude/plan-atlas-v1`.
P1-02 is in progress on `codex/p1-02-tab-model`; its validation/review are
pending. No remote, issue or pull request has been created. Analytics still
requires the proposed amendment below before collection is implemented.

## What this review established

The review read the repository rules, owner decisions, spec, design snapshot,
four plans and the Phase 0 verdict. Three independent readers checked the
Windows embedding, plugin/Notes and Forge claims against primary sources.
The exact spike manifests and cached dependency source were inspected without
opening test browser profiles, raw measurement captures or personal files.
Phase 0 numbers below are reported evidence, not new measurements.

The machine currently reports Windows `10.0.26300.0`, Rust `1.97.1`,
Cargo `1.97.1`, Node `24.15.0`, pnpm `10.34.4` and Git for Windows
`2.55.0.windows.2`. P1-01 now provides the product `Cargo.toml` and
`docs/LOCAL-DEV.md`; the working branch is `codex/p1-02-tab-model`, with no remote.

“Verified” below means the cited source supports that particular API or
fact. It does not certify Atlas's future implementation. “Conditional”
requires a runtime, compatibility or performance test. ⚠️ marks evidence
not yet available. Owner choices are requirements, not facts to re-decide.

## Plan 1 evidence and corrections

| Claim group | Evidence and disposition | Gate / consequence |
|---|---|---|
| D2–D4: Windows, WebView2 and GPUI | Verified as owner decisions and a conditional Phase 0 go in `docs/research/09-phase-0-verdict.md`. The source available to this review is the retained spike, not a product implementation. | Keep the stack. No engine replacement is proposed. |
| Exact GPUI/wry baseline | `spike/gpui:spike-gpui/Cargo.toml` pins `gpui-pre =0.3.8`, `gpui-pre-platform =0.3.8`, `gpui-component =0.7.1`, `wry =0.57.0`, `webview2-com =0.39.1`, direct `windows =0.62.2`. | Carry the baseline forward only after the dependency audit; do not move pins as part of scaffolding. |
| D37: idle repaint and measurement | Cached `gpui-pre-windows-0.3.8/src/platform.rs` invalidates windows each vsync. The verdict records 3.75% of one core and the 0.5% target. ⚠️ No fixed-patch result exists. | Phase 1 must measure the patch. Repeat the same start-to-first-page endpoint in both throwaway candidates; previous endpoints differed. |
| D38: executable policy | Release builds ran in the spike. This is an observation, not a guarantee that Cargo profile names bypass Windows policy. [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html) independently control optimization, assertions and overflow checks. | Scaffold must run its app and tests with the chosen profile and unchanged Windows settings. Preserve test checks. |
| Shared environment, page isolation and D11 | Cached wry source supports supplying the environment and confirms that an installed IPC handler injects `window.ipc`. Its default environment arguments disable SmartScreen. [Password autosave](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2settings4) is separate from general autofill. | Set environment options before supplying it; keep distinct page/editor constructors. Explicitly disable password saving; this does not erase saved passwords. |
| Popup adoption and overlays | [New-window adoption](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2newwindowrequestedeventargs) requires the same environment/profile and an unnavigated target. The spike demonstrated popup ownership/focus. Visible Acrylic did not follow from successful API calls. | Retest product focus, stacking, main-window close, minimize and restore. Own the popup, not the child page. Blur remains optional under D7. |
| Shortcut dispatch | [AcceleratorKeyPressed](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller#add_acceleratorkeypressed) supplies Ctrl/Alt and non-character keys. It is synchronous for windowed controls. | Mark handled promptly, queue application work, avoid reentrant cross-process calls, filter repeats and test from a focused page. |
| Suspension and process recovery | [TrySuspend](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_3) is conditional and needs a hidden view. [Process failure](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-related-events) distinguishes reloadable renderer loss from browser-process loss affecting all its views. | Test failed/late suspend callbacks and both crash paths. D28's “one renderer” is an aim: the [runtime controls process sharing](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/process-model). |
| Built-in browser facilities | Find/print accelerators, [download-dialog APIs](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_9), [notification UI](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2notificationreceivedeventargs) and [persisted permissions](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2permissionrequestedeventargs3) exist. | Runtime tests remain necessary. Route Ctrl J explicitly. Include research note 02's omitted Musts: zoom, HTTPS/certificate handling and clear browsing data. |
| Zoom and certificates | Pinned wry disables zoom hotkeys by default; use `with_hotkeys_zoom(true)`. The native [controller](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller) supports zoom. Default [certificate handling](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_14) preserves the TLS warning. ⚠️ No complete successful-page security-state API was established. | Test focused-page Ctrl plus/minus/0. Do not promise per-site zoom persistence. Use a limited scheme/error indicator unless stronger evidence exists; never label an HTTPS URL alone “secure” or set `ALWAYS_ALLOW`. |
| Clear browsing data | [Profile2](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2profile2) supports kinds and Unix-time intervals `[start,end)` across the profile. Aborted operations can be partial; closing the view can suppress completion. | Keep the controller alive, await completion and clear matching Atlas copies. No user-data-folder deletion. Define deletion-journal or snapshot treatment before recovery design is finalized so clearing survives older snapshots. |
| D13: SQLite durability and snapshots | [WAL](https://www.sqlite.org/wal.html) needs same-machine coordination. [NORMAL synchronization](https://www.sqlite.org/pragma.html#pragma_synchronous) does not guarantee recent commits survive power/OS failure. [VACUUM INTO](https://www.sqlite.org/lang_vacuum.html) creates a snapshot, but interrupted output can be invalid. | The plan now promises the last committed session after application crash, not zero loss. Validate a completed snapshot before rotating seven copies; test only disposable stores. |
| Tier 1 speed | [frizbee](https://github.com/saghen/frizbee) supplies fuzzy matching; its microbenchmarks do not measure Atlas's ranking, boosts, counts and highlights. ⚠️ The 5 ms / 100,000-row result is unmeasured. | Benchmark the full path and record distributions, corpus and machine conditions. No personal history corpus. |
| Tier 2 choice and D18/D35 | [SQLite FTS5](https://www.sqlite.org/fts5.html) and [Tantivy](https://docs.rs/tantivy/latest/tantivy/) are candidates. ⚠️ Neither a winning benchmark nor an approved extraction method exists. | Phase 3 bake-off measures build/query/update/expiry/recovery on one synthetic corpus. Verify eligibility before extraction, including redirects, frames and `no-store`; do not add scripts/filters to excluded sites. |
| D6/D21/D22/D36: sites and future runtime | Phase 0 records passkey, Workspace, YouTube and Disney+ passing. ⚠️ Password plus phone prompt, persistence after restart, Hulu and 13 other daily sites remain owner-operated checks. Future Google behavior is undocumented. | Keep the Phase 2 exit gate and D22 stop-for-owner rule. A runtime-version notice cannot guarantee future compatibility. Never automate real sign-in. |
| Extensions and runtime availability | Current WebView2 has optional [extension APIs](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2environmentoptions6). Microsoft also recommends [checking Runtime availability](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution), even on Windows 11. | No extensions remains the approved Atlas scope. Correct the old impossibility rationale without adding support. Missing runtime needs an explanation, not an automatic install. |
| Privacy promise | [Microsoft's privacy documentation](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/data-privacy) describes required diagnostics outside Atlas's control and SmartScreen collection. The custom-crash flag is narrower. | The owner kept the engine and SmartScreen (D39). Disclose runtime collection; crash dumps stay local. Atlas's own analytics is limited to local counters and explicit report export. |
| Design, accessibility and shipping | Local `design/canvas/project/Main.dc.html` contains five palette tokens. ⚠️ The live private canvas, NVDA behavior, actual contrast, updater host/settings and unsigned installer viability are unverified. | Tokens: owner-authorized canvas comparison. Phase 5: accessibility and shipping gates. Update-file signatures do not confer Windows signing/reputation. |

The decision table is retained as history. Its selected outcomes stand.
⚠️ Broad historical comparisons in rejected alternatives—Servo/CEF/Gecko
capabilities, service resolution caps, Zed development timelines, Git 3's
future default and third-party API deployment state—are not current verified
facts. No implementation should depend on them. D21's Workspace warning does
not establish the owner's admin configuration. D24–D26's real Fledge facts
come from the owner and await the later Stage C contract; this review did
not inspect Fledge. D14's selected HTML format is the scope; its categorical
claim about every browser's export support is not a supported universal claim.

S1–S10 and D1, D5, D7–D20, D23–D34 primarily record owner scope and workflow.
They were cross-checked against `docs/decisions.md`, `docs/spec.md`,
`design/README.md` and the accepted answers, not reopened. Their embedded
technical caveats are covered here or in the sibling verification tables.

## Plans 2–4 evidence and corrections

Each sibling plan contains its own primary-source verification table. This
table records the cross-plan findings and the first point where they matter.

| Plan | Finding | Required evidence before building on it |
|---|---|---|
| 2: native plugins | Host namespace checks are enforceable API rules, but native Rust plugins are not sandboxed. The plan now distinguishes architecture from security isolation. Dynamic trait signatures, panic behavior and cooperative cancellation need actual tests. | Phase 1 compiles object-safe signatures and checks the dependency direction. Never claim a timeout can interrupt non-yielding code or contain a native abort. |
| 2: Fledge adapter and feed | The plugin-to-Host-to-app-adapter-to-`FledgeClient` route satisfies the crate boundary. Demo state and search entries expire at restart. Feed checkpoints need actual durable index commit/replay boundaries. | Synthetic error/cancellation tests, independent consumer checkpoint tests and crash-position tests; no real API or authentication. |
| 3: safe save shared with Forge | Equal editor/Rust checksums do not prove unchanged disk. Fingerprint-then-replace has a race, and Windows replacement has partial-failure states. CodeMirror's separator setting alone cannot preserve mixed original line endings. | Phase 2 proves byte preservation, offset conversion, outside-write races, flushing/recovery and draft revision cleanup before ordinary saves. Forge reuses the result. |
| 3: Obsidian compatibility | `.base` folder/tag scope is a filter, and an Atlas notes table needs an explicit Markdown-file filter. `order` means columns. Markdown itself is permissive; malformed frontmatter and unsupported encodings are distinct errors. | ⚠️ Disposable-vault fixtures establish sort serialization, property-type metadata and duplicate-name wikilink resolution. Unsupported syntax stays unchanged. |
| 4: Git | `--` does not disable pathspec wildcards; use literal selected paths. A gix candidate needs exact-version format/security checks, including the maintainer's `.mailmap` advisory. libgit2's GPL-plus-exception licence is not silently permitted. | Phase 3 tests disposable repositories, literal `[draft].txt`, hooks and failures. Verify traditional refs, reftable and linked worktrees. Unsupported cannot look clean. |
| 4: installed tooling | Installed language servers may execute project code or download dependencies. rustup can install before Cargo sees `--offline`. GDScript connects to a running Godot project over local TCP, not only child-process stdio. | Phase 4 checks server launch settings, ownership, capabilities and sync. Phase 5 uses installed-toolchain discovery or command-scoped `RUSTUP_AUTO_INSTALL=0`. No autonomous install/network shortcut. |
| 4: editing and terminal | Language Server Protocol edits can be unversioned; retain request snapshots. xterm accepts writes before parsing finishes; backpressure must span the queues. Ctrl C acceptance does not prove termination. | Reject stale/out-of-root edits; advertise only implemented transaction support. Prove split-buffer consistency, output-flood responsiveness and explicit Stop cleanup on Windows. |

The unresolved details are bounded technical gates, not omitted product
scope. All named owner answers about vaults, saving, attachments, Bases,
Forge languages/Git/shortcuts and the normal-app Fledge demo are recorded.
Exact dependency licences, advisories and compatibility must be checked again
when the actual versions are selected; this source review is not that audit.

## Questions and approvals

| ID | Question and recommendation | Status / blocks |
|---|---|---|
| Q1 | Keep the engine after reviewing runtime diagnostics; clarify Atlas's analytics policy. | Answered 2026-10-10 (D39): keep WebView2/SmartScreen, and add local counters with explicit export and owner-reviewed model suggestions. The earlier blanket no-analytics recommendation was not accepted. No automatic uploads or adaptive UI. |
| Q2 | Apply seven-day retention to Atlas-created page-text copies, including backups. | Approved 2026-10-10 (D40): separate expiring page-text cache/index, excluded from the seven daily durable-state snapshots. This is an application retention policy, not forensic erasure or control of owner-managed backups. |
| Q3 | Replace D16's GitHub prerequisite with local branches, independent review and owner-approved merges. | Approved 2026-10-10 (D41). Commits still require the owner's request; preserve the planning baseline first. No remote or push authorized. |
| Q4 | Approve the corrected four plans and the 40-slice plan 1 breakdown. | Approved 2026-10-10 (D41). All four plans are active, and the table is recorded in plan 1's Issue map. The new analytics amendment remains separately proposed. |

The licence blocks public release, not this local planning. The owner still
controls spike-worktree/profile deletion; none is requested or performed
here. The idle-patch submission, design-canvas access, real-account tests,
updater network behavior and Windows registration each retain their own
later gate. They are not bundled into readiness approval.

## Approved local review workflow

D41 explicitly replaces D16's GitHub prerequisite. Use one local `codex/`
branch per approved row, a diff small enough to review in
one sitting, an independent review, then the owner's approval before merge.
Commits still require the owner to ask; approving this workflow alone does
not authorize automatic commits. The owner requested the documentation
baseline, preserved as `d79fc29` before creating `codex/p1-01-scaffold`.
After the gate and independent review passed, the owner explicitly approved
P1-01's commit and local merge into `claude/plan-atlas-v1`. That approval does
not authorize later commits or merges. No remote, push, hosted issue or PR is
part of this exception.

`CLAUDE.md` and the workflow references in `AGENTS.md` now record the approved
exception while preserving “no remote, no pushes.” The scaffold records
its concrete development commands and gate results in `docs/LOCAL-DEV.md`.
Once the owner creates GitHub, each row becomes one issue and one PR under
its phase milestone; the owner still approves every merge. A materially
larger slice is split and re-shown before work, rather than stretching the
approved row into an unreviewable change.

## Approved plan 1 breakdown

These are approved local review labels, **not published GitHub issues**.
The table is also recorded in plan 1's Issue map. Work runs in numeric order,
one row at a time. Q1–Q4 referenced in the table are resolved above. “Blocked by” names the
immediate preceding row plus any external gate; earlier dependencies are
transitive. Each phase is a future milestone. Every code row includes the
applicable gate, core/search tests and independent review. Visible changes
also require running and inspecting the Windows app on synthetic data.

The owner-approved D43–D45 amendment preserves all 40 IDs and their order.
P1-02 supplies a small flat saved-entry lifecycle; P1-03 persists it and
P1-09 renders it without temporary pin semantics. Full folders and HTML
import stay in P1-21/P1-22. The 20-entry history includes all closed web
instances, including saved-page instances, but excludes plugins. Reopen
focuses an existing live instance or loads the fixed destination of a
surviving saved row. If that row was removed, history may reopen an ordinary
page but never recreates the saved record. Sidebar width states and
Favorites enter only in their named UI slices. Exact drag thresholds and
compact-folder affordances remain later owner-review details. The screenshot
is direction only; no personal image is copied into this repository.

### Phase 1 milestone: foundations

| ID | Why this slice exists; deliverable and done condition | Main files / decisions | Blocked by |
|---|---|---|---|
| P1-01 | A reproducible local build prevents later work resting on an unusable toolchain. Create the minimal Cargo workspace, dependency-boundary check, passing test and runnable app; document and execute the D38 gate. Audit maintenance, security and licences for the resolved graph before adding dependencies, here and in every later slice. Record the approved local workflow. Do not define the future Plugin trait early. | `Cargo.toml`, `crates/xtask`, `docs/LOCAL-DEV.md`, `CLAUDE.md`, `AGENTS.md`; D16/D38. | Q3/Q4, a preserved planning baseline and the dependency pre-add audit. |
| P1-02 | One model prevents sidebar/palette disagreement and replacing temporary pins later. Implement live tab identity/kinds, a flat saved-page lifecycle with independent saved identity and live association, selection/order, save/unpin, unload/open-or-focus and a 20-entry closure stack for ordinary and saved web instances. Test D42/D43 without a window; folders and plugin registration remain later work. | `crates/atlas-core`; S3, D42/D43. | P1-01. |
| P1-03 | Saved destinations must survive independently of live pages. Open a local unsynced store, migrate it and persist/reload flat saved entries plus live-tab snapshots and their associations through a serialized writer. Prove rollback, unloaded-entry preservation and errors on disposable databases. | `crates/atlas-store`, core persistence boundary, filesystem-only `atlas-platform` directory guard; D13/D43. | P1-02; Q2 for durable/cache separation. |
| P1-04 | A bad backup must not replace a good one. Add idle daily snapshots, seven-copy rotation, clean-shutdown checking and a tested recovery path; interruption leaves the last valid copy usable. Establish how later explicit history clearing survives restoring older snapshots. | `crates/atlas-store`; D13. | P1-03. |
| P1-05 | The palette needs fast local ranking before UI work. Add the tier 1 catalogue, boosts, counts and match positions with incremental updates and a reproducible 100,000-row benchmark. | `crates/atlas-search`, `crates/xtask`; D9. | P1-04. |
| P1-06 | Continuous repainting defeats the performance goal. Implement the small pinned GPUI idle patch, add focused regression evidence and remeasure eight suspended tabs and equivalent first-page startup in both throwaway candidates using fresh synthetic profiles. Prepare a reviewable upstream patch. | Pinned GPUI patch and measurement report; D37/D12. | P1-05; passing measurements. Public offer requires separate owner approval. |
| P1-07 | Shared tokens and width states keep the shell consistent. Render the five palettes, accent and transparency fallback with wide, favicon-only, hidden and peek sidebar samples. Review D44 drag thresholds with the owner and inspect contrast against the authorized design direction. | `crates/atlas-ui`; S10/D44, `design/README.md`. | P1-06; live-canvas comparison or owner-approved snapshot, with D43–D45 amendments. |

### Phase 2 milestone: browser shell

| ID | Why this slice exists; deliverable and done condition | Main files / decisions | Blocked by |
|---|---|---|---|
| P1-08 | Browsing needs a safe engine boundary that follows sidebar size. Create the shared environment and one page in a GPUI card; wire smooth drag transitions through wide, favicon-only and hidden states, including reverse drag, resize and DPI changes. Verify page guardrails, runtime detection and approved privacy disclosure; retain native permission prompts and disable clipboard auto-grant. | `crates/atlas-platform`, `crates/atlas-app`; D3/D11/D44, Q1. | P1-07; Q1. |
| P1-09 | One sidebar must represent saved rows and live pages without duplicates. Wire flat saved and ordinary rows to D42/D43 creation, save/unpin, unload/open-or-focus, ordering and close/reopen. Render wide/compact states, retain hidden-state selection, and prove repeated lifecycle changes create no duplicate or leaked views. Full folders remain P1-21. | `atlas-core`, `atlas-app`, `atlas-ui`; D42–D44. | P1-08. |
| P1-10 | Navigation must work from a focused page in every sidebar width state. Wire live address/title updates without rewriting saved targets, back/forward/reload, the owned address-entry popup and native shortcut router. Ctrl L/K opens entry; Ctrl T/W/Tab/Shift T follows D42/D43. Test focus through drag/hidden transitions; Phase 4 extends this same popup. | `atlas-core`, `atlas-platform`, `atlas-app`; D8/D9/D42–D44. | P1-09. |
| P1-11 | OAuth popups require an opener relationship. Adopt new-window requests as tabs without previous navigation; synthetic opener/focus tests pass. | `atlas-platform`, `atlas-app`; page rules. | P1-10. |
| P1-12 | Sites need usable permission prompts without automatic grants. Integrate engine permission and notification UI; test decisions and persistence using disposable profiles. | `atlas-platform`; privacy guardrails. | P1-11. |
| P1-13 | Browser essentials must be reachable. Wire find, downloads, print, context menu and zoom controls, including explicit Ctrl J routing and focused-page tests. | `atlas-platform`, command registry; note 02 Musts. | P1-12. |
| P1-14 | The address display must not misstate security. Implement a truthful scheme/status display and native certificate-error behavior; verify invalid certificates never silently succeed. | `atlas-platform`, `atlas-ui`; note 02 Musts. | P1-13; supported native security-state approach. |
| P1-15 | Streaming and long-lived tabs need correct lifecycle handling. Add fullscreen and hidden-tab suspension with late-callback guards; test video layout and resume. | `atlas-platform`, `atlas-app`; D6. | P1-14. |
| P1-16 | Losing one process must not lose the session model. Recover renderer and shared-browser-process failures distinctly; synthetic fault checks prove no duplicate recovery or missing tabs. | `atlas-platform`, `atlas-app`; crash recovery. | P1-15. |
| P1-17 | Dragging the sidebar hidden must preserve access over live pages. Add the owned hover-peek popup and Ctrl Alt S sidebar pinning, integrated with D44 width states, without resizing the page during peek. Test focus, stacking, Alt Tab and window lifecycle; move navigation/address controls to the title bar while hidden. | `atlas-ui`, `atlas-platform`, `atlas-app`; D7/D8/D44, `design/README.md`. | P1-16. |
| P1-18 | The Phase 0 conditions must hold in the product. Record shell idle/startup checks and the owner's remaining site/sign-in tests plus one day of browsing. This is an evidence/report slice; failures return to their owning implementation slice. | Phase 2 acceptance report; D6/D21/D22/D37. | P1-17; owner account tests and day of use. |

### Phase 3 milestone: browser memory and home

| ID | Why this slice exists; deliverable and done condition | Main files / decisions | Blocked by |
|---|---|---|---|
| P1-19 | Session recovery must not load every saved destination. Restore committed live-tab addresses/titles/order and saved associations, preserving saved rows that were unloaded. Retain D42's in-session reopen behavior and prove crash/restart recovery on disposable data. Back/forward lists are not promised. | `atlas-core`, `atlas-store`, `atlas-app`; D13/D42/D43. | P1-18. |
| P1-20 | Navigation must become searchable memory. Persist history events and update tier 1 incrementally, with a usable history list and no duplicate entries from stale events. | `atlas-store`, `atlas-search`, `atlas-ui`; D10. | P1-19. |
| P1-21 | Saved destinations need inline organization without a second bookmark model. Extend the flat D43 foundation with folder and saved-page creation, editing, moves, ordering and removal through core/UI/store/search. Review compact-folder controls, and prove lifecycle, hierarchy and restart consistency. | `atlas-core`, `atlas-store`, `atlas-search`, `atlas-ui`; D43/D44. | P1-20. |
| P1-22 | Imported bookmarks must join the same inline saved-page model without opening every page. Parse bounded HTML exports, preview/confirm and preserve folder hierarchy/order as unloaded saved entries. Test synthetic Chrome/Edge/Firefox-shaped exports, nested folders and malformed input; read no browser profile. | Saved-page import and UI; D14/D15/D43. | P1-21. |
| P1-23 | Full-text selection needs evidence. Run the FTS5/Tantivy bake-off on one synthetic corpus, including update/delete/expiry and recovery costs. Deliver the benchmark and reasoned choice for review before production integration. | Benchmark harness/report; D18. | P1-22; candidate dependency audits. |
| P1-24 | Search needs a tested implementation of the chosen store. Implement the `FullText` adapter and durable recovery against synthetic documents; interrupted writes, deletions and replay preserve the intended results. | `atlas-search`; reviewed P1-23 choice. | P1-23. |
| P1-25 | Capture must be safe before real browsing can enable it. Prepare the concrete page-access method for owner approval before any guarded page API use; then prove eligibility/extraction on synthetic pages, redirects, frames and `no-store`. Deliver evidence and the production recommendation. | Synthetic feasibility harness/report; D18/D35. | P1-24; owner approval of the concrete page-access method before prototype use. |
| P1-26 | Page recall needs bounded, expiring data. Integrate the proven extraction path with the page-text cache/index, disable setting, expiry and restart/restore rules; excluded content never enters storage. | `atlas-platform`, `atlas-search`, cache store; D18/D35, Q2. | P1-25 and review of its evidence; Q2. |
| P1-27 | Clearing browsing data must include Atlas's copies. Add explicit kind/time choices and coordinated WebView2/history/search clearing; partial failures remain visible, and replay/restore does not silently resurrect cleared entries. | `atlas-platform`, `atlas-store`, `atlas-search`, settings UI. | P1-26; retention-aware backup/recovery design. |
| P1-28 | Quick destinations need one truthful action path. Build the canvas home page, editable Quick access tiles and separate square sidebar Favorites, with durable ordering/labels. Favorites can target websites, plugin pages or other supported Atlas destinations. Plugin targets retain their unavailable indication until the normal plugin lifecycle exists; add no early host. | `atlas-app`, `atlas-ui`, `atlas-store`; D45, spec home/sidebar. | P1-27. |
| P1-29 | Appearance and sidebar space should remain as chosen. Persist Customize palette/accent, Reduce transparency and sidebar width/presentation state. Inspect all five palettes and wide/compact/hidden restoration across restart without changing hover-peek behavior. | `atlas-ui`, settings; S10/D44. | P1-28. |

### Phase 4 milestone: command palette

| ID | Why this slice exists; deliverable and done condition | Main files / decisions | Blocked by |
|---|---|---|---|
| P1-30 | Ctrl K needs one command source and predictable focus. Extend P1-10's live owned entry popup into the full command palette with registry-backed rows, keyboard selection, Esc and focus restoration. Do not create a second overlay mechanism. | `atlas-core`, `atlas-ui`, `atlas-platform`; S5/D7. | P1-29. |
| P1-31 | Search and sidebar must act on the same destinations. Add scope counts, highlights and bounded previews across tier 1/tier 2. Shared stable targets open/focus saved rows, live tabs and supported Favorite destinations through the same actions; stale results cannot replace newer queries or duplicate a saved row's open instance. | `atlas-search`, `atlas-core`, palette UI; D43/D45. | P1-30. |
| P1-32 | The palette is also the address bar. Add explicit URL navigation, command prefixes and Google Search the web via Alt Enter; verify typing sends no suggestion requests. | Command parsing and palette actions; D9. | P1-31. |
| P1-33 | Central UI speed must be measured after composition. Verify one-frame opening, complete ranking latency and live-overlay focus/stacking regressions. Record methodology and failures without hiding slow cases. | Palette performance/acceptance report; D6/D7. | P1-32; measured budgets pass. |

### Phase 5 milestone: daily driver

| ID | Why this slice exists; deliverable and done condition | Main files / decisions | Blocked by |
|---|---|---|---|
| P1-34 | External links must reach one running Atlas. Implement single-instance forwarding with validated URL arguments and startup-race tests. | `atlas-platform`, `atlas-app`; external links. | P1-33. |
| P1-35 | Evergreen changes can affect sign-in. Store the last observed runtime version, show D36's notice on change and provide the explicit open-in-Edge action; no sign-in canary. | `atlas-store`, `atlas-app`, `atlas-ui`; D22/D36. | P1-34. |
| P1-36 | Compact or hidden chrome must remain usable without dragging or reading an icon alone. Implement F6 traversal, accessible names/states and keyboard access across wide/compact/hidden/peek modes, inline folders and Favorites. Record an NVDA pass across the chrome. | `atlas-ui`, `atlas-platform`; accessibility Musts, D43–D45. | P1-35; owner authorization if software installation is required. |
| P1-37 | Installation must be reproducible without secretly changing Windows. Prepare and validate the installer and registration path; owner performs/approves installation and default-browser selection. Preserve data on upgrade. | Packaging, installation documentation; S8. | P1-36; packaging/policy feasibility and owner operator actions. |
| P1-38 | Updates must reject tampering. Implement local signed-package verification and failure handling first, using synthetic packages and an independently pinned verification key. | Updater core/tests; signed updates. | P1-37; reviewed format and key-handling design. |
| P1-39 | Delivery needs explicit network and restart behavior. Connect the verifier to the owner-approved host/check/install policy; prove failed download, invalid signature and interrupted install preserve the working version. | Updater delivery/UI. | P1-38; owner approval of host, network calls and install/restart behavior. |
| P1-40 | “Daily driver” needs evidence from use. Record one week as the owner's default browser and the product's D6 memory/startup results; reconcile failures before closing plan 1. | Final acceptance report; D6. | P1-39; owner default-browser action and week of use. |

Plan 2 begins only after P1-40 and the owner's plan 1 acceptance. Its detailed
phase outline, and those of plans 3 and 4, will be broken into their own
review-sized issues when their foundations exist. This avoids pretending
today's unimplemented Plugin or document APIs are already stable enough for
a precise PR map of every later feature.

## Proposed analytics amendment (D39)

The owner wants evidence that helps improve Atlas. They approved local
aggregate counters, explicit report export and model suggestions they review.
This is an amendment to scope, not an authorization for automated collection
in development or an automatic upload/model service. The 40 slices above
remain the approved baseline while the two added slices below are reviewed.

Collect only fixed, allowlisted Atlas metrics. Examples are palette opens,
built-in action outcomes, coarse latency buckets and fixed error categories.
Do not accept arbitrary event names, user-defined commands/labels, error
strings, account/install/device/session IDs or hashes of them. URLs, queries,
titles, note/code/page text, paths, terminal input/output and screenshots
remain excluded. Update bounded counters directly; do not retain raw events
and remove their identifiers later. Measure the overhead against the idle
and interaction budgets so measurement does not defeat the performance goal.

A synthetic report shape for review, not a finalized schema:

```json
{
  "schema_version": 1,
  "counts": {"palette_opened": 84, "bookmark_created": 6},
  "latency_buckets": {
    "tier1_search_ms": {"le_1": 105, "gt_1_le_5": 43, "gt_5_le_16": 2, "gt_16": 0}
  },
  "errors": {"storage_unavailable": 1}
}
```

There is no account field or event timeline. That is not proof of absolute
anonymity: Atlas v1 has one user, and sharing through an identified account
can disclose the sender. This is an inference from the deployment, consistent
with [NIST's guidance on de-identification](https://csrc.nist.gov/pubs/sp/800/188/final).
Atlas creates a report only on explicit export; the owner chooses whether
and where to share it. No model provider, training permission, background
analysis or automatic UI changes are selected. A model's output should be
reviewable hypotheses supported by the counters, not claims that frequency
alone proves why somebody acted.

Before collection implementation, put one concrete enable/reset/retention
proposal and the export preview to the owner. Recommendation: collection is
off until enabled, counters have a visible reporting scope and Reset control,
and exported reports contain only the displayed aggregates. Decide the exact
reporting period and treatment of local analytics backups there; do not
silently reuse page-text's seven-day rule for a different data category.
These choices block analytics work, not P1-01's scaffold.

| Proposed slice | Why and done condition | Placement / blocked by |
|---|---|---|
| AN-01: local aggregate counters | Make improvement evidence useful without recording content. Implement the approved fixed schema, enable/reset/retention controls, local persistence and bounded timing buckets. Synthetic tests reject private/free-form fields; disabled collection performs no collection or persistence, and measured overhead meets the approved budget. | Proposed for plan 1 Phase 5 after P1-39. Requires owner approval of this amendment and the concrete schema/control proposal. |
| AN-02: inspect and explicitly export a report | Let the owner decide exactly what leaves the app. Preview the aggregates, export only after an explicit action, and document how to ask a model for reviewed suggestions. Verify the exported file matches the preview and contains no extra metadata. No model or upload integration. | Depends on AN-01. Proposed before the P1-40 week of use so the owner can choose whether to gather improvement evidence. |

If approved, add these dependency edges to the Issue map before starting
either slice. Do not renumber the accepted P1 labels or treat approval of the
40-row map as approval of unspecified telemetry behavior.

## Validation of this planning change

The original readiness pass validated Markdown tables, frontmatter, code
fences, document references and the 40-row dependency chain. Independent
review resolved the early shortcut dependency and split two research-heavy
slices. P1-01 subsequently supplied `docs/LOCAL-DEV.md`, passed the product
gate and independent review, and merged as `ed19d6d`.

The D43–D45 documentation amendment records the owner's approved revised
scope and keeps both 40-row maps identical. An independent documentation
audit confirmed their identity/order and agreement with D42–D45. Separately,
P1-02's headless implementation passed the full local gate (24 model tests,
seven architecture tests and console launch) and independent code review.
The owner then requested its commit and approved its local merge, completed
as `892d3bc` in `claude/plan-atlas-v1`. That P1-02 amendment added no
third-party dependency, registry setting, browser profile or Windows policy.
P1-03 storage is implemented on `codex/p1-03-storage`, based on the completion
record `9a6ae35`. Its 77-test local gate and independent reviews passed; the
owner requested its commit and approved its local merge, completed as
`c4a6631` in `claude/plan-atlas-v1`. Its dependency review and
narrow licence allowances are recorded in `docs/dependency-audit.md`;
implementation and verification status are tracked in plan 1's Super-Pass
ledger. No later
product behavior is claimed complete.
The analytics amendment remains proposed separately; no collector or real
usage report has been created.
