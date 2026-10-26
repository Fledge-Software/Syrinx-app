# Syrinx everyday features — included conveniences and paid opportunities

> **Status:** Research recommendations, 2026-10-25, requested by the owner
> during review of PR #163. Researched by a Codex subagent and reviewed by
> Codex against the current architecture. No feature, price, provider contact,
> implementation order or new data collection is approved by this report.
>
> **Owner selection, 2026-10-25:** quick capture, saved-tab reopening and
> accessible reading have moved into the [included-features plan](browser-protections-plan.md)
> as epics QC, ST and AR. Encrypted attachment sharing and household Sync
> have moved into the [paid-services plan](paid-services-plan.md) as later
> epics. This report retains the research rationale; the plans own their
> issue drafts. Split view, translation and email aliases remain research only.
>
> **Commercial approval, 2026-10-25:** the owner subsequently approved Sync at
> $4/month or $40/year with sharing included, Household at $10/month or $100/year
> for six people, and repeatable 30 GB storage units at $2/month. Cloudflare R2
> stores encrypted files; Supabase manages accounts/subscriptions. The paid plan
> is authoritative for all approved limits and later billing/security details.

## Recommendation

Make the free browser better at saving context and returning to unfinished
work. The strongest first research bundle is quick capture into Notes, easier
reopening of saved tab groups, and accessible reading. These improve daily
use without introducing a new account, business application or subscription.
For a later paid expansion, investigate encrypted attachment sharing within
Sync before a separate product; household sharing follows only after individual
Sync is reliable. Email aliases are a lower-priority supplier investigation.

The [paid-services plan](paid-services-plan.md) covers Sync, hosted AI, VPN,
later password sync and the selected sharing epics. The
[included-features plan](browser-protections-plan.md) covers free ad/tracker
blocking and the three selected conveniences. This report supplements them;
it is not another implementation backlog. Convert the plans' drafts only when
the owner requests issue publication, checking duplicates and phase dependencies.

These priorities are product judgments based on concrete use cases and existing
products, not evidence of demand, willingness to pay or completed WebView2
feasibility. No customer data, provider account or supplier terms were accessed.

## Included features worth validating first

| Priority | Everyday problem | Recommendation | Fit and prerequisite |
|---|---|---|---|
| 1 | "I saved the link but forgot why." | Quick capture of explicitly supplied text, a source link and an optional chosen attachment into a Notes inbox. | Reuse Notes documents and its planned tasks. Start with paste and a file picker; do not assume permission to read the clipboard or extract a page. |
| 2 | "I want to finish this later without keeping twenty tabs open." | Save selected tabs into an existing saved-page folder and reopen the group from the palette or Home. | Improve saved folders/session behavior within the single workspace. Do not create profiles, a second session store or another workspace system. |
| 3 | "This page is hard to read." | Accessible reading with adjustable text size, spacing and contrast; local read-aloud where verified. | First prove permitted extraction into a sanitized local surface, or start with explicitly pasted text. Accessibility should not require an AI subscription. |
| 4 | "I keep switching between two things." | Two visible panes for a comparison, a booking beside an itinerary, or a page beside Notes. | Requires an approved layout and focus design, generic tab-model ownership and Windows resource measurements. It is not automatically supplied by WebView2. |
| 5 | "I need to understand this text." | Translate selected or pasted text locally if a suitable engine is practical; offer explicitly chosen hosted translation through the existing AI allowance. | Audit local engine/model licences, storage and performance. Downloads and whole-page rewriting need separate approval. Do not introduce another translation subscription. |

For capture, define success as creating a source-linked note that can be found
again through the existing palette and turned into an already-planned task.
Vivaldi demonstrates a text/link-to-notes workflow; its screenshot behavior is
not a recommendation to capture Syrinx pages automatically.
[Vivaldi Notes](https://help.vivaldi.com/desktop/panels/notes/)

For resume, test the action against current saved folders before designing
anything new. A convenience action must not close tabs with unsaved forms or
dirty editors without the existing confirmation/lifecycle handling. Saved
sessions in Vivaldi show the user benefit of reopening a set; Syrinx can offer
that benefit within its settled single-workspace design.
[Vivaldi sessions](https://help.vivaldi.com/desktop/panels/sessions-panel/)

For reading and translation, first verify the boundary between the remote page
and Syrinx's own local reading surface. D35's page-text indexing permission is
not a general capture permission, and its seven-day cache is not an offline
reading archive. Firefox provides a useful comparison for adjustable Reader
View/read-aloud and on-device translation, but those implementations do not
establish Syrinx's route. Verify installed voice availability, language quality
and whether a selected voice makes network calls before calling it local.
[Firefox Reader View](https://support.mozilla.org/en-US/kb/firefox-reader-view-clutter-free-web-pages),
[Firefox translation](https://support.mozilla.org/en-US/kb/website-translation)

For split view, measure keyboard routing, focus, overlay placement, accessibility
and memory with two active views, including a plugin beside a remote page. Edge
offers a comparable user-facing feature; embedding WebView2 does not reproduce
Edge's entire browser UI or feature set.
[Edge Split Screen](https://www.microsoft.com/en-us/edge/features/split-screen)

These included features have engineering, maintenance and accessibility-test
costs. Local processing avoids a per-call cloud bill, but downloaded models or
voices may add distribution costs. Do not make a claim of zero operating cost.

## Paid opportunities after the existing services

### 1. Encrypted attachment sharing within Sync

The everyday problem is sending a travel document, receipt or note attachment
without creating a permanent public copy. Recommend evaluating immutable,
explicitly selected attachments/exports with expiring and revocable links.
Keep local file export free. The owner subsequently approved bounded sharing
within Sync at no additional subscription charge, and repeatable storage units
of 30 GB for $2/month. These increase storage only; sharing limits stay fixed.
The paid-services plan records the authoritative prices and limits.

This requires a separate recipient-access and key protocol. Private Sync alone
does not support safe public links. Define recipient keys or link secrets,
expiry, revocation, optional passwords, brute-force and abuse controls, quotas,
safe previews/downloads and deletion. If a key is carried in a URL fragment,
review recipient-page scripts and logging: a fragment alone is not proof that
the service cannot learn it. Link possession may grant access. Revocation cannot
erase a file someone already downloaded. Exclude password-vault sharing and
live collaborative editing from the first investigation.

Proton Drive demonstrates expiration and password-protected links. Bitwarden
Send illustrates the distinction between text sharing and paid file sharing.
They establish useful product patterns, not reusable protocols or reseller
rights for Syrinx.
[Proton sharing controls](https://proton.me/support/password-protect-files-proton-drive),
[Bitwarden Send](https://bitwarden.com/help/create-send/)

Costs to measure: stored ciphertext and versions, transfer/provider charges,
request volume, abuse handling, recovery and support. Public downloads can cost
much more than the subscriber's own device sync. Validate costs against the
approved 50 GB individual / 250 GB household monthly shared-download budgets
and the paid plan's safe exhaustion policy before launch. R2 holds all encrypted
Sync content; never store file bytes in Supabase, which holds accounts,
subscriptions and minimal service metadata. The paid plan recommends device-side
AES-256-GCM and requires independent protocol and key-management review.

### 2. Household shared Notes collections

The everyday problem is keeping a shopping list or itinerary available to the
people who need it. Investigate a later Sync extension with separate service
accounts and explicit collection membership. This is a recommendation about
sharing data, not adding browser profiles or promising a family password vault.

Prerequisites include multi-user encryption keys, invitations, roles, member
removal, concurrent edits, recovery, billing ownership and export after
cancellation. Removing a member can stop future access but cannot revoke data
already copied. Deleting the paying account must not silently delete another
member's only data. Password-vault keys and AI access stay outside a shared
Notes collection's authority.

Validate demand on the supported platform before building: a Windows-only
household feature may be less useful than users expect if family members need
phones. A mobile or web client is a separate scope decision. Cost drivers are
member activity, storage, support and substantial key-management engineering;
the later owner-approved price and seat allowance are recorded in the paid plan.

### 3. Supplier-backed email aliases

The everyday problem is giving every shop a permanent personal email address.
Investigate a palette action to create, copy and disable an alias, initially
without injecting anything into sign-up forms. Prefer a supplier integration or
bring-your-own-provider option over operating mail servers.

SimpleLogin provides a retail comparison, but its consumer offering does not
establish a wholesale API agreement, redistribution rights or a Syrinx margin.
Do not quote a Syrinx price until the owner authorizes contact and written
commercial terms exist. Forwarding also has different privacy properties from
encrypted Sync: a supplier handles mail routing and may process unencrypted
messages. Evaluate reply support, spam/deliverability, retention, account
recovery, alias continuity after cancellation and supplier shutdown.
[SimpleLogin pricing](https://simplelogin.io/pricing/),
[SimpleLogin privacy policy](https://simplelogin.io/privacy/)

Cost drivers include supplier minimums or per-user charges, mail volume, abuse
handling and delivery support. Avoid bundling this with VPN simply because both
are described as privacy services; their dependencies and operating risks differ.

## Architecture and decision checklist for selected ideas

- Preserve [owner decisions](decisions.md), the [spec](spec.md),
  [architecture decisions](architecture-decisions.md) and existing phase order.
  Saved folders, daily snapshots, session recovery and Notes tasks are already
  foundations; do not repackage them as new paid products.
- Use the generic tab model and current WASM host contract. New exports need
  compatibility review; the latest #158 pre-build report leaves the WASI/network
  gate choice pending. A feature proposal does not authorize new capabilities.
- Keep page identity, injection, Google, indexing and runtime security rules.
  Manual user-provided content is a narrower starting point, not blanket
  approval for screenshots, scraping, autofill or clipboard monitoring.
- Preserve the approved session-only download list. A document organizer must
  not silently become a permanent download-history database.
- Keep local convenience and accessibility usable without sign-in. Offer hosted
  processing through explicit choices with clear destination and allowance
  disclosure. No automatic browsing uploads or new telemetry schema.
- Validate with synthetic travel plans, notes, receipts and reserved domains.
  Show users a mock workflow before investing in implementation; customer
  research and supplier contact require their own authorization.

Defer automatic shopping/price monitoring, inbox/calendar aggregation and
website-acting agents. They introduce recurring remote access, credentials,
broader page powers and supplier dependencies before current foundations are
ready. A useful proposal should identify a specific daily frustration and
reuse existing capabilities before adding another system to maintain.
