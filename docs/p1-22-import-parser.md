# P1-22: HTML bookmark import parser

Date: 2026-10-11, completed 2026-10-12. Status: **implemented on branch
`claude/p1-22-import-commit`, awaiting independent review and the owner's
manual check.** The parser merged first; this branch adds the shared address
rule (DW-1), the commit step, the file dialog and the preview window. See
"The import step" below.

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

**Why `atlas-core` and not a new crate.** The parser needs only `url`, through the shared address rule, and
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

Only `http` and `https` addresses that navigation would open become
bookmarks. **There is one address rule** (DW-1, done 2026-10-12):
`atlas_core::web_address` in `crates/atlas-core/src/web_address.rs`, built on
the `url` crate. Navigation (`crates/atlas-app/src/navigation.rs`) and the
parser both call it, and the commit step calls it again on every bookmark
before writing. The owner approved `atlas-core` taking `url` on 2026-10-12;
it is the exact version already pinned for `atlas-app` (`=2.5.8`), so the
lockfile gains one dependency edge and no package.

Before that rule, the parser only refuses what an export never contains raw:
an address that does not start `scheme://`, and whitespace, control
characters or bidirectional override/isolate characters (U+202A–U+202E,
U+2066–U+2069). The scheme is read first so the preview can say why
something was skipped. Skipped entries carry a reason:

| Reason | Cause |
| --- | --- |
| `ScriptAddress` | `javascript:` bookmarklets. A saved page must never run script on open. |
| `DataAddress` | `data:` addresses, which can hide arbitrary content. |
| `UnsupportedScheme(name)` | `file`, `place`, `chrome`, `edge`, `about`, `ftp`, … The scheme name is shortened to 32 characters. |
| `CredentialsInAddress` | `user:password@host`. Navigation already refuses these, and Atlas never saves credentials. |
| `InvalidAddress` | Refused by the shared rule (no host, a bad port, an invalid IPv6 literal, a forbidden host character such as `%`), or by the pre-filter above. |
| `MissingAddress` | No or empty `HREF`. |
| `AddressTooLong` | Over the address limit, checked on the raw address and again after normalizing, which can lengthen it; a shortened address would point somewhere else. |
| `Separator` | A Firefox `<HR>`. |

**Addresses are now saved in the shared rule's normalized form**: the scheme
and host lower-cased, international hosts in punycode, default ports
dropped, and an empty path written as `/`. This is what navigation would
open, so a saved row and a typed address compare equal. Three earlier parser
expectations changed with it, deliberately: `https:///path` is read as
`https://path/` and `https://a!b.test/` is accepted, as navigation already
did, and `https://Upper.Example.Test/Path` is saved as
`https://upper.example.test/Path`. Duplicate counting uses the normalized
form, so `https://Example.test` and `https://example.test/` now count as
duplicates.

### Duplicates

Duplicates are matched as exact strings after entity decoding and
normalizing with the shared address rule. A bookmark whose address appeared
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

## The import step

**Entry point.** There is no command registry or menu yet, so the palette's
submission path carries the one command: typing `>import bookmarks` (the
spec's `>` command prefix; spacing and letter case do not matter) and
pressing Enter starts the import. If a local search result is highlighted,
Enter opens it instead, so press Alt+Enter, which always submits the typed
text. The interception is a single guarded arm in `ui.rs`'s palette handler;
`palette.rs` is unchanged. When the command registry (P1-08 onwards) exists,
the command should move there.

**File dialog.** `atlas_platform::file_dialog::choose_file` shows Windows'
own file-open dialog (`IFileOpenDialog`) through the `windows` features
`atlas-platform` already enables, so no crate or feature was added. Only
existing file-system files can be chosen. It runs on a dedicated thread with
its own COM apartment, owned by the main window, so the interface thread
never blocks or re-enters. The same thread reads the file (refusing more
than 32 MiB from its size before reading, and reading at most one byte past
the limit) and parses it. Nothing from the file or its path is logged.

**Preview.** `crates/atlas-app/src/ui/import.rs` opens a popup owned by the
main window, like the palette, drawn with `atlas-ui` tokens (spacing, radii,
type sizes, the panel, line, text and accent colours). It names the new
folder, then shows pages, folders and empty folders, the deepest nesting,
duplicate addresses, shortened titles and closed lists (zero notes are left
out), and the skipped entries grouped by reason with the first 50 named by
line and title. Import and Cancel finish it; closing the window cancels. A
refused file (not an export, UTF-16, invalid UTF-8, too large or too deep)
is reported in the status line with a request to re-export, and no preview
opens.

**Commit.** On Import the parsed tree goes to the session worker as
`SessionCommand::ImportBookmarks`. `atlas_core::commit_bookmark_import`
converts it and calls the new `TabModel::append_saved_folder`, which:

- adds one new top-level folder at the end of the saved rows, named after
  the file (`import_folder_title`: the file name without extension, with
  control and bidirectional characters removed, or "Imported bookmarks");
- keeps the export's hierarchy and order beneath it; toolbar and unfiled
  folders become ordinary folders (owner decision, 2026-10-12), and an
  unnamed exported folder becomes "Untitled folder" because empty folder
  names are refused;
- writes every page as an unloaded saved row, opening nothing; an untitled
  bookmark shows its address;
- re-checks each address with `web_address` and returns refusals as
  `RejectedBookmark`s, which are never written;
- checks that enough folder and page identities remain before changing
  anything, and appends in one pass (the per-row `add_saved_page` would be
  quadratic at 50,000 entries).

The worker then saves the whole snapshot once. The store writes a snapshot
in one SQLite transaction, so an import that fails part-way, or a crash
before commit, leaves the previous state intact; the owner can simply run it
again. The status line reports the counts and any refusals. Export dates
(`ADD_DATE`) are not kept, because saved rows have no date field yet.

**Tests.** All synthetic, all databases disposable:

- `crates/atlas-core/src/web_address.rs`: normalization and refusals.
- `crates/atlas-core/tests/bookmark_commit.rs`: the Chrome fixture becomes
  one folder in export order with nothing opened and a valid, restorable
  snapshot; the import appends after existing rows and leaves live tabs and
  selection alone; refused addresses are reported and not written; unnamed
  folders and untitled pages; identity exhaustion leaves the model
  unchanged; folder naming; and a 40,000-page, 20-deep import.
- `crates/atlas-store/src/engine/tests/import.rs`: an import is exactly one
  revision and survives reopening; a failure injected part-way through the
  page rows, or on the last layout row, leaves the previous revision and
  rows unchanged, and the same import then commits.
- `crates/atlas-app/src/ui/import.rs`: command recognition, skip grouping,
  summary lines, the bounded read and refusal messages.

**Gate record (import step).** In this worktree's own target directory,
Windows Application Control blocked the freshly built `cookie 0.18.2` build
script (os error 4551) twice, once on an unchanged retry. No setting was
changed. Building into a separate directory,
`target/agent-p1-22-commit` in the main checkout (via `CARGO_TARGET_DIR`),
was not blocked, and `cargo xtask check` passed there: dependency check,
formatting, Clippy, 542 tests and the smoke run
(`target/agent-p1-22-commit/p1-22-commit-gate.log`). The first gate run
stopped only on formatting, fixed with `cargo fmt`.

## Known limits

- The preview sidebar still lists saved pages flat, without folders or
  virtualization, until P1-21's folder UI lands; a very large import makes
  a long list.
- Imported pages appear in search through the existing catalogue sync; no
  folder path is searchable yet (see `docs/p1-21-folders-core.md`).
- Importing the same file twice makes a second folder; duplicates against
  existing saved pages are not detected.

## Owner's manual check

The owner, not an agent, exports their own bookmarks and runs the import:
export from Chrome, Edge or Firefox as HTML; in Atlas press Ctrl K, type
`>import bookmarks` and press Enter (or Alt+Enter); choose the file; check
that the preview's counts and skipped entries look right; press Import;
check the status line and the new saved rows; restart Atlas and check that
they are still there and unloaded. Also try Cancel, and a non-export file.
The default limits stay as decided; if the owner's file is refused for size
or depth, that is the moment to revisit them.
