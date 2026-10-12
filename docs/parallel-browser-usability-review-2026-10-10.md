# Browser usability source review — 2026-10-10

## Current assessment — 2026-10-11, baseline 8239511

Build a usable, good browser first. The owner's latest instruction defers
Plugin/Fledge, Notes and Forge work; it does not remove browser requirements
or waive their dependencies. The smallest useful next milestone is repeatable
D49 browsing: type an address, browse and switch pages, save/unload/reopen,
restart without losing committed state, and close cleanly. This is narrower
than completing Plan 1 or declaring Atlas a daily driver.

**Recommendation:** visually validate the now-implemented stale open-palette correction
described below. Integration assigned it after freezing its `ui.rs` smoke-error
changes; the latest full gate now passes with 411 tests, including the later
recovery, shutdown and palette-shortcut corrections. The latest visible attempt was inconclusive because computer-use
activation failed; the before/after reproduction remains open. Complete
the compact acceptance matrix while the search team continues P1-05. The
bounded failed-start recovery, late-action shutdown and palette-shortcut corrections described
below passed the gate; their visual acceptance remains open. Keep native diagnosis with its existing
owner. All execution remains
serialized by integration. Do not start the deferred plugins or promote a
diagnostic prototype as a browser fix.

### What is implemented, validated and merged

Read-only Git inspection confirms `8239511` on
`codex/integrated-browser-preview`. The working tree was clean at the beginning
of this assessment. This is a committed preview checkpoint, not an approved
merge or completion of the slices whose code it contains. P1-01–P1-04 retain
their recorded accepted merges. This chat made no commit or merge.

The current evidence supersedes the October 10 assessment retained below:

| Evidence | What it establishes | What remains open |
| --- | --- | --- |
| Latest `continuation-palette-refocus-gate.log`, ending `Local gate passed.` | 411 passing tests, including 78 app and 175 search unit tests, plus the full gate and app smoke. This chat verified the preserved log hash and test totals. Both local source reviews and integration's independent review passed the queued-refocus correction. | Recovery-card, shutdown/popup, stale-palette and focused shortcut visual acceptance remain open. No D48 performance acceptance follows from this gate. |
| Earlier `continuation-shutdown-actions-gate.log`, ending `Local gate passed.` | 407 passing tests, including 75 app and 174 search unit tests, plus the full gate and app smoke. This chat verified the preserved log hash and test totals; integration's independent review also passed. | This predates palette shortcut forwarding and its queued-refocus correction. |
| Earlier `continuation-browser-first-formatted-gate.log`, ending `Local gate passed.` | 382 passing tests, including 60 app tests, plus the full gate and app smoke after integration applied formatting. This chat verified the preserved log hash and test totals. | The stale-palette before/after UI check remains open. The first attempt stopped at formatting; its separate log is preserved. |
| Full `continuation-socket-close-commit-gate.log`, ending `Local gate passed.` | 376 passing tests, including 54 app and 164 search unit tests, plus dependency checks, formatting, Clippy, documentation tests and app smoke. The independent reviewer read and summed the preserved test results. | This is a correctness/build gate, not D48 performance or complete visible browser acceptance. Historical error 4551 failures remain historical. |
| Stable visible Go to action, `palette.rs::PaletteChoices` and tests | Explicit-address selection survives local-result arrival; deliberate result activation remains available. | Broader actual keyboard/destination coverage. This is implemented now and is no longer the recommended missing feature. |
| `ui/navigation_status.rs::NavigationStatus` | Per-tab loading/completion/failure feedback, stale-tab rejection and callback-order regressions. | Events lack an individual navigation identity within a tab; overlapping navigations still need diagnosis. The original missing normal completion feedback has been addressed. |
| Owner's `about:blank` check and one `beforeafter` fixture cycle | Text entry, stable Go to selection after two seconds, Enter popup closure, page-focused Ctrl K, palette typing and Escape focus return in the observed paths. | Repeated reliability, Enter activation into existing page inputs, other destinations, stacking, resize and restart. A tool targeting error is not proof of an app typing defect. |
| Native diagnostics | Narrow construction/lifecycle cases and several instrumented series passed. | Earlier native navigation failures, pending-environment coverage and the unsuccessful isolated page-host prototype remain; none establishes the original breakpoint's cause or resolution. |
| P1-05 complete-query benchmark | All 1,040 results retain their semantics; 10/26 D48 cases pass. | Sixteen cases still fail. Maximum p95 is 13.9768 ms; three passing fresh-process acceptance runs are still required. |

The baseline application SHA-256 is
`0fe3b8daad72ddf3383f025756a5174b39f098d2fa7355d81bfd45eaec89c858`;
the gate log SHA-256 is
`ca28dc0a417a3e00929ece8c652c21dc8b7b2e58da6acd820fec72af0a03ca62`.
See the current handoff, lines 13–40 and 150–197, and the implementation
ledger's opening checkpoint. This chat did not rehash or execute the binary.
The newer owner/integration UI authorization supersedes the old stopped-UI
checkpoint; this chat does not operate the UI.

The new formatted gate log SHA-256 is
`5cc1618aed8eda7ece671bceeb3d4f465ff4c2996e6b150f139a57d2b0dcae68`.
Integration reports the corresponding application SHA-256 as
`0da392f0800e5e3b739bfeabe8122903111d38301e41a6d610168e0b882debd3`;
this chat verified the log hash, not the binary hash. The first gate attempt,
`target/continuation-browser-first-gate.log`, stopped at `cargo fmt --check`;
integration then ran `cargo fmt` and the full gate again. A fresh synthetic
visible launch returned window `1184554`, but computer-use activation failed
twice, including its prescribed refresh/retry, with “failed to activate
captured window.” Integration stopped UI work. This establishes neither an
application failure nor a new visual pass; the acceptance matrix remains open.

The earlier shutdown gate log SHA-256 is
`a9a3838e16c4916f394b7aded39daee7401841ba1c82ce39aefe641813ad9e8b`.
Integration reports its application SHA-256 as
`2e3bc975b0b1ea894c12a4c94f5171401635324712b1e0d378f72cf558b47b9e`.
This chat checked the log and its 407-test total, not the binary hash, and ran
no Cargo or native checks. The earlier reported 70/70 app-test pass for recovery
is superseded by this full gate's 75/75 app tests after the shutdown correction.

The latest corrected palette-shortcut gate log,
`target/continuation-palette-refocus-gate.log`, has SHA-256
`10ddb5b7f6b9167614d710f049305859c52499855c4eb993cfe766403e945742`.
This chat verified that hash and the 411-test total, including 78 app and 175
search tests, with no failed, ignored or filtered tests. The log ends with
the application smoke run and `Local gate passed.` Integration reports the
current application SHA-256 as
`4c437c976290615a00ee06287b5338b94b26267e1546002d8b307674d55cdfde`;
this chat did not rehash or run the binary. Source is frozen. The earlier
`target/continuation-palette-shortcuts-gate.log` passed before the queued-refocus
defect was corrected and is preserved as historical evidence, not acceptance
of that version's popup behavior.

### Quantified browser completion estimate

Both reviewers counted unique two-digit slice IDs in the approved maps,
excluding decision IDs, historical mentions and proposed analytics additions.
Classification follows the full slice deliverable and its exit evidence,
not the presence of a crate, a commit or a passing unit test.

| Plan 1 status | Count | IDs |
| --- | ---: | --- |
| Complete, accepted and merged | 4 | P1-01–P1-04 |
| Partial implementation | 12 | P1-05, P1-07–P1-10, P1-12, P1-16, P1-19, P1-28, P1-30–P1-32 |
| Required implementation or acceptance missing | 24 | P1-06, P1-11, P1-13–P1-15, P1-17–P1-18, P1-20–P1-27, P1-29, P1-33–P1-40 |

The ledger groups P1-28/P1-29 as “Partial placeholder Home.” This assessment
credits that placeholder only to P1-28; durable Customize/settings remain
missing in P1-29. P1-07's fixed dark styling and P1-12's native defaults receive
only thin partial credit. P1-04's accepted library slice stays complete. Its
bounded failed-start application prompt is now implemented; missing-primary
preflight and visual recovery acceptance remain open.
Sources: `docs/implementation-status.md:383–407` and Plan 1's map at
`docs/plans/Atlas v1 — Plan.md:924–984`.

Strict accepted completion is **4/40, or 10%**. Code touches 16/40 slices,
which is **40% coverage, not 40% completion**. A useful rough planning estimate
is **15–25% of the approved browser plan implemented**, using these explicit
assumptions: full credit for the four complete slices; 25–75% credit for
substantial partials 05/08/09/10/19/32; 0–25% for thin partials
07/12/16/28/30/31; zero for the remainder. The arithmetic yields 13.75–25%,
rounded for communication. These are subjective slice credits, not measured
effort weights, test coverage, a reliability score or hours remaining.

A basic useful preview can arrive before the complete browser plan because
the denominator also includes installer/updater and default-browser acceptance.
No defensible calendar completion range follows from these counts. In
particular, full Plan 1 acceptance before October 12 is unsupported: P1-40
requires a week of default-browser use after its predecessors, and that has
not begun. Keep the near-term goal a proven browsing preview rather than
quietly dropping the full plan's acceptance requirements.

For secondary scope context only, the four plans contain **40 + 12 + 16 + 21
= 89 slices**. The other 49 remain queued. Only 4/89, about 4.5%, are accepted;
applying the same partial-credit assumptions yields roughly 6–11% of all v1.
Those numbers do not drive the current browser-first work. No plugin work is
recommended or authorized early by this assessment.

### Earliest gates and shortest permitted path

1. **Now, within D49:** verify the implemented preview and fix reproduced
   integration defects. Add the missing recovery offer through existing APIs
   when integration assigns ownership. Preserve the distinction between
   ordinary committed-session restart and restoring a damaged database.
2. **Normal implementation order:** finish P1-05's unchanged complete-query
   performance acceptance. Test-only range-index and grouped-bound experiments
   are not production delivery. Public API changes still need approval.
3. **Then P1-06 and P1-07:** meet the GPUI idle/startup condition and deliver
   five-palette tokens, contrast and reviewed D44 width thresholds. The current
   dark preview does not complete these foundations.
4. **Then complete P1-08–P1-18 in their approved order:** page sizing and
   sidebar states, lifecycle/reordering/navigation, opener-preserving popups,
   permissions, browser essentials, security state, suspension/fullscreen,
   process recovery, peek and product site/day acceptance. D49 brought some
   composition forward; it did not waive these exits.
5. **Continue P1-19–P1-33, then P1-34–P1-40:** browser memory/history,
   folders/import, full-text/retention/clearing, Home/Favorites/settings, full
   palette, then external links/runtime notice/accessibility/installation/
   updates and daily-driver evidence. Privacy, operator and network approvals
   remain where the plan puts them. Plugins wait.

The sequence comes from Plan 1's phase exits at lines 665–766 and issue map,
with D49 at `docs/decisions.md:10–18`. The other plans explicitly require their
predecessors' exits; reading them here does not begin their implementation.

### New reproduced gap: an open palette does not follow model changes

Integration reported this bounded observation on the live `0fe3b8da…` synthetic
build: starting with one Home, Ctrl T created a second Home and opened a
palette showing “2 tabs” and two `about:blank` rows. Sending Ctrl W to the
**main window** activated that window and closed its selected second Home.
The sidebar then showed one tab, while the palette still showed “2 tabs” and
both rows in a later settled screenshot. Integration confirmed the stale
display after several seconds. This is evidence of a main-window mutation
while the palette is open, not evidence of page-focused or palette-focused
Ctrl W routing. No owner typing occurred during that observation.

Independent review of the pre-fix source explains the mismatch. In `ui.rs::session_event`,
the State branch at lines 234–278 installs the new model and reconciles pages,
but does not refresh the existing palette. `palette.rs` starts queries only
at construction or `InputEvent::Change` (lines 154–174). The worker already
saves and synchronizes its catalogue before publishing State
(`session.rs:315–329`), so rebuilding the index is not the remedy. Query IDs
prevent an older query from replacing a newer query; without a new query,
they cannot refresh results after the catalogue changes.

**Pre-fix stale activation was model-safe but confusing.** Enter/click emits
a target and closes the popup (`palette.rs:196–201,256`). The UI sends
FocusTab/OpenSaved (`ui.rs:737–747`); `TabModel::select` and `open_saved` reject
missing IDs. The worker emits a nonfatal failure and does not publish a mutated
model (`session.rs:299–313,434–438`). A removed tab or saved row is not recreated
through this stale activation. A surviving saved row may legitimately open
its fixed destination after its live instance closes. The user can still lose
the popup and see an error for a row that should have disappeared.

**Minimal proposed correction, before edits:** refresh the open palette when
a committed session revision changes, using its unchanged input text. Invalidate
its old local query ID immediately, preserve Go to selection and any deliberate
surviving local selection by stable Target, and issue a query through the
existing worker route. Remove or clearly invalidate obsolete rows/counts while
refreshing; never retain an actionable removed row as though it were current.
Do not reset selection by row position or silently reinterpret an invalidated
selection as a web search. A refresh must not steal focus, select input text,
reopen a dismissed popup, or issue another query for an unchanged/no-op State.
Use the existing global-query and popup-lifetime guards. Coalesce superseded
refreshes through the current request mechanism rather than add polling.

Assigned production files are **only `crates/atlas-app/src/ui.rs` and
`crates/atlas-app/src/palette.rs`**, plus focused tests alongside the pure
refresh/selection policy. Add a worker regression in
`crates/atlas-app/src/session/worker_tests.rs` only for the missing-target
safety/catalogue-order behavior. No platform, vendor, search, dependency or
public API change is needed. Root froze its separate smoke send-error outcome
correction in `ui.rs`, then explicitly assigned this palette implementation.
Both of its `self.fail(...)` changes remain intact. No worker source/test file
was changed for this correction.

Required regression cases and the UI acceptance repeat are:

1. Two Home rows/count two become one after committed close without an input
   edit. Closing an ordinary tab removes its hit; closing a saved live instance
   retains the saved hit. Saving, unpinning and metadata changes update the
   corresponding counts/text from the worker's current catalogue.
2. A delayed pre-mutation result cannot restore the removed row/count; a text
   edit during refresh and a dismissed/reopened popup reject old results.
3. Go to remains selected for the same literal address. A deliberately selected
   surviving Target stays selected when rows reorder; removing it cannot select
   an unrelated row by old index or silently submit a different action.
4. Same-revision/no-op State emits no new refresh, query replies cannot create
   a refresh loop, and shutdown sends no refresh. Input value, caret and focus
   remain unchanged by model refresh.
5. A stale removed Tab/Saved activation reports a nonfatal error without
   resurrection or persisted mutation; later valid commands still work. A
   surviving saved row retains legitimate open-or-focus behavior.
6. Integration repeats the exact main-window Ctrl T/Ctrl W observation and
   records synchronized sidebar/palette rows and counts after settling, then
   checks deliberate selection and continued typing. Keep this separate from
   later page-focused shortcut acceptance. Integration runs the full gate.

**Implementation handoff:** `BrowserView::session_event` now refreshes an open
palette on a changed committed revision while not closing, clearing the old
global pending-query mapping first. `PaletteChoices::refresh` allocates a fresh
local request ID, removes outdated rows/counts and retains the selected Target
through repeated refreshes. A selected Go to action remains available; a
pending local action cannot accidentally submit the input as a search. A
surviving local result returns by identity; a removed ordinary selection stays
unselected until a deliberate arrow/click or input change. The existing
address-query fallback to Go to remains. Mouse callbacks reject obsolete
request identities. No refresh changes input value, caret, focus or popup
activation.

Six added pure-model regressions cover count/row replacement, repeated refresh
identity, stale replies/clicks, removed selection, failure/input reset, explicit
Go to choice and metadata/saved-count updates. Existing tests retain their
assertions with the new optional selection/event representation. Both subagents
independently reviewed the implementation and reported no actionable finding.
`git diff --check` passed for the changed files. No formatter, Cargo command,
build, test or UI check was run here. Integration applied formatting after the
first gate stopped at that check, then passed the full gate with 382 tests,
including all 60 app tests, and smoke. The six new regressions are therefore
covered by the new gate, while the 376-test checkpoint remains the baseline.
The exact UI reproduction remains unverified because activation of the fresh
synthetic window failed twice in the computer-use tool. Keep the source frozen
while integration prepares its diagnostic snapshot; do not call visual behavior
validated from the test or smoke result.

### Concurrent work with exact ownership

| Lane | Useful authorized work | Proposed ownership and boundary |
| --- | --- | --- |
| Integration | Run focused manual acceptance, collect failures and run the gate/benchmarks/native experiments in one execution lane. | Own shared handoff/status records and assign all shared files. Its frozen smoke outcome patch is preserved in the palette handoff. Do not benchmark beside native/UI work or builds. |
| Existing search team | Continue exact-semantics P1-05 analysis and its assigned implementation. | Keep `crates/atlas-search/` and search benchmark files with their assigned owner. No range-index promotion or public API change is implied. |
| Existing native investigation team | Analyze preserved failures and prepare bounded evidence that distinguishes host, fixture and timing hypotheses. | Retain existing platform/vendor/disposable-copy ownership. Do not infer a product fix from instrumented passes; integration executes. |
| Assigned palette worker | The stale open-palette correction passed source review and the gate; visible validation remains open. Shortcut forwarding is still review-only. | This chat's assigned files are `palette.rs`, minimal `ui.rs` wiring and focused private tests plus this report. Source remains frozen during integration validation. |
| Recovery application worker, now assigned | Bounded failed-start offer and explicit confirmation passed the gate; the actual card still needs visual validation. | This chat owns `crates/atlas-app/src/session.rs`, new `session/recovery.rs`, `session/worker_tests.rs`, new `ui/recovery.rs` and minimal `ui.rs` wiring after integration released source ownership. Search, platform, store, vendor and public APIs remain outside this assignment. |

Recovery was the highest-value independently assignable missing D49 behavior:
startup previously called `Store::open` directly without an application offer.
Integration subsequently assigned the bounded failed-start implementation.
The existing platform functions at
`atlas-platform/src/storage/recovery.rs:93,148,153` already provide the offer,
date and explicit confirmation. Acceptance must cover a damaged synthetic
primary, a dated offer before replacement, cancel preserving bytes, explicit
confirm only, preserved displaced files, no valid backup, ordinary I/O refusal,
changed identities, unsupported deletion state and restart. Do not classify
every open error as corruption or overwrite a daily database.

**Recovery design, independently reviewed:** retain the exact `RecoveryOffer`
on the session worker after failed startup handles have been released; send
only a private offer ID and backup date to the main-window confirmation card.
Only a matching explicit confirmation consumes the offer. Cancel, disconnect
or close drops an unconfirmed offer; confirmation failure remains visible and
must not silently choose another backup. Reopen normal storage only after the
offer is consumed. Ordinary I/O, locking, foreign/future schema and cleanup
errors are not evidence of corruption: the platform API remains the authority
for eligibility. Preserve unsupported-deletion refusals and the existing
healthy-header/hot-journal regression for SQLite code 776. Test app routing,
stale/duplicate confirmation, cancellation, restored restart and shutdown
during confirmation with synthetic data; reuse existing platform checks.

An error-triggered offer is only a bounded failed-start increment, not complete
D46 integration. `WindowsDirectoryLease::prepare` creates a missing primary,
so waiting for normal open to fail misses an absent database without a pending
recovery intent. Full integration needs a separately reviewed guarded preflight
that distinguishes fresh initialization from missing established data;
`RecoveryError::NoValidBackup` alone does not make that distinction. Integration
explicitly authorized only the failed-start increment after this design review.

**Implementation handoff, gate passed and visual validation pending:** `session.rs::open_session`
finishes failed startup cleanup before `session/recovery.rs` can probe the
existing guarded API. Cleanup, rollback/close and worker-ownership uncertainty
remain errors. Ordinary database I/O can permit a guarded inspection, never
restoration by itself; probe failures retain both structural error causes.
The worker retains one actual offer, publishes only a private ID/date, rejects
browsing and stale decisions while waiting, and consumes the offer only on
matching confirmation. Event-receiver loss also releases ownership. One normal
reopen follows successful confirmation, without a recursive recovery retry.

`ui/recovery.rs` displays the backup date in UTC, warns about changes since the
backup, and supplies explicit Restore and Cancel controls. Startup blocks
browsing/palette entry; closing blocks new page creation even if restored state
arrives late. Sticky engine/storage failures remain visible. Independent review
identified misleading progress text after a fatal restore error; the UI now
suppresses that progress after failure or session stop. Both independent final
source reviews passed with no remaining actionable finding. Seven new worker
regressions cover confirmation/cancellation, stale IDs across worker lifetimes,
restored restart, receiver loss, busy storage, absent backups, healthy-primary
I/O and uncertain cleanup/rollback refusal. Two UI policy/date tests cover
one-time confirmation and UTC calendar boundaries. `git diff --check` passed;
no formatting, compilation, test or UI execution was performed in this chat.
Integration subsequently passed all 70 app tests, then the full 407-test gate
with 75 app tests after the shutdown correction below. The earlier 382-test
gate predates recovery; the latest gate covers its regressions. The actual
recovery card still requires visual inspection. The separate missing-primary
API/policy proposal is recorded in `docs/missing-primary-startup-review.md` and
was explicitly approved by the owner on 2026-10-11. Its implementation is
frozen after independent source review and is not covered by the 411-test
gate. The owner also approved exactly three Windows-binding features for
atomic directory creation. Integration extended ownership of one private
recovery constructor for continuation boxing; the approved public API shape
remains unchanged. Integration's first platform run recorded 55 passed,
2 failed and 8 filtered tests in
`target/continuation-atomic-startup-platform-tests.log`. The two failures exposed
metadata-only handles that did not enforce rename refusal. Retained directory
and existing-primary opens now request list/read access; metadata revalidation
remains compatible with exclusive control handles. Both failing assertions are
unchanged, and three added regressions cover legacy acquisition and release.
The corrected source and 25 new platform/worker regressions passed independent
source review and await integration's formatting, tests and full gate. The
proposal records the diagnosis, source and log hash. No execution occurred here.

### Late user actions during shutdown

Two independent source reviews found that closing the browser left its popup
and action routes live until the page host stopped. A late Enter, tab action or
focus request could therefore be queued after shutdown began. Integration
assigned this bounded correction separately from shortcut forwarding.

`ui.rs::request_close` now retires the palette and clears pending query/Home
intent before native shutdown. An already-removed popup with a queued Submit
event is treated as an expected lifetime case. Session dispatch rejects new
user/recovery actions while allowing final native metadata and Shutdown;
page dispatch accepts only Shutdown once closing begins. Explicit palette,
shortcut and focus guards prevent late focus activation. Actual send failures
still become sticky errors, and the startup recovery guard is preserved.

Five regressions in `ui/shutdown_tests.rs` exercise real session-channel
ordering, rejected user actions, Home intent, startup cancellation, send
failures and page-command policy. Both independent source reviews passed
after a test import correction. Integration's independent review and full
407-test gate plus smoke passed; visible close-with-popup acceptance remains
open. No public API, dependency, native shortcut or `palette.rs` change was
needed for this correction.

### Existing browser shortcuts while the palette has focus

The palette is a separate window, so the main window's action handlers do not
receive its focused input actions. The approved correction adds queue-only
handlers in `palette.rs` for the eleven existing GPUI bindings: Ctrl T/W/L/K,
Ctrl Tab/Shift Tab, Ctrl Shift T, Ctrl D, Alt Left/Right and Ctrl R. No binding,
F5 route or held-key repeat policy was added. `PaletteEvent::Shortcut` reaches
the existing `BrowserView::shortcut` path. Both receiver termination and entry
cleanup use `finishes_entry`, which keeps Query and Shortcut events nonterminal.
Input, caret, selection, Go to intent, popup identity and shutdown guards remain
in place; committed model changes use the existing refresh path.

Integration's independent reviewer then found a concrete ordering defect missed
by the initial local reviews. Ctrl L/K can be queued immediately before Enter
or Escape removes the native popup. The older refocus request would reach
`open_palette`, whose stale-handle recovery cleared the retained popup identity,
causing the queued completion to fail its identity guard. The final correction
uses `dispatch_palette_shortcut` to skip only popup-origin Address/Palette
refocus when that popup's native window is already absent. It retains the
identity until completion drains. Live refocus, the other nine shortcuts and
main-window stale-handle recovery remain unchanged. The finding is recorded
in `docs/trapstoknow.md`.

Three regressions cover the event stream through all eleven shortcuts and four
completion paths; both refocus actions followed by queued completion, with an
actual dispatch callback that would invalidate the identity; and preservation
of ordinary shortcut delivery with either popup lifetime. Both final local
source reviews and integration's independent review passed. The corrected
411-test gate and smoke passed. This establishes source and automated regression
coverage; native key dispatch, fast refocus/completion, typing, caret and focus
acceptance remain open. The missing-primary API/policy is now approved, with
implementation and the atomic-creation feature amendment tracked separately
above; none of that work is validated by this gate.

### Compact synthetic manual matrix

Integration should prepare a fresh disposable profile and identify its current
loopback fixture explicitly: two named pages A/B, a plain typing field on each,
and links between them. A reserved failing address supplies a failure case.
Do not reuse the historical port without verifying the fixture. For each row,
record binary hash, fixture identity, fixed repetition count, exact observation
and every failure. Ten prescribed cycles below are a bounded diagnostic sample,
not a reliability guarantee. No accounts, real browsing data or personal
profiles are needed.

| Check | Expected observation | Current evidence |
| --- | --- | --- |
| Escape focus, ten cycles | Type a fresh marker in A; Ctrl K; type `about:blank`; Escape; append a marker without clicking. All text reaches A in order, with no stray page/palette input. | One owner-reported `beforeafter` cycle passed; repeated behavior is open. |
| Address submission | Ctrl L from A; enter B's explicit address; wait for results. Go to stays selected; Enter closes the popup and replaces the current tab's destination/title with B. | Policy tests and `about:blank` submission passed; this HTTP journey remains open. |
| Local result selection and refresh | With saved/live B present, type its address, deliberately select its local result and Enter. Existing B becomes active with one live instance. Separately change tabs through the main window while palette stays open; its count/rows must follow committed state. | Main-window close reproduced stale results. Refresh source and six regressions passed independent review and the 382-test gate; the visible repeat remains pending after inconclusive tool activation failures. |
| Enter focus return | Focus B's input, switch to A, use the palette to activate existing B, then type without clicking B. Input reaches B. | Open. A new document is not required to retain the old document's focused field. |
| Reactivation/stacking | Open palette, Alt Tab to a benign app, return and reactivate it. Typing goes only to the palette; it does not stay above the other app. Escape returns to the page. | Ownership/reactivation source exists; this sequence is unverified. |
| Page-focused shortcuts | Ctrl T/W, Ctrl Tab/Shift Tab and Ctrl Shift T each act once, follow live visible order, skip unloaded rows and keep last-tab Home open. | Core/worker regressions exist; actual focused-page sequence is open. |
| Palette-focused shortcuts | Exercise the eleven existing bindings while typing; input/caret and Go to intent survive. Rapid Ctrl L/K followed by Enter, local activation or Escape must complete once and return focus normally. | Queue routing and refocus regressions passed in the corrected 411-test gate. Native dispatch and timing acceptance remain open. |
| Navigation and feedback | A→B, Alt Left→A, Alt Right→B, Ctrl R. Title/address agree. A deliberate failure reports failure rather than loading forever/success. | Source and status tests exist; full native interaction is open. Test overlapping same-tab navigation separately and retain its result. |
| Save/unload/reopen | Save A, navigate its live page to B, close it; saved A survives. Reopen loads A; unpin leaves an open page ordinary. Repeated open focuses rather than duplicates. | Core/session tests exist; visible integrated lifecycle is open. |
| Resize, restart and shutdown | Resize/maximize and exercise available DPI settings without changing Windows configuration; page bounds follow. Restart only this synthetic profile: committed rows/order/live addresses survive, unloaded saved rows stay unloaded. Close with a palette/page active and observe clean exit. | Basic smoke and worker restart evidence exist; this complete visible journey is open. D44 width modes are missing, not test passes. |

Two concerns require precise interpretation. `ui.rs::focus_page` at lines
757–761 requests main-window activation then queues native focus without an
activation-completion handshake; `sync_pages` can also request focus after
selection. This is a source ordering concern, **not a reproduced current bug**;
the owner observed a successful Escape cycle. `NavigationStatus::observe` at
lines 44–64 keys completion by tab, not individual navigation: a late completion
can be attributed to a newer navigation in the same tab. Its real UI symptom
is unverified. A correction touching the native event contract belongs to
the existing navigation/native owner and may need explicit public-API approval;
this report authorizes no such change.

### Review and validation of this update

The slice-count subagent independently inspected all four maps, the ledger,
Git baseline and preserved gate results. The usability subagent independently
checked current source and prepared the manual matrix. Both distinguish the
implemented Go to/status fixes from the then-missing recovery prompt. The
primary reviewer checked the counts and relevant source paths. The readiness
update passed independent scope/count/evidence review. The usability reviewer
then independently confirmed the new stale-palette cause and missing-target
safety. The design review and two independent implementation reviews passed;
they do not replace the integration-owned execution gate.

The initial readiness update was documentation-only. The subsequent explicit
assignment authorized this chat's narrow palette/UI implementation above.
No Cargo, application, native test, benchmark or UI execution occurred here;
no integration app or real profile was used. Shared status/handoff, unrelated
product source and the root's smoke changes remain untouched by this chat.

## Historical assessment — 2026-10-10

The remainder records the earlier review at its original checkpoint. Its
“next,” “current,” missing-Go-to and missing-navigation-feedback statements
are historical and are superseded by the October 11 assessment above.

The next small D49 implementation should make literal address submission
reachable and predictable in the existing palette. Today, the arrival of a
local search result can change Enter from navigating the current tab to
opening or focusing a different destination. Complete the existing navigation
team's work and hand this correction to that team; do not start a competing
edit of its files. End-to-end acceptance and the unresolved WebView2 failure
remain higher-priority release blockers than adding more browser features.

This report is a source review, not execution or visual acceptance. Two
independent subagents audited user journeys and acceptance/dependency evidence;
the primary reviewer checked the central findings against source. This session
created only this report. It ran no Cargo, build, test, benchmark, browser or
native fixture, used no integrations or Computer Use, and read no personal
profile. Line references describe the working tree inspected for this review;
function names remain the anchors if concurrent authorized work moves them.

## Evidence and scope

The current checkpoint is the opening section of
[`handoff-integrated-preview-2026-10-10.md`](handoff-integrated-preview-2026-10-10.md),
lines 11–52. Its reported combined binary SHA-256 is
`3ca0bf045fc0826e7300a647307845733fb5249b085f52feeadbebf0abd4c276`.
This review did not independently hash or execute it.

- The latest gate passed dependency checks, formatting, Clippy and 301 tests,
  then Windows error 4551 prevented the public `frizbee_padding` executable
  from running. Later stages were unreached: the full gate failed.
- Separate rebuilt application smoke and patched native lifecycle runs passed.
  They do not establish interactive usability or explain the original
  `0x80000003` WebView2 failure. Construction fault-injection validation was
  still being prepared at the checkpoint.
- All 1,040 benchmark results retained their exact semantics, but only 10/26
  D48 cases passed; maximum p95 was 13.9768 ms and the worst sample 17.7388 ms.
- The owner stopped Computer Use. The existing synthetic window remains
  untouched, and popup typing/navigation and current visual acceptance remain
  unverified. Earlier permission and passing-gate prose lower in the handoff,
  the implementation ledger, or Plan 1 does not supersede this checkpoint.

[`decisions.md`](decisions.md), lines 10–18, and Plan 1's D49 decision bring
existing tabs, saved-page storage, backups and search into the browser window,
navigation and Ctrl K ahead of normal slice order. They preserve all other
requirements. The normal next unfinished slice is P1-05, followed by P1-06's
idle work and P1-07's design tokens; Plan 1's issue map begins these at lines
929–931. D49 permits corrections to the current integration, not a general
exception for P1-11 onward or Plans 2–4. The live design canvas was not opened;
this review makes no claim of design conformity.

## Journey audit

| Journey | Source-backed state | Remaining work and approved owner |
| --- | --- | --- |
| Open a tab or address | `session.rs::apply_model` resolves queued new-tab/navigation commands on the worker; `Worker::handle` saves before publishing. `ui.rs::sync_pages` creates only the desired active native page and waits for creation acknowledgement. | These paths exist. Native typing, first navigation, rapid queued actions and no duplicate views remain unverified together. P1-08–P1-10 under D49. |
| Save, unload, open/focus and unpin | `session.rs::apply_model` routes the shared core operations; `ui.rs::sidebar` presents saved and ordinary rows. `session/worker_tests.rs:176,704,946` cover fixed saved destinations, closure/restart and removed rows staying removed. | Core/worker behavior is implemented, rather than a missing bookmark feature. Actual saved-row clicks, persistence, native unloading and replacement-page ownership need integrated acceptance. P1-09; folders/import remain P1-21/P1-22. |
| Close and reopen | `ui/page_lifecycle.rs::observed_close` queues closure; `removed` releases native ownership only after the committed model loses the tab. `ui.rs:451–456` closes removed instances; late creation callbacks cannot claim a replacement. | Verify active-right-then-left selection, last-page Home, ordinary and saved reopen, repeated/stale native closes and failed persistence without native teardown. Existing tests at `worker_tests.rs:704,794,946` support parts of this, not the full UI journey. P1-09/D42/D43. |
| Back, forward and reload | `browser/page.rs:123–153` calls native Reload/GoBack/GoForward and checks availability. Source/title callbacks feed the session (`ui.rs:314,362`; `page.rs::register_source`). Toolbar and shortcuts are wired (`ui.rs:519–545,829–855`). | Missing implementation must not be inferred merely from absent visual evidence. Verify two synthetic destinations, title/address changes, saved-target independence and shortcuts with page focus. P1-10. |
| Palette, address and focus | Popup ownership, deferred input focus, reactivation and page focus return exist (`ui.rs:549–622,681–729`; `sync_pages:474–480`). Query identities reject stale replies (`palette.rs:95–119`; `ui.rs:264–276`). | Literal submission has the concrete gap below. Actual typing, Enter/Esc, reopen, Alt Tab, native focus return and stacking remain unverified. P1-10 and D49; full registry/scopes/previews remain P1-30–P1-33. |
| Normal startup and interrupted exit | `session.rs:204–247` opens the guarded store and restores its snapshot. `NavigationIntents::desired_page` and `sync_pages` load the active destination lazily. `worker_tests.rs:176,323` cover restart and queued shutdown. | App restart with several live tabs plus unloaded saved rows needs execution evidence. The unclean-session notice (`ui.rs:235–239`) describes reading committed state, not restoring a damaged database. P1-19 composition brought forward by D49. |
| Damaged database recovery | `session.rs:209–214` directly opens the store and propagates failure. `ui.rs:278–286` reports storage failure. No application recovery offer/confirmation flow exists. | This is missing integration, not merely an unverified UI. Existing `offer_recovery`, `RecoveryOffer::metadata` and `confirm` are available in `atlas-platform/src/storage/recovery.rs:93,147–153`. P1-04/D46, within D49's named backup scope. |
| Shutdown and engine failure | `ui.rs:746–804` waits for browser and session shutdown; browser Stopped triggers store shutdown (`ui.rs:383–397`). `session.rs:214–222` observes store close errors. `browser.rs::run` keeps the apartment alive around its browser state. | Separate smoke/lifecycle evidence supports limited shutdown behavior. Repeat navigation/palette/creation shutdown stress and durable restart through the integration owner. `ui.rs:354–360` reports process failures but does not implement renderer/shared-browser recovery; that feature remains P1-16. Do not equate shutdown ordering with a diagnosed crash. |

Paths in this table are under `crates/atlas-app/src/` except `browser.rs`
and `browser/page.rs`, which are under `crates/atlas-platform/src/`.

## Smallest proposed implementation: stable literal navigation

The source confirms a timing-dependent action. In
[`palette.rs`](../crates/atlas-app/src/palette.rs), `query` clears results and
selects index zero (lines 80–86), `receive` installs results and selects zero
(105–117), and `submit(false)` activates any selected hit (128–138).
[`ui.rs`](../crates/atlas-app/src/ui.rs), lines 700–709, sends a submission to
current-tab navigation but turns activation into FocusTab or OpenSaved. Ctrl L
only supplies initial text; its address intent does not reach `PaletteView`
(571–616).

For a typed `https://example.test/` with a matching saved/live destination,
Enter before the query reply submits that address; Enter afterward activates
the first local result. The displayed fallback is “Search the web,” which
cannot request literal navigation. The exact default ranking of a future
address row is not a settled decision in the documents. The recommendation
here is a concrete implementation proposal for review, not a new decision.

Add a visible “Go to …” action for input recognized by the existing address
parser. Keep it selected across asynchronous result arrival until the user
deliberately selects a local result. Mouse/arrow selection must still activate
that result, and Alt Enter must still search the web. The selected accent must
describe what Enter does, as required by `design/README.md`; silently overriding
Enter while highlighting a different result is rejected. Reuse
`navigation.rs::destination` and its existing validation rather than building
a second URL policy. This needs no full command registry, new dependency,
native page-content access or public crate API change.

**Proposed future file ownership:** integration should assign the navigation
team exclusive ownership for this correction of `crates/atlas-app/src/palette.rs`,
`crates/atlas-app/src/navigation.rs`, and only the necessary palette
construction/event wiring in `crates/atlas-app/src/ui.rs`. Put the pure action
selection tests beside that policy; no new shared source file is required.
The integration session owns acceptance records and serialized execution.
The recorded navigation allocation already includes `navigation.rs`,
`ui/navigation_intents.rs` and `ui/page_lifecycle.rs`; the proposed palette/UI
allocation is additional and requires integration coordination. This report's
author claims no edit ownership over those files. If the navigation team is
finished, integration must explicitly transfer ownership before assigning
implementation elsewhere.

Acceptance checks for that change are:

1. The same explicit synthetic URL chooses the same visible navigation action
   before and after matching results arrive, including empty/failed/stale
   replies and reopening a popup whose local request IDs restart.
2. Deliberately selecting a saved or live hit still follows its shared
   open/focus action; a saved row never gains a second instance. Alt Enter
   retains web-search behavior. The accent and Enter action agree.
3. Mixed-case schemes, refused schemes, embedded credentials, ordinary words
   and bare domains keep the existing parser's behavior. Pure tests cover
   action selection rather than duplicating validation code.
4. Integration runs the required gate and relevant regressions in its one
   execution lane. Error 4551 or an unreached stage stays a failure. Once UI
   testing is authorized again, use a fresh synthetic window to verify Ctrl L/K
   typing, result selection, submission, Esc, focus return and repeated opening.
   No Computer Use retry is authorized by this report.

## Next gaps after that correction

The navigation team should also review normal navigation failure feedback.
`ui.rs:338–352` consumes `NavigationCompleted.success` only in smoke mode,
and lines 363–366 set “Loading…” only on a loading-start event. The application
therefore lacks a corresponding normal completion/failure transition in these
handlers. A native error page may still communicate the failure; that behavior
was not inspected here. This is a D49/P1-10 feedback gap, not proof that a
failed page is blank or silently succeeds. A synthetic successful/failed
navigation sequence should verify truthful status and preserve serious errors.

Recovery is the next concrete missing D49 integration identified by both
reviewers. A future recovery owner can use the unchanged platform API and own
`crates/atlas-app/src/session.rs`, a new `session/recovery.rs`,
`session/worker_tests.rs`, a new `ui/recovery.rs`, and limited `ui.rs` wiring.
These are proposed paths, not files created by this review. Integration must
coordinate exclusive session/UI ownership and transfer any overlapping
palette/UI allocation before recovery implementation begins.
Show the verified backup date and require explicit confirmation before
`RecoveryOffer::confirm`; cancellation must leave the original data alone.
Synthetic checks must distinguish ordinary I/O refusal from proven corruption,
retain exclusive ownership while offering, preserve displaced files, reject
changed identities, handle no valid backup and unsupported deletion state,
and prove restored state can restart. Do not infer corruption from any generic
store-open failure or automatically replace a database. UI rendering and
confirmation remain necessary acceptance steps.

The remaining browser features retain their normal plan dependencies:

| Confirmed gap or incomplete delivery | Existing slice and evidence |
| --- | --- |
| Site-created windows are denied | P1-11; `browser/page.rs:80–83` explicitly returns `NewWindowResponse::Deny`. A successful simple page does not prove popup OAuth works. |
| Permission/notification workflow, reachable browser essentials and security state | P1-12–P1-14. The current browser command/event surface in `browser.rs:57–118` does not supply complete Atlas prompt/find/download/print/zoom/security flows; native defaults do not establish acceptance. |
| Hidden-tab suspension, fullscreen and process recovery | P1-15/P1-16; `Page::show` only changes visibility, and `ui.rs::browser_event` only reports process failure. |
| Draggable wide/compact/hidden sidebar, row reordering and hover peek | P1-07–P1-10/P1-17; fixed `SIDEBAR_WIDTH` and `sidebar` rendering in `ui.rs`, plus the absence of a reorder session command, leave these undelivered. Do not invent D44 thresholds without the planned review. |
| History, folders/import, full text/retention/clearing, complete Home/Favorites/settings and palette | P1-20–P1-33; the implementation ledger lists these open exits. Flat saved rows and typed local catalogue entries do not implement these journeys. |
| Daily-driver delivery and later plugins | P1-18/P1-34–P1-40 and then Plans 2–4. Product site tests, accessibility, default-browser use and performance evidence are still required; placeholder plugin rows are not implementations. |

P1-05 performance work remains the earliest normal implementation lane and
must preserve D48's complete-query semantics and three passing fresh-process
runs. P1-06's idle condition remains open. Neither the proposed palette fix
nor a recovery prompt closes either requirement. Owner-only sign-in, DRM and
daily-use checks remain with the owner.

## Independent review result

The journey reviewer and evidence/dependency reviewer independently confirmed
the missing recovery flow and the difference between source support and native
acceptance. The second reviewer separately checked the palette timing finding
and required a visible action matching Enter, rather than an invisible priority
override. Both agreed that D49 permits the narrow integration correction but
does not advance later features or waive predecessor exits. The final journey
review passed. The final evidence review requested one ownership correction,
now applied: existing navigation allocation is distinguished from proposed
session/palette/UI allocation, which integration must coordinate.

No product, vendor, plan, handoff or other report was edited. No commit, merge,
push, dependency, security setting or public API was changed. The integration
session retains all execution and final acceptance ownership.
