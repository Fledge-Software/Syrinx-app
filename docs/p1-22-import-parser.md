# P1-22: HTML bookmark import parser

Date: 2026-10-11. Status: **partial — parser and preview summary implemented
and tested on branch `claude/p1-22-import-parser`; no preview/confirm UI, no
store writes, no folder-model integration.**

## Why this exists

The owner imports bookmarks once, from the HTML file that Chrome, Edge and
Firefox all export (D14). Atlas never reads another browser's profile, and v1
imports no passwords or history (D15). Under D43, imported pages become
saved rows that start unloaded, with the export's folder hierarchy and order
preserved. Opening every imported page would waste memory, and a lost folder
or a reordered list would make the import useless.

The import has two halves. This branch delivers the first: a pure function
that turns the file's bytes into a tree and a summary the owner can review
before anything is written. The second half — showing that preview, asking
for confirmation and writing the tree into the folder model — depends on
P1-21's folders, which another branch is building. Keeping the parser
independent means it can be reviewed and merged without waiting on that.

## What was built

`crates/atlas-core/src/bookmark_import.rs` and its four submodules:

| File | Role |
| --- | --- |
| `bookmark_import.rs` | Public types, limits, errors, encoding and declaration checks, line numbers. |
| `bookmark_import/tokenizer.rs` | Hand-written tokenizer: tags, attributes, text, comments, declarations. Borrows from the input; no allocation per token. |
| `bookmark_import/builder.rs` | Tree builder: folder stack, pending heading/link, entry and depth bounds, summary counts. |
| `bookmark_import/address.rs` | Which addresses become saved pages, and the skip reason otherwise. |
| `bookmark_import/text.rs` | Entity decoding, whitespace collapsing and title truncation. |

The public entry points are `parse_bookmark_html(bytes)` and
`parse_bookmark_html_with_limits(bytes, limits)`. They return a
`BookmarkImport`: top-level `ImportedItem`s (folders and bookmarks, in export
order) and an `ImportSummary` (bookmark, folder and empty-folder counts,
deepest nesting, duplicate addresses, shortened titles, unclosed lists and
every skipped entry with its line and reason).

**Why `atlas-core` and not a new crate.** The parser has no dependencies and
produces data the core model will consume when P1-21's folders exist.
`crates/xtask/src/dependency_policy.rs` only stops core crates from reaching
the shell, GPUI or WebView2; a module adds nothing it would need to check.
A separate crate would add a workspace member, a manifest and a dependency
edge for about 900 lines of code with one consumer. The main alternative,
`atlas-store`, was rejected because parsing must not touch the database.

**Why hand-written.** The export format is a tiny, fixed subset of old HTML.
A general HTML parser (for example `html5ever`) would add a large dependency
tree to audit, and its error recovery would not match what this format
needs, such as treating `<DT>` as an implicit end of a link.

## The format, and how the parser reads it

All three browsers write the "Netscape bookmark file":

```html
<!DOCTYPE NETSCAPE-Bookmark-file-1>
<META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=UTF-8">
<TITLE>Bookmarks</TITLE>
<H1>Bookmarks</H1>
<DL><p>
    <DT><H3 ADD_DATE="…" PERSONAL_TOOLBAR_FOLDER="true">Bookmarks bar</H3>
    <DL><p>
        <DT><A HREF="https://…" ADD_DATE="…" ICON="data:…">Title</A>
    </DL><p>
</DL><p>
```

- **Declaration.** The first thing other than whitespace or a comment must be
  `<!DOCTYPE NETSCAPE-Bookmark-file-1>` (any letter case), or the file is
  refused as `NotABookmarkFile`. This stops an ordinary web page from being
  "imported".
- **Folders.** `<H3>` names a folder; the next `<DL>` holds its contents and
  `</DL>` ends them. A heading never followed by `<DL>` is an empty folder.
  `PERSONAL_TOOLBAR_FOLDER="true"` (Chrome's bookmarks bar, Edge's favorites
  bar, Firefox's toolbar) and Firefox's `UNFILED_BOOKMARKS_FOLDER="true"`
  ("Other Bookmarks") become `FolderRole::Toolbar` and `FolderRole::Unfiled`,
  so the later UI can decide where they go. Nothing is placed yet.
- **Bookmarks.** `<A HREF=…>` with its text as the title and `ADD_DATE` as
  whole seconds since 1970. A non-numeric date is treated as absent.
  `ICON`, `ICON_URI`, `LAST_MODIFIED`, `SHORTCUTURL` and `TAGS` are ignored:
  embedded favicons are untrusted `data:` content, and Atlas fetches favicons
  itself.
- **Missing closing tags.** Real exports never close `<DT>` and sometimes omit
  `</A>` or `</H3>`. A link's title ends at `</A>` or at the next structural
  tag (`<DT>`, `<A>`, `<H3>`, `<DL>`, `</DL>`, `<HR>`, `<DD>`, `<H1>`). Other
  tags inside a title, such as `<B>`, are dropped and their text kept. A
  stray `</DL>` with nothing open is ignored. Lists still open at the end of
  the file are closed and counted as `unclosed_lists`.
- **Firefox extras.** `<HR>` separators are skipped and reported, because
  saved rows have no separators. `<DD>` descriptions are ignored. Firefox's
  `place:` smart-folder queries are skipped as an unsupported scheme.
- **Entities.** `&amp; &lt; &gt; &quot; &apos; &nbsp;`, upper-case variants of
  the first four, and decimal and hexadecimal numeric references are decoded
  in titles and addresses. Invalid code points become U+FFFD, as in browsers.
  Unknown named references (`&copy;`) stay as typed: the browsers never
  write them, and a full table is not worth the code.
- **Hidden characters.** Titles and folder names lose control characters
  other than whitespace, including NUL, and bidirectional embedding,
  override and isolate characters (U+202A–U+202E, U+2066–U+2069), whether
  raw or entity-encoded. Such characters could make a title such as
  `Pay<U+202E>fdp.exe` display differently from what it contains. Ordinary
  right-to-left text and the U+200E/U+200F marks are kept.
- **Whitespace.** Runs of whitespace in titles, including CR and LF, collapse
  to one space, and the ends are trimmed. CRLF and LF files give the same
  result, and a UTF-8 byte-order mark is accepted (test
  `crlf_lf_and_byte_order_mark_produce_the_same_import`).
- **Attribute quirk.** A raw `>` inside a quoted attribute value ends the tag.
  All three browsers escape `>` in attributes, so this only affects
  hand-edited files, where the bookmark is then skipped or its address
  rejected rather than misread silently.

### Which addresses are imported

Only `http` and `https` addresses with a host become bookmarks. This matches
what Atlas's page web views will open (`validate_url` in
`crates/atlas-platform/src/browser/page.rs`). The only other address the page
host accepts, `about:blank`, makes no sense as a saved page. The scheme is
lower-cased; the rest of the address is kept exactly. Skipped entries carry a
reason:

| Reason | Cause |
| --- | --- |
| `ScriptAddress` | `javascript:` bookmarklets. A saved page must never run script on open. |
| `DataAddress` | `data:` addresses, which can hide arbitrary content. |
| `UnsupportedScheme(name)` | `file`, `place`, `chrome`, `edge`, `about`, `ftp`, … The scheme name is shortened to 32 characters. |
| `CredentialsInAddress` | `user:password@host`. Navigation already refuses these, and Atlas never saves credentials. |
| `InvalidAddress` | No scheme or `//`; an empty host; a host with characters other than letters, digits, `-`, `.` and `_` (for example `%`); an unclosed or non-hex `[…]` literal; a port that is not a number up to 65535; whitespace, control characters or bidirectional override/isolate characters (U+202A–U+202E, U+2066–U+2069) anywhere in the address. |
| `MissingAddress` | No or empty `HREF`. |
| `AddressTooLong` | Over the address limit; a shortened address would point somewhere else. |
| `Separator` | A Firefox `<HR>`. |

The importer does not normalise addresses beyond the scheme (no host
lower-casing, no punycode).

**This is a second address rule, on purpose and temporarily.** Navigation's
rule is `web_address` in `crates/atlas-app/src/navigation.rs`, built on the
`url` crate. The parser cannot reuse it without moving it into `atlas-core`
and giving the core its first third-party dependency. `atlas-core` is
documented as dependency-free (`docs/LOCAL-DEV.md`), so that change goes to
review rather than being made quietly here. The parser's syntactic check is
stricter than before. The independent review found that
`https://[/`, `https://x.test:99999/`, `https://a%zz/` and addresses
containing U+202E were accepted; all are now skipped, with regressions in
`address.rs`. It is still not identical to `url`'s rules: it accepts no
percent-encoded or punycode hosts and does not check IPv4 or IPv6 literal
structure.

**Acceptance criterion for P1-22:** before anything is written, the commit
step re-checks every imported address with navigation's rule. That means
one shared function, not a copy. Entries that fail are reported as
skipped, never written. The commit step is the natural place to unify the
two rules, because it already sits where `url` is available. Unifying them
is recorded with the P1-22 acceptance note in `docs/plans/Atlas v1 — Plan.md`.

### Duplicates

Duplicates are matched as exact strings after entity decoding and
lower-casing the scheme. `https://Example.test/` and `https://example.test`
therefore count as different addresses. A bookmark whose address appeared
earlier in the file is kept, because
the owner may have filed the same page in two folders on purpose, and counted
in `duplicate_addresses` so the preview can mention it. Whether the import
should also flag addresses that already exist as Atlas saved pages is for the
commit step, which can see the store.

### Encoding: UTF-8 only

All three browsers write UTF-8 and say so in the `META` tag. The parser
accepts UTF-8 (with or without a byte-order mark) and refuses everything else
with a typed error: `UnsupportedEncoding` for a UTF-16 byte-order mark,
`NotUtf8 { offset }` for invalid bytes. It does not guess a legacy encoding
or replace bad bytes, because a silently altered address would import a
wrong destination with no warning. Hand-made files in Windows-1252 will
therefore be refused; the preview should tell the owner to re-export from a
browser.

## Bounds

Every bound is a field of `ImportLimits`, so tests can use small values.
The defaults:

| Bound | Default | Over the bound | Reasoning |
| --- | --- | --- | --- |
| `max_input_bytes` | 32 MiB | `InputTooLarge`, before parsing | Exports embed a base64 favicon per bookmark; tens of thousands of bookmarks with icons stay under this. |
| `max_depth` | 32 levels | `TooDeep { line }`, at the folder's heading | Real exports rarely exceed ten. Refusing is honest; flattening deeper folders would change the owner's hierarchy silently. |
| `max_bare_lists` | 4 open at once | `TooManyBareLists { line }` | `<DL>` lists without a heading. Exports have exactly one, the outer list. They are bounded separately so `<DL><DL><DL>…` cannot grow the stack, and so a damaged file is not misreported as folders nested too deep. |
| `max_entries` | 50,000 | `TooManyEntries { line }` | Bookmarks, folders and skipped entries together, so a file of separators cannot grow the skipped list without bound. Refusing avoids a half-imported tree. |
| `max_title_chars` | 1,024 characters | Shortened at a character boundary and counted in `truncated_titles` | Losing the end of a very long title is harmless; losing the bookmark is not. |
| `max_address_bytes` | 8 KiB | Bookmark skipped as `AddressTooLong` | Long search and tracking URLs fit; a cut address would be wrong. |

Memory stays proportional to the input, with a constant factor of roughly
three. The input is held once. Decoded titles and addresses add up to about
its size again. The duplicate check keeps a second copy of each address.
Tree and summary overhead come on top. The tokenizer borrows from the input
and scans each byte a bounded number of times, so time is linear: there is
no backtracking and no rescanning of earlier text. Malformed input should
never panic: every slice boundary comes from a `find` of an ASCII byte or the
input's length, and the generated-input test checks thousands of damaged
files.

## Privacy

Titles and addresses are the owner's data. `Debug` on `ImportedBookmark`,
`ImportedFolder` and `SkippedEntry` prints no title or address, and the
`Display` text of errors and skip reasons quotes nothing from the file except
a bounded scheme name. Test `diagnostics_never_show_titles_or_addresses`
checks this. `SkippedEntry::title` is public so the preview can name what
was skipped; it must not be logged.

## Tests and fixtures

All fixtures are hand-written and synthetic: invented titles, addresses on
`example.test` and `example.com`, and placeholder icon data. No browser
profile or real export was read. They are in
`crates/atlas-core/tests/fixtures/bookmarks/`:

- `chrome.html`: bookmarks bar with toolbar marker, three nested levels, a
  bookmarklet, a duplicate address, an empty folder, escaped titles and
  query strings.
- `edge.html`: favorites bar, an `http` address with a port, and `file:` and
  `edge:` addresses that are skipped.
- `firefox.html`: Firefox's CSP `meta`, `place:` query, `<HR>` separators,
  `<DD>` descriptions on folders and bookmarks, toolbar and unfiled markers,
  extra attributes, a `data:` address and a duplicate.
- `malformed.html`: lower-case declaration, missing `</A>`, `</H3>` and
  `</DL>`, single-quoted and unquoted attributes, a missing address, an
  address with credentials, a bogus date, a heading without a list, stray
  `</DL>`s and a tag inside a title.

Because this machine checks files out with CRLF line endings (see
`docs/trapstoknow.md`), the tests normalise each fixture to LF first and
build the CRLF variant themselves, so results do not depend on checkout
settings.

`crates/atlas-core/tests/bookmark_import.rs` checks each fixture's full
outline, roles, dates, counts and skipped lines. It also checks the refusal
of non-exports, UTF-16 and invalid UTF-8, and each bound at and beyond its
limit, including an entry left open at the end of the file. Its fuzz-style
test runs 3,000 deterministic cases from a seeded xorshift generator: half
random sequences of format fragments and markup debris, half random byte
edits of the fixtures. For every case that parses, it checks that the
summary matches the tree, every bound held and every address is http(s).
`proptest` is not in the lockfile, so no dependency was added. Unit tests in
each submodule cover the tokenizer, entity decoding and address rules.

**Gate record.** `cargo xtask check` passed on the first commit
(`target/p1-22-import-parser-gate-final.log`). The first run after the
review fixes stopped because Windows Application Control blocked the
unchanged `atlas-search` library test executable
(`atlas_search-b1ee3862afdb890c.exe`, os error 4551;
`target/p1-22-review-fixes-gate.log`). An unchanged retry passed the full
gate (`target/p1-22-review-fixes-gate-retry.log`). No Windows setting was
changed, and the block's cause is unproved, as in `docs/trapstoknow.md`.

## What is left for P1-22

1. **Preview and confirm UI.** Show the summary (counts, depth, duplicates,
   skipped entries grouped by reason, with their titles) and import only on
   the owner's confirmation. It needs a file picker for an exported file;
   reading a browser's profile directly stays out of scope (D14).
2. **Writing into the folder model.** Once P1-21 lands, map each
   `ImportedFolder`/`ImportedBookmark` to folders and unloaded saved pages in
   export order, in one store transaction, so a failed import leaves nothing
   half-written. Re-check every address with navigation's own rule first,
   through one shared function (the acceptance criterion above).
3. **Placement of special folders.** Decide where `Toolbar` and `Unfiled`
   folders go: kept as ordinary folders, merged into Favorites (D45) or
   placed at the top. This is a product question for the owner (below).
4. **Search and session.** Imported saved pages must appear in tier-1
   search and survive restart unloaded (P1-19, P1-21's consistency tests).

## Decisions for the owner

- **Toolbar folder placement.** Recommendation: import the bookmarks bar or
  favorites bar as an ordinary folder named as exported, and do not populate
  Favorites (D45) automatically. The owner can then drag what they want into
  Favorites.
- **Encoding.** Recommendation: keep UTF-8 only and tell the owner to
  re-export if a file is refused. All three supported browsers write UTF-8.
- **Shared address rule.** Recommendation: when the commit step is built,
  move navigation's `url`-based `web_address` to one place both callers use,
  and drop the parser's syntactic check down to a pre-filter. If that place
  is `atlas-core`, it gains its first third-party dependency (`url`, already
  audited and pinned for `atlas-app`), so the owner should approve it.
- **Bounds.** The defaults above are judgement calls (32 MiB, 32 levels,
  4 bare lists, 50,000 entries, 1,024-character titles, 8 KiB addresses). They are easy to
  change if the owner's own export is larger. Only the owner can check that,
  by running the preview on their own file; no agent should see it.
