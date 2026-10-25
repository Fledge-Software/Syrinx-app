# Syrinx built-in browser protections — ad and tracker blocking

> **Status:** Planning draft, 2026-10-25. The owner requested a separate plan
> in the paid-services planning PR, using Brave as the product comparison.
> This document prepares future milestones, epics and issues; it implements
> no protection and changes no standing product or security decision.
>
> **Target tracker:** `Fledge-Software/Syrinx-app`. The BPR identifiers below
> are draft conversion keys, not GitHub issue numbers. Prepared by Codex.

## 1. Purpose and relationship to paid services

Users should be able to reduce unwanted advertising and cross-site tracking
without installing an extension or buying a subscription. The recommendation
is to make these protections free, built into Syrinx, and available without
sign-in, Supabase or a Stripe entitlement. The [paid-services plan](paid-services-plan.md)
covers optional cloud Sync, hosted AI and a device-wide VPN separately.
The owner has requested this plan; the defaults and delivery date below remain
proposals for review.

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

**Acceptance criteria:** synthetic tests cover exceptions, domain boundaries,
internationalized names, malformed lists and overlapping rules; rules cannot
invoke scripts or load external resources; benchmarks meet BPR-03 budgets;
known-good rules remain usable when a new set fails validation.

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

**Acceptance criteria:** synthetic servers confirm blocked requests do not arrive;
allowed traffic still works; no stale tab or exception state crosses contexts;
protected origins are never intercepted by a prohibited route; unsupported API
or matcher failure produces the approved degraded state, never a false active
indicator; the UI does not change profile-wide protection for one site.

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

**Acceptance criteria:** tampered, oversized, truncated and unauthorized old
snapshots are rejected; interrupted updates retain usable rules; authorized
rollback works; content cannot execute code; offline browsing still works;
status accurately reports stale or missing protection; no new release or CI
configuration changes occur without their required approval.

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
