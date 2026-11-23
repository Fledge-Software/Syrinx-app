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
security record looks like, every feature that reaches the network, and the
owner's bundling decision.

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
2026-11-21, and `main` has moved on since. And Trilium's `SECURITY.md` says
that "only the latest stable minor release receives security fixes", so a
fork based on a stable tag can take upstream's patch releases as they come,
while a fork based on an arbitrary `main` commit sits between releases with no
fix line of its own. Every finding below was read at `v0.107.0` unless it says
otherwise.

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
  credentials of #281, the changes endpoint, and the source link discussed
  under "§13" below. Each is its own commit with a message saying why, and
  new behaviour goes in new files where Trilium's structure allows it,
  rather than in edits spread through upstream's files.
- Rebasing the fork's branch onto upstream was rejected: it rewrites a
  public branch's history, which `AGENTS.md` forbids for shared branches, and
  it loses the record of which upstream release each fork build contains.

Because `SECURITY.md` supports only the latest stable minor release, the fork
has to follow every upstream minor release, not just patch releases; a fork
that falls a minor version behind receives no upstream security fixes.

## What a Trilium build is made of

Trilium is a pnpm workspace (pnpm is an npm-compatible package manager)
with 25 member packages, its `apps/` and `packages/` folders. Three of them
are the program Syrinx would run: `apps/server` (the HTTP server),
`apps/client` (the web application it serves) and, for the desktop build,
`apps/desktop` (an Electron wrapper). Root `package.json` declares
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

`pnpm-lock.yaml` at `v0.107.0` (lockfile format 9.0, `packageManager`
`pnpm@12.18.0`) resolves **2,364 packages**, every one from the npm registry
(no git or tarball sources). A pnpm lockfile does not record licences, so
each package's `license` field was read from the registry
(`https://registry.npmjs.org/<name>/<version>`), one request per exact
version, six at a time, with every answer cached; all 2,364 answered. These
are the licences the authors declare in their published metadata, not the
result of reading every package's licence files; the exceptions below were
checked further where it matters.

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
do; #283 (packaging) should, if the fork's build is ever distributed by
Fledge.

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
  LICENSE IN README.md". Its README places use under Anthropic's Commercial
  Terms of Service and describes the data it collects; it is not an open
  source licence. The server loads it on demand for its AI chat feature
  (`apps/server/src/services/llm/providers/claude_agent.ts`), and it is
  inside what a server build ships.
- **`elkjs@0.12.1`, EPL-2.0 OR GPL-3.0-or-later (ships):** a graph-layout
  library pulled in by `mermaid@12.3.0`, the diagram renderer. Either licence is copyleft; the
  Eclipse Public License is the weaker, file-level one.
- **`lightningcss@1.33.0` and its eleven platform binaries, MPL-2.0
  (build):** a CSS compiler used while building. MPL-2.0 is Syrinx's own
  licence and file-level; it matters only if it ships, which a build tool
  does not.
- **No licence in the registry metadata:** `buffers@0.1.1` and
  `khroma@2.1.0` (ships), `color-convert@0.5.3` and `exif-parser@0.1.12`
  (build), `union@0.5.0` (outside). Their repositories were not checked.
- **`@img/sharp-*@0.35.5` and `@img/sharp-libvips-*@1.3.4`, LGPL-3.0-or-later
  alone or with Apache-2.0 and MIT (14 packages, outside):** the libvips
  image library's prebuilt binaries for the sharp image package, used only
  outside the shipping apps.
- **MPL-2.0 tooling for the web clipper (outside):** `addons-linter@10.14.0`,
  `addons-moz-compare@1.4.0`, `addons-scanner-utils@16.0.0`,
  `eslint-plugin-no-unsanitized@4.1.5`, `fx-runner@1.7.0`, `web-ext@10.8.0`.
- **Fonts and data under Creative Commons or OFL:** `caniuse-lite`
  (CC-BY-4.0, build data), `@fontsource/arimo@5.3.0` (OFL-1.1, outside), and
  `boxicons@2.1.4` (CC-BY-4.0 OR OFL-1.1 OR MIT, ships; MIT may be chosen).
- **Dual licences where a permissive choice exists:** `jszip@3.10.2`
  (MIT OR GPL-3.0-or-later), `node-forge@1.4.2` (BSD-3-Clause OR GPL-2.0),
  `dompurify@3.4.17` (MPL-2.0 OR Apache-2.0), `unorm@1.6.0` (MIT or
  GPL-2.0), `json-schema@0.4.0` (AFL-2.1 OR BSD-3-Clause). A distributor
  takes the permissive option.

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
literal `"GPL"` in `apps/client/src/widgets/attribute_widgets/AttributeEditor.tsx`,
`apps/client/src/widgets/type_widgets/llm_chat/ChatInputBar.tsx` and
`apps/client/src/widgets/type_widgets/mind_map/NodeMemo.tsx`. No other key
appears in the client.

**No premium packages.** The lockfile contains no `ckeditor5-premium-features`
package and none of CKEditor's commercial-only plugins (real-time
collaboration, track changes, comments, revision history, export and import,
AI). It does contain `@ckeditor/ckeditor5-ckbox`,
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
developing it. Contributor statistics were read from git, not the API: in the
twelve months to 2026-11-23, `main` received 17,712 non-merge commits from
267 author names. The two most active authors are Elian Doran (10,466
commits) and Adorian Doran (2,366); the dependency bot `renovate[bot]` made
1,841 and the translation service `Hosted Weblate` 230. Most of the work, and
so the project's continuity, rests on one or two people.

**Release cadence.** Fifteen stable releases were tagged in those twelve
months, eight minor and seven patch releases: `v0.100.0` (2025-12-05),
`v0.101.0` to `v0.101.3` (2025-12-25 to 2026-01-08), `v0.102.0` to
`v0.102.2` (2026-03-01 to 2026-04-05), `v0.103.0` (2026-05-13), `v0.104.0`
and `v0.104.1` (2026-07-18 and 07-25), `v0.105.0` (2026-08-19), `v0.106.0`
and `v0.106.1` (2026-09-25 and 10-23) and `v0.107.0` (2026-11-08), with
`v0.108.0-rc.1` on 2026-11-21. That is a minor release roughly every six
weeks, which the fork must merge to stay supported.

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
is why #277 keeps the fork on loopback with a Syrinx-issued credential, and
why the fork must keep up with upstream's minor releases.

## Features that reach the network

Each item below was read in the source at `v0.107.0`. "Option" means a row in
Trilium's own `options` table, set in its Options screen and stored in its
database; defaults are in `packages/trilium-core/src/services/options_init.ts`.
"config.ini" means a key in the data directory's `config.ini`, whose
template is `apps/server/src/assets/config-sample.ini`; every such key can
also be set by an environment variable named `TRILIUM_<SECTION>_<KEY>`
(`apps/server/src/services/config.ts`). #281 is the slice that turns these
off by default in the fork; the list says where each one is switched.

**Who listens.**

1. **The listening address.** The server listens on **all interfaces by
   default**: `Network.host` defaults to `0.0.0.0` in
   `apps/server/src/services/config.ts`, overridable by `config.ini`
   `[Network] host`, `TRILIUM_NETWORK_HOST`, or `TRILIUM_HOST`, which
   `apps/server/src/services/host.ts` checks first. The port defaults to
   8080 (`DEFAULT_NETWORK_PORT`), set by `[Network] port`,
   `TRILIUM_NETWORK_PORT` or `TRILIUM_PORT` (`services/port.ts`).
   `apps/server/src/www.ts` also accepts a Unix socket path as the host.
   Only the Electron desktop build binds `127.0.0.1` unless its
   `allowLanAccess` setting is on. #281 must pin `127.0.0.1`.
2. **Authentication can be switched off.** `config.ini` `[General]
   noAuthentication` (`TRILIUM_GENERAL_NOAUTHENTICATION`) skips the login
   check (`apps/server/src/services/auth.ts`). It defaults to false and must
   stay so.
3. **ETAPI**, the external REST API under `/etapi/*`
   (`apps/server/src/etapi/`, registered in
   `apps/server/src/routes/routes.ts`), is always mounted and accepts
   requests carrying a token the user created. The Notes plugin's feed will
   use it or #281's changes endpoint; it is the surface the security fixes of
   `v0.107.0` touched.
4. **MCP server**, an AI-agent endpoint at `/mcp`
   (`apps/server/src/routes/mcp.ts`), answers only when the option
   `mcpEnabled` is true (default false) and the request carries an ETAPI
   token.
5. **Web clipper endpoints** under `/api/clipper/*`
   (`apps/server/src/routes/routes.ts`) receive pages from Trilium's browser
   extension; on the server they require an ETAPI token.
6. **Sync target.** The server accepts sync connections from other Trilium
   instances under `/api/sync/*` (`packages/trilium-core/src/routes/api/sync.ts`),
   behind the user's password.
7. **Public sharing.** Notes placed under the `_share` root
   (`packages/trilium-core/src/share/share_root.ts`) are served to anyone,
   without login, under `/share/` (`apps/server/src/share/routes.ts`,
   registered in `routes.ts`). The options `redirectBareDomain` and
   `showLoginInShareTheme` (both default false) shape it; the route itself is
   always mounted. On loopback only the machine can reach it, but every web
   page the owner opens can.

**What reaches out.**

8. **Update check.** The option `checkForUpdates` defaults to **true**. The
   web application, not the server, asks
   `https://api.github.com/repos/TriliumNext/Trilium/releases/latest`
   (`RELEASES_API_URL` in `apps/client/src/widgets/buttons/global_menu.tsx`).
   Inside Syrinx this request comes from a page tab.
9. **Sync to another Trilium.** Off until a sync server is named: the option
   `syncServerHost`, or `config.ini` `[Sync] syncServerHost`
   (`TRILIUM_SYNC_SYNCSERVERHOST`, alias `TRILIUM_SYNC_SERVER_HOST`), with
   `syncProxy` and `syncServerTimeout` beside it. `isSyncSetup` in
   `packages/trilium-core/src/services/sync_options.ts` treats an empty value
   or the literal `disabled` as off; once set, `sync.ts` syncs every 60
   seconds.
10. **Automatic image download.** The option `downloadImagesAutomatically`
    defaults to **true**: when a note contains a remote image, the server
    fetches it and stores a copy
    (`packages/trilium-core/src/services/image_download.ts`).
11. **Link previews.** Pasting a link into a note makes the server fetch the
    page, its oEmbed data and thumbnails (YouTube's included), with its own
    `User-Agent` (`packages/trilium-core/src/routes/api/link_embed.ts`). No
    option switches this off; `SECURITY.md` says such fetches are limited to
    public addresses.
12. **AI chat and its providers.** The option `aiEnabled` (default false
    unless the old `llm` experimental feature was on) gates the chat. With it
    on, the server calls whichever providers are configured in `llmProviders`
    (Anthropic, OpenAI, Google, DeepSeek, OpenRouter or a local model;
    `packages/trilium-core/src/services/llm/providers/`), a web-search
    provider named in `llmWebSearchProvider` (Brave, Tavily, Exa, Serper,
    Perplexity or DeepSeek; `services/llm/web_search.ts`), and it can start
    agent programs: Claude's agent SDK, Codex, Copilot and Google's
    Antigravity (`apps/server/src/services/llm/providers/`). The Options
    screen looks up an Antigravity download from `dl.google.com`
    (`apps/client/src/widgets/type_widgets/options/llm/antigravity_download.ts`).
13. **Text recognition (OCR).** Trilium runs Tesseract
    (`apps/server/src/services/ocr/tesseract_recognizer.ts`) without setting
    a language-data path. In `tesseract.js@7.0.0`, the version the lockfile
    pins, an unset path makes the worker download language data from
    `cdn.jsdelivr.net` (`src/worker-script/index.js` of the published
    package), cached under Trilium's data directory. Automatic OCR of images
    is off by default (option `ocrAutoProcessImages`, false); a user-started
    OCR still downloads.
14. **OpenID Connect login.** Off until `config.ini` `[MultiFactorAuthentication]`
    `oauthBaseUrl`, `oauthClientId`, `oauthClientSecret` and
    `oauthIssuerBaseUrl` are set (or `TRILIUM_OAUTH_*`); then login goes
    through that provider (`apps/server/src/services/open_id.ts`).
15. **OneNote import.** Signs in to Microsoft
    (`login.microsoftonline.com`) and reads from `graph.microsoft.com`
    (`apps/server/src/services/import/onenote/`) when the user starts it.
16. **Map notes.** The geo map view loads tiles from OpenStreetMap,
    VersaTiles or MapGenie and searches places through
    `nominatim.openstreetmap.org`
    (`apps/client/src/widgets/collections/geomap/`), from the page, when a
    map note is opened.
17. **Backend scripts.** Scripts in notes that run on the server have full
    access, network included. Off by default: `config.ini` `[Security]
    backendScriptingEnabled` (`TRILIUM_SECURITY_BACKEND_SCRIPTING_ENABLED`),
    false since `v0.104.0`. Frontend scripts run in the page and can make any
    request a page can; there is no switch for them.
18. **Links to upstream in the interface.** The About dialog
    (`apps/client/src/widgets/dialogs/about.tsx`) and global menu link to
    `github.com/TriliumNext/Trilium`; these open only when clicked.

A search of the server, core and client sources for telemetry, analytics or
crash-reporting services (Sentry, PostHog, Plausible, Umami, Google
Analytics) found none. The Claude agent SDK's README says the SDK itself
collects usage feedback, which applies only once AI chat is on and that
agent is chosen.

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
  whichever option is chosen the fork should point those links at its own
  public repository and exact commit. This costs one small fork commit and
  should be part of #281.
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
proprietary terms, whose redistribution terms this audit did not review).
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
   repository always is; and untick "Copy the `main` branch only". Check that tag `v0.107.0`
   exists in the fork afterwards; if it does not, push it there from a
   clone of upstream. Keep upstream's
   `LICENSE` untouched. Then create the branch `syrinx` from tag `v0.107.0`
   (`d83d7151f1c6d4292be88d8d34008f409b252f7f`), make it the default
   branch, and leave `main` mirroring upstream. Recommended as described;
   #281 can start once it exists.
2. **Decide bundling.** Installed separately, or bundled in Syrinx's release
   (section above). Recommendation: installed separately until #283 and a
   licence review.
3. **Record the 2026-11-23 decision in `docs/decisions.md`.** Epic #277
   cites a `docs/decisions.md` entry for 2026-11-23 that does not exist on
   `main`, and #278's definition of done asks for the bundling decision to be
   recorded there. Agents do not write decisions; the owner (or an agent at
   the owner's request) adds the entry.
4. **The Claude agent SDK inside Trilium's server build.** It ships under
   Anthropic's commercial terms, not an open source licence. Recommendation:
   leave AI chat off by default in the fork (#281 does), and if the owner
   chooses to bundle, have the licence review cover whether that package may
   be redistributed, or build the fork without it.

## Reproducing this audit

The audit read Trilium from a shallow, blob-filtered git clone kept outside
the repository, checked out at `v0.107.0`; it ran no npm install and no
build. The licence lookup is one GET per lockfile entry to
`https://registry.npmjs.org/<name>/<version>`, reading `license` (or the old
`licenses` array), with scoped names written `@scope%2Fname`. The shipping
bounds come from walking `pnpm-lock.yaml`'s `importers` and `snapshots`
sections from `apps/server`, `apps/client` and `apps/desktop`, following
`link:` entries to workspace packages, once with `devDependencies` and once
without. Rerun it against each upstream release the fork merges, and update
this file where the answers change.
