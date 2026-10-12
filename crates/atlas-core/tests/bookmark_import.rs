//! Exercises HTML bookmark import on synthetic Chrome-, Edge- and Firefox-shaped exports.

use atlas_core::{
    BookmarkImport, FolderRole, ImportError, ImportLimits, ImportedBookmark, ImportedFolder,
    ImportedItem, SkipReason, parse_bookmark_html, parse_bookmark_html_with_limits,
};

const CHROME: &str = include_str!("fixtures/bookmarks/chrome.html");
const EDGE: &str = include_str!("fixtures/bookmarks/edge.html");
const FIREFOX: &str = include_str!("fixtures/bookmarks/firefox.html");
const MALFORMED: &str = include_str!("fixtures/bookmarks/malformed.html");
const HEADER: &str = "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n<DL><p>\n";

/// Checkouts on this machine have CRLF endings; tests start from LF and add CRLF themselves.
fn lf(fixture: &str) -> String {
    fixture.replace("\r\n", "\n")
}

fn crlf(fixture: &str) -> String {
    lf(fixture).replace('\n', "\r\n")
}

fn parse(text: &str) -> BookmarkImport {
    match parse_bookmark_html(text.as_bytes()) {
        Ok(import) => import,
        Err(error) => panic!("synthetic export failed to parse: {error}"),
    }
}

fn folder(item: &ImportedItem) -> &ImportedFolder {
    match item {
        ImportedItem::Folder(folder) => folder,
        ImportedItem::Bookmark(_) => panic!("expected a folder"),
    }
}

fn bookmark(item: &ImportedItem) -> &ImportedBookmark {
    match item {
        ImportedItem::Bookmark(bookmark) => bookmark,
        ImportedItem::Folder(_) => panic!("expected a bookmark"),
    }
}

/// Lists every entry depth-first as `depth:kind:title` for order comparisons.
fn outline(items: &[ImportedItem]) -> Vec<String> {
    let mut lines = Vec::new();
    append_outline(items, 0, &mut lines);
    lines
}

fn append_outline(items: &[ImportedItem], depth: usize, lines: &mut Vec<String>) {
    for item in items {
        match item {
            ImportedItem::Folder(folder) => {
                lines.push(format!("{depth}:folder:{}", folder.title));
                append_outline(&folder.items, depth + 1, lines);
            }
            ImportedItem::Bookmark(bookmark) => {
                lines.push(format!(
                    "{depth}:link:{}={}",
                    bookmark.title, bookmark.address
                ));
            }
        }
    }
}

fn reasons(import: &BookmarkImport) -> Vec<(usize, SkipReason)> {
    import
        .summary
        .skipped
        .iter()
        .map(|skipped| (skipped.line, skipped.reason.clone()))
        .collect()
}

#[test]
fn chrome_export_keeps_hierarchy_order_and_metadata() {
    let import = parse(&lf(CHROME));
    assert_eq!(
        outline(&import.items),
        [
            "0:folder:Bookmarks bar",
            "1:link:Alpha Synthetic=https://alpha.example.test/",
            "1:link:Beta & Gamma <Test>=https://beta.example.test/search?q=one&page=2",
            "1:folder:Reading 'list'",
            "2:link:Gamma article=https://gamma.example.test/article",
            "2:folder:Deeper",
            "3:link:Delta café=https://delta.example.test/",
            "0:folder:Other bookmarks",
            "1:link:Alpha again=https://alpha.example.test/",
            "1:folder:Empty folder",
        ]
    );
    let bar = folder(&import.items[0]);
    assert_eq!(bar.role, FolderRole::Toolbar);
    assert_eq!(bar.added, Some(1_700_000_000));
    assert_eq!(bookmark(&bar.items[0]).added, Some(1_700_000_001));
    assert_eq!(folder(&import.items[1]).role, FolderRole::Ordinary);

    let summary = &import.summary;
    assert_eq!(summary.bookmarks, 5);
    assert_eq!(summary.folders, 5);
    assert_eq!(summary.empty_folders, 1);
    assert_eq!(summary.max_depth, 3);
    assert_eq!(summary.duplicate_addresses, 1);
    assert_eq!(summary.unclosed_lists, 0);
    assert_eq!(reasons(&import), [(21, SkipReason::ScriptAddress)]);
    assert_eq!(summary.skipped[0].title, "Synthetic bookmarklet");
}

#[test]
fn edge_export_skips_local_and_browser_internal_addresses() {
    let import = parse(&lf(EDGE));
    assert_eq!(
        outline(&import.items),
        [
            "0:folder:Favorites bar",
            "1:link:Made-up dashboard=https://work.example.com/dashboard",
            "1:folder:Projects",
            "2:link:Invented wiki=http://intranet.example.test:8080/wiki?page=Home&lang=en",
            "0:folder:Other favorites",
            "1:link:Fictional news=https://news.example.com/",
        ]
    );
    assert_eq!(folder(&import.items[0]).role, FolderRole::Toolbar);
    assert_eq!(
        reasons(&import),
        [
            (15, SkipReason::UnsupportedScheme("file".to_owned())),
            (21, SkipReason::UnsupportedScheme("edge".to_owned())),
        ]
    );
    assert_eq!(import.summary.max_depth, 2);
}

#[test]
fn firefox_export_handles_separators_descriptions_and_special_folders() {
    let import = parse(&lf(FIREFOX));
    assert_eq!(
        outline(&import.items),
        [
            "0:folder:Mozilla-shaped folder",
            "1:link:Get Help=https://support.example.test/help",
            "1:link:Get Help (duplicate)=https://support.example.test/help",
            "0:folder:Bookmarks Toolbar",
            "1:link:Toolbar entry=https://toolbar.example.test/",
            "0:folder:Other Bookmarks",
            "1:link:Unfiled entry=https://unfiled.example.test/",
        ]
    );
    assert_eq!(folder(&import.items[1]).role, FolderRole::Toolbar);
    assert_eq!(folder(&import.items[2]).role, FolderRole::Unfiled);
    assert_eq!(
        reasons(&import),
        [
            (12, SkipReason::UnsupportedScheme("place".to_owned())),
            (13, SkipReason::Separator),
            (25, SkipReason::Separator),
            (26, SkipReason::DataAddress),
        ]
    );
    assert_eq!(import.summary.duplicate_addresses, 1);
    assert_eq!(import.summary.bookmarks, 4);
}

#[test]
fn malformed_export_recovers_from_missing_and_stray_tags() {
    let import = parse(&lf(MALFORMED));
    assert_eq!(
        outline(&import.items),
        [
            "0:link:Unclosed link=https://unclosed.example.test/",
            "0:link:Single quoted=https://single-quoted.example.test/",
            "0:link:Unquoted=https://unquoted.example.test/path",
            "0:folder:Heading without list",
            "0:folder:Unclosed heading",
            "1:link:Spaced title=https://Upper.Example.Test/Path",
            "1:link:Inside a stray tag bold &copy; text=https://stray.example.test/",
            "0:link:After stray closes=https://after-stray.example.test/",
            "0:folder:Never closed",
            "1:link:Last=https://last.example.test/",
        ]
    );
    assert_eq!(bookmark(&import.items[1]).added, None);
    assert_eq!(
        reasons(&import),
        [
            (7, SkipReason::MissingAddress),
            (8, SkipReason::CredentialsInAddress),
        ]
    );
    let summary = &import.summary;
    assert_eq!(summary.folders, 3);
    assert_eq!(summary.empty_folders, 1);
    assert_eq!(summary.unclosed_lists, 1);
}

#[test]
fn crlf_lf_and_byte_order_mark_produce_the_same_import() {
    for fixture in [CHROME, EDGE, FIREFOX, MALFORMED] {
        let expected = parse(&lf(fixture));
        assert_eq!(parse(&crlf(fixture)), expected);
        let with_bom = [b"\xEF\xBB\xBF".as_slice(), lf(fixture).as_bytes()].concat();
        assert_eq!(parse_bookmark_html(&with_bom), Ok(expected));
    }
}

#[test]
fn files_that_are_not_utf8_bookmark_exports_are_refused() {
    assert_eq!(
        parse_bookmark_html(b"<html><body>page</body></html>"),
        Err(ImportError::NotABookmarkFile)
    );
    assert_eq!(parse_bookmark_html(b""), Err(ImportError::NotABookmarkFile));
    assert_eq!(
        parse_bookmark_html(b"text first <!DOCTYPE NETSCAPE-Bookmark-file-1>"),
        Err(ImportError::NotABookmarkFile)
    );
    assert_eq!(
        parse_bookmark_html(b"<!DOCTYPE html>"),
        Err(ImportError::NotABookmarkFile)
    );
    assert_eq!(
        parse_bookmark_html(b"\xFF\xFE<\0!\0"),
        Err(ImportError::UnsupportedEncoding)
    );
    // Windows-1252 bytes (here an e-acute) are refused rather than guessed.
    let latin1 = [
        HEADER.as_bytes(),
        b"<DT><A HREF=\"https://x.test/\">Caf\xE9</A>",
    ]
    .concat();
    assert_eq!(
        parse_bookmark_html(&latin1),
        Err(ImportError::NotUtf8 {
            offset: HEADER.len() + 33
        })
    );
    let comment_first = format!("\n<!-- synthetic -->\n{HEADER}</DL>");
    assert!(parse_bookmark_html(comment_first.as_bytes()).is_ok());
}

#[test]
fn input_size_is_bounded_before_parsing() {
    let limits = ImportLimits {
        max_input_bytes: 64,
        ..ImportLimits::default()
    };
    let input = format!("{HEADER}{}", " ".repeat(64));
    assert_eq!(
        parse_bookmark_html_with_limits(input.as_bytes(), limits),
        Err(ImportError::InputTooLarge {
            limit: 64,
            actual: input.len()
        })
    );
}

fn nested_folders(depth: usize) -> String {
    let mut text = HEADER.to_owned();
    for level in 0..depth {
        text.push_str(&format!("<DT><H3>Level {level}</H3>\n<DL><p>\n"));
    }
    text.push_str("<DT><A HREF=\"https://deep.example.test/\">Deep</A>\n");
    text
}

#[test]
fn folder_depth_is_bounded_with_the_failing_line() {
    let limits = ImportLimits {
        max_depth: 4,
        ..ImportLimits::default()
    };
    let at_limit = parse_bookmark_html_with_limits(nested_folders(4).as_bytes(), limits);
    let Ok(import) = at_limit else {
        panic!("four levels should be accepted: {at_limit:?}");
    };
    assert_eq!(import.summary.max_depth, 4);
    assert_eq!(import.summary.unclosed_lists, 5);
    assert_eq!(
        parse_bookmark_html_with_limits(nested_folders(5).as_bytes(), limits),
        Err(ImportError::TooDeep { line: 11, limit: 4 })
    );
    // An empty heading with no list still counts as a level.
    let empty_heading = format!("{}<DT><H3>Too deep</H3>", nested_folders(4));
    assert_eq!(
        parse_bookmark_html_with_limits(empty_heading.as_bytes(), limits),
        Err(ImportError::TooDeep { line: 12, limit: 4 })
    );
}

#[test]
fn bare_lists_have_their_own_bound_and_do_not_reduce_folder_depth() {
    let limits = ImportLimits {
        max_depth: 2,
        max_bare_lists: 3,
        ..ImportLimits::default()
    };
    // The outer list plus two bare lists is three, and two folders still fit inside them.
    let within = format!(
        "{HEADER}<DL>\n<DL>\n<DT><H3>One</H3>\n<DL>\n<DT><H3>Two</H3>\n<DL>\n\
         <DT><A HREF=\"https://inner.example.test/\">Inner</A>\n"
    );
    let Ok(import) = parse_bookmark_html_with_limits(within.as_bytes(), limits) else {
        panic!("bare lists must not count as folder depth");
    };
    assert_eq!(import.summary.max_depth, 2);
    assert_eq!(
        outline(&import.items),
        [
            "0:folder:One",
            "1:folder:Two",
            "2:link:Inner=https://inner.example.test/",
        ]
    );
    // A fourth bare list is refused with its own error, not as folder depth.
    let too_many = format!("{HEADER}<DL>\n<DL>\n<DL>\n");
    assert_eq!(
        parse_bookmark_html_with_limits(too_many.as_bytes(), limits),
        Err(ImportError::TooManyBareLists { line: 5, limit: 3 })
    );
    // Bare lists between folders stay bounded however the two are interleaved.
    let interleaved = format!("{HEADER}<DT><H3>F</H3>\n<DL>\n<DL>\n<DL>\n<DL>\n");
    assert_eq!(
        parse_bookmark_html_with_limits(interleaved.as_bytes(), limits),
        Err(ImportError::TooManyBareLists { line: 7, limit: 3 })
    );
}

#[test]
fn titles_drop_hidden_characters_from_exports() {
    let input = format!(
        "{HEADER}<DT><H3>Folder&#x202E;txt</H3>\n<DL>\n\
         <DT><A HREF=\"https://x.example.test/\">Pay\u{202E}fdp.exe\u{0}</A>\n"
    );
    let import = parse(&input);
    assert_eq!(
        outline(&import.items),
        [
            "0:folder:Foldertxt",
            "1:link:Payfdp.exe=https://x.example.test/"
        ]
    );
}

#[test]
fn entry_count_is_bounded_including_skipped_entries() {
    let limits = ImportLimits {
        max_entries: 3,
        ..ImportLimits::default()
    };
    let three = format!("{HEADER}<HR>\n<DT><H3>F</H3>\n<DT><A HREF=\"https://a.test/\">A</A>\n");
    assert!(parse_bookmark_html_with_limits(three.as_bytes(), limits).is_ok());
    let four = format!("{three}<DT><A HREF=\"javascript:1\">J</A>\n");
    assert_eq!(
        parse_bookmark_html_with_limits(four.as_bytes(), limits),
        Err(ImportError::TooManyEntries { line: 6, limit: 3 })
    );
    // A final entry left open at the end of the file is counted too.
    let unclosed = format!("{three}<DT><A HREF=\"https://b.test/\">B");
    assert_eq!(
        parse_bookmark_html_with_limits(unclosed.as_bytes(), limits),
        Err(ImportError::TooManyEntries { line: 6, limit: 3 })
    );
}

#[test]
fn long_titles_are_shortened_and_long_addresses_skipped() {
    let limits = ImportLimits {
        max_title_chars: 5,
        max_address_bytes: 32,
        ..ImportLimits::default()
    };
    let input = format!(
        "{HEADER}<DT><H3>Folder name</H3>\n<DL>\n<DT><A HREF=\"https://a.test/\">ééééééé</A>\n\
         <DT><A HREF=\"https://a.test/{}\">Long</A>\n",
        "p".repeat(32)
    );
    let Ok(import) = parse_bookmark_html_with_limits(input.as_bytes(), limits) else {
        panic!("bounded input should parse");
    };
    assert_eq!(
        outline(&import.items),
        ["0:folder:Folde", "1:link:ééééé=https://a.test/"]
    );
    assert_eq!(import.summary.truncated_titles, 2);
    assert_eq!(reasons(&import), [(6, SkipReason::AddressTooLong)]);
    assert_eq!(import.summary.skipped[0].title, "Long");
}

#[test]
fn diagnostics_never_show_titles_or_addresses() {
    let import = parse(&lf(CHROME));
    let debug = format!("{import:?}");
    for private in [
        "Alpha",
        "alpha.example.test",
        "Bookmarks bar",
        "bookmarklet",
    ] {
        assert!(!debug.contains(private), "debug output leaked {private}");
    }
}

/// A small deterministic generator, so failures reproduce from the printed seed.
struct Generator(u64);

impl Generator {
    fn next(&mut self) -> u64 {
        // xorshift64: adequate spread for picking fragments; not for anything secret.
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound as u64).unwrap_or(0)
    }
}

const FRAGMENTS: &[&str] = &[
    "<DT>",
    "<DL><p>",
    "</DL><p>",
    "<H3>",
    "</H3>",
    "<A HREF=\"https://f.example.test/",
    "\">",
    "</A>",
    "<HR>",
    "<DD>",
    "<A>",
    "javascript:x",
    "&amp;",
    "&#x",
    ";",
    "&",
    "<",
    ">",
    "\"",
    "'",
    "\r\n",
    "\n",
    " ",
    "é",
    "🙂",
    "<!--",
    "-->",
    "<!",
    "</",
    "=",
    "ADD_DATE=\"1\"",
    " PERSONAL_TOOLBAR_FOLDER=\"true\"",
    "<A HREF='http://g.example.test/'>",
    "title text",
    "\0",
    "<H1>",
    "<B>",
];

fn generated_export(generator: &mut Generator) -> Vec<u8> {
    let mut text = HEADER.to_owned();
    for _ in 0..generator.below(200) {
        text.push_str(FRAGMENTS[generator.below(FRAGMENTS.len())]);
    }
    text.into_bytes()
}

fn mutated_fixture(generator: &mut Generator) -> Vec<u8> {
    let fixtures = [CHROME, EDGE, FIREFOX, MALFORMED];
    let mut bytes = lf(fixtures[generator.below(fixtures.len())]).into_bytes();
    for _ in 0..=generator.below(8) {
        let index = generator.below(bytes.len());
        match generator.below(3) {
            0 => bytes[index] = generator.next().to_le_bytes()[0],
            1 => bytes.insert(index, generator.next().to_le_bytes()[0]),
            _ => {
                bytes.remove(index);
            }
        }
    }
    bytes
}

/// Checks that the summary agrees with the tree and every bound held.
fn check_invariants(import: &BookmarkImport, limits: &ImportLimits) {
    let (bookmarks, folders, depth) = measure(&import.items, 1, limits);
    assert_eq!(bookmarks, import.summary.bookmarks);
    assert_eq!(folders, import.summary.folders);
    assert_eq!(depth, import.summary.max_depth);
    assert!(depth <= limits.max_depth);
    assert!(bookmarks + folders + import.summary.skipped.len() <= limits.max_entries);
}

fn measure(items: &[ImportedItem], depth: usize, limits: &ImportLimits) -> (usize, usize, usize) {
    let mut totals = (0, 0, 0);
    for item in items {
        match item {
            ImportedItem::Bookmark(bookmark) => {
                assert!(
                    bookmark.address.starts_with("https://")
                        || bookmark.address.starts_with("http://")
                );
                assert!(bookmark.title.chars().count() <= limits.max_title_chars);
                totals.0 += 1;
            }
            ImportedItem::Folder(folder) => {
                let (bookmarks, folders, deepest) = measure(&folder.items, depth + 1, limits);
                totals.0 += bookmarks;
                totals.1 += folders + 1;
                totals.2 = totals.2.max(deepest.max(depth));
            }
        }
    }
    totals
}

#[test]
fn generated_and_mutated_inputs_never_panic_and_keep_invariants() {
    let limits = ImportLimits {
        max_depth: 6,
        max_entries: 60,
        max_title_chars: 12,
        max_address_bytes: 48,
        ..ImportLimits::default()
    };
    for seed in 1..=3_000_u64 {
        let mut generator = Generator(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let input = if seed % 2 == 0 {
            generated_export(&mut generator)
        } else {
            mutated_fixture(&mut generator)
        };
        if let Ok(import) = parse_bookmark_html_with_limits(&input, limits) {
            check_invariants(&import, &limits);
        }
    }
}
