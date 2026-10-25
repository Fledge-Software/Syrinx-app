# Syrinx paid services — Sync, AI and VPN

> **Status:** Planning draft, 2026-10-25. The owner requested this local
> document for later conversion into GitHub milestones, epics and issues.
> No implementation, provider agreement, deployment or GitHub publication is
> recorded by this document.
>
> **Target tracker:** `Fledge-Software/Syrinx-app`. Milestone and issue IDs are
> unassigned. The draft identifiers below are conversion keys, not issue numbers.
>
> **Owner decisions in this conversation:** the browser remains free; AI must
> interact with plugins; AI costs are priced at 3 times provider cost; monthly
> AI subscriptions cost $20, $100 and $200; annual subscriptions cost twelve
> monthly payments and retain the same monthly allowance; Sync will include
> passwords once a password manager exists; VPN protection is device-wide,
> with Guardian the preferred provider to evaluate.
>
> **Accounts follow-up, 2026-10-25:** Supabase is the owner's preferred database
> candidate, using the existing Fledge Software account if suitable. This plan
> adds the recommended private services repository, account/subscription schema
> and password-manager security boundaries. No account was inspected and no
> repository, Supabase project or paid resource was created by this work.

## 1. How to use this document

Free local software needs funding without making ordinary browsing dependent
on a subscription. These services charge for optional hosting, model usage and
VPN access. They leave local browsing and local plugin functionality usable
without a paid account.

This is the owner-requested preparation document, rather than a second live
backlog. Its format follows the repository's
[Fledge issue format](../.claude/skills/planissues/issue-format.md) and the local
Fledge-App epic/task templates. It keeps the reasoning beside proposed
milestones and issue bodies so a future session can publish them without
reconstructing this conversation.

[The 2026-10-16 tracking decision](decisions.md) ordinarily keeps work in issues
and uses no milestones. The owner's request authorizes this preparatory
document in milestone form; it does not silently change the standing tracker,
create labels or publish issues. Confirm the tracker migration when publishing.
Once published, issues become the source of work status, and this document
becomes the rationale and source map. Do not maintain two completion lists.

Use native GitHub sub-issues for parent/child relationships. Create milestones
and epics first, replace draft dependency keys with real issue links, and add
native blocking relationships where supported. Never fabricate issue numbers.
Inspect existing issues and labels before creating anything. Use Syrinx's
existing labels unless the owner approves additions; decision issues use
`question` under the present policy. Priorities and sizes below are planning
metadata, not instructions to create labels.

Each work draft has a reason, scope, acceptance criteria and dependencies. A
draft that grows beyond one reviewable pull request must be split before
implementation. Attribute any generated issue truthfully to Codex; do not copy
the Claude attribution from the reference format.

## 2. Source of truth and boundaries

Read [owner decisions](decisions.md), [the spec](spec.md),
[the design](../design/README.md), [architecture decisions](architecture-decisions.md),
and [development traps](dev/trapstoknow.md) before implementation.

The 2026-10-15 decision and its 2026-10-24 refinement supersede the earlier in-process plugin layout:
plugins are Rust compiled to sandboxed WebAssembly (WASM), built in separate
repositories from a template. A WebAssembly Interface Types (WIT) contract
and Rust host mediate capabilities. The approved package is `syrinx:plugin`
at `wit/v0_1/`, and the host crate is `crates/syrinx-plugin-host`; these are
approved implementation locations, not a claim that every capability exists.
AI must use those same capabilities;
this document does not revive native built-in plugin crates or unrestricted
direct calls into plugin storage. Existing tracking references are #25 for the
plugin runtime, #32 for Fledge (on hold), #35 for Notes and #42 for Forge;
recheck their current status before scheduling.

The existing work order remains binding: finish the earlier approved browser
work, then the plugin host and template, then Notes; Forge waits until Notes
is live as the owner's daily notes tool, as decided on 2026-10-20.
These service milestones do not authorize skipping that order. Fledge's real
client still waits for Stage C (D24), Fledge's API and the owner's explicit
resumption of its hold; AI does not bypass those dependencies.

The following are product guardrails for every milestone:

- The free browser works without a service account, subscription or connection
  to the service backend. Supporter payments remain separate from services.
- Windows 11 is the initial platform. No mobile or macOS deliverable is promised.
- Rust owns application logic. The local CodeMirror editor retains its D28
  buffer/selection/undo exception; disk changes obey the checksum handshake.
- Paid services are explicitly enabled. Review each new network request,
  account identifier, retention rule and permission before implementation.
  Billing/service records are not permission to expand D39 browsing analytics.
- Never send real customer content into development chats, fixtures, logs or
  issues. Tests use synthetic records and reserved domains.
- Existing page identity, IPC, script-injection, indexing exclusions,
  SmartScreen and WebView2 protections remain intact.
- Password sync depends on a separately approved password manager. WebView2
  password saving remains disabled (D11); this is not approval to enable it.
- Provider selection, dependency licences, public host-interface changes,
  system service installation and release changes receive the required review.
- No changes belong in Fledge repositories. Needed Fledge-side capabilities
  remain owner dependencies, not issues agents create in those repositories.

## 3. Product catalogue and commercial policy

### 3.1 Prices

All amounts are USD. Tax presentation, refunds and selling regions need an
owner-approved launch policy. Services are bought separately; no bundle is
promised before their actual costs are measured.

| Product | Monthly | Annual | Status and inclusion |
|---|---:|---:|---|
| Support Syrinx | $5 | $50 | Separate voluntary support; no service entitlement. |
| Syrinx Sync | $4 | $40 | Proposed price; encrypted notes, attachments, bookmarks and portable settings, with password sync added after its prerequisite. |
| Syrinx AI 20 | $20 | $240 | Owner-set price; $20 of retail-rated AI usage per monthly allowance period. |
| Syrinx AI 100 | $100 | $1,200 | Owner-set price; $100 of retail-rated AI usage per monthly allowance period. |
| Syrinx AI 200 | $200 | $2,400 | Owner-set price; $200 of retail-rated AI usage per monthly allowance period. |
| Syrinx VPN | $7.99 | $79.99 | Proposed price, contingent on Guardian terms; device-wide Windows protection. |

AI tier names are working identifiers. All three tiers have the same plugin
capabilities and permission rules; the purchased amount of usage differs.
Annual AI plans receive twelve monthly allowances, not one immediately
spendable annual balance. The owner's annual choice is twelve months' price
with the same monthly allowance, so there is no annual AI discount.

### 3.2 What 3 times AI cost means

The planning interpretation of the owner's instruction is:

```text
retail-rated usage = 3 × attributable provider cost
provider-cost allowance = retail allowance / 3
```

Thus $1 of provider usage consumes $3 of the customer's included usage.
This is a 3-times selling price (a 200% markup on cost), not cost plus 300%.
It leaves a two-thirds spread before payment fees and all other expenses.

| AI tier | Monthly retail allowance | Maximum provider spend, approximately | Annual provider spend at full usage |
|---|---:|---:|---:|
| AI 20 | $20 | $6.666667 | $80 |
| AI 100 | $100 | $33.333333 | $400 |
| AI 200 | $200 | $66.666667 | $800 |

Keep the ledger in fixed-point sub-cent units and round only for display.
Do not grant $6.67 twelve times and claim an exact $80 annual cap. Admission
control uses the exact budget and reserves the maximum approved request cost
before dispatch. Reconcile the reservation against actual billed usage.

Attributable cost includes provider-billed input, cached input, cache writes,
output/reasoning and separately charged tools where applicable. Record a
versioned rate card, model, service tier, currency conversion if needed and
effective time. Reconcile against supplier billing. Do not base pricing on an
unavailable promotional discount, forecast cache hit or invented token count.
Shared hosting and support are paid from the spread, not disguised as model
cost. Define treatment of provider refunds and discounts before launch.

Proposed usage policy: allowances reset on the subscription's monthly
anniversary, do not roll over, and are not cash balances. Show the remaining
allowance and an estimate before expensive work. Exhaustion stops new paid
requests; it does not disable local plugins. Additional usage requires an
explicit purchase at the same 3-times rule, never an automatic overage.
Top-up amounts, expiry and refund rules need a decision before top-ups launch.
Upgrades, downgrades and proration must not mint a second full allowance.
Internal failed retries are a Syrinx expense unless a clearly disclosed,
owner-approved policy says otherwise; bound them separately.

### 3.3 Payment fees and contribution

The research baseline is US domestic-card processing at 2.9% plus $0.30,
with another 0.7% for pay-as-you-go Stripe Billing. Payment Links do not remove
the recurring Billing fee. Actual fees vary by account, payment method,
international card, currency, refunds and optional services. [Source S1](#9-sources)

| AI tier | Monthly receipt after estimated Stripe fees | Provider cost at full allowance | Monthly contribution before other costs | Annual contribution after fees and full provider allowance |
|---|---:|---:|---:|---:|
| AI 20 | $18.98 | $6.666667 | $12.31 | $151.06 |
| AI 100 | $96.10 | $33.333333 | $62.77 | $756.50 |
| AI 200 | $192.50 | $66.666667 | $125.83 | $1,513.30 |

Annual contribution assumes one annual payment, twelve fully used monthly
allowances and no refunds. These amounts are not profit or demand forecasts.
They exclude hosting, engineering, security review, support, tax and losses.

| Other service | Monthly price | After estimated Stripe fees | Assumed variable service cost | Contribution before other costs |
|---|---:|---:|---:|---:|
| Sync | $4 | $3.56 | $0.75 target ceiling | $2.81 |
| VPN | $7.99 | $7.40 | $3 hypothetical provider quote | $4.40 |

The VPN annual price leaves about $3.40 per month after those same fees and
a hypothetical $3 monthly supplier cost. Minimum commitments could change
this entirely. The shared backend's $25–$100 monthly allowance is only an
initial planning budget, excluding storage, AI calls and VPN charges. Replace
it with an architecture-based estimate before contracting or launch. No
credible one-time development or security-review quote has been obtained.

## 4. Milestone map

These are proposed feature milestones, not replacements for the current v1
phase numbers. Every milestone gets one epic and its native sub-issues.

| Draft milestone | Epic title | Prerequisite | Exit |
|---|---|---|---|
| Services — Accounts and billing | Epic: Services — Accounts and billing | Owner approval of scope, hosting and data boundaries; existing work-order clearance. | Optional accounts, verified entitlements, cancellation and cost accounting work without affecting free browsing. |
| Sync — Encrypted personal data | Epic: Sync — Encrypted personal data | Services foundation and stable Notes persistence. | Two Windows devices safely sync selected encrypted records and recover conflicts/deletions. |
| AI — Plugin actions and metered usage | Epic: AI — Plugin actions and metered usage | Services foundation, reviewed WASM host contract and relevant plugins. | All three tiers enforce 3-times pricing and AI performs permitted plugin workflows with reviewable changes. |
| VPN — Device-wide protection | Epic: VPN — Device-wide protection | Services foundation, Guardian agreement and approved Windows integration. | Paid users receive verified device-wide protection with safe failure and removal behavior. |
| Sync — Password vault extension | Epic: Sync — Password vault extension | Encrypted Sync and a separately approved, implemented password manager. | Password records sync securely without exposing secrets to AI or the cloud service. |

Researching Guardian's terms does not authorize implementation ahead of earlier
work. The real Fledge AI adapter is explicitly held until Stage C and the
approved real-client work. Password sync is a follow-on milestone rather than
a reason to hold the initial Notes sync release.

## 5. Epic: Services — Accounts and billing

### What it is

An optional service identity and billing layer provides one place to purchase
and manage subscriptions. An entitlement is the backend's verified record of
which paid service an account may use. Sharing this layer prevents three
different cancellation flows and inconsistent access decisions.

### Why it comes where it does

Every paid service needs verified access before it provisions storage, model
calls or VPN credentials. Voluntary supporter checkout can ship independently
through hosted Payment Links because it grants no application features.

### Shape and guardrails

Keep service records separate from browsing information and local analytics.
The free app must not depend on the backend. Stripe owns payment collection;
Syrinx's backend verifies subscription events and owns service entitlements.
Never trust a return URL, browser-stored flag or desktop claim of payment.
Do not put payment or provider secrets in the distributed app.

### Repository and deployment ownership

Recommend a new **private** `Fledge-Software/Syrinx-services` repository for the
hosted account, subscription and service backend. The name is proposed; the
owner creates the repository and chooses its final name and location. SVC-01
also produces an architecture report covering accounts and subscriptions, so
that reasoning is preserved with the code.

| Location | Contents and responsibility |
|---|---|
| Public `Syrinx-app` | Browser service UI, local encryption, permission enforcement and a public versioned client/API contract. |
| Existing plugin repositories | Notes/Forge behavior and approved host-tool integration; no shared provider secrets. |
| Proposed private `Syrinx-services` | API implementation, database migrations and access policies, Stripe processing, AI ledger/gateway, sync authorization, Guardian provisioning, synthetic tests and deployment/operations documentation. |
| Hosted Supabase project | Private customer/service records and managed authentication. The repository holds schema definitions, never production database contents. |
| Private object storage | Encrypted sync blobs; Supabase Storage and R2 remain alternatives to evaluate for this layer. |
| Hosting secret manager | Stripe/webhook, model-provider, Guardian, database and Supabase server credentials. No secrets in either public or private Git history. |

Public source visibility is not the security boundary. A modified desktop app
can claim any subscription, so the backend must authorize each protected
operation and enforce the authoritative balance. Keeping the backend private
is an organizational/commercial choice, not a replacement for access controls.
Guardian's contract may separately constrain publication of its components.

Keep the existing public plan and issue tracking in `Syrinx-app` unless the
owner later changes the tracking policy. Private implementation pull requests
can reference those issues; public issue bodies and CI output contain only
sanitized architecture and synthetic evidence. No work is placed in a Fledge
application repository. Backend API compatibility is versioned so public
browser releases do not require users to read the private repository.

### Supabase recommendation and project isolation

Recommend **Supabase Auth plus managed PostgreSQL** for the initial service.
The combined identity and relational database fit accounts with several
subscriptions, billing-event reconciliation and transactional usage budgets.
Keep the choice provisional until SVC-01 confirms deployment region, auth
flows, costs, recovery and the service API runtime. The alternative is a
managed PostgreSQL service with separate identity and API hosting; that offers
more independent choices but adds integration and operations work. There is
no identified requirement in this plan that justifies that extra work yet.

The owner reports an existing free Supabase account for Fledge Software.
Use that administrative account/organization if capacity and permissions allow,
but create a **dedicated Syrinx project**. Do not share Fledge's database,
`auth.users`, credentials, storage buckets or migrations. Sharing an
organization still shares some administrative and billing exposure; review
membership and billing before selecting it. No project-slot availability or
account plan has been independently verified.

Separate synthetic development/staging from production. Start with local or
dedicated development infrastructure; no production customer data is copied
into fixtures, previews or agent environments. Project creation, paid upgrades,
email/domain setup and secret installation are owner/operator tasks.

Supabase's published Free plan currently includes two active projects,
500 MB database storage, 50,000 monthly active users, 1 GB file storage and
5 GB egress plus 5 GB cached egress. Inactive free projects can pause after
one week. These are platform quotas, not per-customer entitlements; Free's
1 GB does not fund the proposed 10 GB per Sync subscriber. [Source S9](#9-sources)

Use Free for development and validation. For paid production, recommend
budgeting **Pro from $25/month**, with the first project included and additional
projects starting at $10/month under the current pricing. Region, compute,
usage and organization-level billing affect the actual bill. This is part of,
not an addition to, the earlier $25–$100 shared-backend planning allowance.
SMTP email, API hosting, encrypted object storage, AI and VPN costs need
separate estimates; upgrading is not authorized by writing this plan.
[Source S9](#9-sources)

Configure a production email sender for confirmation/recovery messages.
Supabase's default SMTP is intended for testing and restricts recipients and
volume. A paid Supabase plan alone does not configure production email.
[Source S12](#9-sources)

### Account and subscription data model

These are proposed logical records, not an approved migration. Use a stable
internal account ID linked to the Supabase Auth user ID. Email may change and
must not be the subscription ownership key. An account can simultaneously hold
annual Sync, monthly AI and VPN with a different billing period.

| Logical record | Purpose | Authority |
|---|---|---|
| Account | Auth-user linkage, account state and minimum profile data. | Supabase establishes identity; the service controls lifecycle fields. |
| Billing customer | Stable account-to-Stripe-customer mapping. | Backend only; never accept a client-chosen Stripe customer as proof of ownership. |
| Subscription | Service, Stripe subscription/item and price IDs, currency, interval, status, period dates and cancellation. | Verified Stripe state reconciled by the backend. |
| Entitlement | Service/tier access and effective expiry or grace policy. | Backend only; never user-editable profile metadata. |
| AI allowance period | Exact retail allowance, monthly boundary and plan version. | Backend; annual payment creates monthly periods, not an immediate annual balance. |
| AI usage entry/reservation | Idempotent debit, reserved amount, settled provider cost and rate-card version. | Transactional backend ledger; no prompts or tool-result bodies. |
| Device/session grant | Device authorization, public key where needed, revocation and expiry. | Backend plus approved device-pairing protocol; no raw bearer secrets in logs. |
| Sync object/version | Ownership, opaque object ID, encrypted-object reference, version/deletion state and quota usage. | Backend verifies ownership; sensitive content/metadata remains client-encrypted. |
| VPN provisioning | Provider reference and activation/revocation state. | Backend with protected credentials. |
| Billing event receipt | Provider event ID, processing outcome and reconciliation state. | Backend; unique event IDs prevent replayed grants. |

Do not store payment-card details, browsing history, note plaintext, master
passwords or vault decryption keys in these records. Define retained billing
metadata and account-deletion behavior before migrations; a blanket cascading
delete must not destroy records required for reconciliation or create orphaned
paid services. Store the minimum operational event metadata needed for recovery.

### Authorization and authentication controls

Enable Row Level Security (RLS), PostgreSQL policies restricting which rows a
user can access, on every client-exposed table. Combine it with explicit schema,
table, column and function privileges. Deny unauthenticated access by default.
Clients may see only their permitted records and must never write subscription
state, entitlements, rate cards or usage balances. Keep administrative tables
out of exposed schemas where possible. [Source S10](#9-sources)

Supabase publishable keys can be distributed; they identify the project, not a
trusted user. Server secret keys and legacy `service_role` keys bypass RLS and
must remain server-side. Privileged handlers therefore perform their own
account/resource checks even when database RLS tests pass. [Source S11](#9-sources)

The API verifies token signatures, issuer, audience, expiry and subject, and
checks current authorization/revocation for sensitive operations. Do not trust
editable user metadata, a request's account ID, or stale token plan claims.
Bind checkout and billing-portal sessions to the authenticated internal account.
Use transactional reservations and unique idempotency keys for concurrent AI
requests. Test table access, database functions, object storage and service APIs
with two synthetic users, anonymous access, expired tokens and forged identifiers.

Design desktop sign-in using a reviewed authorization-code/PKCE flow (Proof Key
for Code Exchange), explicit redirect allowlists and per-attempt state. Keep
the verifier bound to the initiating device/attempt; verify compatibility with
the chosen Supabase flow rather than copying a web-only SDK example. Secure
refresh tokens through Windows credential protection and rotate/revoke sessions.
Configure signup/recovery abuse limits and multi-factor authentication for
administrators; approve the customer MFA policy before launch. [Source S14](#9-sources)

Require private buckets and ownership-checked, short-lived object access.
Encrypted blobs still need authorization, quotas and deletion controls. RLS and
TLS do not replace end-to-end encryption: Supabase authentication must never
give the service plaintext access to a password vault.

### Recovery and production readiness

Test database restoration and encrypted-object restoration as separate paths.
Supabase database backups do not include the bytes stored through its Storage
API, only their metadata. Recovery must reconcile metadata, blobs, deletion
markers and billing/provider state before reopening access. Publish recovery
targets and backup-expiry limits; deleting a live record is not immediate
erasure from every backup. [Source S13](#9-sources)

SVC-07 defines the production acceptance report: access-isolation results,
restore drill, email delivery, rate limits, key rotation, spending alerts and
measured capacity. Database-at-rest encryption is not a substitute for the
client-side encryption design in SYN-01 and PWS-01.

The foundation's dependency order is SVC-01, SVC-05, SVC-06, SVC-02, SVC-03,
SVC-04, then SVC-07. Identifiers remain stable for conversion; their numeric
order does not override these dependencies. Service adapters can be developed
once their prerequisites pass, but paid production releases also require SVC-07.

### Done when

Synthetic sandbox purchases, renewals, failures, refunds, cancellation and
event replay produce the expected entitlement exactly once. A service outage
does not prevent free local browsing. Published policies match behavior.

### Proposed issues

#### SVC-01 — Decide service hosting and customer-data boundaries

**Why this matters:** accounts and billing introduce remote identifiers and
retention that the current local-only analytics approval does not cover.

**What exists today:** `docs/decisions.md`, D39 and the usage-report documents
define local aggregate analytics. No hosted architecture is approved here.

**Work:** write the accounts/subscriptions architecture report described above;
evaluate Supabase as the preferred option, define record categories
and retention, prepare costs and account-recovery design, and decide refund,
tax presentation, selling-region and cancellation policies. Recommend minimum
service metadata rather than collecting browsing telemetry.

**Acceptance criteria:** an owner-reviewed decision confirms or rejects the
Supabase recommendation above and names the hosting boundary,
data flows, minimum records, deletion process and itemized budget. It explicitly
excludes browsing content from operational logs and explains tradeoffs.

**Blocked by:** owner decisions and work-order clearance. **Parent:** Services.
**Type:** decision (`question`). **Priority/size:** P1 / M, provisional.

#### SVC-02 — Add optional accounts and device authorization

**Why this matters:** service credentials need revocation without making an
ordinary browser session require a remote account.

**What exists today:** the local browser and state boundaries described in
`docs/spec.md` and `docs/decisions.md`; service identity is proposed.

**Work:** implement the approved sign-in/session flow and device authorization,
with secrets in an approved Windows credential facility. Distinguish account
recovery from decryption-key recovery and prevent cross-account access.

**Acceptance criteria:** synthetic accounts cannot access one another's service
records; expired/revoked sessions fail; local browsing works signed out and
during an outage; secrets do not appear in logs or exports.

**Blocked by:** SVC-01 and SVC-06. **Parent:** Services. **Priority/size:** P1 / M.

#### SVC-03 — Verify Stripe events and maintain service entitlements

**Why this matters:** delayed or replayed billing events must neither grant
unpaid access nor revoke a current customer's service incorrectly.

**Work:** configure sandbox products/prices, verify webhook signatures, process
events idempotently, reconcile out-of-order state and separate each service's
entitlement. Support renewals, payment failure, scheduled cancellation,
refunds/disputes and recovery after backend downtime. Never provision from
the checkout thank-you page alone.

**Acceptance criteria:** duplicate, reordered and delayed synthetic events
converge on Stripe's authoritative subscription state; each approved lifecycle
policy has a regression case; annual AI payments do not issue a year's usage
at once; no production charges are used for validation.

**Blocked by:** SVC-01 and SVC-02. **Parent:** Services. **Priority/size:** P0 / M.

#### SVC-04 — Expose service management and cancellation controls

**Why this matters:** users must understand their bill and retain access to
their local work after canceling.

**Work:** show subscription and service status, link the hosted billing portal,
and implement the approved cancellation/deletion messages. Define outages and
grace periods per service, because sync, AI and VPN have different failure risks.

**Acceptance criteria:** monthly/annual prices and renewal dates are accurate;
cancellation never deletes local files or passwords; account deletion clearly
explains cloud deletion and its retention limits; supporter purchases do not
grant service entitlements. Verify desktop accessibility and required themes.

**Blocked by:** SVC-03. **Parent:** Services. **Priority/size:** P1 / M.

#### SVC-05 — Establish the private services repository and isolated environments

**Why this matters:** accounts and supplier credentials need clear ownership
without coupling Syrinx's deployments or customer records to Fledge's systems.

**Work:** prepare the proposed `Syrinx-services` repository structure, public API
contract boundary, synthetic configuration and operator setup instructions. The
owner creates the private repository and dedicated Supabase project(s); confirm
existing organization capacity and paid-plan implications first. Document
development/production separation and where secrets are installed.

**Acceptance criteria:** repository name, private visibility and ownership are
confirmed; project identifiers belong to Syrinx, not a reused Fledge project;
environment documentation contains no secrets or customer data; synthetic setup
is reproducible; no upgrade or live deployment occurs without authorization.

**Blocked by:** SVC-01 and owner/operator provisioning. **Parent:** Services.
**Type:** operator/scaffold. **Priority/size:** P1 / M.

#### SVC-06 — Add account and billing schema with enforced access boundaries

**Why this matters:** an authenticated customer must not read another account's
records or grant themselves a paid plan through a direct database request.

**Work:** create reviewed migrations in the private backend repository for the
logical records above. Define foreign keys, uniqueness, lifecycle rules, private
schemas, RLS, privileges and safe database functions. Build two-user and anonymous
access tests, including the privileged service API's own authorization checks.

**Acceptance criteria:** migrations work on a fresh synthetic database; one
account can hold multiple independently billed services; client writes cannot
change subscriptions, entitlements or usage balances; forged account/Stripe IDs
are rejected; concurrent ledger reservations cannot exceed an allowance;
account deletion follows the approved retention/reconciliation policy.

**Blocked by:** SVC-01 and SVC-05. **Parent:** Services. **Priority/size:** P0 / M;
split schema and authorization changes before implementation if needed.

#### SVC-07 — Prove account-service recovery and production readiness

**Why this matters:** functioning signup and checkout do not prove recoverable
billing, reliable account recovery or safe operation when storage is restored.

**Work:** configure authorized production email and operational controls;
exercise key/session revocation, backup restoration, object/metadata consistency,
Stripe reconciliation, rate limits and spend alerts using synthetic data. Record
actual costs and obtain approval before a paid production deployment.

**Acceptance criteria:** restoration cannot resurrect deleted content or grant
stale paid access; account reset cannot decrypt a vault; backup limits and data
retention are documented; email/recovery and abuse-limit tests pass; Free-plan
pausing and quotas are not relied on for the paid production service. Subsequent
Sync, AI and VPN releases rerun the relevant checks with their own adapters.

**Blocked by:** SVC-04, SVC-06 and owner-approved production configuration.
**Parent:** Services. **Priority/size:** P0 / M.

## 6. Epic: Sync — Encrypted personal data

### What it is

Sync keeps selected personal data available across Windows devices and offers
encrypted cloud recovery. Encrypt content and sensitive metadata on the device
before upload. Search indexes remain local and are rebuilt from decrypted data.
The service may still observe account, traffic-size and timing metadata; do
not claim it learns nothing about service use.

### Scope and shape

| Record category | Scope |
|---|---|
| Notes, attachments, folders, tags and links | Initial release after Notes persistence is stable. |
| Bookmarks and Favorites | Initial release, individually enabled. |
| Appearance and keyboard preferences | Initial release through an explicit portable-settings allowlist. |
| Password vault records | Included later, after the password-manager prerequisite and separate security milestone. |
| Open tabs and saved sessions | Later candidate requiring explicit opt-in because URLs can be sensitive. |
| Other plugin preferences | Later candidate after each plugin declares a safe portable schema. |
| Forge projects | Outside initial sync; retain existing version-control workflows. |
| Fledge records | Remain under Fledge's own authority and integration decisions. |
| Cookies, OAuth tokens, history, machine paths and page-text cache | Excluded. Password inclusion does not include these other secrets. |

Proposed package: $4/month or $40/year, 10 GB total encrypted storage including
attachments and history, and up to 30 days of versions within that quota.
Warn before quota exhaustion and never silently discard current files. Decide
version eviction and maximum attachment size before launch.

Synchronization transfers records, never a live SQLite database. Protect
conflicting edits and deletion propagation. Revoking a device prevents future
access and uses reviewed key rotation; it cannot erase copies a device already
downloaded. Recovery uses a trusted device or recovery key, not an account
password-reset backdoor.

Proposed cancellation policy: stop sync at the end of paid access, retain local
files, allow 30 days for cloud export, then delete cloud data under the published
policy. Explicit deletion and backup expiry need separate documented bounds.
Sync is not an independent backup merely because it propagates files; recovery
depends on its version history and separately tested disaster recovery.

### Cost and alternatives

Cloudflare R2 is a candidate, not a selection. Its standard storage rate in the
research snapshot is $0.015/GB-month, or $0.15 for 10 GB before operations and
other services. Writes cost $4.50/million Class A operations; reads cost
$0.36/million Class B operations; internet egress is free under the published
pricing. Request patterns and retained versions still matter. [Source S2](#9-sources)

The initial variable-infrastructure target is at most $0.75 per active user per
month, subject to load testing. Engineering, recovery and support are separate
costs. Plaintext server storage would simplify recovery but breaks the proposed
privacy promise; copying the SQLite file breaks D13 and is rejected.

### Done when

Two Windows devices safely handle concurrent/offline edits, recovery, revocation,
quota exhaustion and deletion without silent loss. Service storage contains no
plaintext document content or sensitive metadata covered by the encryption
design. Cancellation leaves local use intact.

### Proposed issues

#### SYN-01 — Review the encrypted record and recovery protocol

**Why this matters:** encryption without a usable key lifecycle loses data or
creates an unnoticed account-recovery backdoor.

**Work:** specify record identifiers, authenticated encryption, encrypted
metadata, key hierarchy, trusted-device pairing, recovery, rotation, rollback
detection and revocation. Select maintained, licence-compatible components;
do not invent a cryptographic primitive.

**Acceptance criteria:** the threat model distinguishes content from metadata;
independent review covers lost keys, compromised devices and rollback; an owner
decision selects the protocol and recovery experience before implementation.

**Blocked by:** SVC-01 and stable Notes persistence design. **Parent:** Sync.
**Type:** decision/review. **Priority/size:** P0 / M.

#### SYN-02 — Implement encrypted record transport and local synchronization

**Why this matters:** transport retries and offline edits otherwise create
duplicates or overwrite newer work.

**Work:** implement SYN-01's versioned records, authenticated transport,
idempotent uploads, resumable attachment handling and offline queue. Sync only
selected categories and keep the local durable store in local app data.

**Acceptance criteria:** synthetic two-device retries converge; interrupted
uploads resume without corruption; cross-account access fails; the server
cannot decode protected content; excluded categories never enter upload queues.

**Blocked by:** SVC-02, SVC-03 and SYN-01. **Parent:** Sync. **Priority/size:** P0 / M;
split transport and client work if they cannot fit one reviewed change.

#### SYN-03 — Preserve edits, conflicts and deletions across devices

**Why this matters:** silent overwrite and resurrection of deleted notes destroy
trust even when transport and encryption work correctly.

**Work:** implement conflict preservation, deletion markers, stale-device
handling and restore semantics. Apply incoming edits through the Notes/host
contract and D28 checksum handshake, including dirty editor buffers.

**Acceptance criteria:** concurrent edits retain recoverable versions; an
offline device cannot silently restore deleted content; restore has explicit
semantics; restart and CRLF/LF cases pass; no live SQLite files are copied.

**Blocked by:** SYN-02 and reviewed Notes host operations. **Parent:** Sync.
**Priority/size:** P0 / M.

#### SYN-04 — Add sync selection, quota and recovery controls

**Why this matters:** users need to choose what leaves their device and recover
mistakes without understanding the storage protocol.

**Work:** add per-category switches, quota warnings, version restore,
device/recovery controls and the approved cancellation/export policy. Confirm
the 10 GB quota and history eviction rules with the owner.

**Acceptance criteria:** disabling a category has documented local/cloud
behavior; quota exhaustion preserves current work; export and recovery succeed;
cloud deletion follows the approved retention policy; tests and UI inspection
cover Graphite, Paper and one custom theme.

**Blocked by:** SYN-03, SVC-04 and retention/limit decisions. **Parent:** Sync.
**Priority/size:** P1 / M.

#### SYN-05 — Validate recovery, isolation and operating costs for release

**Why this matters:** a happy-path sync demo proves neither recoverability nor
the subscription's economics.

**Work:** run synthetic two-device failure/load scenarios, review encryption
implementation, exercise disaster recovery and measure storage/operation costs
at the proposed quota. Resolve critical findings before a limited beta.

**Acceptance criteria:** evidence covers lost/revoked devices, corruption,
replay, offline conflicts, deletion, quota and outages; restoration preserves
deletion policy; costs meet the approved budget; public claims match evidence.

**Blocked by:** SYN-04. **Parent:** Sync. **Priority/size:** P0 / M.

## 7. Epic: AI — Plugin actions and metered usage

### What it is

Syrinx AI searches selected plugin content and carries out permitted work. The
three tiers purchase different usage allowances, not different access rights.
Examples are drafting a note from selected sources, explaining a project file,
proposing a patch, and eventually drafting a Fledge follow-up.

### Why it comes where it does

AI requires the reviewed WASM host and plugin operations first. It cannot
implement missing plugin features by bypassing their sandbox. Notes assistance
comes first, followed by approved edits and Forge capabilities. The real Fledge
adapter remains blocked by Stage C and the real client.

### Shape and guardrails

The model proposes a structured action. Rust checks its schema, permissions,
resource scope and current record version, then invokes the plugin through
approved host capabilities. Tool calling lets the application remain the
executor rather than giving a model raw access. [Source S4](#9-sources)

| Plugin | First useful capability | Extension |
|---|---|---|
| Notes | Search selected notes, cite sources, summarize and draft. | Preview and apply edits with undo and version checks. |
| Forge | Explain selected code, search approved projects and propose patches. | Apply approved patches and run explicitly approved tests. |
| Fledge | Synthetic workflows on the stand-in client. | Approved real-client record search and actions after Stage C. |

The WIT/host action contract records inputs, outputs, resource permissions and
read/write effects. External sending, deletion, shell execution and account
changes require explicit user authorization. Retrieved content is untrusted:
instructions in notes or tool results cannot grant capabilities or approve an
action. Password-vault content is excluded from AI tools, retrieval and context.

Retrieval stays local; only selected, relevant context is sent for hosted
processing. Encrypted Sync does not make hosted AI end-to-end encrypted: the
device decrypts permitted excerpts, which the selected model provider and any
processing gateway can see. Explain this before enabling hosted AI. Preserve
the existing page indexing exclusions; this plan adds no general web-page
scraping or background upload of browsing content.

Keep transcripts local by default and exclude prompts, outputs, tool arguments
and customer content from backend logs. Set provider storage controls explicitly
and disclose its actual retention. OpenAI documents separate training,
application-storage and abuse-monitoring policies; `store=false` alone is not
a guarantee of zero retention. [Source S5](#9-sources)

### Cost and alternatives

Section 3's 3-times ledger replaces the earlier $15 plan and arbitrary $5 spend
cap. Provider selection remains open. Evaluate models using synthetic Syrinx
tasks and use an adapter so the product is not tied to one vendor's API.

As a dated benchmark, standard short-context GPT-6.1 Sol was listed at $2 per
million input tokens and $10 per million output tokens in the retrieved pricing
page. A total of 20,000 input and 2,000 output tokens is $0.06 provider cost and
$0.18 retail-rated usage at 3 times cost. Tool charges, repeated context,
reasoning output and subsequent model calls can increase that total; a user
message is not a fixed-cost unit. Recheck model availability and prices before
selection and encode the contracted rate card rather than this prose example.
[Source S3](#9-sources)

Unlimited subscriptions are rejected because agent loops have unbounded cost.
Message-count quotas are rejected because two messages may have very different
provider costs. No tier grants unattended privileged actions.

### Done when

Each monthly and annual tier enforces section 3's allowance and 3-times rate.
Selected plugin workflows produce cited answers and reviewable edits. Concurrent
requests, retries, forged calls and instructions hidden in documents cannot
escape permission or cost limits. Higher tiers receive useful additional usage,
not merely a higher bill for identical practical limits.

### Proposed issues

#### AIP-01 — Approve AI provider, retention and plugin action contracts

**Why this matters:** a model must not invent permissions or expose more content
than the user selected, and the service needs a verifiable supplier cost basis.

**Work:** compare provider retention, pricing and synthetic task quality;
specify versioned WIT action descriptions and review boundaries; approve the
3-times cost basis, allowance reset, top-up, retry and proration policies.

**Acceptance criteria:** recorded decisions name the provider baseline,
retention controls, model evaluation results and action permissions; the
password vault is explicitly inaccessible; contract changes receive review.

**Blocked by:** SVC-01 and approved plugin host/interface. **Parent:** AI.
**Type:** decision/review. **Priority/size:** P0 / M.

#### AIP-02 — Build the fixed-point usage ledger and request budget controls

**Why this matters:** concurrency, retries and annual renewals can otherwise
spend beyond the allowance or bill a task twice.

**Work:** implement versioned rates, per-request reservations, settlement,
idempotency, monthly allowance boundaries and supplier-cost reconciliation.
Bound tool steps, context, output and retry spending. Separate customer usage
from internally absorbed failures and retain only minimal billing metadata.

**Acceptance criteria:** all six subscription prices map to correct allowances;
annual plans issue twelve monthly periods; exact thirds do not drift through
cent rounding; concurrent reservations cannot overspend; retries and duplicate
events do not double-charge; proration cannot create free extra allowances.

**Blocked by:** SVC-03 and AIP-01. **Parent:** AI. **Priority/size:** P0 / M.

#### AIP-03 — Route permitted read-only plugin tools through the Rust host

**Why this matters:** plugin-aware assistance needs useful context without raw
filesystem access or a second permission system.

**Work:** expose selected Notes search/read operations through approved host
capabilities, validate model tool arguments and return bounded results with
stable source references. Connect the provider adapter to the usage ledger.

**Acceptance criteria:** answers cite accessible source records; excluded
notes are absent from context; forged tool calls and cross-scope references
fail; adversarial document instructions cannot widen access; costs settle once.

**Blocked by:** AIP-02 and stable Notes plugin operations. **Parent:** AI.
**Priority/size:** P1 / M.

#### AIP-04 — Preview and apply approved Notes changes

**Why this matters:** an AI proposal must not overwrite content edited while
the model was thinking or silently alter unrelated notes.

**Work:** present a change preview with source version, validate approval for
that exact change and apply it through the Notes host operation. Preserve
undo, reject stale revisions and distinguish a proposal from completed work.

**Acceptance criteria:** stale approvals fail safely; edits respect D28 and
sync conflict rules when Sync is enabled; cancellation changes nothing;
undo restores the previous content; no direct database writes bypass the plugin.

**Blocked by:** AIP-03. **Parent:** AI. **Priority/size:** P1 / M.

#### AIP-05 — Add Forge project context and reviewed patch proposals

**Why this matters:** code assistance needs project scope and meaningful diffs,
not access to arbitrary machine files.

**Work:** connect Forge's approved read/search capabilities and patch preview.
Exclude secrets and files outside the selected project; revalidate file versions
and path boundaries before applying an approved patch.

**Acceptance criteria:** synthetic out-of-project and secret-file requests
fail; previews show exact changes; stale patches do not overwrite edits;
approved patch application can be reviewed and reversed.

**Blocked by:** AIP-04 and approved Forge host capabilities. **Parent:** AI.
**Priority/size:** P1 / M.

#### AIP-06 — Execute explicitly approved Forge tests through host capabilities

**Why this matters:** proposed code changes need verification, but model output
must not become an unrestricted shell session.

**Work:** request approval for a concrete command and working directory; invoke
the approved Forge process capability with cancellation, resource limits and
bounded output. Treat command output as untrusted model context.

**Acceptance criteria:** changed commands require new approval; cancellation
stops owned child work; execution cannot silently widen scope; secrets in output
are not logged or transmitted indiscriminately; model/tool loops remain bounded.

**Blocked by:** AIP-05 and reviewed Forge process capabilities. **Parent:** AI.
**Priority/size:** P0 / M.

#### AIP-07 — Expose AI allowances, estimates and controlled plan changes

**Why this matters:** users need to understand how much work a subscription buys
and why a long task may require more allowance.

**Work:** show the three tiers, annual totals, monthly reset date, remaining
retail-rated usage and substantial-task estimates. Implement approved plan-change
rules and exhaustion behavior. Leave top-ups unavailable until their terms land.

**Acceptance criteria:** no annual allowance is shown as immediately available;
no automatic overage occurs; exhausted accounts retain local plugin use; both
monthly and annual explanations match the ledger; UI is keyboard-accessible.

**Blocked by:** AIP-02 and SVC-04. **Parent:** AI. **Priority/size:** P1 / M.

#### AIP-08 — Evaluate AI safety, task quality and unit economics

**Why this matters:** successful chat responses do not establish reliable tools,
safe edits or a viable high-usage subscription.

**Work:** build synthetic evaluations for citations, tool permissions, prompt
injection, stale edits, repeated calls, concurrent high-tier jobs and provider
failure. Measure actual cost and latency against the documented rate card.

**Acceptance criteria:** no unresolved critical permission or money-loss defect;
every supported tier stays within its budget at full utilization; provider
retention is accurately disclosed; model changes must rerun relevant evaluations.

**Blocked by:** AIP-04 and AIP-07 for Notes beta; AIP-06 for advertised Forge
execution. **Parent:** AI. **Priority/size:** P0 / M.

#### AIP-09 — Add Fledge actions when the real client is ready

**Why this matters:** business workflows require real API permissions and cannot
be shipped by treating the stand-in client as production integration.

**Work:** map supported Fledge actions to reviewed host tools, preserve its OAuth
scopes and user approvals, and allow explicit selection of Notes context for a
Fledge draft. Develop tests against the stand-in before real-client acceptance.

**Acceptance criteria:** supported actions match real client capabilities;
external sends and record changes require the appropriate user authorization;
cross-plugin transfers disclose the destination; missing scopes fail clearly.

**Blocked by:** AIP-01, the approved real Fledge client, Stage C (D24) and
the owner's explicit resumption of the Fledge plugin's hold.
**Parent:** AI. **Priority/size:** P1 / M; split by action family before building.

## 8. Epic: VPN — Device-wide protection

### What it is

The owner chose device-wide Windows VPN protection controlled from Syrinx.
Guardian is the preferred provider to evaluate, not an agreed supplier. Guardian
publicly describes its Brave partnership and Windows availability in partner
offerings, but not Syrinx-specific pricing, eligibility or SDK rights.
[Source S6](#9-sources)

### Why it comes where it does

A supplier agreement and supported Windows integration must precede sale.
Service billing can be shared, but a billing outage or expired subscription
must never silently turn protected traffic into unprotected traffic.

### Shape and alternatives

Use supported provider networking components with Syrinx controls for
connection, region and protection status. Include sleep/network recovery and
a tested kill switch: blocking traffic when the protected connection fails.
Decide startup, browser-close, expiry and user-requested disconnect behavior
explicitly. Explain that the connection protects the device, not just a tab.

Do not write a VPN protocol or operate exit servers. Browser-only proxying was
the alternative and does not meet the owner's device-wide selection. No
filtering/ad-blocking feature is implied by a provider's broader product name.

The target price is $7.99/month or $79.99/year. Target supplier cost is at most
approximately $3 per subscriber/month with no substantial minimum commitment;
this is a commercial target, not a quote. Device allowance remains undecided.
Brave's researched comparison was $9.99/month or $99.99/year, with broader
platform/device coverage; do not imply equivalent coverage. [Source S7](#9-sources)

### Done when

The contract permits the integration, protection claims are supported by tests,
credentials are revocable, failure behavior is explicit, uninstall is clean,
and a limited beta shows sustainable support and economics.

### Proposed issues

#### VPN-01 — Obtain and review Guardian commercial and Windows integration terms

**Why this matters:** retail pricing cannot be viable if minimum payments,
redistribution restrictions or unsupported APIs make the service uneconomic.

**Work:** prepare an owner-approved inquiry covering setup/minimum fees,
per-subscriber/usage charges, device limits, Windows APIs/SDKs, licensing,
redistribution, security evidence, logging, support, outages and termination.
Contact Guardian only when the owner authorizes the message.

**Acceptance criteria:** written terms or a documented refusal; a cost model
at low and high adoption; an owner decision on proceeding. No public promise
or sale precedes this result.

**Blocked by:** owner-authorized contact and provider response. **Parent:** VPN.
**Type:** operator/decision. **Priority/size:** P1 / M; external lead time unknown.

#### VPN-02 — Approve Windows service lifecycle and protection semantics

**Why this matters:** a device-wide tunnel changes system networking and must
handle crashes, expiry and removal without leaving the device unexpectedly open
or permanently disconnected.

**Work:** design optional privileged installation, signed component updates,
credential storage, connect/disconnect, kill switch, network changes, sleep,
browser closure, account expiry and uninstall. Audit dependency licensing.

**Acceptance criteria:** reviewed state/failure diagram, exact protected scope,
installation permissions and rollback procedure; approved behavior for explicit
disconnect versus unexpected loss; no change to WebView2 identity or security.

**Blocked by:** VPN-01 and SVC-01. **Parent:** VPN. **Priority/size:** P0 / M.

#### VPN-03 — Implement the provider adapter and connection lifecycle

**Why this matters:** the interface must show actual tunnel state rather than
assuming that a successful button click means protection is active.

**Work:** connect approved provider components through a narrow platform
adapter, implement reviewed state transitions and cancellation, and exercise
failures with synthetic traffic in a test environment.

**Acceptance criteria:** connection state matches independent network evidence;
sleep, network switch and crash recovery follow VPN-02; diagnostics contain no
visited sites or credentials; service installation occurs only with authorization.

**Blocked by:** VPN-02 and work-order clearance. **Parent:** VPN.
**Priority/size:** P0 / M; split provider setup from lifecycle if necessary.

#### VPN-04 — Provision paid VPN access and expose controls

**Why this matters:** provider credentials and the billed entitlement must agree
without making subscription events silently alter protection guarantees.

**Work:** provision/revoke provider access from verified entitlements; add
connect/disconnect, location, status and approved startup/close controls. Explain
device scope and surface failures with a recovery path.

**Acceptance criteria:** lifecycle tests cover purchase, renewal, cancellation,
payment failure and outage; expiry obeys VPN-02; no secret ships in the app;
UI never reports protection merely because an entitlement is active.

**Blocked by:** VPN-03, SVC-03 and SVC-04. **Parent:** VPN. **Priority/size:** P1 / M.

#### VPN-05 — Validate leaks, removal and beta readiness

**Why this matters:** a tunnel can appear connected while DNS or another traffic
path bypasses it, and a provider audit does not cover Syrinx's integration.

**Work:** test IPv4/IPv6, DNS, WebRTC, process crashes, network transitions,
other VPN coexistence and uninstall recovery. Commission integration security
review and document support ownership and measured operating costs.

**Acceptance criteria:** no unresolved critical leak or lifecycle defect;
kill-switch behavior is verified during failures; removal restores expected
networking; provider/owner responsibilities are documented. Google sign-in and
protected playback acceptance remain owner-operated under repository rules.

**Blocked by:** VPN-04. **Parent:** VPN. **Priority/size:** P0 / M.

## 9. Sources

External facts are a research snapshot from this conversation on 2026-10-25,
not permanent contracts. Recheck before procurement or publication.

| Key | Source | Supports |
|---|---|---|
| S1 | [Stripe Billing pricing](https://stripe.com/billing/pricing) | Pay-as-you-go Billing fee and standard US card-processing baseline. |
| S2 | [Cloudflare R2 pricing](https://developers.cloudflare.com/r2/pricing/) | Candidate storage, operations and egress cost benchmark. |
| S3 | [OpenAI API pricing](https://developers.openai.com/api/docs/pricing) | Dated token-cost example; not a model selection. |
| S4 | [OpenAI function calling](https://developers.openai.com/api/docs/guides/function-calling) | Model proposals are executed through application-controlled tools. |
| S5 | [OpenAI data controls](https://developers.openai.com/api/docs/guides/your-data) | Distinct training, application-state and abuse-retention policies. |
| S6 | [Guardian partnerships](https://guardianapp.com/company/partners/) | Existing partnerships; no public Syrinx quote or SDK grant. |
| S7 | [Brave premium](https://brave.com/premium/) | VPN retail comparison. |
| S8 | [Stripe subscription webhooks](https://docs.stripe.com/billing/subscriptions/webhooks) | Asynchronous payment and subscription lifecycle events. |
| S9 | [Supabase pricing](https://supabase.com/pricing) | Free quotas/pausing and Pro starting price; account capacity is unverified. |
| S10 | [Supabase Row Level Security](https://supabase.com/docs/guides/database/postgres/row-level-security) | Database row authorization; still requires appropriate grants and API authorization. |
| S11 | [Supabase API keys](https://supabase.com/docs/guides/getting-started/api-keys) | Publishable versus server credentials and RLS bypass. |
| S12 | [Supabase production email](https://supabase.com/docs/guides/auth/auth-smtp) | Default SMTP restrictions and need for production delivery configuration. |
| S13 | [Supabase database backups](https://supabase.com/docs/guides/platform/backups) | Database backups exclude Storage object bytes. |
| S14 | [Supabase PKCE](https://supabase.com/docs/guides/auth/sessions/pkce-flow) | Code-verifier binding; desktop callback implementation still needs review. |
| S15 | [Supabase Vault](https://supabase.com/docs/guides/database/vault) | Server-side secrets can be decrypted through SQL; this is distinct from client-side password-vault encryption. |

The owner messages in this session set AI prices, annual parity, plugin-aware
AI, device-wide VPN and future password sync, and identify Supabase as the
preferred candidate. Other limits, cancellation terms and final supplier
configuration remain proposals until explicitly accepted.

## 10. Epic: Sync — Password vault extension

### What it is

The owner wants passwords included in Sync once Syrinx has a password manager.
This extends the encrypted service to vault records; it does not add a password
manager to the current v1 scope or enable WebView2 password saving.

### Why it comes where it does

A vault needs its own encryption, locking, recovery and autofill design before
records can safely move between devices. Existing D11/D15 constraints remain
until the password-manager work changes the relevant scope explicitly.

### Shape and guardrails

Reuse reviewed sync transport and account authorization, but give the vault a
separate reviewed key/access boundary. Syncing notes must not implicitly unlock
passwords. Never expose vault plaintext to the hosted service, model providers,
AI retrieval, generic plugin search, logs or analytics. Do not sync cookies,
OAuth tokens or the WebView2 user-data folder as a shortcut.

Password sync is planned within the Sync subscription without a separate fee;
final storage/device limits remain subject to the service policy decision.
Already downloaded vault data cannot be remotely erased merely by revoking a
device. Explain that limit and rotate access for future data as designed.

### Password-manager and account security requirements

These requirements apply to the separately approved password manager before
password sync ships, as well as to the sync adapter. The manager's own
implementation still needs its existing scope and review process.

1. **Separate sign-in from unlocking.** Supabase proves the user owns an account;
   it must not know the vault decryption secret. An account-password reset,
   billing administrator or support agent cannot decrypt existing vault records.
   A lost recovery secret may make the vault unrecoverable; explain that before
   onboarding rather than introducing a hidden recovery backdoor.
2. **Encrypt before upload.** Use a reviewed key hierarchy and authenticated
   encryption with separate vault and note access boundaries. Encrypt protected
   record fields and sensitive metadata on the device. If a vault passphrase is
   chosen, use a reviewed memory-hard key derivation scheme with random salts
   and calibrated parameters. Design independent recovery and migration; never
   derive the only vault key directly from a Supabase login password or token.
3. **Protect local unlock material.** Use appropriate Windows credential/key
   protection for device-held secrets, with explicit locking and unlock rules.
   Limit plaintext lifetime in memory, clear sensitive UI on lock and prevent
   secrets entering diagnostics. Local operating-system protection is not a
   portable encryption scheme for syncing keys between machines, and cannot
   promise protection after the device itself is compromised.
4. **Review device enrollment and revocation.** Pair new devices through an
   existing trusted device or the approved recovery process. Account MFA helps
   protect enrollment but is not a substitute for vault encryption. Revoke
   future access and rotate keys as designed; do not promise remote erasure of
   old downloaded copies or revocation of passwords already learned.
5. **Treat filling and copying as sensitive actions.** The password-manager
   prerequisite must review exact site/origin matching, deceptive domains,
   frames, explicit user gestures and clipboard exposure. Clipboard clearing
   is best-effort and cannot retract copies made elsewhere. Autofill is a
   separately approved page-content interaction; this document does not waive
   Syrinx's page-view or Google restrictions or enable WebView2 password saving.
6. **Keep credentials out of AI and general plugins.** Vault plaintext, master
   keys and recovery secrets are unavailable to model context, generic search,
   exports used for analytics and unrelated plugin capabilities. Test every
   context-building and tool-result path, not just the visible AI screen.
7. **Make restore and cancellation safe.** Authenticate record versions and
   review rollback/replay behavior. Restore cannot silently resurrect deleted
   credentials. Subscription cancellation preserves the local manager and
   local vault access; cloud expiry follows a disclosed policy. Recovery drills
   use synthetic credentials and cover both database metadata and encrypted
   blobs, including backup deletion limits.

Do not put vault plaintext or unwrapped decryption keys in Supabase PostgreSQL,
Storage, Edge Function memory, service logs or a server-side secrets product.
Supabase's server-side "Vault" feature is not the customer's end-to-end
encrypted password vault. [Source S15](#9-sources) Any encrypted key envelopes stored remotely must
remain undecryptable to the service under the reviewed threat model. Independent
review covers these boundaries before customer passwords are accepted.

### Done when

Two authorized Windows devices synchronize synthetic vault records through
the approved vault interface, preserve lock/recovery boundaries and correctly
handle conflicts, deletion, offline devices and revocation. Local passwords
remain usable after subscription cancellation. Independent security review
findings are resolved before release.

### Proposed issues

#### PWS-01 — Approve the password-vault sync and recovery boundary

**Why this matters:** a generic note-sync key or account reset must not become
an unintended way to unlock saved passwords.

**Work:** review the implemented password manager's vault interface and threat
model against all seven requirements above; specify key separation, lock state,
device pairing, recovery, rotation, deletion and how vault metadata is protected.
Document AI exclusion and account-reset versus vault-unlock separation explicitly.

**Acceptance criteria:** an owner-reviewed security design references the actual
password-manager implementation; lost-key and revoked-device limits are clear;
no plaintext vault interface is available to AI or generic plugin tools; the
password-manager prerequisite has evidence for local locking, safe filling and
clipboard limits; Supabase administrators cannot recover vault plaintext.

**Blocked by:** separately approved and implemented password manager, SYN-05.
**Parent:** Password sync. **Priority/size:** P0 / M.

#### PWS-02 — Synchronize encrypted vault records through the approved interface

**Why this matters:** vault changes need reliable conflict/deletion handling
without bypassing the password manager's lock and save rules.

**Work:** adapt the reviewed vault interface to encrypted sync records and
handle offline updates, deleted credentials, concurrent edits and key rotation.
Avoid writing directly into the vault's private storage format.

**Acceptance criteria:** synthetic records converge without plaintext exposure;
locked-vault behavior matches PWS-01; revoked devices lose future access;
deletion does not silently resurrect; cancellation preserves local vault use.

**Blocked by:** PWS-01. **Parent:** Password sync. **Priority/size:** P0 / M.

#### PWS-03 — Review and validate password sync before release

**Why this matters:** ordinary file-sync tests do not establish credential safety.

**Work:** obtain independent review and exercise compromised/revoked devices,
recovery, rollback, lock/unlock, deletion and cross-account access with synthetic
vault data. Verify the exclusion from every AI/tool context route, account-reset
and new-device takeover scenarios, and database/object backup restoration.

**Acceptance criteria:** no unresolved critical findings; vault recovery and
revocation claims match measured behavior; logs and provider requests contain
no vault secrets; the owner approves release after reviewing evidence.

**Blocked by:** PWS-02. **Parent:** Password sync. **Priority/size:** P0 / M.

## 11. Conversion and validation checklist

Before creating issues, reconcile these drafts with the live tracker and check
that prerequisites have not already been implemented elsewhere. Use the source
sections here for each issue's rationale and observable completion criteria.
Proposed storage limits, cancellation terms and supplier choices go into
decision issues rather than being silently accepted by implementation.

Creation order: Services, Sync, AI, VPN, Password sync. Publish the five epic
bodies from their What/Why/Shape/Guardrails/Done sections, then the 29 work or
decision drafts beneath their appropriate epic. Attach native sub-issues and
translate draft dependency keys into issue links. One milestone assignment per
issue; other prerequisites are blocking links. Do not create Fledge-side work.

At implementation time, each slice needs relevant synthetic tests, independent
review and the repository's required gate. For changes to Syrinx itself, run
`cargo xtask check` and inspect visible behavior on Windows; service/website
repositories also run their own gates. New UI acceptance uses Graphite, Paper
and one custom theme. Changes to privileged installation or release scripts
retain their explicit owner-approval requirements.

This document's own validation is structural: verify prices, fixed-point
allowance arithmetic, draft dependency references, relative links, source
attribution and the distinction between proposals and owner decisions. No
production service or payment is exercised by writing it.

Prepared by Codex from the owner's instructions and the cited research.
