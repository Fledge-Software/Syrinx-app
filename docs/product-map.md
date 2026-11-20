# Syrinx product map ("Version A2")

> **Status:** recorded on 2026-10-26 from the owner's Miro board "Version A2"
> and the owner's answers in chat. The decisions are entered in
> [`decisions.md`](decisions.md) under 2026-10-26; this page transcribes the
> board, lists every place it disagreed with an earlier record and says how
> each disagreement was settled. Work is tracked in GitHub issues, not here.

## Why this page exists

On 2026-10-25 the owner approved two detailed plans: the
[paid-services plan](paid-services-plan.md) (accounts, Sync, AI, VPN,
sharing, Household) and the [included-features plan](browser-protections-plan.md)
(ad and tracker blocking, quick capture, saved tabs, reading). The next day
the owner drew the whole product on one board, free features and paid ones
together. Most of the board matches those plans, but some prices and shapes
changed. Without one list of those differences, a later session would find
two sets of numbers and not know which one wins. The rule is the usual one:
`decisions.md` wins, and its 2026-10-26 entry records what the board changed.

## The board

Syrinx Browser sits at the centre. It is free and open source, it can
"provide supplemental income for Fledge Software", and it aims to shift
"productivity, creative, and every day work apps into a fast and efficient
browser with everything you need sync'd". The board also says "Syrinx does
not collect user information" and "If the user purchases Sync their data is
encrypted".

### Features and products

| Feature | Price | What the board says | Where it is planned |
|---|---|---|---|
| Plugins | Free | "Allows users to create personal or publishable add-ons to the source." | Epic #25 (runtime) and #30 (template); publishing is a later question. |
| Customize | Free | "Currently a small visual editor for how Syrinx looks. I want to grow it to be an editor for how Syrinx looks, functions, and allows the user to create workflows and such." | [`theme-editor.md`](theme-editor.md) for looks; functions and workflows need a plan. |
| Ad/Tracker Blocker | Free | — | [Included-features plan](browser-protections-plan.md), epics BP1–BP3. |
| Sync | $4/month | "10gb cloud syncing for plugin data and Browser syncable data. Such as bookmarks, favorites, passwords, etc." Add-on: "$2mo addon pricing per 30gb addon". | [Paid-services plan](paid-services-plan.md), section 6. |
| VPN | $8/month | "Guardian vpn wrapped and integrated into Syrinx." | Paid-services plan, section 8. |
| AI | $20/month | "Built in AI harness/chat area. Integrates with Notes, Forge, and more plugins coming soon." "User can also hook in their personal API key for one of the many providers." "We can later offer all kinds of models through openrouter, but for now openAi, claude, and User API key." | Paid-services plan, section 7. |
| Support Syrinx | $5 | "Support Syrinx as a Free Open-source software." | Paid-services plan, section 3.1; the website's Donate page. |
| More AI usage add-on | "$? = credit?" | — | Credit packs; sizes still open (AIP-01). |
| More storage add-on | $2 = 30 GB | — | Paid-services plan, SVC-08. |
| Household Sync | $20/month | — | Paid-services plan, section 12. |
| Browser Package | $30/month, $246/year | "Includes VPN, 50gb of Sync, and $20 of AI usage." | An idea only; tracked as a question. |

### Directions beside the products

- **Syrinx Distros.** "Support and promote creating 'Syrinx Distros'":
  "Devs and vibe coders taking the source of Syrinx and creating their own
  browser ontop of it. Still being able to add plugins and the
  subscriptions." "Distros can also be a preconfigured 'plugin-pack' that has
  community and/or dev made plugins, and customizations."
- **A Fledge companion app.** "While at the same time providing a desktop or
  phone 'app' that would directly integrate with Fledge App."
- **One account.** In chat: the website "will also need to be able to be
  logged into to edit account info, that same log in will need to be used to
  log into Syrinx App to use any of the features paid for."

## Conflicts with earlier records, and how each was settled

"Owner, chat" means the owner chose that answer on 2026-10-26 when asked.

| # | Conflict | Earlier record | Settled as |
|---|---|---|---|
| C1 | Household price | $10/month or $100/year (2026-10-25). | **$20/month or $200/year**, six people and 100 GB pooled unchanged (owner, chat). |
| C2 | AI tiers | Three tiers: $20, $100 and $200 a month. | **One $20 tier plus one-off credit packs** at the same three-times cost rule, never automatic (owner, chat). Pack sizes, expiry and refunds stay open in AIP-01. |
| C3 | Own API key | Not planned; all model calls went through Syrinx's server-side gateway. | **Free, with no subscription** (owner, chat). The person pays the provider; the key stays on the device in Windows' credential store and never reaches Syrinx's servers. It is a new network call Syrinx makes, so it goes through the same review as other service transports (paid-services plan, "Desktop networking"). |
| C4 | Hosted AI providers | "Provider selection remains open." | **OpenAI and Anthropic first, OpenRouter later** (board). Retention and pricing are still checked in AIP-01. |
| C5 | VPN price | $7.99/month or $79.99/year, proposed. | **$8/month** (board). The annual price is open; $80 is proposed. Still contingent on Guardian's terms. |
| C6 | Browser Package | "No additional cross-service bundle is promised", and annual AI has no discount. | **Idea only, not decided** (owner, chat). The earlier rule stands until the owner decides. Note for that decision: bought separately the parts cost about $36 a month, $246 a year is about a 32% discount, and 50 GB is not reachable in the 10 GB + 30 GB add-on steps. |
| C7 | Ad and tracker blocking | The spec and `AGENTS.md` list "no ad blocking in v1"; `AGENTS.md` forbids request filters on Google's sites. | **No change to v1.** The blocker is free and planned after v1 in the included-features plan, and BPR-01 must still show a design that leaves Google's pages untouched. The `AGENTS.md` lines stand. |
| C8 | Publishable plugins | "Syrinx fetches no plugin over the network in v1"; others install from a folder. | **No change to v1.** Publishing is a later question: a plugin directory on the website that hands out signed packages, still installed and updated only when the person chooses. |
| C9 | Customize scope | `theme-editor.md` covers looks only. | **Looks first, as built.** Functions and workflows need their own plan; anything a workflow runs goes through the command registry and the same plugin grants, so a workflow can never do what its plugins are not allowed to do. |
| C10 | Syrinx Distros | MPL-2.0 already allows forks; terms for the Syrinx name and logo are undecided (website README, "Still open"). | **Direction recorded; needs a policy.** A distro using Syrinx subscriptions needs the account API to be a public, versioned contract with server-side checks, which the paid-services plan already requires. |
| C11 | "Syrinx does not collect user information" | Accounts hold an email address, billing records and device grants (paid-services plan, section 5). | **Read as: no browsing data or content is collected; an account holds only what sign-in and billing need, and Sync is end-to-end encrypted.** The website's privacy page says this. |
| C12 | Sync scope | Plugin preferences are a later candidate; passwords need a password manager first (#97). | **Same order.** The board's list is the goal; bookmarks, Favorites and Notes come first. |
| C13 | Fledge companion app | Windows 11 only in v1 (D2); the Fledge plugin is on hold (2026-10-20). | **Idea for after v1.** The wiki's "Ideas: Mobile sync" page holds the mobile reasoning. |
| C14 | One login for website and app | The plan had optional desktop accounts but no website account portal. | **Added**: the website gets account pages, scaffolded now and switched off until the Supabase project exists (owner, chat); the app signs in through the browser with PKCE. |

The board agreed with the earlier records on: the free browser, Sync at
$4 with 10 GB, the $2-per-30 GB add-on, the $5 support, encrypted Sync and
Guardian as the VPN to evaluate.

## Where the work is tracked

Opened on 2026-10-26 from this map:

| Issue | What |
|---|---|
| #174 | Epic: Services — accounts, billing and one Syrinx login for the website and the app |
| #175 | One Syrinx account on syrinxapp.dev and in the app (SVC-09), under #174 |
| #176 | Epic: Sync — encrypted personal data, storage add-ons and Household Sync |
| #177 | Epic: AI — built-in AI area on a $20 plan or your own API key |
| #180 | Your own API key, free, without a subscription (AIP-10), under #177 |
| #178 | Epic: VPN — Guardian, device-wide, $8/month |
| #179 | Epic: Free ad and tracker blocker (after v1) |
| #181 | Question: the Browser Package bundle (C6) |
| #182 | Question: Customize grows into functions and workflows (C9) |
| #183 | Question: publishing plugins and a plugin directory (C8) |
| #184 | Question: Syrinx Distros (C10) |
| #185 | Idea: a Fledge companion app (C13) |

The password manager that password sync waits for is #97. The other draft
slices in the two plans become sub-issues of these epics when their turn
comes. Prices on the website's Pricing page come from this page and are
marked "planned" until each service is on sale.
