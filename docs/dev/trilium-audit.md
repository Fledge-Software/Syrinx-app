# Trilium fork audit (#278, for epic #277)

On 2026-11-23 the owner halted the Notes plugin epic (#35) and chose to build
Notes on a fork of [Trilium Notes](https://github.com/TriliumNext/Trilium)
instead (epic #277). Trilium is a notes server written in TypeScript that runs
on Node.js (a JavaScript runtime), keeps notes in its own SQLite database and
serves a web application. It is licensed AGPL-3.0-or-later, the network
variant of the GNU General Public License, and Syrinx is MPL-2.0 with a "no
GPL code" rule (`AGENTS.md`, "Dependencies and licence"). The two can sit side
by side only as separate programs that talk over HTTP, and that only holds if
the boundary is written down and the fork's contents are known before anyone
builds on it. This file is that record. It covers where the fork starts and
how it follows upstream, what a Trilium build is made of (Node.js and its npm
packages, CKEditor 5's terms included), how Trilium is maintained and what its
security record looks like, every feature that listens or reaches the
network, and the bundling choice the owner has to make.

Two later slices of #277 are named throughout: #279, the slice that starts
the fork (Syrinx launching a program a plugin names), and #281, the slice
that hardens the fork's defaults (loopback only, Syrinx-issued credentials,
and a changes endpoint, an HTTP route that tells the Notes plugin which notes
changed since it last asked). Where this file says what those slices should
do, it is a recommendation; their scope is the owner's, and the proposals are
gathered under "Needs the owner".

**Where the decision is recorded.** Epic #277 says the owner's decision is
recorded in `docs/decisions.md` under 2026-11-23. On `main` at the time of
this audit that file has no such entry; its latest entry is 2026-11-20. This
audit therefore relies only on issues #277 and #278, which the owner filed on
2026-11-23, and asks the owner to record the decision (see "Needs the owner").

**What was done, and what was not.** The audit was carried out on 2026-11-23
by reading Trilium's source in a throwaway clone outside this repository, the
npm registry, and Node.js's own repository at the tag Trilium pins. Nothing
from Trilium was copied into Syrinx: this file quotes only licence names,
package names, option names and file paths, with their source named. No
npm install and no build of Trilium was run, so everything about "what a
build ships" is read from the build scripts, not observed in a built
artefact. GitHub's API refused this session's requests for Trilium's
repository (releases, security advisories and contributor statistics), so
release dates, the maintainer list and the security record below come from
git itself and from the release notes Trilium keeps in its repository; the
published advisory list was not read. Each section says which it is.

## The commit the fork starts from

**Recommendation: start the fork at the `v0.107.0` tag, commit
`d83d7151f1c6d4292be88d8d34008f409b252f7f`, committed 2026-11-08.** It is
Trilium's latest stable release on the day of this audit, and it is an
ancestor of the default branch's head, so later upstream work merges onto it
cleanly.

The alternative was the default branch's head on the day of this audit,
`598e1446946bc78ebb6d02417490c7beea340c14` (2026-11-23, version string
`0.108.0-rc.1` in its root `package.json`). It was rejected for two reasons.
It is a release candidate: `v0.108.0-rc.1` was tagged at `a0757f43` on
2026-11-21, and `main` has moved on since. And Trilium fixes security
problems only in its latest stable minor release (`SECURITY.md`, see
"Security policy" below), so a fork based on a stable tag can take upstream's
patch releases as they come, while a fork based on an arbitrary `main` commit
sits between releases with no fix line of its own. The same rule means the
fork must merge every upstream minor release, not just patch releases: a fork
that falls a minor version behind receives no upstream security fixes. Every
finding below was read at `v0.107.0` unless it says otherwise.

Trilium's tags are lightweight (they carry no tagger date), so the dates in
this file are the tagged commits' commit dates, read with git. The GitHub
release pages' publication dates could not be read (see above) and may differ
by hours.

### How upstream changes are taken

The fork keeps Trilium's history and takes upstream changes by an ordinary
merge, never by rebasing or copying patches:

- The fork's repository keeps upstream's `main` untouched, mirroring
  `TriliumNext/Trilium`. The fork's own work lives on a separate long-lived
  branch (a name such as `syrinx` is suggested) started at `v0.107.0`, and
  that branch is the fork's default.
- When upstream publishes a stable release, the fork merges that release's
  tag into its branch (for example `git fetch upstream --tags` and then
  `git merge v0.108.0`), resolves any conflicts there, and runs Trilium's own
  tests before Syrinx uses the result. Release candidates are not merged.
- The fork's changes are kept few, small and separate so these merges stay
  clean: configuration defaults, the loopback-only listener and Syrinx-issued
  credentials, the changes endpoint, and the source link discussed under
  "Bundling" below (AGPL §13). Each is its own commit with a message saying
  why, and new behaviour goes in new files where Trilium's structure allows
  it, rather than in edits spread through upstream's files.
- Rebasing the fork's branch onto upstream was rejected: it rewrites a
  public branch's history, which `AGENTS.md` allows on a shared branch only
  with the owner's say-so, and it loses the record of which upstream release
  each fork build contains.

## What a Trilium build is made of

Trilium is a pnpm workspace (pnpm is an npm-compatible package manager)
with 24 member packages, 13 in `apps/` and 11 in `packages/`; the lockfile
lists 25 importers, the root included. Three of them are the program Syrinx
would run: `apps/server` (the HTTP server), `apps/client` (the web
application it serves) and, for the desktop build, `apps/desktop` (an
Electron wrapper). Root `package.json` declares
`"license": "AGPL-3.0-or-later"`, as do `apps/client` and `apps/desktop`; the
repository's `LICENSE` is the GNU Affero General Public License, version 3.

The server build (`apps/server/scripts/build.ts`) bundles the server and its
imports with esbuild into `dist/`, copies `better-sqlite3`'s native module
and three CKEditor style sheets beside it, and builds the client into the
same folder. `apps/server/scripts/build-server.sh` then downloads the official
Node.js binary named by `.nvmrc` from nodejs.org, strips npm and corepack out
of it, and packs Node and the bundle into one archive per platform. A
Trilium server release therefore carries its own Node.js.

### Node.js

**Version.** `.nvmrc` pins Node.js **24.22.2**. The server's Dockerfiles use
the same release (`apps/server/Dockerfile`: `node:24.22.2-trixie-slim`;
`Dockerfile.alpine`: `node:24.22.2-alpine3.23`); `Dockerfile.legacy` uses
22.23.4. The desktop build instead runs on Electron **45.2.3**
(`apps/desktop/package.json`), which embeds its own Node.js; which Node.js
release Electron 45.2.3 embeds, and Electron's own licences (it contains
Chromium), were not audited, because #277's design runs the server build
under Syrinx and never the Electron desktop app.

**Licence, verified.** The `LICENSE` file of Node.js at tag `v24.22.2`
(commit `f7f273a9bb6445f6f7fb6349105e7532972045fb`, 2026-10-27, read from
`github.com/nodejs/node`) licenses Node.js itself under the MIT licence and
then lists the licences of 44 bundled third-party components, among them
V8, libuv, OpenSSL (Apache-2.0), ICU, llhttp, undici, zlib, brotli, zstd,
nghttp2/nghttp3/ngtcp2, c-ares, simdjson, simdutf, ada, npm and corepack.
All are permissive. The only GNU General Public License text in the file
covers two build scripts in ICU's source tree (`aclocal.m4` and
`config.guess`, "only for ICU4C"), each under the Autoconf exception, which
lets them be distributed with a program under that program's own terms;
they are configuration scripts, not code compiled into the binary. Trilium's
build also removes npm and corepack from the bundled runtime, so their
entries do not apply to what it ships. Whether every listed component is
compiled into the Linux binary that `build-server.sh` downloads was not
checked against the binary itself.

### npm packages

`pnpm-lock.yaml` at `v0.107.0` (SHA-256
`5363b2a22aeb1010b6882af2a7bc97d7db0d229b671f3a0032c3ec50b7e27825`; lockfile
format 9.0, `packageManager` `pnpm@12.18.0`) resolves **2,364 packages**,
every one from the npm registry (no git or tarball sources). A pnpm lockfile
does not record licences, so each package's `license` field was read from the
registry, one request per exact version, six at a time, with every answer
cached; all 2,364 answered. These are the licences the authors declare in
their published metadata, not the result of reading every package's licence
files; the exceptions below were checked further where it matters. The
per-package list is not committed: it is 2,364 rows that the method under
"Reproducing this audit" regenerates from the lockfile.

**What ships and what is tooling.** The lockfile cannot say exactly which
packages end up in a build, because the server's esbuild step bundles
whatever its source imports, `devDependencies` included: `express`,
`express-session`, `helmet` and most of the server are declared as
`devDependencies` of `apps/server` and still ship inside the bundle. So the
audit computed two bounds by walking the lockfile's dependency graph from the
three shipping apps (`apps/server`, `apps/client`, `apps/desktop`) and the
workspace packages they link (`packages/ckeditor5`, `codemirror`, `commons`,
`highlightjs`, `share-theme`, `splitjs`, `trilium-backup-container`,
`trilium-core` and `turndown-plugin-gfm`):

- **upper bound, 1,812 packages:** everything reachable through
  `dependencies`, `optionalDependencies` and `devDependencies`. It still
  includes build and test tools (Vite, Vitest, TypeScript, Playwright and
  the like) that a build uses but does not ship;
- **lower bound, 991 packages:** everything reachable through
  `dependencies` and `optionalDependencies` only. It misses most of the
  server, for the reason above.

The remaining 552 packages belong only to the website, documentation
builders, the web clipper browser extension, the mobile and standalone apps,
and the repository's own tooling. Determining exactly what a build ships
means building it and listing the bundle's inputs, which this audit did not
do.

**Licences across the whole lockfile (2,364 packages),** as declared on the
registry:

| Licence | Packages |
|---|---|
| MIT | 1,784 |
| Apache-2.0 | 199 |
| ISC | 137 |
| "SEE LICENSE IN LICENSE.md" (CKEditor 5, below) | 61 |
| BSD-2-Clause | 46 |
| BSD-3-Clause | 45 |
| MPL-2.0 | 18 |
| BlueOak-1.0.0 | 15 |
| LGPL-3.0-or-later, alone or with Apache-2.0/MIT | 14 |
| CC0-1.0 | 6 |
| No licence declared | 5 |
| MIT-0, Unlicense (4 each) | 8 |
| Other permissive (0BSD, BSD, MIT/X11, Python-2.0, PSF-2.0, WTFPL, MIT AND Zlib) | 9 |
| Dual or multiple licences with a permissive choice | 13 |
| EPL-2.0 OR GPL-3.0-or-later | 1 |
| CC-BY-4.0 | 1 |
| OFL-1.1 | 1 |
| "SEE LICENSE IN README.md" (proprietary, below) | 1 |

**Within the upper bound of what ships (1,812 packages):** MIT 1,399,
Apache-2.0 127, ISC 103, CKEditor 5's "SEE LICENSE IN LICENSE.md" 61,
BSD-3-Clause 39, BSD-2-Clause 25, BlueOak-1.0.0 12, MPL-2.0 12, and 34 more
spread over the other permissive licences and the packages named below.

**Every package that is not plainly permissive** is named here, with where it
sits ("ships" means inside the lower bound, so certainly reachable at run
time; "build" means only in the upper bound; "outside" means not reachable
from the shipping apps at all).

- **CKEditor 5, GPL-2.0-or-later or a commercial licence (61 packages):**
  `ckeditor5@49.0.1` and 59 `@ckeditor/ckeditor5-*@49.0.1` packages, from
  `adapter-ckfinder` to `word-count` (ships), all declaring "SEE LICENSE IN
  LICENSE.md"; plus `@ckeditor/ckeditor5-inspector@5.0.4` (build only, a
  development tool). Set out in full below.
- **`@anthropic-ai/claude-agent-sdk@0.3.318` (ships):** declares "SEE
  LICENSE IN README.md". The package's `LICENSE.md` reads "All rights
  reserved" and makes use subject to Anthropic's Legal Agreements; it is not
  an open source licence. (Its README separately describes the usage data
  the SDK collects.) The server loads it on demand for its AI chat feature
  (`apps/server/src/services/llm/providers/claude_agent.ts`), and it is
  inside what a server build ships.
- **`elkjs@0.12.1`, EPL-2.0 OR GPL-3.0-or-later (ships):** a graph-layout
  library pulled in by `mermaid@12.3.0`, the diagram renderer. Either
  licence is copyleft; the Eclipse Public License is the weaker, file-level
  one.
- **`lightningcss@1.33.0` and its eleven platform binaries, MPL-2.0
  (build):** a CSS compiler used while building. MPL-2.0 is Syrinx's own
  licence and file-level; it matters only if it ships, which a build tool
  does not.
- **No licence in the registry metadata:** `buffers@0.1.1` and
  `khroma@2.1.0` (ships), `color-convert@0.5.3` and `exif-parser@0.1.12`
  (build), `union@0.5.0` (outside). Their repositories were not checked.
- **`@img/sharp-*@0.35.5` and `@img/sharp-libvips-*@1.3.4`,
  LGPL-3.0-or-later alone or with Apache-2.0 and MIT (14 packages,
  outside):** the libvips image library's prebuilt binaries for the sharp
  image package, used only outside the shipping apps.
- **MPL-2.0 tooling for the web clipper (outside):** `addons-linter@10.14.0`,
  `addons-moz-compare@1.4.0`, `addons-scanner-utils@16.0.0`,
  `eslint-plugin-no-unsanitized@4.1.5`, `fx-runner@1.7.0`, `web-ext@10.8.0`.
- **Creative Commons and OFL:** `caniuse-lite` (CC-BY-4.0, build data) and
  `@fontsource/arimo@5.3.0` (OFL-1.1, outside).
- **The 13 dual licences with a permissive choice,** which a distributor
  takes:

  | Sits | Packages |
  |---|---|
  | ships | `jszip@3.10.2` (MIT OR GPL-3.0-or-later), `dompurify@3.4.17` (MPL-2.0 OR Apache-2.0), `json-schema@0.4.0` (AFL-2.1 OR BSD-3-Clause), `boxicons@2.1.4` (CC-BY-4.0 OR OFL-1.1 OR MIT), `@maplibre/mlt@1.3.0` (MIT OR Apache-2.0) |
  | build | `unorm@1.6.0` (MIT or GPL-2.0), `sanitize-filename@1.6.4` (WTFPL OR ISC), `utf8-byte-length@1.0.5` (WTFPL OR MIT), `type-fest@5.10.0` (MIT OR CC0-1.0) |
  | outside | `node-forge@1.4.2` (BSD-3-Clause OR GPL-2.0), `opener@1.5.2` (WTFPL OR MIT), `rc@1.2.8` (BSD-2-Clause OR MIT OR Apache-2.0), `type-fest@4.41.0` (MIT OR CC0-1.0) |

None of this changes what Syrinx may contain: Syrinx takes none of these
packages, because Syrinx takes nothing from the fork. It matters for what
the fork's own builds carry and, if the owner ever bundles a build, for the
licence notices Fledge would then redistribute.

### CKEditor 5's terms

Trilium's text editor is CKEditor 5 at **version 49.0.1**, declared by the
workspace package `packages/ckeditor5/package.json` (`"ckeditor5":
"49.0.1"`, with `@ckeditor/ckeditor5-icons` 49.0.1). The registry metadata of
`ckeditor5@49.0.1` gives no SPDX licence, only "SEE LICENSE IN LICENSE.md".
That `LICENSE.md`, read from the published package, says CKEditor 5 is
"licensed under a dual-license model": the GNU General Public License
version 2 or later, or commercial licence terms from CKSource Holding sp.
z o.o.

**Trilium takes the GPL path, with no commercial licence key.** CKEditor 5
asks every application to pass a `licenseKey`; the value `"GPL"` selects the
open source licence. Trilium passes exactly that: `OPEN_SOURCE_LICENSE_KEY =
"GPL"` in `apps/client/src/widgets/type_widgets/text/config.ts`, and the
literal `"GPL"` in
`apps/client/src/widgets/attribute_widgets/AttributeEditor.tsx`,
`apps/client/src/widgets/type_widgets/llm_chat/ChatInputBar.tsx` and
`apps/client/src/widgets/type_widgets/mind_map/NodeMemo.tsx`. No other key
appears in the client.

**No premium packages.** The lockfile contains no
`ckeditor5-premium-features` package and none of CKEditor's commercial-only
plugins (real-time collaboration, track changes, comments, revision history,
export and import, AI). It does contain `@ckeditor/ckeditor5-ckbox`,
`@ckeditor/ckeditor5-cloud-services` and `@ckeditor/ckeditor5-easy-image`,
which come with the open source `ckeditor5` package; they are clients for
CKSource's paid cloud services and do nothing without that service's token.
Whether Trilium's editor configuration enables any of them was not checked.

**What this means.** GPL-2.0-or-later code may be combined with an
AGPL-3.0 program: "or later" allows GPL-3.0, and GPL-3.0 §13 and AGPL-3.0 §13
expressly allow linking the two. So CKEditor raises no problem for the fork
itself. It does mean the fork's web application contains GPL code as well as
AGPL code, which is one more reason the fork's source must never enter
Syrinx: Syrinx's no-GPL rule would be broken by CKEditor alone, before
Trilium's own code is counted.

## Maintenance and security record

**Who maintains it.** Trilium lives in the `TriliumNext` organisation on
GitHub, which took over the project after its original author stopped
developing it. Contributor statistics were read from git, not the API, in a
clone that fetched only commits (no file contents) back to 2025-11-23, and
names were merged by Trilium's `.mailmap`. In the twelve months to
2026-11-23, `main` received 17,712 non-merge commits from 267 author names.
The two most active authors are Elian Doran (10,466 commits) and Adorian
Doran (2,366); the dependency bot `renovate[bot]` made 1,841 and the
translation service `Hosted Weblate` 230. Most of the work, and so the
project's continuity, rests on one or two people.

**Release cadence.** Fifteen stable releases were tagged in those twelve
months, eight minor and seven patch releases: `v0.100.0` (2025-12-05),
`v0.101.0` to `v0.101.3` (2025-12-25 to 2026-01-08), `v0.102.0` to
`v0.102.2` (2026-03-01 to 2026-04-05), `v0.103.0` (2026-05-13), `v0.104.0`
and `v0.104.1` (2026-07-18 and 07-25), `v0.105.0` (2026-08-19), `v0.106.0`
and `v0.106.1` (2026-09-25 and 10-23) and `v0.107.0` (2026-11-08), with
`v0.108.0-rc.1` on 2026-11-21. That is a minor release roughly every six
weeks for the fork to merge.

**Security policy.** `SECURITY.md` (at `v0.107.0`) asks for reports through
GitHub's private security advisories, promises a first answer within 7 days
and triage within 14, and publishes each advisory 2–4 weeks after the fixed
release. It supports only the latest stable minor release. It lists as out of
scope several things that matter for Syrinx's design: scripts a user puts in
their own notes ("self-XSS"), the content of notes the owner publishes
through sharing, and the server connecting to a local network address that a
user configured as an AI provider. Its reporting link still points at
`TriliumNext/Notes`, the project's former repository name.

**Security fixes, from the release notes kept in the repository**
(`docs/Release Notes/Release Notes/v0.*.md`), since the published advisory
list could not be read:

- `v0.101.0` and `v0.101.1` each note a reported vulnerability fixed, with
  details withheld.
- `v0.102.2` says it "contains important security fixes" for content
  handling and the desktop app.
- `v0.103.0` names two low-severity advisories: an SQL injection through a
  URL parameter (GHSA-64fm-4j23-99j2) and a stored cross-site scripting flaw
  in the share index page (GHSA-97mx-2qjm-qhfx).
- `v0.104.0` lists sixteen security fixes and turned backend scripting and
  the SQL console off by default.
- `v0.106.0` has a security fixes section; `v0.106.1` is a security release
  for the server that updates Node.js for a Brotli decoder out-of-bounds
  write which, the notes say, any client that could reach the server could
  trigger before authentication.
- `v0.107.0` updates Node.js for the same issue and fixes "a few low and
  medium-priority security reports regarding ETAPI", Trilium's external API.

The record shows an active project that fixes and discloses security
problems, and also a large attack surface: a server that parses untrusted
HTML, imports, uploads and requests from any client that can reach it. That
is why #277 keeps the fork on loopback with a Syrinx-issued credential.

## Features that listen or reach the network

Each row was read in the source at `v0.107.0`. "Option" means a row in
Trilium's own `options` table, set in its Options screen and stored in its
database, with defaults in
`packages/trilium-core/src/services/options_init.ts`. "ini" means a key in
the data directory's `config.ini` (template
`apps/server/src/assets/config-sample.ini`); each ini key can also be set by
an environment variable `TRILIUM_<SECTION>_<KEY>`, and an environment
variable wins over the file (`apps/server/src/services/config.ts`). Paths
are relative to the Trilium repository; `server/` stands for
`apps/server/src/`, `core/` for `packages/trilium-core/src/` and `client/`
for `apps/client/src/`. #281 is the slice proposed to change these defaults.

**Who listens.**

| Feature | Default | Where switched | Source |
|---|---|---|---|
| Listening address | `0.0.0.0` (all interfaces), port 8080 | ini `[Network] host`, `port`; env `TRILIUM_NETWORK_HOST`, `TRILIUM_HOST`, `TRILIUM_NETWORK_PORT`, `TRILIUM_PORT` | `server/services/config.ts`, `server/services/host.ts`, `server/services/port.ts`, `server/www.ts` |
| Login check | on | ini `[General] noAuthentication` (default false); env `TRILIUM_GENERAL_NOAUTHENTICATION` | `server/services/auth.ts` |
| CORS headers | no `Access-Control-*` headers; CORP `same-origin` | ini `[Network] corsAllowOrigin`, `corsAllowMethods`, `corsAllowHeaders` (empty), `corsResourcePolicy` | `server/services/config.ts`, `server/app.ts` |
| First-run setup routes | `/api/setup/*` open before the database is initialised; `/set-password` open after it until a password is set | no setting; turning it off needs a fork change | `server/routes/routes.ts`, `core/routes/index.ts`, `server/services/auth.ts` |
| ETAPI, `/etapi/*` | mounted; needs a user-created token | no setting; turning it off needs a fork change | `server/etapi/`, `server/routes/routes.ts` |
| `/custom/*path` | mounted; no login, no CSRF (cross-site request forgery) check | no setting for the route; handlers need `backendScriptingEnabled`, resource providers do not | `server/routes/custom.ts`, `core/routes/custom.ts` |
| MCP (Model Context Protocol, an endpoint AI agents call) server, `/mcp` | off | option `mcpEnabled` (false); needs an ETAPI token | `server/routes/mcp.ts` |
| ACP (Agent Client Protocol) agents' MCP listener | off (only while an ACP agent runs) | follows the AI chat options | `server/services/llm/providers/acp_mcp_endpoint.ts` |
| Web clipper, `/api/clipper/*` | mounted; needs an ETAPI token | no setting; turning it off needs a fork change | `server/routes/routes.ts` |
| Sync target, `/api/sync/*` | mounted; needs the password | no setting; turning it off needs a fork change | `core/routes/api/sync.ts` |
| Public sharing, `/share/` | mounted; notes under `_share` served without login | no setting for the route; options `redirectBareDomain`, `showLoginInShareTheme` (false) | `server/share/routes.ts`, `core/share/share_root.ts` |

**What reaches out.**

| Feature | Default | Where switched | Source |
|---|---|---|---|
| Update check (from the page) to `api.github.com` | **on** | option `checkForUpdates` (true) | `client/widgets/buttons/global_menu.tsx` |
| Sync to another Trilium, every 60 s | off | option or ini `[Sync] syncServerHost` (empty or `disabled` is off), `syncProxy`, `syncServerTimeout`; env `TRILIUM_SYNC_SYNCSERVERHOST` | `core/services/sync_options.ts`, `core/services/sync.ts` |
| Download of remote images in notes | **on** | option `downloadImagesAutomatically` (true) | `core/services/image_download.ts` |
| Link previews (page, oEmbed, thumbnails) | on when a link is pasted | no setting; turning it off needs a fork change | `core/routes/api/link_embed.ts` |
| AI chat, providers and web search | off | option `aiEnabled` (false), `llmProviders`, `llmWebSearchProvider` | `core/services/llm/`, `server/services/llm/providers/` |
| OCR language data from `cdn.jsdelivr.net` | on a user-started OCR | option `ocrAutoProcessImages` (false) covers automatic OCR only | `server/services/ocr/tesseract_recognizer.ts` |
| OpenID Connect login | off | ini `[MultiFactorAuthentication] oauth*`; env `TRILIUM_OAUTH_*` | `server/services/open_id.ts` |
| OneNote import (Microsoft sign-in and Graph) | on when the user starts it | no setting; turning it off needs a fork change | `server/services/import/onenote/` |
| Map notes: tiles and place search | on when a map note opens | no setting; turning it off needs a fork change | `client/widgets/collections/geomap/` |
| Web View notes embed any address | on when such a note opens | label `webViewSrc` on the note; no global setting | `client/widgets/type_widgets/WebView.tsx` |
| Backend scripts (full server access) | off | ini `[Security] backendScriptingEnabled`; env `TRILIUM_SECURITY_BACKEND_SCRIPTING_ENABLED` | `server/services/scripting_guard.ts` |
| Frontend scripts (any request a page can make) | on | no setting; turning it off needs a fork change | no single file; scripts run in `client/` |
| Links to upstream's repository | open only when clicked | no setting | `client/widgets/dialogs/about.tsx` |

Notes on the tables:

- **A loopback recipe for #281.** Three things together, not one:
  (a) `127.0.0.1` must be set where nothing can override it. Environment
  variables win over `config.ini`, and `server/services/host.ts` reads
  `TRILIUM_HOST` before
  either, so the launcher (#279) must set or clear every `TRILIUM_*`
  variable it passes on, or the fork must change the default in code.
  (b) Loopback alone is no boundary against a web page that uses DNS
  rebinding (a name that first resolves to a public server and then to
  `127.0.0.1`, so the page's script reaches the local port as its own
  origin). The server checks no `Host` header. `/mcp` sets
  `enableDnsRebindingProtection: false` (`server/routes/mcp.ts`) but is off
  by default and relies on its required ETAPI bearer token instead; the
  routes that check no token (setup, `/custom/*path`, `/share/`) are the
  reason a `Host` allow-list (`127.0.0.1:<port>` only) is a recommended
  #281 change. (c) The three CORS keys stay empty, so no
  other origin is invited to read responses.
- **First-run setup routes.** `POST /api/setup/new-document`,
  `POST /api/setup/sync-from-server` and `POST /api/setup/sync-seed`
  (`core/routes/index.ts`) answer before the database is initialised;
  `GET` and `POST /set-password` (`server/routes/routes.ts`) answer after
  it, until a password is set. None needs a login: `checkSetupAuth` in
  `server/services/auth.ts` applies only once data exists and a password is
  set (`isSetupAuthRequired` in `core/services/setup_auth.ts`). Whoever
  reaches the port first can set the password or seed the database from a
  sync server of their choosing. The
  recommendation for #281 is to initialise the database and set the
  Syrinx-issued credential before the listener opens, or to keep these
  routes closed until then.
- **`/custom/*path`.** `server/routes/custom.ts` mounts it for every method,
  "explicitly" without CSRF protection, and `core/routes/custom.ts` answers
  from the first note labelled `#customRequestHandler` or
  `#customResourceProvider` whose pattern matches. A resource provider
  serves the note's content to anyone, with no login, and is not gated by
  `backendScriptingEnabled`; a request handler runs the note as a script,
  as a public endpoint, once backend scripting is on. #281 should unmount
  the route or put it behind authentication.
- **ACP agents' listener.** When the AI chat runs one of the ACP agents
  (GitHub Copilot, Google Antigravity or OpenAI Codex through `codex-acp`;
  `server/services/llm/providers/acp_agent.ts`), the server opens a second HTTP
  listener on `127.0.0.1` at a random port, under an unguessable path, and
  checks its `Host` header. It exists only while AI chat is on, which it is
  not by default upstream.
- **AI providers and agents.** With `aiEnabled` on, the server calls the
  providers in `llmProviders` (Anthropic, OpenAI, Google, DeepSeek,
  OpenRouter or a local model), a web-search provider (Brave, Tavily, Exa,
  Serper, Perplexity or DeepSeek; `core/services/llm/web_search.ts`), and
  can start agent programs (Claude's agent SDK, Codex, Copilot,
  Antigravity). The Options screen looks up an Antigravity download from
  `dl.google.com`.
- **OCR.** Trilium starts Tesseract without a language-data path; in
  `tesseract.js@7.0.0`, the version the lockfile pins, that makes the worker
  download language data from `cdn.jsdelivr.net`
  (`src/worker-script/index.js` of the published package).
- **The Trilium tab inside Syrinx (for #279 and #281).** If the fork's web
  application is shown in an ordinary page tab, D35's page-text capture and
  Syrinx's history would record note text and addresses unless the fork's
  loopback origin is excluded. Requests the page itself makes (update check,
  map tiles, frontend scripts) leave from that tab. The Notes plugin's own
  HTTP goes only through #280, the slice that gives the Notes plugin HTTP
  to one owner-approved address, limited to the fork's address.
- **Telemetry.** A search of the server, core and client sources for
  telemetry, analytics or crash-reporting services (Sentry, PostHog,
  Plausible, Umami, Google Analytics) found none. The Claude agent SDK's
  README says the SDK itself collects usage feedback once it is used.

## Bundling: installed separately, or shipped in Syrinx's release

This section is analysis for the owner, not legal advice and not a decision.
It sets out what each choice means under the two licences; the decision is
the owner's (#278), and a qualified licence review should come first if
bundling is chosen.

**What the licences say.**

- **AGPL-3.0 §5, aggregates.** Putting an AGPL program on the same storage or
  distribution medium as "separate and independent works", which are not
  "combined with it such as to form a larger program", does not make the
  licence apply to the other works. Two programs that run as separate
  processes and exchange HTTP messages are the standard example of separate
  works; sharing a process, linking each other's code or exchanging
  internal data structures would not be.
- **AGPL-3.0 §6, corresponding source.** Whoever conveys the program in
  object form (a built archive) must also provide its complete
  corresponding source, including the scripts used to build it, for
  example from a public server at no charge, kept available as long as the
  build is offered.
- **AGPL-3.0 §13, network use.** A *modified* version must "prominently
  offer all users interacting with it remotely through a computer network"
  the chance to get its source. The fork is modified, and the owner reaches
  it over HTTP, even if only on loopback. Trilium's interface links to
  upstream's repository and commit (`about.tsx`), not to the fork's, so
  whichever option is chosen, the recommendation is that the fork points
  those links at its own public repository and exact commit, one small fork
  commit (proposed for #281 under "Needs the owner").
- **MPL-2.0 §3.3, Larger Work.** Syrinx's licence lets Syrinx be distributed
  inside a larger work "under terms of Your choice", provided Syrinx's own
  files stay under MPL-2.0 and their source stays available. MPL-2.0 even
  names AGPL-3.0 among the "Secondary Licenses" it can be combined with. So
  MPL-2.0 itself does not stop a Syrinx installer from also carrying an AGPL
  program; the stricter constraint is Syrinx's own "no GPL code" rule in
  `AGENTS.md`.

**Option 1: installed separately.** Syrinx's release contains no Trilium
code. The fork publishes its own builds from its own repository, under the
AGPL, with source on the same page, and the owner installs one; the Notes
plugin finds it where the owner put it and Syrinx starts it (#279). Syrinx's
MPL-2.0 release stays exactly what `AGENTS.md` says it is, Syrinx's weekly
preview workflow is unchanged, and Fledge's AGPL duties are the ones any
public fork already has. The costs are a second download, and a Syrinx
version and a fork version that can drift apart, so the plugin has to check
the fork's version when it connects. It also matches the owner's words in
#277, "if the user has installed the notes app in syrinx plugins".

**Option 2: bundled in Syrinx's release.** The installer would carry the
fork's build (Node.js 24.22.2 and the bundle, in its own folder with its
`LICENSE`) beside Syrinx. Under §5 that is an aggregate, so Syrinx stays
MPL-2.0, provided the two remain separate programs. But Fledge then conveys
an AGPL program in every Syrinx release, so §6 applies to each release: the
exact fork commit of every build must stay published, the release must point
to it, and the archive must carry the AGPL text and the notices of the
packages inside it (including CKEditor's GPL and the Claude agent SDK's
all-rights-reserved terms, whose redistribution this audit did not review).
It would change the weekly release workflow, which `AGENTS.md` lists under
"ask before proceeding", and it would make the "no GPL code" rule untrue of
the release archive, if not of the repository. The gain is one download and
versions that always match.

**Recommendation: install separately (option 1),** at least until #283
(packaging) and a licence review by a qualified person. It keeps the AGPL out
of Syrinx's release without depending on a legal reading of "aggregate",
keeps the rule in `AGENTS.md` literally true, needs no change to release
scripts, and can be revisited later without undoing anything, since the
separation is the same in both options. Whatever is chosen, the fork should
be built from its public repository at a tagged commit, and its interface
should link to that commit's source (§13).

## Needs the owner

1. **Create the fork's repository (operator work, `AGENTS.md`).** On GitHub,
   open `TriliumNext/Trilium` and choose *Fork*; set the owner to
   `Fledge-Software`; name it `Syrinx-Trilium` (suggested, in the pattern of
   `Syrinx-app` and `Syrinx-Notes-Plugin`, and plain that it is Syrinx's
   fork rather than upstream); leave it public, which a fork of a public
   repository always is; and untick "Copy the `main` branch only". Check
   that tag `v0.107.0` exists in the fork afterwards; if it does not, push
   it there from a clone of upstream. Keep upstream's `LICENSE` untouched.
   Then create the branch `syrinx` from tag `v0.107.0`
   (`d83d7151f1c6d4292be88d8d34008f409b252f7f`), make it the default
   branch, and leave `main` mirroring upstream. Recommended as described.
2. **Decide bundling.** Installed separately, or bundled in Syrinx's release
   (section above). Recommendation: installed separately until #283 and a
   licence review.
3. **Record the 2026-11-23 decision in `docs/decisions.md`.** Epic #277
   cites a `docs/decisions.md` entry for 2026-11-23 that does not exist on
   `main`, and #278's definition of done asks for the bundling decision to be
   recorded there. Agents do not write decisions; the owner (or an agent at
   the owner's request) adds the entry.
4. **Reconcile `AGENTS.md` in that entry.** These passages still describe
   the halted Notes plugin and now contradict the Trilium direction: in
   "Layout", the bullet saying the built-in plugins, "Notes and later
   Forge", each live in their own repository as signed packages, and the
   bullet on Notes and Forge editing in one CodeMirror 6 component (D28);
   and in "Work order", "Plugins go runtime, template, Notes, Forge", with
   Notes as #35. This audit leaves them as they are.
5. **The Claude agent SDK inside Trilium's server build.** Its `LICENSE.md`
   reserves all rights and makes use subject to Anthropic's Legal
   Agreements. Recommendation: keep AI chat off by default in the fork, and
   if the owner chooses to bundle, have the licence review cover whether
   that package may be redistributed, or build the fork without it.
6. **Proposed additions to #281 and #279,** recommendations not yet in
   either issue; each is set out under "Notes on the tables" or "Bundling":
   the loopback recipe; closing the first-run setup routes; unmounting or
   authenticating `/custom/*path`; turning `checkForUpdates` and
   `downloadImagesAutomatically` off, with AI chat (`aiEnabled`) staying
   off; the fork's source links (AGPL §13); and excluding the fork's origin
   from D35 and history.

## Reproducing this audit

The audit read Trilium from a shallow clone outside the repository; it
ran no npm install and no build. The tag came in without file contents,
which git then fetched on demand at checkout:

```sh
git fetch --filter=tree:0 --depth=1 --no-tags origin \
  +refs/tags/v0.107.0:refs/tags/v0.107.0
git worktree add --detach <scratch>/v107 v0.107.0
git fetch --filter=tree:0 --shallow-since=2025-11-23 origin main  # history
```

The licence lookup and the graph walk are, in outline:

```sh
# 1. One registry lookup per lockfile entry ("name@version" under packages:)
#    scoped names are written @scope%2Fname; read .license (or .licenses[])
curl -s "https://registry.npmjs.org/$name/$version" | jq -r '.license'

# 2. Shipping bounds: from importers apps/server, apps/client, apps/desktop,
#    follow "link:" versions to workspace importers, then each snapshot's
#    dependencies and optionalDependencies, transitively.
#    Upper bound: importers' dependencies + optionalDependencies
#                 + devDependencies.  Lower bound: without devDependencies.
```

Check the lockfile's SHA-256 against the one above first. Rerun the audit
against each upstream release the fork merges, and update this file where
the answers change.
