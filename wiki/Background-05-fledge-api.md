> **Research note dated 2026-10-09.** Read for reasoning, not current fact; later decisions in [`docs/decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/decisions.md) and [`docs/architecture-decisions.md`](https://github.com/Fledge-Software/Syrinx-app/blob/main/docs/architecture-decisions.md) win.
>
> Fledge is a separate project. Fledge-side findings are a list for the owner, never issues or pull requests in Fledge's repositories. The real Fledge client waits for Fledge's Stage C (D24). Notes 07 and 08 correct parts of the earlier notes, and note 03 assumes Tauri.
>
> Moved from `docs/research/05-fledge-api.md` ([original at af3de53](https://github.com/Fledge-Software/Syrinx-app/blob/af3de53/docs/research/05-fledge-api.md)) on 2026-10-16.

# Fledge's API surface for Syrinx's Fledge plugin

Date: 2026-10-09. Read-only research over source code; no Fledge service, Supabase project or
production URL was called. Repositories are under `C:\Users\<user>\Documents\GitHub\`. Branches
read: Fledge-App `restructure` (HEAD `f7d0a753`), fledge-nest `restructure/n2b`, fledge-connect
`restructure/a6-connect`, fledge-desk `restructure/a6-desk`, and Fledge-Workspace `main` (what is
in production today). Confidence is marked (high), (medium) or (low) on each claim.

## Questions

1. What API could a desktop client call for leads, contacts and jobs, mail, calendar and docs, and
   which surfaces are stable rather than internal?
2. How would a desktop app authenticate as the user, and can a third-party client use anything today?
3. Is there an MCP server, what tools does it expose, and could Syrinx use it instead of a bespoke API?
4. What do the modules' `/api/platform/{search,series-context,mcp-tools}` endpoints return, and
   could Syrinx's shared index use the search endpoint?
5. What shape should Syrinx's `FledgeClient` trait take, and what must Fledge build for a real client?

## Short answer

- **No public, stable client API exists for leads, mail, calendar or docs.** Every module's REST
  routes are internal UI routes behind the browser's cookie session, and Nest's writes are Next.js
  Server Actions, whose IDs change between builds. Do not build Syrinx against either.
- **The sanctioned way for an outside client to act for a person is the platform's OAuth 2.1
  authorization server**, the one Fledge's MCP server uses. It supports dynamic client
  registration, PKCE and `http://127.0.0.1` loopback redirects, which is exactly the desktop flow.
  It landed on the local `restructure` branch today and is not deployed.
- **The restructured MCP server registers 8 tools**: `search`, `mail_thread`, `mail_draft`,
  `calendar_list`, `calendar_find_free_time`, `calendar_create_event`, `chat_post` and
  `desk_search`. It has **no leads or contacts tool**, because no consent scope covers customer
  data yet, and **no tool lists an inbox or opens a document**.
- **MCP alone is a poor fit for Syrinx's UI**: its results are prose for a model, fenced as
  untrusted, with no structured output. Use the same OAuth tokens on a small structured REST
  surface; the "delegated surface" mechanism for that exists but honours identity only today.
- **`/<module>/api/platform/*` is shell-to-module plumbing** on the person's Supabase session token,
  not a client API. Its search answers a live query with at most 5 hits per group and no change
  feed, so it can back a live palette provider but cannot feed Syrinx's local index.
- **"New lead" maps cleanly onto Nest**: a lead is a contact plus a job at the Lead stage, created
  through `nest.capture_lead()`. But it is service-role only, it takes no value, and Nest has no
  Company field, so the palette form needs Fledge-side work.
- **Build the stub now, and wait for Stage C for the real client.** Nothing in the restructure is
  deployed until Stage C, and existing OAuth grants are re-issued at the cutover.

## Findings

### 1. The API surface by capability

| Need | What exists | Where | Usable by Syrinx? |
|---|---|---|---|
| Create a lead | `nest.capture_lead(p_org, p_name, p_phone, p_email, p_source, p_notes, …)` returns `(job_id, contact_id, contact_outcome)` | fledge-nest `supabase/migrations/0006_nest_fold_leads.sql:71-91`, grants `:474-477` | No: service role only (and Growth's `growth_reader` role, `0017:101`) |
| Create a contact | `nest.match_or_create_contact` (duplicate-matching insert) | fledge-nest `0001_nest_hub_contacts.sql:501`, grants `:661-664` | No: service role only |
| Create a job, list contacts, jobs, tasks | Server Actions `createJob`, `findContacts`, `loadContacts`, `loadJobs`, `addTask` | fledge-nest `apps/app/app/(workbench)/{contacts,jobs,tasks}/actions.ts` | No: Server Action IDs are "periodically recalculated between builds" (Fledge-App `node_modules/next/dist/docs/01-app/02-guides/data-security.md:280`) |
| List contacts, jobs (database) | `nest.list_contacts(org, search, cursor…)`, `nest.list_jobs(…)`, granted to `authenticated`, keyset-paged | fledge-nest `0001:697,750`, `0005:779` | Internal; would need a raw Supabase session and PostgREST access to the `nest` schema |
| Mail list, read, draft | `GET /connect/api/mail/threads?mailbox=&folder=&cursor=&limit=` (limit 1–50, opaque `nextCursor`), `GET …/mail/thread?mailbox=&id=`, `GET/POST …/mail/draft` | fledge-connect `apps/app/app/api/mail/{threads,thread,draft}/route.ts` | Not today: cookie session only; a bearer token hits a stand-in that refuses every token (`apps/app/lib/platform/access.ts:50-56`, `lib/connect-auth.ts:273`) |
| Calendar list, create | `GET /connect/api/calendar/events?start=&end=&calendars=`, `POST …/calendar/events` | fledge-connect `apps/app/app/api/calendar/events/route.ts:39,77` | Not today, same stand-in |
| Docs search, open | `GET /desk/api/docs/search?q=`, `GET /desk/api/docs/[id]` | fledge-desk `apps/app/app/api/docs/search/route.ts`, `docs/[id]/route.ts` | Not today: bearer refused (`apps/app/lib/access.ts:178-183`) |
| Mail, calendar, docs for agents | MCP tools (see question 3) | Fledge-App `packages/modules/src/mcp-tools.ts:87-146` | Partly, after Stage C |

- **Leads are not a separate thing any more.** Nest folds Growth's leads into contacts with a job
  at the Lead stage, and `capture_lead` is "the one way a lead comes in" (fledge-nest
  `0006_nest_fold_leads.sql:2-16,56-60`). (high)
- **The palette's four fields do not all have a home.** Name maps to the contact, Source to
  `nest.jobs.source` (a slug matching `^[a-z][a-z0-9_-]{0,31}$`, `0005_nest_pipelines_jobs.sql:313`)
  and Est. value to `nest.jobs.value_cents` (integer cents, `:308`). But `capture_lead` has no
  value parameter (`0006:71-89`), and no Company column or field exists anywhere in Nest (a
  repository-wide search for "company" finds nothing). Company would be a custom field
  (`0013_nest_custom_fields_views.sql`) or a new column. (high)
- **Stable versus internal.** The only surfaces written as cross-deployment contracts are the
  SERIES wire types (Fledge-App `packages/series/src/contract.ts`, "types only, deliberately"), the
  MCP forwarding contract (`packages/auth/src/mcp-forward.ts`), the search hit shape
  (`apps/app/lib/search.ts:84`) and the OAuth endpoints, whose discovery documents follow RFCs
  8414 and 9728. Everything else is a module's internal route and may change with its screens.
  (medium: no document declares a public API, so "stable" is inferred from how the code is
  written.)

### 2. Authentication

- **Sign-in is Supabase Auth** (password, or Google through `apps/app/components/auth/GoogleButton.tsx`)
  and the session is a host-only cookie on `app.*` (Fledge-App `AGENTS.md`). Syrinx's web tabs will
  carry that cookie; Syrinx's Rust core cannot use it. (high)
- **The platform's OAuth 2.1 authorization server** (ported from Workspace): Fledge-App
  `apps/app/lib/oauth/server.ts`, `apps/app/app/api/oauth/{register,token,revoke}/route.ts`, the
  consent page `apps/app/app/(auth)/oauth/authorize/page.tsx` and `apps/app/app/.well-known/`.
  OAuth 2.1 requires PKCE, a one-time secret proving that the app redeeming a code is the one that
  asked for it. What it supports:
  - Open dynamic client registration (RFC 7591), 10 per hour per IP; a client without a secret is
    a public client authenticated by PKCE alone (`register/route.ts:22,48`). (high)
  - Redirect URIs must be HTTPS or HTTP on `localhost`, `127.0.0.1` or `[::1]`; **custom schemes
    such as `syrinx://` are refused** (`server.ts:174-209`). Matching is an exact string, so a
    desktop client "has to register every port it might use", up to 20 (`server.ts:127,157`;
    the reasoning is in Workspace `main:apps/connect/lib/oauth/server.ts:332-341`). (high)
  - Access tokens last 1 hour and refresh tokens 30 days (`server.ts:38-40`). **Refresh tokens
    rotate, and reusing an old one revokes the whole grant** (`server.ts:559-563`). Tokens are
    opaque and stored only as SHA-256 hashes. (high)
  - Each token is bound to one audience (RFC 8707 resource indicator): the MCP server
    (`${APP_ORIGIN}/api/mcp`) or the REST surface (`${APP_ORIGIN}/api`), never both
    (`packages/auth/src/oauth-token.ts`, `tokenAudienceAllows`). (high)
  - A grant names one person and one business. Only an owner, admin or member may approve (never a
    guest), and only after their second factor if they use one (`apps/app/lib/oauth/consent.ts:26-40`).
    Connected clients appear and can be revoked in Settings → Agents
    (`apps/app/app/dashboard/settings/agents/page.tsx`). (high)
- **Scopes** (`apps/app/lib/oauth/scopes.ts:47-67`): `profile:read` (always granted), `mail:read`,
  `mail:draft`, `calendar:read`, `calendar:write`, `docs:read`, `files:read`, `chat:write`, and
  `docs:write`, which is marked unreachable and silently dropped because nothing serves it yet.
  There is deliberately **no `mail:send`** (`:18-21`), and `calendar_create_event` refuses
  attendees because an invitation is an email (`packages/modules/src/mcp-tools.ts:81-85`). **There
  is no scope for contacts, jobs or leads** (`mcp-tools.ts:29-35`). The REST audience honours
  `profile:read` alone (`scopes.ts:88`); the deny-by-default, per-route "delegated surface" table
  a module would use to accept agent tokens is in `packages/auth/src/delegation.ts`. (high)
- **`getClaims()` bearer tokens on `/<module>/api/platform/*`** are the person's **Supabase session
  JWT**, verified with `getClaims()`. The business is resolved from `organization_members`, guests
  are refused, and `public.has_module_access` is asked as that user (fledge-nest
  `apps/app/lib/platform-api.ts:56-95`; Connect also refuses a token owing its second factor). The
  shell calls them for the person (Fledge-App `apps/app/lib/search.ts`); outside clients do not. (high)
- **Supabase JWTs are deliberately not agent credentials.** The verifier refuses them
  (`oauth-token.ts:169`), Connect refuses them as bearers because they could skip the second factor
  (`connect-auth.ts:262-268`), and the forwarding contract says minting one "would hand the agent
  everything their session reaches" (`mcp-forward.ts`). **Module service secrets**
  (`MODULE_SERVICE_SECRET_<SLUG>`, `packages/auth/src/service.ts`) are server-to-server only and
  refuse browser headers; they must never ship in a client. (high)
- **Usable by a third-party desktop client today**: only production Workspace's OAuth and MCP
  server at `workspace.fledgesoftware.com/api/mcp` (Workspace `main:handoff.md:40,298`), with 15
  tools and 24 REST routes accepting agent tokens (`main:apps/connect/lib/oauth/delegated-surface.ts`).
  It has no leads, and it is retired at Stage C, when grants are re-issued ("Re-grant the Claude
  connector", Fledge-App `DOCS/RESTRUCTURE-PLAN.md:360-361`), so work built on it would be thrown
  away. (high)

### 3. The MCP server

MCP (Model Context Protocol) is a JSON-RPC protocol that AI clients use to discover and call
"tools" on a server.

- **Workspace's MCP server and OAuth server now live in the shell**: `POST /api/mcp` (Fledge-App
  `apps/app/app/api/mcp/route.ts`, `apps/app/lib/mcp/*`). It is stateless, refuses batches, and
  speaks protocol versions `2025-11-25` and `2025-06-18` (`lib/mcp/protocol.ts:18-38`). It offers
  tools only, with `resources/*` answered with an explanation (`lib/mcp/server.ts:116-135`). It
  merged into the local `restructure` branch on 2026-10-09 while this research ran; `checklist.md:176`
  still lists PLAT-MCP as in flight. (high for the code; medium for its status)
- **The tool registry** (`packages/modules/src/mcp-tools.ts:87-146`), deny-by-default:
  `search` (the platform's own fan-out; scopes `docs:read`, `mail:read` or `files:read`);
  Connect's `mail_thread` (`mail:read`), `mail_draft` (`mail:draft`), `calendar_list` and
  `calendar_find_free_time` (`calendar:read`), `calendar_create_event` (`calendar:write`) and
  `chat_post` (`chat:write`); and Desk's `desk_search`. Nest, Growth, Rankings and Reach each
  serve a `<slug>_search` tool, but the registry leaves them out until consent wording exists.
  Production Workspace had 15 tools, including `docs_get`, `docs_tree`, `files_list`, `docs_create`
  and `docs_append` (Workspace `main:apps/connect/lib/mcp/catalog.ts:213-702`); the document tools
  went to Desk's code, and Desk does not serve them yet. (high)
- **The modules don't run forwarded calls yet.** The shell forwards `tools/call` to
  `/<slug>/api/platform/mcp-tools` with the module's service secret plus the agent token in
  `x-fledge-agent-token`, and each module must re-verify with `verifyForwardedCall`
  (`packages/auth/src/mcp-forward.ts`). No branch of fledge-nest, fledge-connect or fledge-desk
  references it, and Connect's endpoint has no POST at all yet
  (fledge-connect `apps/app/app/api/platform/mcp-tools/route.ts:12-25`). So no tool call reaches
  module data end to end today. (high)
- **Results are prose, fenced as untrusted.** Every result is wrapped in a nonce-marked
  "BEGIN WORKSPACE CONTENT" block (`apps/app/lib/mcp/untrusted.ts`), and module tools return
  `{ status, text }` (fledge-connect `apps/app/lib/mcp-tools/index.ts:54-77`). No tool declares
  an `outputSchema` or returns `structuredContent`, which MCP has supported since `2025-06-18`.
  (high for Fledge; medium for the MCP spec detail, from memory)
- **Limits**: 60 tool calls per token and 120 requests per IP per minute, per serverless instance;
  a write that times out is reported as "it may have happened" (`lib/mcp/limits.ts:21-27`,
  `mcp-forward.ts` response table). (high)
- **Could Syrinx use MCP instead of a bespoke API?** For authentication, yes: the same OAuth server.
  For data, not well: Syrinx would parse text written for a model, there is no inbox list, document
  read or leads tool, and an MCP-audienced token cannot call REST routes. MCP suits SERIES and
  Claude; Syrinx needs structured JSON. (medium: a recommendation)

### 4. The per-module platform endpoints

| Endpoint | Request | Response | Caller today |
|---|---|---|---|
| `GET /<m>/api/platform/search?q=` | query, cut to 200 characters; under 2 characters returns recent records | `{ module, groups: [{ key, label, hits: [{ id, label, detail, href }] }] }`, at most 5 hits per group, `href` a path on the app origin (fledge-nest `apps/app/lib/search.ts:20-46`) | The shell's top-bar search, with a 2.5-second timeout per module (Fledge-App `apps/app/lib/search.ts:56,84`); the MCP `search` tool |
| `POST /<m>/api/platform/series-context` | `{ orgId, source, ref?, query?, limit }` | `{ items: [{ id, module, kind, title, text, href? }], unavailable? }`, at most 3 items and about 4,000 characters each (Fledge-App `packages/series/src/contract.ts`; fledge-desk `apps/app/lib/series-context.ts:37-40`) | SERIES's attach (`+`) menu. Sources: Nest `contacts`, `jobs`, `tasks`; Connect `email`, `messages`; Desk `files`, `documents` |
| `GET` / `POST /<m>/api/platform/mcp-tools` | POST `{ name, arguments }` | GET `{ tools: [{ name, description, inputSchema }] }`; POST `{ text }` or an outcome | The shell's MCP server (forwarding not yet adopted by modules) |

- Search groups: Nest returns `contacts`, `jobs` and `tasks` (deep links to the record); Connect
  returns `mail`, `calendar` and `chat` (links to `/connect/inbox`, `/connect/calendar` and
  `/connect/chat`, not to the item, fledge-connect `apps/app/lib/search.ts:77-79,139-141`); Desk
  returns `documents`, `tables` and `files` (fledge-desk `apps/app/lib/search.ts:255-257`). (high)
- **Can Syrinx's shared index use the search endpoint?** Not to build it. An index must list
  everything and then fetch changes; this endpoint answers one query with five hits per group, no
  cursor, on a Supabase JWT. It can serve as a live palette provider (query as you type, groups
  under a "Fledge" scope) once a client-facing route accepts an OAuth token. The owner export
  (`/<m>/api/platform/export`, owner only, up to 150 MB, `checklist.md:143`) is a backup, not a
  sync feed. (high)

## What this means for Syrinx v1

1. **Build the plugin against a trait with a stub now.** Nothing real can be called before Stage C.
2. **Authenticate the real client with the platform's OAuth server**, asking for the REST
   resource rather than MCP once Fledge's delegated REST surface exists. Register once per install
   with a few fixed loopback ports (`http://127.0.0.1:<port>/fledge/callback`). Open the authorize
   URL in an Syrinx web tab, where the person is already signed into Fledge, catch the code with a
   one-shot listener in the Rust core, and redeem it with PKCE (S256). Keep tokens in the OS
   keychain, and save each rotated refresh token before using it, since reusing an old one revokes
   the grant.
3. **"Open" means a web tab** at the item's `href`, which uses the person's normal Fledge session.
4. **Offline**: one cache layer (Syrinx's SQLite) wraps whichever client is in use. Offline reads
   come from the cache, marked stale. Writes go to an outbox with a client-made idempotency key.
   A write whose outcome is unknown is shown to the person, never retried, because Fledge's writes
   are not idempotent today.

Recommended trait sketch (async, so it uses the `async-trait` crate to stay usable as
`Box<dyn FledgeClient>`):

```rust
pub struct Session { pub user_id: String, pub org_id: String, pub business: String,
                     pub scopes: Vec<String>, pub modules: Vec<Module> }      // what is reachable
pub enum Module { Nest, Connect, Desk }
pub struct Cursor(pub String);                                     // opaque, server-issued
pub struct Page<T> { pub items: Vec<T>, pub next: Option<Cursor> }
pub struct IdempotencyKey(pub uuid::Uuid);                         // minted once per user action

pub struct NewLead { pub name: String, pub company: Option<String>, pub source: String /* slug */,
    pub est_value_cents: Option<u64>, pub email: Option<String>, pub phone: Option<String> }
pub struct Lead { pub job_id: String, pub contact_id: String, pub name: String, pub stage: String,
    pub value_cents: Option<u64>, pub source: Option<String>, pub href: String, pub updated_at: OffsetDateTime }
pub struct SearchHit { pub id: String, pub label: String, pub detail: String, pub href: String }
pub struct SearchGroup { pub module: Module, pub key: String, pub label: String, pub hits: Vec<SearchHit> }
// ThreadSummary, Thread, NewDraft, Event, NewEvent, TimeRange (UTC plus an IANA zone),
// DocHit, DocText (Markdown), ChangeBatch { items, deleted_ids, next: SyncCursor } likewise.

#[async_trait::async_trait]
pub trait FledgeClient: Send + Sync {
    async fn session(&self) -> Result<Session, FledgeError>;
    async fn list_leads(&self, page: Option<Cursor>) -> Result<Page<Lead>, FledgeError>;
    async fn create_lead(&self, lead: NewLead, key: IdempotencyKey) -> Result<Lead, FledgeError>;
    async fn list_threads(&self, folder: &str, page: Option<Cursor>) -> Result<Page<ThreadSummary>, FledgeError>;
    async fn get_thread(&self, thread_id: &str) -> Result<Thread, FledgeError>;
    async fn save_draft(&self, draft: NewDraft, key: IdempotencyKey) -> Result<DraftRef, FledgeError>;
    async fn list_events(&self, range: TimeRange) -> Result<Vec<Event>, FledgeError>;
    async fn create_event(&self, event: NewEvent, key: IdempotencyKey) -> Result<Event, FledgeError>;
    async fn read_doc(&self, doc_id: &str) -> Result<DocText, FledgeError>;
    async fn search(&self, query: &str) -> Result<Vec<SearchGroup>, FledgeError>;  // live, capped
    async fn changes(&self, since: Option<SyncCursor>) -> Result<ChangeBatch, FledgeError>; // index feed
}

pub enum FledgeError {
    Offline,                                     // no network: serve cache, queue the write
    SignedOut,                                   // 401: token gone or revoked, re-authorize
    MissingScope { scope: String },              // 403 insufficient_scope: ask again for it
    ModuleNotOwned { module: Module, store_href: String },   // "Add Connect to ..." prompt
    Invalid { field: Option<String>, message: String },      // shown inline in the palette form
    RateLimited { retry_after: Duration }, Unavailable { retry_after: Duration }, // never re-auth
    OutcomeUnknown,                              // write timed out: it may have happened
    Forbidden(String), NotFound, Server(String),
}
```

Every item carries an `href`, so "open" needs no method of its own. Money is integer cents (as in
`nest.jobs.value_cents`) and ids are opaque strings. Pagination is an opaque cursor, which matches
both Connect's keyset `nextCursor` and Nest's keyset listings; `search` is unpaged because Fledge
caps it at five hits per group. `Session.modules` and `Session.scopes` let the plugin lock a command
with a prompt rather than show a dead one, as Fledge's own rule says (Fledge-App `AGENTS.md`). The
stub (`StubFledgeClient`) holds seeded fixtures in memory with a fault switch for each error
variant, so the palette's error, offline and lock states can be built now; a later
`HttpFledgeClient` must pass the same contract tests.

## Work needed on the Fledge side

Candidate issues, in rough order. Each is one shippable unit.

1. **Fledge-App: consent scopes for Nest data** (`contacts:read`, plus `contacts:write` or a
   narrower `leads:write`) in `apps/app/lib/oauth/scopes.ts` and the MCP registry. The wording is
   a product decision; without it nothing outside Fledge can reach leads.
2. **fledge-nest: `POST /nest/api/leads` and a keyset-paged `GET`** under a Nest delegated-surface
   table, calling `nest.capture_lead` after verifying the token. Give `capture_lead` a
   `value_cents`, accept an idempotency key, and decide where Company lives.
3. **fledge-connect: replace the bearer stand-in** (`apps/app/lib/platform/access.ts`) with the
   platform verifier and a delegated-surface table (mail threads, thread, search and draft;
   calendar events GET and POST), adding the scopes to `REST_SURFACE_SCOPES`. Start from
   Workspace's table on `main`.
4. **fledge-desk: the same for documents**, plus a Markdown document read under `docs:read`, the
   REST twin of Workspace's `docs_get`.
5. **Every module: adopt `verifyForwardedCall`** so the shell's MCP tools run at all. SERIES and
   Claude need this anyway.
6. **Fledge-App: a versioned, types-only client contract** (modelled on
   `packages/series/src/contract.ts`), defining the envelopes, error codes (`insufficient_scope`,
   `module_not_owned` with `store_href`, `temporarily_unavailable` with `retry-after`), cursors and
   a version header, so the routes in items 2–4 cannot drift with the screens.
7. **All modules: idempotency keys on writes.** There is precedent in the shell's charge key and
   the push `dedupe_key` (`checklist.md:139,461`).
8. **Shell plus modules: a change feed for outside indexes** (for example
   `GET /<m>/api/platform/changes?since=`), returning id, kind, title, snippet, `href`,
   `updated_at` and deleted ids, filtered by what the grant may read. Unread counts for tile badges
   could ride along.
9. **Optional, Fledge-App: accept any port on loopback redirects** (RFC 8252 §7.3) in
   `isRegisteredRedirect`. Registering up to 20 ports works without it.

## Open questions for the owner

- Is Syrinx an ordinary third-party client that goes through the same consent screen as Claude
  (recommended), or a first-party client with a pre-registered id?
- What should the consent wording for contacts and leads say, and is creating a lead from outside
  Fledge acceptable?
- Should the Fledge rules (no `mail:send`, no calendar invitations with attendees) hold for Syrinx?
  The recommendation is yes: Syrinx drafts, and the person sends from a Connect tab.
- Where does a lead's Company live in Nest, and should `capture_lead` take an estimated value?
- REST delegated surface (recommended) or MCP with structured output as Syrinx's data channel?
- When does Stage C happen? It decides when Syrinx's real client can be built, since grants made
  before it are re-issued.

## Sources

- Fledge-App (`restructure`): `AGENTS.md`; `checklist.md`; `DOCS/RESTRUCTURE-PLAN.md`;
  `apps/app/lib/oauth/{server,scopes,consent,protected-resource}.ts`; `apps/app/app/api/oauth/*`;
  `apps/app/app/(auth)/oauth/authorize/page.tsx`; `apps/app/app/api/mcp/route.ts`;
  `apps/app/lib/mcp/{protocol,server,limits,untrusted}.ts`; `apps/app/lib/search.ts`;
  `packages/modules/src/mcp-tools.ts`; `packages/auth/src/{oauth-token,mcp-forward,delegation,service}.ts`;
  `packages/series/src/contract.ts`; `node_modules/next/dist/docs/01-app/02-guides/data-security.md`.
- fledge-nest: `apps/app/lib/{platform-api,search,mcp-tools,series-context}.ts`, `apps/app/lib/jobs/source.ts`,
  `apps/app/app/(workbench)/*/actions.ts`, `supabase/migrations/{0001,0005,0006,0013,0017}_*.sql`.
- fledge-connect: `apps/app/lib/{connect-auth,platform/access,platform-api,search}.ts`,
  `apps/app/lib/mcp-tools/*`, `apps/app/app/api/{mail,calendar,platform}/**/route.ts`.
- fledge-desk: `apps/app/lib/{access,mcp-tools,search,series-context}.ts`, `apps/app/app/api/docs/**/route.ts`.
- Fledge-Workspace (`main`): `apps/connect/lib/oauth/{server,delegated-surface}.ts`,
  `apps/connect/lib/mcp/catalog.ts`, `handoff.md`.
