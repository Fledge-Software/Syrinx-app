# CodeMirror 6 audit

Notes and Forge edit text in CodeMirror 6, a JavaScript editor component that
runs in a local web view (D28). Since decision 15 of 2026-10-21 the editor is a
host component: Syrinx ships one audited CodeMirror bundle on its own editor
origin, and no plugin JavaScript runs inside it in v1. `AGENTS.md` requires every
dependency to be checked for licence, maintenance and security record before it
is added, and until this file `cargo xtask check` read `Cargo.lock` only, so an
npm package could have entered with no check at all (#98, row 5).

The owner settled the rule on 2026-10-24 (`docs/decisions.md`, decision 12):
CodeMirror and every npm package it brings are audited here, pinned exactly in
a lockfile with integrity hashes, and shipped as a committed, built bundle; an
`xtask` check refuses any lockfile package missing from this audit and compares
the bundle's hash. Building the bundle from npm inside the gate was rejected,
because every gate run would then download code.

**Status, 2026-11-10.** This file audits the proposed package set in
`editor/package.json` and `editor/package-lock.json`. ⚠️ The owner has not yet
signed off on that set. No bundle is built or committed, and nothing in Syrinx
loads the editor: building the bundle, serving it and wiring the editor view
are #37's (P3-04).

## What the gate checks

`cargo xtask check-javascript`, which `cargo xtask check` runs after the plugin
contract, reads only files in the repository and makes no network call
(`crates/xtask/src/javascript_packages.rs`). It fails when:

- an npm manifest or lockfile (`package.json`, `package-lock.json`,
  `npm-shrinkwrap.json`, `yarn.lock`, `pnpm-lock.yaml`, `bun.lock`) exists
  anywhere but `editor/`, outside build output, installed packages and agent
  tooling;
- a dependency in `editor/package.json` is not an exact `x.y.z` version, or
  differs from the version the lockfile holds;
- the lockfile is not npm's version 3 format, or one of its packages is a
  linked folder, comes from anywhere but `https://registry.npmjs.org/`, has no
  SHA-512 integrity hash, or carries a licence that `licences.toml`'s `allowed`
  list does not satisfy (the same list and SPDX reading as the Cargo check;
  that file's exact-version exceptions are for Cargo packages only);
- a lockfile package is missing from the package table below, or the table
  records a different licence or integrity hash for it, or the table lists a
  package the lockfile no longer holds;
- `editor/dist/syrinx-editor.js` exists and its SHA-256 differs from the hash
  recorded below, exists while the audit records none, or is missing while the
  audit records a hash.

The gate cannot download tarballs, so it trusts the integrity hashes recorded
here; this audit recomputed each one from the published tarball (see
"Method"). npm itself checks the same hashes when #37 installs from the
lockfile with `npm ci`.

## The proposed package set

Eight direct runtime packages, one build tool, and what they pull in: 50
lockfile entries in all.

| Direct package | Version | Why |
|---|---|---|
| `@codemirror/state` | 6.7.6 | The document, selection and transaction model every other package builds on |
| `@codemirror/view` | 6.43.15 | The editor view: drawing, input, and the `EditorView.cspNonce` facet (see "Content-Security-Policy") |
| `@codemirror/commands` | 6.11.1 | Default key bindings and history (undo), which D28 leaves to CodeMirror while typing |
| `@codemirror/language` | 6.13.1 | Syntax-tree integration, folding and highlighting styles |
| `@codemirror/lang-markdown` | 6.5.2 | Markdown parsing for Notes' notes; it brings `lang-html`, `lang-css` and `lang-javascript` for HTML blocks inside Markdown |
| `@codemirror/search` | 6.7.2 | Find and replace inside a note |
| `@codemirror/autocomplete` | 6.20.4 | The completion surface for the wikilink completions Notes computes in Rust (decision 15) |
| `@codemirror/lint` | 6.9.7 | The diagnostics surface for language data the plugin sends |
| `esbuild` (development only) | 0.28.2 | Builds the one bundle file; never shipped |

This is the set #98 row 5 expected (`state`, `view`, `commands`, `language`,
`lang-markdown`, `search`, `autocomplete`, `lint`, the `@lezer/*` parsers and
`style-mod`, `w3c-keyname` and `crelt`), plus three groups it did not name:
`@codemirror/lang-html`, `lang-css`, `lang-javascript`, `@lezer/html`, `css`
and `javascript`, which `lang-markdown` brings for HTML inside Markdown;
`@codemirror/streamparser`, which `language` depends on; and
`@marijn/find-cluster-break`, which `state` uses for grapheme boundaries. Every version is the latest published
release on 2026-11-10.

**The bundler is esbuild.** It is one MIT package with no JavaScript
dependencies: its compiled program arrives as an optional package per platform
(`@esbuild/linux-x64`, `@esbuild/win32-x64` and 24 others), and npm installs
only the one whose `os` and `cpu` match the machine. All 26 are locked, because
a lockfile records every platform, and all are audited below. Rejected:
Rollup, which needs separate plugins to resolve npm packages and also ships
native code per platform, so its tree is larger for the same output; and Vite,
which wraps Rollup and a development server Syrinx does not want.

`editor/.npmrc` sets `save-exact`, so a future `npm install <package>` writes an
exact version, and `ignore-scripts`, so installing runs no package's install
script. Only `esbuild` declares one (`postinstall: node install.js`), which
checks and, on some platforms, copies its platform binary; esbuild's
JavaScript entry point finds the binary through its optional package without
it. ⚠️ #37 confirms that the bundle builds with scripts off.

## Method

Done on 2026-11-10 on the development Linux machine with Node.js 26.13.0 and
npm 11.20.0. Nothing was installed into the repository.

1. `npm install --package-lock-only` in `editor/` resolved the lockfile from the
   registry's metadata without downloading or unpacking any package.
2. Each of the 50 tarballs was downloaded from the lockfile's `resolved` URL into
   a scratch folder outside the repository. Its SHA-512 was recomputed and
   matched the lockfile's `integrity` for all 50.
3. The licence was read from each tarball, not assumed: the `license` field of
   its own `package/package.json`, and its licence file, whose text was
   checked for the MIT permission and warranty clauses.
4. The shipped packages' JavaScript was searched for `eval(`,
   `new Function(`, `fetch(`, `XMLHttpRequest`, `WebSocket`, `importScripts`
   and dynamic `import(` (results under "Code that runs in the editor view").
5. `npm audit` ran against the lockfile, and GitHub's advisory database was
   queried for every package name. The registry metadata gave each pinned
   version's publication date, the release count over the last twelve months,
   and whether the version is deprecated.

## Licences

All 50 packages are MIT, in their own `package.json` and in the lockfile. The
23 shipped packages and `esbuild` each carry a licence file with the MIT terms:
Marijn Haverbeke, CodeMirror's author, for the 23 CodeMirror, Lezer and helper
packages, and Evan Wallace for esbuild (`LICENSE.md`). The 26 `@esbuild/*`
platform packages hold only the program, a `package.json` declaring MIT and a
README; their licence text is esbuild's. The program is written in Go and
links the Go runtime (BSD-3-Clause); it runs on the build machine and never
ships. No package is GPL, and no new licence is needed.

When #37 commits the bundle, the MIT notices of the 23 shipped packages go
beside it in `editor/dist/`, because MIT requires the notice in copies.

## Maintenance

CodeMirror 6 and Lezer are maintained by Marijn Haverbeke, who also wrote
CodeMirror 5 and ProseMirror, and are under active development: in the twelve
months to 2026-11-10 `@codemirror/view` had 46 releases, `state` 10,
`language` 7 and `commands` 6, and `@lezer/markdown`, `lr` and `css` 9 each.
The main repositories moved from GitHub to the author's own forge
(`code.haverbeke.berlin`) during 2026; `lang-css`, `lang-javascript`,
`@lezer/html` and `w3c-keyname` still point at GitHub. A few small packages
rarely change because they are finished: `w3c-keyname 2.2.8` dates from
2023-06-07 and `lang-css 6.3.1` from 2024-11-26. None of the 50 versions is
deprecated, and each is the latest release of its package.

`@codemirror/streamparser 6.0.0` is a new package (first published
2026-10-07) that `@codemirror/language 6.13.1` depends on. Two pinned versions are recent: `@lezer/common 1.5.5` and
`@lezer/lr 1.4.12` were published on 2026-10-30, eleven days before this audit.
The exact pin absorbs that; moving any version is a reviewed change that
updates the table below.

esbuild (Evan Wallace) has been developed since 2017 and had 9 releases in the
last twelve months; it remains on 0.x, where a minor version may break its
interface, which only matters when the pin moves.

## Advisories

- `npm audit --package-lock-only` on the pinned lockfile: **0 vulnerabilities**.
- GitHub's advisory database (`/advisories?ecosystem=npm&affects=<name>`), queried
  on 2026-11-10: **no advisory** for any of the 23 CodeMirror, Lezer and helper
  packages. For `esbuild` it lists three, none of which affects 0.28.2:
  GHSA-67mh-4wv8-2f99 (development server readable by any website, fixed in
  0.25.0), GHSA-g7r4-m6w7-qqqr (arbitrary file read through the development
  server on Windows, fixed in 0.28.1) and GHSA-gv7w-rqvm-qjhr (the Deno
  module's binary download, withdrawn, fixed in 0.28.1). Syrinx never runs
  esbuild's development server.

This is a scoped check on the review date, not a guarantee that no
vulnerability exists. Repeat it whenever a version moves.

## Code that runs in the editor view

The search in step 4 found no `eval(`, `new Function(`, `XMLHttpRequest`,
`WebSocket`, `importScripts` or dynamic `import(` in any shipped package. The
one `fetch(` is in `@marijn/find-cluster-break`'s `bin/build-extenders.js`, the
maintainer's script that regenerates its Unicode tables; the package's
`main` and `exports` name only `src/index.js` and `dist/index.cjs`, so the
bundle never includes it. CodeMirror makes no network request of its own.

The bundle is served to the editor view by Syrinx itself, through the plugin-view
type's custom protocol, with **no network fetch at run time and no CDN**
(`AGENTS.md`, "JavaScript packages follow the same audit").

## Content-Security-Policy

Plugin and editor views run under a strict Content-Security-Policy that Syrinx
sets (decisions 6 and 7 of 2026-10-21). ⚠️ **Unconfirmed: whether CodeMirror
runs under a policy with no `'unsafe-eval'`.** The static evidence is
favourable: the search above found no `eval` or `new Function` in the shipped
code; `@codemirror/view` reads an `EditorView.cspNonce` facet and gives that
nonce to the style element it creates; and `style-mod`, which injects the
editor's styles, uses constructed stylesheets (`adoptedStyleSheets`) where the
engine supports them. Static reading does not prove the runtime behaviour on
WebView2 and WebKitGTK, nor whether a `style-src` nonce is enough without
`'unsafe-inline'`. #138's native fixture, the first plugin view, checks it.

## How the bundle will be vendored (#37)

1. `npm ci` in `editor/` installs exactly the lockfile, checking every
   integrity hash, with scripts off.
2. esbuild bundles one entry point into `editor/dist/syrinx-editor.js`.
3. The bundle and the MIT notices of the shipped packages are committed in
   `editor/dist/`, and the bundle's SHA-256 replaces `none` in the line below,
   in the same commit.
4. Syrinx serves the committed file to the editor view from its own custom
   protocol; nothing is fetched at run time.

`editor/node_modules/` is never committed (`.gitignore`), and `editor/dist/` is
kept byte for byte (`.gitattributes`), so line-ending conversion on Windows
cannot change the hashed bytes (`docs/dev/trapstoknow.md`, "Files on disk have
CRLF line endings").

## Changing the set

Moving a version or adding a package needs the same review as a Cargo
dependency (`AGENTS.md`). Update `editor/package.json`, regenerate the
lockfile with `npm install --package-lock-only` in `editor/`, repeat the
method above for every changed entry, replace the changed rows of the table
(the gate refuses a stale or missing row), and rebuild the bundle and its hash
if one is committed.

Bundle SHA-256: `none`

## Package table

The gate reads the Package, Version, Licence and Integrity columns.
"Bundled" packages end up in the editor bundle; "build tool" packages run on
the build machine only.

| Package | Version | Licence | Role | Licence file | Integrity |
|---|---|---|---|---|---|
| `@codemirror/autocomplete` | 6.20.4 | MIT | bundled, direct | LICENSE | `sha512-yF8RhOf2SXlg00+qtupzLhrglcas61boqaONiNcbPIebHCpJocpqTBA6dyrtUrQO47hqyTdeob/3XnYilT7TaQ==` |
| `@codemirror/commands` | 6.11.1 | MIT | bundled, direct | LICENSE | `sha512-O/4hG3SC1YwcmQ0d2UVNDs+AsaNWd1iHVxbTeEBuqH+6bExAiPK3iS/BvpY6rZGURALv4ZD3sIgcCmRvw3ehBg==` |
| `@codemirror/lang-css` | 6.3.1 | MIT | bundled, transitive | LICENSE | `sha512-kr5fwBGiGtmz6l0LSJIbno9QrifNMUusivHbnA1H6Dmqy4HZFte3UAICix1VuKo0lMPKQr2rqB+0BkKi/S3Ejg==` |
| `@codemirror/lang-html` | 6.4.12 | MIT | bundled, transitive | LICENSE | `sha512-pw2ReWKUqSkbvh76RAT4NYxiogRu+PWkR2ukAwO9uOgrm8uipkzjtKKtNpyeAQwHOqxEeSvAXZ6vr3AfyB9y/w==` |
| `@codemirror/lang-javascript` | 6.2.5 | MIT | bundled, transitive | LICENSE | `sha512-zD4e5mS+50htS7F+TYjBPsiIFGanfVqg4HyUz6WNFikgOPf2BgKlx+TQedI1w6n/IqRBVBbBWmGFdLB/7uxO4A==` |
| `@codemirror/lang-markdown` | 6.5.2 | MIT | bundled, direct | LICENSE | `sha512-AwBOdkWYuA//WcM0xO5PfHPUcmz/O2i5o0Nsg1U69SII/loCJlFI1Romd9xp2HYb1kYJRGZotyqRghuHH5n8Kw==` |
| `@codemirror/language` | 6.13.1 | MIT | bundled, direct | LICENSE | `sha512-zfvU2r3TyylQLMj/te+5CnQ0rSqZi8B1oTCBbQkagcLW4Xc6imab6qN+pEJhsuNOLaJqiRyBXp9UhWA1DhVvDw==` |
| `@codemirror/lint` | 6.9.7 | MIT | bundled, direct | LICENSE | `sha512-28/+iWLYxKxsvGYhSYL7zaCZqLz5+FFFDq9tVsvGv9kv8RY4fFAchJ5WX9M3YrrRlTIsECjsXPqeNgnSmNP2dg==` |
| `@codemirror/search` | 6.7.2 | MIT | bundled, direct | LICENSE | `sha512-gUYkYhT2+n/+VGZ+8EzE5WFkYZUZYm1VOKDudIsNqh42uRVQJ0a6Yss9sdKT3MeOYfuL1N6AZA57oza0Oyr0LA==` |
| `@codemirror/state` | 6.7.6 | MIT | bundled, direct | LICENSE | `sha512-kAz+AncRtKuIknedxT1bq4XwXv4UowhbkHU1myPrtVb/jZtImWuV5BXzv5vK6i3kYACsdiZiQKFQQ5Mq7elW8w==` |
| `@codemirror/streamparser` | 6.0.0 | MIT | bundled, transitive | LICENSE | `sha512-JTFPF6xrfjt0mnfGPgzcLiMwYmvIyNaoizp8BYzv8FE8KrT6GBGDZ190rtt+O7Wke3gG/MFHyiY9YGNeimof4w==` |
| `@codemirror/view` | 6.43.15 | MIT | bundled, direct | LICENSE | `sha512-HHsfpqFeHyXLs983pSRe38Xajs0vFWZQ2BuX0m7Ki8w3jZ5iD8cuR3lBz2/5/X53Dhf32SBHKh6rtSTB6tKeGw==` |
| `@esbuild/aix-ppc64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-XExcO+dvLKvVtNTibSTBej1NCAbaGhWn9Ww1ZPx80qsahhPFe/8jgWP0IchNe0F3HwkU7n8ejhH8bjonqht8mQ==` |
| `@esbuild/android-arm` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-kXXoiPVVGQcnIYGOeaovwOURpniDBpSq4A03qkQ+BMQqtGG6HYap3xne9C1O1yo4TR3qxlCX5IqqmX6fFo2Lqg==` |
| `@esbuild/android-arm64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-5YfKeeI8qWfBZIX+u2xZC3Zlb3Os/gLS2sbEKM+I4ZOcsWmHS2WLysCcQZDAFRslDUU5Oiq44gf6PYN1vGwG5A==` |
| `@esbuild/android-x64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-O387ite7SzUyCcy3JQX4P4bLtEA7bLLkx+esve5JHnyYfNTxcVpXZo9jhdB0lTKN44gztELTdU7nS8Nr16Fs1Q==` |
| `@esbuild/darwin-arm64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-n4KqkOQrraxHJcgjM1RvwbigfQKIKJVpM7xp+KsxiyUSrRdIXnt73VhrPAx0fV44hgfmIVKjxMN9J1t5jySVkw==` |
| `@esbuild/darwin-x64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-uq6suIWYP37qzGddBKPw5QEQPi6HiLGsO7UmkpfyaYNQ3D+rN6w6WfwH+nuqcGXWvawGwxOEroO4YGnFh95azw==` |
| `@esbuild/freebsd-arm64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-n+I0BTSRIoy+d6RPKnEVwql5UwBJolytvY4mAOIEJorKlqgPII8ix6slVVrfZ5Tnj7glIZvloylbB/EJPMWEXw==` |
| `@esbuild/freebsd-x64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-78XJTJkvPs0kz2w61301PJjXl4g7q3JqiYMZ/M/yVI73EHBrCRTgkhu9oqG7vPqq+a/yadEW8aD+agKlk5xrmg==` |
| `@esbuild/linux-arm` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-XlDnu2q5yoqems+xay6wSAcg9DDD7K9RLKZEBOMZm3ckNpJBvOX20tSfby8KfrrhINDyv9V2YVZKY/SpoGJI8w==` |
| `@esbuild/linux-arm64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-pW4AC0P3it8c7do9MVM4p51FzHzdM/TZrerurgRcHJ2WTa1VQ1CIq18xncfpBJw4ojkiZZrKW2yIBWBP92j6Ug==` |
| `@esbuild/linux-ia32` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-CYbnj78HsIeA+DhgUKgFCfvNsTHFhMMrinUrMZpDXJXKN8T3XViTZ/+wtHeVxEWY8ewSzTFN+nRmSwO2tZaLUQ==` |
| `@esbuild/linux-loong64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-buwkd8nsph4R+ajRvw0qM5Hja/TXQow3ptzWO2EbG/cqcIkHloRrdlBtQlshyYGTNFvfkfJ5tpPLVkY4DtsPfQ==` |
| `@esbuild/linux-mips64el` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-ZVykbDyk7519VwiNb9Lcj9m8XM6v5V9uKPvrEMkkEedVewf+0itkhahp4HDpgERXhwLRpWFypsGbG/J8s0QjJA==` |
| `@esbuild/linux-ppc64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-CAXl+Dtd9UUuJd8pKKdwh6MLm3MUMiqMPmhZ3tTSXPqfyQ3vDl6R5hZdZ/kYojK4ofXtdfSv1tFq8XzWx3heNQ==` |
| `@esbuild/linux-riscv64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-GeXCej4IQtU1B+QlDV8W/RRvbzI3O/Stss+/bCXv4lZls5WGRtu2a+3JkA3i4qIUlMXpcHebWpF8AkJhATowuA==` |
| `@esbuild/linux-s390x` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-3H1weTYZPxt/WOhByszQZybS9w5lKzUn1FDMsgEChbHWQwHYQQRfBxgCcZvPhjHfKyJjIievvMmEUawJrdY9Dg==` |
| `@esbuild/linux-x64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-4xTZr1FUmSoQW4XIWmit3tzQrUTZM+N3P0XV8xROKYF50XfI7xeO90+1bZvNwxIufQ9hDQVRJH5YhgPVF8A/HQ==` |
| `@esbuild/netbsd-arm64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-sSATRjPeDBg3pdgHoQfoYBob11Kk1FGa9lui5RIHZCoCkJa9QKlvl3/vKz2usCmYYjs7ymJR/2Nnsqe+Hjt5nw==` |
| `@esbuild/netbsd-x64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-lqnzCV+mM0gIADaKihiCg6ifgfU2L3h5E33rNQBN1Y4MaVGnzryzmvvf7UHxprpQdE8hpqLolJ9Rl+SkIRDpyw==` |
| `@esbuild/openbsd-arm64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-AL2qJILH7lNjrDmCQDvdxMfAUIv8KMNZOvrwAQ8i8//ntL9FflhOyMJ8OZSMBb8/AWXe3/5v5S20y3zCoZWKoQ==` |
| `@esbuild/openbsd-x64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-QtiuPytchRyC4rwUKhexJdQKvDuZ6hWloi3igqPQNUJCS1/v9EiO3UTOXR6A3FoMo4fnAKbWJdqaIwhOzh8qEw==` |
| `@esbuild/openharmony-arm64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-WkhYDmpTjLvGlScA1rwjRUmhl4k8oXR3cIbtqWmELgU/dFeHHlEllxDvdWcNJV9rbzCexB5vz8gtNewWLgCT7Q==` |
| `@esbuild/sunos-x64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-GPMSkTOtMnv2U2F8gxe4Io6qmVs+YKyp832Etqqxr0hFngmXQ3rzwytelm3GIn7T4VviRUlf3sOgBOiTdvaf7g==` |
| `@esbuild/win32-arm64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-PIhhEkE9uPBleRBrQEJpUn7MBnibZzbGzYWPmY3x+YoVg/95zbjB4CxPPOQ8l5tYYM4mMaCthF8/1DIfBQQyWQ==` |
| `@esbuild/win32-ia32` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-YmJbfTlvU7Sdn9BB+4PRES4oB6pxgS37MAONj+hBr/cpXS1aBPKXxNnDbu+QCWPj0o9dgyxeq79g6c5P8KeuYA==` |
| `@esbuild/win32-x64` | 0.28.2 | MIT | build tool, one platform binary | none (manifest only) | `sha512-5ebpxr3nWMzrL/rnUI755Jkuee0bHL/Gq0WTF9lvcpv73wAp5eu8MfBUgWK9bhWvZjj7yX8etf/8tI8Ney695g==` |
| `@lezer/common` | 1.5.5 | MIT | bundled, transitive | LICENSE | `sha512-Pw7PVCndruF1pb3ebCxpSVpkMHSDjwRCLcTRw+RizUKco5QbWylwq2g4lnG/RYN7yTTQDXE9F77MwtkAyphrjg==` |
| `@lezer/css` | 1.3.9 | MIT | bundled, transitive | LICENSE | `sha512-ZmzgvTOBdJWGIsY/ItAfN7UW8DV802oZG+jeG6E508SwEzz5YhuAajptpWPICvIyqYAD2+VjMdOS40GgGkyEOQ==` |
| `@lezer/highlight` | 1.2.5 | MIT | bundled, transitive | LICENSE | `sha512-O1GMVKgtf5YspFaRzpmqkVgMtIk2HG9uMAbH6wdAIdNh3TVzCsch/+cRi+6l2//UGDKe6NB9BW3cvuR5ezMQ0w==` |
| `@lezer/html` | 1.3.13 | MIT | bundled, transitive | LICENSE | `sha512-oI7n6NJml729m7pjm9lvLvmXbdoMoi2f+1pwSDJkl9d68zGr7a9Btz8NdHTGQZtW2DA25ybeuv/SyDb9D5tseg==` |
| `@lezer/javascript` | 1.5.6 | MIT | bundled, transitive | LICENSE | `sha512-sEvOqpl4vJQca0NugSQK02vQPbc5KrvoojnYGoqqFgFO0MtcYhwGiHKVEB+LuXYJnfyXo4QPA8ztbrEa4Fw5NA==` |
| `@lezer/lr` | 1.4.12 | MIT | bundled, transitive | LICENSE | `sha512-On9HU4oMMiHPMkQDgoTnGahjgG5n/MkN38CgG77sMraKLL+3JBbeQNi1FcCXV9QRqwF9/BUXP3caHyJC1fBOTQ==` |
| `@lezer/markdown` | 1.8.1 | MIT | bundled, transitive | LICENSE | `sha512-M5YmEcKWO4mJ45Y1hECwqyu8cLv25EiXOK554PVOvDOWOP0olwelHHQUWr9sjF+d1bENVhy0nfJ8E57XZUTiXg==` |
| `@marijn/find-cluster-break` | 1.0.4 | MIT | bundled, transitive | LICENSE | `sha512-Wy0V7+SGUjnF9/TkiM1hKVDPj7jKXduPNboMVtHTA8dySMURWqfg/JZ9E2Sq8JgSJmkl7k7Qe9FLeMSrSraWmQ==` |
| `crelt` | 1.0.7 | MIT | bundled, transitive | LICENSE | `sha512-aK6BbWfhf4U/wCcLHKPJl/xa6VkVstRaPywWtMKGwuOLc/wZTyQYuoxgvZnNsBvv7Kg3YTBQYYBCggcviQczuA==` |
| `esbuild` | 0.28.2 | MIT | build tool, direct | LICENSE.md | `sha512-HKVLS8dvII+xoKW9kmqxbRKrnWEXfJJr/FZhhJmiqIB0e053QNYFqOBouTMO/k5sID4MvCiUCvv8b9M4h32wIA==` |
| `style-mod` | 4.1.4 | MIT | bundled, transitive | LICENSE | `sha512-XXWIQt633/EpAFx8aZDOTjBzrCaGmhvEQlQo6MVPfa2OzO2cWo+4hV9h+6UkHYlXGfy+ODXKUdP7Pthmcu5ATw==` |
| `w3c-keyname` | 2.2.8 | MIT | bundled, transitive | LICENSE | `sha512-dpojBhNsCNN7T82Tm7k26A6G9ML3NkhDsnw9n/eoxSRlVBB4CEtIQ/KTCLI2Fwf3ataSXRhYFkQi3SlnFwPvPQ==` |
