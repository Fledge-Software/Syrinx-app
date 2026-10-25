# Syrinx included features — protections, capture, saved tabs and reading

> **Status:** Planning draft, 2026-10-25. The owner requested a separate plan
> in the paid-services planning PR, using Brave as the product comparison.
> This document prepares future milestones, epics and issues; it implements
> no feature and changes no standing product or security decision.
> The owner also selected quick capture, saved-tab reopening and accessible
> reading for this included-features plan. All three remain free and local;
> their implementation designs and scheduling still follow the existing gates.
>
> **Target tracker:** `Fledge-Software/Syrinx-app`. The draft identifiers below
> are draft conversion keys, not GitHub issue numbers. Prepared by Codex.

## 1. Purpose and relationship to paid services

Users should be able to reduce unwanted advertising and cross-site tracking
without installing an extension or buying a subscription. The recommendation
is to make these protections free, built into Syrinx, and available without
sign-in, Supabase or a Stripe entitlement. The [paid-services plan](paid-services-plan.md)
covers optional cloud Sync, hosted AI and a device-wide VPN separately.
The owner has requested this plan; the defaults and delivery date below remain
proposals for review.

The owner-selected conveniences from the
[everyday-feature research](everyday-features-research.md) are now planned in
sections 5–7. They require no account, Sync or hosted AI subscription. Cross-device
availability remains an optional Sync service. The existing filename is retained
so earlier links continue to work; this document now covers six milestones.

| Milestones | Included feature | Prerequisites |
|---|---|---|
| BP1–BP3 | Ad/tracker protection, controls and safe rule updates. | The existing scope, page-security, licensing and feasibility decisions below. |
| QC | Quick capture into Notes. | Stable Notes documents, reviewed host capabilities and owner-approved UI. |
| ST | Save and reopen selected tab groups. | Existing saved pages/folders, generic tab model and safe lifecycle handling. |
| AR | Accessible reading and verified local read-aloud. | Approved content acquisition, isolated rendering and voice feasibility. |

These are separate dependency paths, not permission to skip the existing phase
order. The three convenience milestones do not depend on the blocker shipping.

Blocking in this plan applies to Syrinx's browsing traffic. It does not provide
device-wide filtering, hide an IP address, replace the VPN, or guarantee that
all advertisements and tracking disappear. It also does not add Chrome
extension support. The controls should feel understandable in the same way as
Brave's site protections: visible status, a small set of defaults, and a clear
way to fix a broken site.

Use the [Fledge issue format](../.claude/skills/planissues/issue-format.md) and
the conversion rules in [the services plan](paid-services-plan.md). Create an
epic and native sub-issues for each approved milestone later; check existing
issues and labels, replace dependency keys with real links, and avoid duplicate
work. The owner's preparatory-document request is not a change to the present
issues-only tracking policy. Priorities and sizes here are planning metadata.

## 2. Authority, scope and feasibility

[The spec](spec.md) and [agent rules](../AGENTS.md) exclude ad blocking from v1.
[Owner decisions](decisions.md), [architecture decisions](architecture-decisions.md)
and [the design](../design/README.md) remain authoritative. This is proposed
post-v1 work; scheduling it earlier or changing those decisions needs the
owner's explicit approval. It does not jump the existing browser/plugin/Notes/
Forge work order. Research and issue preparation do not authorize a prototype
that intercepts real browsing.

Review baseline: `origin/main` at `b2825da` on 2026-10-25. The existing
[dependency policy](../crates/xtask/src/dependency_policy.rs) still restricts
desktop networking; `syrinx-update-net` is a narrow application-update
exception. BPR-07 must propose and obtain approval for the rule-feed transport
and its exact gate changes, not treat the updater as general network access.
Keep the matcher and core model free of networking. A future page-filter gate
change must remain narrow enough to keep remote-page IPC, initialization
scripts and plugin custom protocols forbidden on ordinary page views.

Brave's Shields combines ad/tracker blocking with other protections. Its filter
lists and browser integration are a useful reference, not proof that WebView2
can reproduce its behavior. Syrinx needs its own measured coverage and claims.
[Brave Shields](https://brave.com/shields/)

| Capability | Proposed first release | Boundary or later work |
|---|---|---|
| Ad request blocking | Match approved network rules locally and stop matching requests before dispatch. | No promise to remove every ad, including ads delivered with wanted content. |
| Tracker request blocking | Use separately attributed tracker rules with the same local matcher. | Tracking that does not match a rule may remain. |
| WebView2 tracking prevention | Inspect and preserve the runtime's existing protection; expose only verified controls. | Its profile-wide setting is not a per-site Syrinx switch. |
| Site controls | Show actual protection status and let users pause Syrinx filtering for a site. | Exceptions must never silently change every tab's runtime protection. |
| Filter updates | Ship a reviewed baseline and an approved update mechanism with rollback. | Background downloads require a specific network/privacy decision. |
| Cosmetic filtering | Excluded from the first release. | Hiding page elements needs separate feasibility and page-content approval. |
| Scriptlets, fingerprint randomization, cookie-banner automation | Excluded from the first release. | These change page behavior and cannot be imported as ordinary list data. |
| HTTPS upgrades, cookie/storage controls, query-parameter removal | Record separately if requested later. | Do not bundle unrelated navigation or authentication changes into this plan. |

Microsoft documents `WebResourceRequested` for host-controlled request handling.
This is a possible implementation route, not a completed feasibility result.
The newer source-kind API distinguishes document, shared-worker and
service-worker requests; Microsoft warns about duplicate handling when worker
filters are attached to multiple views. Attribution and lifecycle need an
explicit design before per-site exceptions can be trusted.
[Request handling](https://learn.microsoft.com/en-us/microsoft-edge/webview2/how-to/webresourcerequested),
[source-kind API](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_22)

The API's URI pattern matches the whole normalized URI, not just its hostname;
a host-looking glob can also match text in a query. Treat patterns as event
selection, never proof of origin authorization. Microsoft also requires filters
before navigation for complete coverage and describes worker events across
views in the environment. BPR-03 must measure these constraints with Syrinx's
page and isolated plugin profiles; timing a filter attachment after navigation
cannot be assumed to preserve both coverage and the Google exclusion.
[Microsoft filter semantics](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_22?view=webview2-1.0.3595.46)

WebView2 enables tracking prevention by default, normally at Balanced or the
profile's persisted level. The environment and profile APIs have different
scopes. Inspect Syrinx's actual configuration before describing this as a new
feature or changing it. Do not implement a site exception by switching the
shared profile's prevention off.
[Microsoft tracking-prevention API](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2environmentoptions5)

### Guardrails that shape the design

- Keep the default page identity, SmartScreen, sandbox, web security and
  Evergreen runtime. Add no remote-page IPC, initialization scripts, header
  rewriting, debugging port, TLS interception or local certificate authority.
- The prohibition on request filters and injection on Google sites remains.
  A global filter that simply returns "allow" for Google still intercepts
  those requests and does not satisfy that rule. BPR-01/BPR-03 must establish
  whether an approved scoped design is possible, including navigation into
  Google, embedded Google resources and shared workers. Otherwise narrow the
  feature or return to the owner with a concrete decision; do not bypass it.
- Page-text indexing exclusions in D35 are a separate policy. Do not assume
  that banking/payment/no-store exclusions automatically define blocker
  exceptions, or use blocking to collect page text. Keep indexing unchanged.
- URLs can be used transiently in local matching, but not written to logs,
  crash diagnostics, analytics, issue reports or cloud accounts. Persisted
  site exceptions are private browsing preferences. Keep them local initially;
  any later encrypted sync inclusion needs explicit scope review.
- Use synthetic `.test`, `.invalid` and `example.com` fixtures. Google sign-in
  and Disney+/Hulu checks remain owner-operated. Never collect their credentials
  or browsing data in test reports.
- The browser host owns protection; sandboxed WASM plugins get no general
  traffic-inspection capability. Do not revive the old native-plugin layout
  or broaden the WIT interface merely to install an ad blocker.
- Protection callbacks must not intercept local plugin assets or create a
  route into their isolated profiles. Approved account, AI, Sync, VPN and rule
  update transports are separate host services; a site toggle cannot authorize
  those services or change their security policy. Test mixed page/plugin views
  in the shared WebView2 environment, not just two ordinary tabs.

### Code, lists and public-source boundaries

Recommend keeping the desktop policy, matcher adapter, UI, tests and updater
client in the public Syrinx repository, subject to the existing publication
process. Reading that code must not confer authority to change installed rules.
Keep signing keys and deployment credentials outside all repositories. This
work does not change repository visibility or publish new services.

Brave's Rust engine, `adblock-rust`, declares MPL-2.0 and is a candidate for
evaluation. Its current version, dependencies, security history, build cost,
maintenance and supported rule syntax still require an audit before selection.
Using its engine does not automatically provide Brave's browser integration.
[Engine source and licence](https://github.com/brave/adblock-rust)

Audit each list and resource independently. EasyList's published terms offer
GPLv3-or-later or CC BY-SA 3.0-or-later for covered material, with exceptions
for externally referenced lists. The CC route is a possible data-licensing
proposal, not an already approved repository exception. Obtain an explicit
licence decision and record attribution, redistribution and modification
obligations before bundling any such list. Do not copy Brave's complete list
catalogue, scriptlet resources or uBlock code without individual review.
[EasyList licence terms](https://easylist.to/pages/licence.html)

## 3. Delivery milestones and issue drafts

Each issue below has the shared current baseline: no blocker implementation
is authorized by this document, and feasibility on Syrinx's pinned stack has
not been demonstrated. Split a task further if it exceeds one reviewable PR.

### Milestone BP1 — Decide scope and prove the engine boundary

**Epic: Built-in protections — scope, licensing and feasibility.**

**What it is:** establish a credible, lawful and compatible route before adding
protection claims or changing the page lifecycle. Skipping this milestone risks
breaking sign-in or distributing lists the project cannot ship.

**Why it comes where it does:** product scope and licence decisions precede
dependency additions and interception experiments. Scheduling follows section 2.

**Shape:** evaluate native WebView2 tracking prevention plus a Rust network
matcher. An extension dependency would conflict with v1's extension limit;
engine replacement and system-wide interception are outside this project.

**Guardrails:** section 2 applies to every slice. A failed feasibility result
is reported to the owner; it does not authorize a workaround.

**Done when:** the owner has an approved scope, a list/dependency decision and
synthetic evidence showing what can be blocked safely and what cannot.

#### BPR-01 — Approve scope, protection defaults and the page boundary

**Why this matters:** the feature request must become precise behavior without
silently reversing v1 and Google compatibility decisions.

**Work:** prepare the decision report covering post-v1 scheduling, free access,
default-on versus opt-in filtering, first/third-party matching, site exceptions,
protected origins, unsupported traffic and failure behavior. Recommend a
conservative default after compatibility testing; defer an aggressive mode.
Name any exact policy change needed and obtain approval before changing it.
Distinguish top-level document origin, requesting frame/worker and destination;
define how each participates in protected-origin exclusion and site exceptions.
Specify fail-open versus fail-closed behavior for each failure class and visible
status; neither choice may be silently selected by an implementation callback.

**Acceptance criteria:** every proposed default has an owner decision or remains
explicitly blocked; runtime tracking prevention and Syrinx filtering have
separate semantics; the Google boundary is not implemented by an allow rule
inside a global interceptor; no implementation proceeds on an unresolved gate.

**Blocked by:** owner scope/scheduling approval and existing phase order.
**Parent:** BP1. **Type:** decision. **Priority/size:** P1 / S.

#### BPR-02 — Audit the matching engine and every distributable rule source

**Why this matters:** a compatible engine licence does not authorize all of the
data, code or script resources its upstream browser consumes.

**Work:** compare `adblock-rust` with a narrower approved matcher, then recommend
one using supported syntax, maintenance, security and performance evidence.
Record exact versions, transitive licences, list sources, update rights,
attribution and the treatment of unsupported rules. Request a specific list
licence exception if required. Use original synthetic rules before approval.

**Acceptance criteria:** the engine and each proposed shipped list have a
separate audit record; no GPL code enters Syrinx; all required exceptions have
owner approval; executable/scriptlet resources are excluded; pins and provenance
are ready for a later implementation PR rather than silently added here.

**Blocked by:** BPR-01. **Parent:** BP1. **Type:** audit/decision.
**Priority/size:** P1 / M.

#### BPR-03 — Prove request coverage and site isolation on synthetic pages

**Why this matters:** incomplete request attribution can leak traffic or apply
one site's exception to another site's worker.

**Work:** after boundary approval, build a disposable local fixture using the
approved stack. Measure top-level navigation, frames, redirects, popups, fetch,
beacons, cached responses, dedicated/shared/service workers and background
worker lifetime. Investigate WebSocket/WebRTC coverage rather than claiming
all traffic is intercepted. Test transitions into protected origins before any
filter attaches. Record API availability and any separately required binding
or wry change. Measure callback latency, memory and page-load overhead.
Test deceptive URI globs, redirects before the first callback, local plugin
assets, mixed profiles and a worker that outlives its originating tab. If the
required exclusions cannot be guaranteed, stop at the report and reduce scope
or seek a specific policy decision rather than starting BP2.

**Acceptance criteria:** a matrix marks each case supported, unsupported or
unproven; shared-profile/worker attribution and duplicate callbacks are resolved;
the protected-origin rule is demonstrated, including redirects and new windows;
unknown contexts have an approved policy; numeric performance budgets and a
go/no-go recommendation are reviewed before production work.

**Blocked by:** BPR-01 and BPR-02, including explicit prototype approval.
**Parent:** BP1. **Type:** feasibility. **Priority/size:** P1 / M.

### Milestone BP2 — Implement local ad and tracker protections

**Epic: Built-in protections — local matching and understandable controls.**

**What it is:** implement the approved subset with verifiable site controls.
Without accurate state, a protection icon can promise more than the engine does.

**Why it comes where it does:** all BP1 decisions and its go/no-go review must
pass before production dependencies, request handling or UI claims are added.

**Shape:** propose a window-independent Rust policy/matcher module for synthetic
tests; keep WebView2 handling in `crates/syrinx-platform`, local preferences in
`crates/syrinx-store`, and thin UI in the approved app/UI layer. The precise
module or new-crate placement receives architecture review before implementation.

**Guardrails:** no cloud classification, browsing telemetry or WASM traffic
access. The UI must distinguish Syrinx matches from opaque runtime protections.

**Done when:** both rule categories work within the measured boundary, site
exceptions are isolated, and users can tell when coverage is reduced.

#### BPR-04 — Implement a bounded local rules engine

**Why this matters:** downloaded text is untrusted input that can exhaust
resources or alter protection beyond the promised rule subset.

**Work:** implement the approved matcher adapter with immutable rule snapshots,
explicit allow/block precedence, request-type and first/third-party semantics,
and reviewed domain normalization. Bound file size, rule count, parsing time
and memory; reject or visibly classify unsupported rules. Keep matching free of
network access and disk writes. Associate matches with ad/tracker categories
without counting a request twice.
Allow only the reviewed block/exception subset. Reject redirect resources,
scriptlets, HTML/CSS injection, header/CSP changes and parameter rewriting even
if the selected engine or list syntax supports them. Parse URLs structurally
for policy decisions; an arbitrary string suffix is not a domain boundary.

**Acceptance criteria:** synthetic tests cover exceptions, domain boundaries,
internationalized names, malformed lists and overlapping rules; rules cannot
invoke scripts or load external resources; benchmarks meet BPR-03 budgets;
known-good rules remain usable when a new set fails validation; unsupported
modifiers cannot degrade into a broader rule that blocks unrelated requests.

**Blocked by:** BPR-02 and BPR-03. **Parent:** BP2. **Type:** implementation.
**Priority/size:** P1 / M.

#### BPR-05 — Apply approved filtering at the WebView2 boundary

**Why this matters:** blocking must occur before dispatch without freezing the
browser or violating the identity and protected-origin rules.

**Work:** implement only the approved interception route with explicit ownership,
callback teardown, bounded processing and safe rule-snapshot swaps. Handle tab
close, navigation changes and worker lifetime. Keep downloads and parsing off
the request callback. Preserve existing runtime tracking prevention and use its
supported APIs only for approved profile settings. No blanket global fallback.
Define the blocked response/error semantics for each supported resource type;
do not synthesize executable pages, rewrite allowed response headers or change
the certificate/security state. Release every native callback/deferral on
cancel, view destruction and error without retaining dead COM objects.

**Acceptance criteria:** synthetic servers confirm blocked requests do not arrive;
allowed traffic still works; no stale tab or exception state crosses contexts;
protected origins are never intercepted by a prohibited route; unsupported API
or matcher failure produces the approved degraded state, never a false active
indicator; the UI does not change profile-wide protection for one site.
Test the approved failure policy under matcher panic/error, deadline expiry,
controller replacement and filter-snapshot changes with requests in flight.

**Blocked by:** BPR-04. **Parent:** BP2. **Type:** implementation.
**Priority/size:** P1 / M.

#### BPR-06 — Add local protection preferences and site controls

**Why this matters:** users need a precise way to restore a broken page without
losing protections across their browsing session.

**Work:** design the controls against `design/README.md` before implementation.
Show Syrinx filtering state, approximate local blocked-request counts and rule
freshness. Define temporary versus saved exceptions, their exact site scope,
reset behavior and whether a reload is necessary. Avoid ambiguous "all
tracking blocked" claims. Store durable preferences locally under D13; clear
temporary state at its documented boundary. Keep exceptions out of Sync for now.

**Acceptance criteria:** controls work by keyboard and assistive technology in
required themes; the owner reviews the window; a site exception does not disable
unrelated tabs or WebView2's shared-profile protection; counters represent only
observed Syrinx matches, reset predictably and do not persist URL histories;
outage, unsupported and stale-list states are understandable.

**Blocked by:** BPR-05 and owner design approval. **Parent:** BP2.
**Type:** implementation/UI. **Priority/size:** P1 / M.

### Milestone BP3 — Maintain rules and verify release behavior

**Epic: Built-in protections — safe updates, compatibility and release.**

**What it is:** keep filtering useful as sites and lists change without turning
updates or breakage reports into a source of browsing surveillance.

**Why it comes where it does:** the working local engine and controls provide
the baseline against which updates, failures and regressions can be evaluated.

**Shape:** recommend a small curated, versioned rule feed rather than arbitrary
user-supplied subscription URLs at first. The owner approves its hosting,
update frequency and cost; accounts and paid entitlements are unnecessary.

**Guardrails:** update distribution is a security boundary. No automatic
exception uploads, remote browsing checks or personal-data fixtures.

**Done when:** update failure/recovery and the compatibility matrix pass, owner
checks are recorded, licence notices ship, and limitations match the evidence.

#### BPR-07 — Approve the rule distribution and operating-cost report

**Why this matters:** even a free local feature has bandwidth, maintenance and
security costs, and automatic downloads are a new network behavior.

**Work:** compare bundling rules with app releases against a curated update
feed. Recommend a release-bundled baseline plus independently updated signed
snapshots, subject to owner approval. Describe endpoints, refresh cadence,
jitter, caching, user controls, retention, publisher access and key rotation.
Estimate cost from compressed update size multiplied by active installs and
refresh frequency, plus hosting, review and incident-response time. Do not
invent a per-subscriber supplier fee or couple updates to Supabase sign-in.
Name the client transport crate and narrowly scoped dependency-policy amendment;
do not implement the feed inside the matcher or rely on the app updater's
existing network exception. Reuse verification concepts only after checking
that application packages and rule-data packages cannot be confused.

**Acceptance criteria:** the report has explicit traffic assumptions and an
owner-approved budget; no URL/history/account identifiers enter requests;
operator IP-access-log retention is stated; signing and publishing authority is
separated from the public client; network and deployment approval precede code
or provisioning. The update signature authenticates provenance, not list safety.

**Blocked by:** BPR-03 and BPR-04. **Parent:** BP3. **Type:** decision/report.
**Priority/size:** P1 / S.

#### BPR-08 — Validate, install and recover rule updates safely

**Why this matters:** a corrupt or hostile update must not disable protection
silently or cause every page to fail.

**Work:** implement BPR-07's approved channel. Validate provenance, version,
download limits and rule syntax before atomic activation; retain a known-good
snapshot. Define anti-rollback policy with an authenticated emergency rollback
path, interrupted downloads, offline operation and stale-list warnings. Review
new source/syntax changes before publishing. Keep credentials in runtime secret
storage, not in the client, fixtures or repository.
Allowlist fetch origins and validate redirects; rules cannot add download
destinations. Bound compressed and expanded size, parsing work and disk use.
Separate rule-package identity and trusted signing authority from executable
updates. Test signing-key rotation and a compromised-key response, not only a
normal snapshot rollback.

**Acceptance criteria:** tampered, oversized, truncated and unauthorized old
snapshots are rejected; interrupted updates retain usable rules; authorized
rollback works; content cannot execute code; offline browsing still works;
status accurately reports stale or missing protection; no new release or CI
configuration changes occur without their required approval.
Application packages cannot be installed as filter data or vice versa, and
decompression bombs and redirects outside approved origins fail safely.

**Blocked by:** BPR-06 and BPR-07. **Parent:** BP3. **Type:** implementation.
**Priority/size:** P1 / M.

#### BPR-09 — Run compatibility, privacy and performance acceptance

**Why this matters:** passing rule tests alone cannot prove that sign-in,
payments, media and navigation remain usable.

**Work:** repeat BPR-03's matrix against production handling and rule updates.
Add synthetic authentication/payment redirects, downloads and embedded media;
exercise two tabs with different exceptions, restart, navigation races and
failure recovery. Run the required gate and normal desktop checks, then ask the
owner to perform the established Google and Disney+/Hulu scenarios. Audit local
logs and exports using synthetic URLs. Compare resource use with protection off.

**Acceptance criteria:** `cargo xtask check` passes for implementation; numeric
budgets pass; exceptions and protected-origin handling pass every lifecycle
case; owner-only checks are recorded as pass/fail or explicitly block release;
no browsing content enters diagnostics; unresolved failures get real issues
and prevent claims of coverage they invalidate.

**Blocked by:** BPR-08 and owner availability for account/media checks.
**Parent:** BP3. **Type:** validation. **Priority/size:** P1 / M.

#### BPR-10 — Publish accurate help and approve the release scope

**Why this matters:** users need to distinguish a site exception, an outdated
list and a traffic type the product cannot protect.

**Work:** prepare help for controls, limitations, updates, exceptions and
privacy. Include licences/attributions and a breakage-report flow that the user
reviews without automatically uploading URLs or page content. Document the
relationship to VPN and SmartScreen. Capture excluded cosmetic/scriptlet and
other future ideas as decision issues only when the owner schedules them.

**Acceptance criteria:** release text matches measured coverage, contains no
Brave-parity or complete-anonymity claim, and clearly states free availability;
reports never request credentials; required publication/release approval is
recorded; implementation and feature availability are not inferred from this
planning PR.

**Blocked by:** BPR-09. **Parent:** BP3. **Type:** documentation/release review.
**Priority/size:** P1 / S.

## 4. Sequencing and unresolved decisions

The main implementation path is BPR-01 → BPR-02 → BPR-03 → BPR-04 → BPR-05 →
BPR-06 → BPR-08 → BPR-09 → BPR-10. BPR-07 can be prepared after BPR-04 and must
finish before BPR-08. These dependencies do not override the existing phase
order or owner approval gates.

Before implementation, resolve the schedule, free/default behavior, protected
origin strategy, list licences, matcher choice, runtime API coverage and update
hosting. Before release, resolve measured performance budgets, compatibility
results and support ownership. No commercial relationship with Brave, list
maintainers or a hosting provider is implied.

## 5. Milestone QC — Quick capture into Notes

**Epic: Included features — quick capture into Notes.**

**What it is:** preserve why a link or snippet matters by creating an inbox
note from text the user supplies, an explicit source link and an optional
chosen attachment. Reuse Notes' existing task affordances rather than build
a second task system. Ordinary capture, search and local export are free.

**Why it comes where it does:** Notes' document, attachment and safe-save
capabilities must be usable first. Capture writes through the same host and
plugin contract as normal editing; it does not invent a second notes store.

**Shape:** start with a small capture form reached through the existing command
system. Explicit paste and file selection are the initial input routes. Review
any later current-page selection or URL shortcut separately; no automatic
clipboard watching, screenshots, page extraction or background capture.

**Guardrails:** honor folder grants, containment, D28 saves and WIT compatibility.
No upload occurs unless the destination is already selected for Sync under its
own controls. Attachments are copied/imported only by the reviewed Notes path;
never delete or move the user's original file as part of capture.

**Done when:** a user can save a snippet with its source, find the resulting
note through the palette, and continue editing without duplicate or lost content.

#### QCP-01 — Define the capture flow and document contract

**Why this matters:** a fast capture command must still have an unambiguous
destination and must not acquire broader access than normal Notes editing.

**Work:** prepare the capture UI and command design; define title/body/source,
inbox selection, attachment limits, cancellation and recovery of an unfinished
form. Map every operation to the current Notes/host capabilities and name any
required compatible interface change. Preserve the existing app shortcuts.

**Acceptance criteria:** the owner reviews the flow; there is an explicit save
action and destination; missing Notes or a revoked folder grant fails clearly;
source links are stored as data and never fetched to create a preview; drafts
have a stated local retention policy; no new background capture is implied.

**Blocked by:** stable Notes document/attachment design, WIT review and existing
work-order clearance. **Parent:** QC. **Type:** design. **Priority/size:** P1 / S.

#### QCP-02 — Save captures through Notes' existing host operations

**Why this matters:** a separate capture writer could bypass safe-save rules,
lose attachments or create a duplicate note when the user retries.

**Work:** implement the approved form and host-mediated save. Preserve source
links without executing them, validate size/type/path limits, and identify
in-flight saves so uncertain results are reconciled before retry. Publish the
saved document through Notes' normal catalogue/search-feed path.

**Acceptance criteria:** synthetic captures reopen with text, link and attachment
intact; duplicate submission cannot create duplicate records silently; disk-full,
restart, cancellation and revoked-grant cases preserve recoverable input;
sign-out or unavailable Sync does not prevent local saving; private data stays
out of logs and unrelated plugin contexts.

**Blocked by:** QCP-01 and implemented Notes save/search capabilities.
**Parent:** QC. **Type:** implementation. **Priority/size:** P1 / M.

#### QCP-03 — Validate capture usability and release behavior

**Why this matters:** capture is useful only if people can save quickly and
find the result again without learning a second organizational system.

**Work:** exercise paste, source-link entry, attachment selection, keyboard-only
use, cancellation and reopening. Verify focus return from the form and access
through existing Notes tasks and search. Run the implementation gate and inspect
Graphite, Paper and one custom theme on Windows.

**Acceptance criteria:** a synthetic capture can be found, opened and edited;
screen-reader labels and save/error states are usable; no account or paid
entitlement is required; documented limits match the implemented input routes;
the owner accepts the visible flow and the required gate passes.

**Blocked by:** QCP-02. **Parent:** QC. **Type:** validation. **Priority/size:** P1 / S.

## 6. Milestone ST — Save and reopen tab groups

**Epic: Included features — resume saved tab groups.**

**What it is:** save selected browsing tabs into an existing saved-page folder,
then reopen the folder through the palette or Home. This is an extension of
saved pages and session handling within the one workspace, not new profiles.

**Why it comes where it does:** inspect existing folder actions and reuse or
extend them before creating an issue for behavior already implemented. The
generic tab model, saved-page persistence and close/navigation rules remain
authoritative.

**Shape:** recommend separate "save selected tabs" and "open saved folder"
actions, with any close-after-save choice explicit. Define ordering, duplicate
handling and lazy/bounded opening before implementation. Do not archive form
contents, cookies or live plugin state. Unsupported tab types are explained.

**Guardrails:** saving URLs is local persistence, never automatic Sync enrollment.
Do not close tabs merely because saving their URLs succeeded: forms, downloads
and dirty editors still require their existing lifecycle handling. Block unsafe
stored URL schemes at open time and preserve the browser's normal security checks.

**Done when:** a saved selection reopens predictably after restart without
duplicating storage or discarding work in existing tabs.

#### STB-01 — Reconcile existing folder actions and approve reopening behavior

**Why this matters:** adding another session mechanism would create conflicting
ownership and make the same folder behave differently across entry points.

**Work:** map current saved-folder, bookmark and restore commands; identify only
the missing convenience actions. Define eligible tab types, nested-folder
behavior, order, duplicate/reuse policy, existing-tab interaction and opening
limits. Review UI and explicit close-after-save semantics with the owner.

**Acceptance criteria:** the design references the existing generic tab and
saved-page model; implemented behavior is reused; no profiles or second session
database are introduced; partial save/open failures and unsaved-content handling
have explicit outcomes; allowed URLs are validated when reopened.

**Blocked by:** current saved-folder/lifecycle review and existing work-order
clearance. **Parent:** ST. **Type:** design. **Priority/size:** P1 / S.

#### STB-02 — Add selected-tab saving and bounded folder reopening

**Why this matters:** reopening a large group must not freeze the browser,
overwrite existing tabs or navigate to unsafe stored commands.

**Work:** implement only STB-01's missing actions through current commands,
persistence and tab lifecycle. Keep folder order deterministic; make partial
failure recoverable and bound simultaneous page creation. Preserve the original
selection if saving fails. Route all close requests through existing guards.

**Acceptance criteria:** synthetic sets survive restart and reopen in the
approved order; repeated activation follows the duplicate policy; malformed URLs,
unsupported tab types and nested folders follow the design; no saved item can
bypass navigation security; canceling an open operation leaves existing work
intact and reports which items opened.

**Blocked by:** STB-01. **Parent:** ST. **Type:** implementation.
**Priority/size:** P1 / M.

#### STB-03 — Verify tab recovery, accessibility and resource limits

**Why this matters:** a convenience feature should reduce tab clutter without
turning reopening or closing into a data-loss or memory problem.

**Work:** test large synthetic folders, duplicate URLs, restart and partial
failures; verify form/download/editor close guards with appropriate fixtures.
Measure reopening responsiveness and memory against budgets set in STB-01.
Inspect keyboard/focus behavior and the required themes; run the implementation gate.

**Acceptance criteria:** opening is bounded and cancelable; close-after-save never
silently bypasses a guard; the saved folder stays usable after failure; the
feature works signed out without a subscription; the owner reviews its UI and
the required gate passes. Do not claim saving a URL preserves unsent form data.

**Blocked by:** STB-02. **Parent:** ST. **Type:** validation. **Priority/size:** P1 / S.

## 7. Milestone AR — Accessible reading

**Epic: Included features — accessible reading and local read-aloud.**

**What it is:** make text easier to read with adjustable typography, spacing,
contrast and, where verified, local read-aloud. This accessibility feature is
free and does not consume AI allowance or require Sync.

**Why it comes where it does:** page content acquisition and the reading surface
need their own security/architecture review. D35 indexing approval does not
authorize a reader extractor, and its seven-day cache is not a reading archive.

**Shape:** first prove a permitted user-invoked page-to-reader route, and support
explicitly pasted text as a narrower input. Sanitize to an inert local document
with no remote scripts, styles, frames or automatic image/font requests. Select
the rendering location through design review; do not repurpose a plugin profile
or add page IPC. Read-aloud uses only verified installed local voices initially.

**Guardrails:** preserve Google/extraction exclusions and page-security rules;
unsupported pages receive an honest unavailable state. Manual-text reading can
ship as such, but must not be described as completed webpage reader support.
No automatic reading history, content snapshots, voice downloads or cloud speech.

**Done when:** approved inputs can be read comfortably with keyboard and assistive
technology, and optional speech has a verified local/offline path or a clear
unavailable state. Unsupported acquisition methods remain explicitly blocked.

#### RDR-01 — Prove content acquisition, rendering and voice boundaries

**Why this matters:** a reader can accidentally weaken page isolation, retain
sensitive content or send text to a speech provider without the user expecting it.

**Work:** prepare synthetic feasibility evidence for a permitted user-invoked
extraction route, sanitization and isolated rendering. Specify source attribution,
supported pages, content lifetime, local preference persistence and navigation.
Verify voice APIs, installed-language support and offline/network behavior.
Obtain approval for any needed page-content, interface or dependency change.

**Acceptance criteria:** a capability matrix separates webpage reading, pasted
text and speech; protected pages are not extracted; remote-resource execution
and fetching are absent; the owner reviews typography and controls; unproven
voice/extraction paths remain unavailable rather than falling back to cloud or
script injection; no implementation proceeds through an unresolved boundary.

**Blocked by:** owner design/security approval and existing work-order clearance.
**Parent:** AR. **Type:** feasibility/design. **Priority/size:** P1 / M.

#### RDR-02 — Implement the approved reading surface and local speech controls

**Why this matters:** accessible typography and speech must remain usable without
creating a second browser surface that silently loads untrusted content.

**Work:** render sanitized supported inputs with text size, spacing and contrast
controls, semantic structure and keyboard navigation. Add start/pause/stop and
voice/rate selection only for verified local voices. Stop speech and clear the
transient document at the defined close/replace boundary. Links open only through
the normal reviewed host navigation route on a user gesture.

**Acceptance criteria:** hostile synthetic markup cannot execute or load remote
resources; input size and processing are bounded; reading preferences persist
without persisting content; unavailable voices fail clearly; text and speech
work offline for supported inputs; no paid service or new browser profile is used.

**Blocked by:** RDR-01. **Parent:** AR. **Type:** implementation.
**Priority/size:** P1 / M; split renderer and speech if needed.

#### RDR-03 — Validate accessibility, privacy and supported-page claims

**Why this matters:** a visually simplified page is not necessarily accessible
or faithful to its source, and speech cancellation must actually stop output.

**Work:** test headings, lists, links, long text, unsupported languages, malformed
markup, screen readers, zoom and keyboard focus. Measure speech stop/replace,
content cleanup, offline operation and absence of unexpected network requests.
Inspect Graphite, Paper and one custom theme and run the implementation gate.

**Acceptance criteria:** controls and reading order are usable with assistive
technology; source text is not rewritten by an AI model; unsupported pages and
missing voices are accurately explained; no content enters history, diagnostics
or Sync automatically; the owner accepts the UI and the required gate passes.

**Blocked by:** RDR-02. **Parent:** AR. **Type:** validation. **Priority/size:** P1 / M.

## 8. Included-feature conversion map

Publish six epics and 19 work/decision drafts if the owner requests tracker
conversion: BP1–BP3 keep BPR-01 through BPR-10; QC owns QCP-01 through QCP-03;
ST owns STB-01 through STB-03; AR owns RDR-01 through RDR-03. Preserve the
existing BPR keys and convert each new chain in numeric order after its stated
prerequisites. Reconcile existing saved-folder and Notes work before publishing
duplicates. Research-only split view and translation are outside this plan.
