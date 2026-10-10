---
title: "Atlas v1 — Plugins and Fledge — Plan (plan 2)"
status: active
created: 2026-10-09
subject: Atlas v1 plan 2 — the plugin runtime and the Fledge plugin on a stand-in client (spec phase 3)
relates_to: "docs/plans/Atlas v1 — Plan.md (plan 1 and the v1-wide decisions); docs/research/03-core-architecture.md; docs/research/05-fledge-api.md"
issues_repo: not yet created
branch: claude/plan-atlas-v1
---

# Atlas v1 — Plugins and Fledge

> **Status: active, approved by the owner on 2026-10-10.** Implementation is
> queued behind plan 1's exit criteria. Work uses plan 1's D41 local branch,
> independent-review and owner-approved-merge workflow; commits still need
> an owner request. This plan's later slice breakdown requires approval.

## Why this exists

Atlas's reason to exist is that code, notes and Fledge live inside the
browser and share one palette and one search (`docs/spec.md`, "What this
is"). That needs a plugin runtime first; Fledge is the first plugin because
its inline palette action ("New lead") proves the runtime end to end without
an editor.

## Scope (D34: thin)

- **The plugin runtime:** `atlas-plugin-api` with the `Plugin` trait,
  manifest, `ViewSpec` (component, form, list), `Outcome` and `IndexFeed`
  contracts; built-ins registered explicitly, activated on first use, with
  namespaced settings and observable command failures. A plugin stays in
  the Plugins list when closed (`design/README.md`).
- **Palette integration:** plugin commands as a scope chip; `FormSpec`
  rendered in the preview pane by `atlas-ui`; Tab between fields, Enter
  creates, Ctrl Enter creates and opens (`design/README.md`).
- **The Fledge plugin on a stand-in:** a `FledgeClient` trait in a
  `fledge-client` crate with a stub implementation; "New lead" with Name,
  Company, Source and Est. value (Company modelled as Nest will hold it,
  D26).

Done means a person can open the palette over a page, choose Fledge's
"New lead", complete the form, and create a clearly labelled demonstration
record. Ctrl Enter additionally opens that record inside the Fledge plugin.
Closing its tab leaves the Fledge row available to reopen. No Fledge account,
credential or production call is involved.

## Entry criteria and decisions

Implementation follows plan 1, not alongside it. Plan 1 must have delivered
the generic tab model, command registry, palette, storage, both search tiers,
shell and local gate; its GPUI idle fix and browser exit criteria must pass.
This plan extends those services rather than creating parallel stores,
tab lists or search engines. Plan 3 starts only after this plan passes.

Product analytics follows plan 1's D39: local aggregate feature/command
counts, timings and error categories, with explicit report export and
suggestions the owner reviews. There is no automatic upload or adaptive UI.
Account/device identifiers, URLs, queries, paths, text and detailed timelines
remain excluded; Fledge form and record contents are not analytics payloads.
This plan adds no separate analytics integration.

| Decision | Choice and reason | Alternative not taken |
|---|---|---|
| D17 and D34 | Built-in Rust plugins and one small Fledge action prove the boundary needed by Notes and Forge. | A third-party plugin installer, WebAssembly runtime and full Fledge UI would expand the first plugin delivery. |
| D23–D26 | The real client later uses ordinary OAuth consent and a structured REST API, after Stage C. Company remains part of the lead model. | Browser cookies, Fledge's internal routes and its MCP tools are not this client's interface. |
| P2-1, owner, 2026-10-10 | Ship the stand-in in the normal app, clearly labelled as a demo. It uses synthetic records and sends nothing to Fledge. | Development builds only would hide the working plugin from the owner. |
| P2-2, owner, 2026-10-10 | Demo leads last until Atlas restarts. Only Name is required; Company, Source and Est. value are optional, and estimates are labelled demo USD. | Durable demonstration business data and real-service field validation would add a store and assumptions before the client contract exists. |
| P2-3, owner, 2026-10-10 | Keep an unsubmitted form while Atlas is running, including across Esc, and clear it after submission, explicit discard or restart. | Discarding it whenever the palette closes would lose unfinished typing. Persistent form drafts are unnecessary for the session-only demo. |

The research in `docs/research/05-fledge-api.md` describes a dated source
inspection. Its endpoint status, field constraints and authentication
details are not a current API contract. D26 supersedes its statement that
Company has no home. Nothing in this plan inspects Fledge repositories or
depends on their current deployment state.

## Architecture and ownership

### One plugin boundary

`atlas-plugin-api` defines the versioned manifest, command identifiers,
owned request and result values, forms, lists, tab references, errors and
index change batches. It depends on no Atlas crate. A plugin's only Atlas
dependency is this API crate; it reaches application services through `Host`.
Serialisable values cross the boundary, never GPUI entities, WebView2
handles, SQLite connections or references into the core's state.

The application registers built-ins in an explicit list and validates
duplicate identifiers, API versions, command ownership and view identifiers
before making a command available. This is easier to trace than implicit
registration. A test plugin must exercise the same API as Fledge; no
Fledge-only shortcut through `atlas-core` is allowed.

The research trait is illustrative, not an API to copy unchanged. For
example, its borrowed `&dyn IndexFeed` contradicts the owned-value boundary.
Expose feed requests and owned batches through the plugin contract instead.
Use an object-safe async boundary, such as explicitly boxed futures, if
the host stores different plugins as trait objects. Rust's current trait
rules do not permit dispatchable native `async fn` methods on `dyn Trait`.
The exact signatures are reviewed when the contract is implemented; future
WebAssembly compatibility is an intent, not a claim of a working sandbox.

### Fledge without a dependency exception

The application wires this path:

`plugin-fledge → Host service request → application adapter → FledgeClient → stand-in`

`fledge-client` owns the `FledgeClient` trait and the stand-in implementation.
The adapter translates versioned, owned API requests into typed client
calls and returns owned results. It lives in application wiring, outside
the plugin. Thus `plugin-fledge` does not import `fledge-client`, yet all
Fledge operations still pass through that trait. This resolves the direct
constructor injection example in research note 03 without weakening the
repository's dependency rule.

Start with only the operations needed by the demonstration: create a lead,
read its details and list the local demonstration records. Use opaque record
identifiers and an explicit amount/currency representation. Do not freeze
Nest's internal identifiers, endpoints or money representation into the
public plugin API. A submission identifier lets the stand-in recognise a
repeated request without creating a second record.

### Rendering and state

The shell is plan 1's existing split: `atlas-ui` renders forms, lists and
registered rich-view components; `atlas-app` wires views to the core;
`atlas-platform` alone manages WebView2 and Win32. A component identifier
selects an application renderer, never arbitrary plugin code or markup.
No additional GPUI-dependent `plugin-*-ui` crates are introduced.

Rust owns field values, validation, selection, command state and outcomes.
GPUI forwards edits and renders snapshots. Plugin views are addressed by
the generic tab model, with their row only in the Plugins section. A closed
plugin has no badge; reopening it selects its existing open instance when
one exists. The demo needs only generic form, list and detail presentation,
so it adds no editor web view or web content bridge.

### Lifecycle and failure handling

Manifest discovery must be cheap. Activate a plugin only when its command
or tab is first used; perform service work away from the UI thread. Track
each request and observe task completion. Cancellation prevents late results
from updating a closed form or a different tab. A timeout is not proof that
a write was reversed: an uncertain outcome stays uncertain and is not
automatically submitted again.

Ordinary failures use explicit error results. Bound queues and result
batches so a plugin cannot accumulate unlimited work. A reported plugin
failure must leave browser navigation usable and retain an editable form
when it is safe to retry. Do not copy the outline's blanket `catch_unwind`
promise: it is not native crash isolation, and a cooperative timeout cannot
interrupt arbitrary non-yielding code. Any eventual narrow panic boundary
needs a tested failure policy and review against `AGENTS.md`; it must not
swallow errors or log a panic payload containing owner data.

### Storage and search

The host scopes settings and stored values to a plugin identifier and rejects
requests for another plugin's namespace. Built-ins must not open Atlas's
database directly; dependency checks and review enforce that architecture.
This is not a security sandbox: trusted native Rust code can still call
ordinary filesystem APIs. Third-party isolation remains later work (D17).
Use plan 1's writer, migrations and local data directory for persistent host
settings. Demo records themselves stay in memory for this run (P2-2), in a
namespace distinct from later real records. Their search contribution is
also temporary: use a session-only feed or remove the entire demo namespace
from durable indexes before serving searches after restart. An empty new
stand-in cannot enumerate the old session's records to delete them one by one.

Add registered commands to tier 1 and expose owned, bounded `IndexFeed`
batches with stable ids, revisions and deletions. Apply changes idempotently.
Advance a persistent source cursor only after durable replayable delivery
or after the durable index commit it represents. Each consumer tracks its
own completion; an in-memory tier 1 update is not a tier 2 commit. Test
interruption before and after delivery, index commit and cursor checkpoint,
then replay without lost records or duplicates. Keep sequence numbers
on palette queries so stale responses cannot replace a newer query. Use
synthetic records to prove both search tiers can consume a plugin feed;
this does not add full Fledge synchronization or keep-local switches.

The chosen tier 2 store determines the commit/checkpoint implementation.
SQLite FTS5 external-content indexes need explicit maintenance and initial
population; merely creating triggers does not index existing rows. Tantivy
can attach a payload to a prepared commit, but that does not make an
independent SQLite cursor update part of the same transaction. Test the
actual recovery protocol selected after plan 1's bake-off, not a mock that
acknowledges every step together.

## User flow and error states

The palette labels the action "New demo lead" while the stand-in is active.
The form, success message and Fledge tab all make clear that nothing has
been created in the real service. Enter submits once and returns a success
state; Ctrl Enter also opens the created detail. Tab and Shift Tab move
between fields. Validation focuses the failing field without losing input.

While a submission is pending, repeated Enter presses do not issue another
write. Closing the palette cancels its view subscription; a completed write
still has a recorded outcome available in the demo list. A failure leaves
the input available to correct or retry. Fault injection in development
tests covers invalid fields, unavailable service, cancellation, timeout
and unknown write outcome; it is not exposed as normal product settings.

Demo records last until restart. Name is required; Company, Source and
Est. value are optional, with amounts labelled demo USD (P2-2). Reject an
empty Name or an invalid amount with a field error rather than silently
coercing it. Keep the unfinished form across palette closes for this session
(P2-3), and clear it only after successful submission, explicit discard or
restart. Pending and completed requests remain distinct from that editable
draft, so closing and reopening the palette cannot resubmit an old request.

## Sequencing

Each phase must finish before the next starts. These are implementation
phases, not published issues. An approved issue breakdown later divides
each into changes reviewable in one sitting with explicit blocking edges.

### Phase 1 — Contract and a test plugin

**Why first:** Notes and Forge need the same public boundary, so the first
consumer must expose ownership and dependency mistakes before UI is added.
Define the API values and lifecycle, registration checks, host service
scoping and error model. Extend `xtask check-deps` to enforce the plugin
and shell boundaries, then drive a synthetic plugin through activation,
commands, view requests and close/reopen without a window.

**Exit:** the gate passes; serialization round trips, API-version rejection,
duplicate-id rejection, cross-plugin access rejection and command ownership
are tested. No public API depends on a toolkit or storage implementation.

### Phase 2 — A plugin command through the real palette

**Why now:** a contract is useful only if the shared shell can render it.
Add the Plugin actions scope, declarative forms and lists, pending/error
states, generic outcomes and plugin tab lifecycle. Use the test plugin to
prove all keyboard paths before Fledge behavior obscures failures.

**Exit:** Enter, Ctrl Enter, Tab, Shift Tab and Esc work over a live page;
focus returns correctly; validation preserves input; duplicate submission
and late responses are tested. Closing the plugin leaves its row available
and does not create a duplicate in Open tabs. All five palettes remain
readable with keyboard focus visible.

### Phase 3 — The Fledge demonstration end to end

**Why now:** one small domain action checks whether the API is sufficient.
Implement the stand-in trait, application adapter and Fledge plugin; seed
only generated records; add lead creation, list and detail flows with the
approved demo rules. Every path uses the host, including the local client.

**Exit:** the owner can create and open a demo lead from the normal app.
Client contract tests cover duplicate submission ids, validation, missing
records and simulated failures. No network client, credentials, OAuth screen
or Fledge production address is needed to run the demonstration.

### Phase 4 — Search, lifecycle and acceptance

**Why last:** data and cleanup behavior must survive repeated use, not just
one successful submission. Connect the demo's entries to the shared search,
exercise feed replay and deletion, and verify close/reopen and restart
behavior under P2-2. Measure activation,
palette ranking and idle resource use on this Windows machine.

**Exit:** no lost, stale or duplicate results after replay; a fresh session
contains no earlier demo records in either search tier; closed views retain no
subscriptions; plugin search does not break plan 1's 5 ms/100,000-row tier 1
budget; there is no busy polling when idle. A measured report distinguishes
application cost from the stand-in's simulated latency. The owner accepts
the flow before plan 3 implementation starts.

## Verification and limits

Use synthetic data throughout: generated names, `.test` or `.invalid`
addresses only where an address is needed, and isolated temporary stores.
Never copy a lead, account response or business record into fixtures,
snapshots, logs or planning documents. Test error codes and request ids,
not private payloads. A developer fault switch is test input, not telemetry.

The implementation gate is the scaffold's `docs/LOCAL-DEV.md` gate, using
D38's release-style profile; until defined, follow `AGENTS.md`. In addition
to core tests, inspect the running Windows app after view changes and repeat
the live-overlay/focus checks with a page underneath. Test the API and host
with fakes, not by signing into an account or opening a debug port on pages.

⚠️ No plugin implementation or measurements exist yet. Concrete timeout
durations, payload bounds and activation budgets are set from a documented
synthetic workload before performance work is judged; current research
numbers do not demonstrate this application's performance.

## Later, with the real client (after Stage C, D24)

OAuth 2.1 as an ordinary client (D23); the structured REST API (D25); the
per-kind keep-local switches (D19); Atlas as Fledge's full desktop app
(D20). The Fledge-side work in `docs/research/05-fledge-api.md` ("Work
needed on the Fledge side") is a list for the owner, not issues here.

Stage C alone does not silently enable a connection. The owner supplies or
authorizes a current public client contract and approves the real-client
work separately. Reverify consent scopes, response types, refresh behavior,
write idempotency, pagination and change feeds then. Keep contract tests
against a controlled local server; only the owner signs into a real account.

Mail, calendar, documents, live search, an offline outbox, account switching,
third-party plugins and a marketplace are outside this plan. The stand-in
does not validate a future API's permissions, availability or performance.

## Remaining implementation checks

The owner approved this plan's readiness on 2026-10-10. The following
technical checks remain phase gates; approval is not a measured success
or permission to start later phases early.

- **Implementation check:** choose the executor and cancellation mechanism
  under plan 1's dependency rules; prove late completion and unknown outcomes
  rather than assuming timeouts undo effects.
- **Dependency check:** before adding any crate, verify its maintenance,
  security record and licence. This plan adds no dependencies or version pins.

## Issue map and approval

Readiness is approved. Prepare this plan's reviewable local slices with
blocking dependencies and obtain the owner's breakdown approval before
implementing them. Plan 1's approved 40-slice map does not stand in for this
later breakdown. D41 permits local implementation without creating a remote:
each slice uses its own branch and independent review, and the owner approves
every merge. Commit only on request. Outstanding work and the future slice
map stay here; publishing issues, adding a remote and pushing require separate
owner authorization.

## Sources checked for this expansion

Checked 2026-10-10. Local decisions outrank the dated research sketches.

- `AGENTS.md`, `docs/decisions.md`, `design/README.md`, plan 1's decisions
  and crate layout, and `docs/research/09-phase-0-verdict.md` establish scope.
- [Rust trait dyn compatibility](https://doc.rust-lang.org/reference/items/traits.html#dyn-compatibility)
  documents the object-safe async constraint; boxed futures are a design
  choice here, not a new dependency.
- [Rust panic handling](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)
  describes the limits of catching unwinding panics.
- [Tokio timeout](https://docs.rs/tokio/latest/tokio/time/fn.timeout.html)
  illustrates cooperative timeout limits; citing it does not select Tokio.

The owner-authorized readiness review on 2026-10-10 also checked:

| Claim | Result and implementation gate |
|---|---|
| Host namespaces constrain application service access. | This is a tested host rule, not native-code confinement; Rust's [file API](https://doc.rust-lang.org/std/fs/struct.File.html) remains available to an in-process built-in. Phase 1 checks request scoping and dependency rules. |
| Feed acknowledgements can be tied to durable indexing. | [SQLite FTS5](https://www.sqlite.org/fts5.html) documents external-content maintenance and initial-population requirements. [Tantivy's prepared-commit source](https://docs.rs/tantivy/latest/src/tantivy/indexer/prepared_commit.rs.html) supports a commit payload. Phase 4 must prove the selected protocol's replay, deletion and checkpoint ordering; neither source proves Atlas's implementation. |
| Async dispatch, panic handling and timeout caveats remain accurate. | The Rust and Tokio references above support the stated limits. Phase 1 still needs executable contract and cancellation tests with the selected runtime; this review selects no new dependency. |

## Iteration log

- **2026-10-10:** Expanded the outline at the owner's request while keeping
  the repository local. Recorded the normal-app demo decision, clarified
  the Fledge host adapter and existing GPUI shell boundary, and replaced
  blanket failure-isolation claims with testable error handling.
- **2026-10-10:** The owner approved session-only demo leads, optional
  Company/Source/Est. value and demo USD (P2-2). Independent review added
  durable feed checkpoint tests and removal of the previous session's demo
  records from search after restart.
- **2026-10-10:** The owner approved retaining an unfinished form until
  submission, explicit discard or restart (P2-3), so Esc no longer implies
  losing work.
- **2026-10-10:** The readiness review distinguished Host namespace checks
  from native-plugin isolation and verified the async/failure caveats.
  Added store-specific feed recovery checks from SQLite and Tantivy's
  primary documentation. Owner decisions and the stand-in scope are unchanged.
- **2026-10-10:** The owner approved all four plans and plan 1's 40-slice
  breakdown, with local branches, independent review and owner-approved
  merges (D41). Marked this plan active and queued behind plan 1. Its own
  breakdown still needs approval, and commits still require a request.
  D39 limits analytics to local aggregates and explicit report export;
  suggestions require owner review and personal/business content stays out.
