//! Reads an exported HTML bookmarks file (D14) into a folder tree for preview before import.

mod address;
mod builder;
mod text;
mod tokenizer;

use std::fmt;

use builder::Builder;
use tokenizer::{Token, Tokenizer};

/// The declaration Chrome, Edge and Firefox put first in every bookmarks export.
const DOCTYPE: [&str; 2] = ["DOCTYPE", "NETSCAPE-Bookmark-file-1"];
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// Bounds that keep a hostile or damaged file from exhausting memory or time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImportLimits {
    /// Largest file accepted, in bytes. Exports with embedded icons are a few MB.
    pub max_input_bytes: usize,
    /// Deepest folder nesting accepted; a top-level folder has depth 1.
    pub max_depth: usize,
    /// Most `<DL>` lists without a folder heading open at once, the export's outer list
    /// included. Exports have exactly one; more means a damaged or hand-made file.
    pub max_bare_lists: usize,
    /// Most bookmarks, folders and skipped entries together.
    pub max_entries: usize,
    /// Titles and folder names longer than this are shortened, in characters.
    pub max_title_chars: usize,
    /// Addresses longer than this are skipped, in bytes; a cut address would be wrong.
    pub max_address_bytes: usize,
}

impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 32 * 1024 * 1024,
            max_depth: 32,
            max_bare_lists: 4,
            max_entries: 50_000,
            max_title_chars: 1_024,
            max_address_bytes: 8 * 1024,
        }
    }
}

/// A parsed export: the tree to import and the summary to show before confirming.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BookmarkImport {
    /// Top-level entries in export order.
    pub items: Vec<ImportedItem>,
    pub summary: ImportSummary,
}

/// One entry in export order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportedItem {
    Folder(ImportedFolder),
    Bookmark(ImportedBookmark),
}

/// A folder and its entries in export order.
#[derive(Clone, Eq, PartialEq)]
pub struct ImportedFolder {
    pub title: String,
    /// Seconds since the Unix epoch, when the export records one.
    pub added: Option<u64>,
    pub role: FolderRole,
    pub items: Vec<ImportedItem>,
}

/// Special folders the exporting browser marked, so the UI can place them sensibly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FolderRole {
    Ordinary,
    /// `PERSONAL_TOOLBAR_FOLDER`: Chrome's bookmarks bar, Edge's favorites bar, Firefox's toolbar.
    Toolbar,
    /// `UNFILED_BOOKMARKS_FOLDER`: Firefox's "Other Bookmarks".
    Unfiled,
}

/// A web bookmark that will become an unopened saved page (D43).
#[derive(Clone, Eq, PartialEq)]
pub struct ImportedBookmark {
    pub title: String,
    /// An `http` or `https` address with a lower-case scheme.
    pub address: String,
    /// Seconds since the Unix epoch, when the export records one.
    pub added: Option<u64>,
}

/// Counts and skipped entries for the preview. Nothing in it has been imported yet.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImportSummary {
    pub bookmarks: usize,
    pub folders: usize,
    pub empty_folders: usize,
    /// Deepest folder nesting found; zero when there are no folders.
    pub max_depth: usize,
    /// Bookmarks whose address appeared earlier in the file. They are kept.
    pub duplicate_addresses: usize,
    /// Titles and folder names shortened to `max_title_chars`.
    pub truncated_titles: usize,
    /// Folder lists still open at the end of the file, closed by the parser.
    pub unclosed_lists: usize,
    pub skipped: Vec<SkippedEntry>,
}

/// An entry the import leaves out, with where it was and why.
#[derive(Clone, Eq, PartialEq)]
pub struct SkippedEntry {
    /// One-based line of the entry's tag in the export.
    pub line: usize,
    pub reason: SkipReason,
    /// The entry's display title, so the preview can name it; empty for separators.
    pub title: String,
}

/// Why an entry is left out of the import.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SkipReason {
    MissingAddress,
    AddressTooLong,
    InvalidAddress,
    /// The address carries a user name or password, which Atlas never saves.
    CredentialsInAddress,
    /// A `javascript:` bookmarklet, which Atlas does not run from a saved page.
    ScriptAddress,
    /// A `data:` address, which can hide arbitrary content.
    DataAddress,
    /// Any other scheme (`file`, `place`, `chrome`, `about` ...), lower-cased and bounded.
    UnsupportedScheme(String),
    /// A Firefox `<HR>` separator; saved rows have no separators.
    Separator,
}

/// Why a file cannot be previewed at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportError {
    InputTooLarge {
        limit: usize,
        actual: usize,
    },
    /// The file starts with a UTF-16 byte-order mark; exports are UTF-8.
    UnsupportedEncoding,
    /// The file is not valid UTF-8; `offset` is the first bad byte.
    NotUtf8 {
        offset: usize,
    },
    /// The file lacks the bookmarks-export declaration.
    NotABookmarkFile,
    TooDeep {
        line: usize,
        limit: usize,
    },
    /// Too many `<DL>` lists without a folder heading are open at once.
    TooManyBareLists {
        line: usize,
        limit: usize,
    },
    TooManyEntries {
        line: usize,
        limit: usize,
    },
}

/// Parses an export with the default limits.
pub fn parse_bookmark_html(input: &[u8]) -> Result<BookmarkImport, ImportError> {
    parse_bookmark_html_with_limits(input, ImportLimits::default())
}

/// Parses an export without touching the store; the caller previews, then imports.
pub fn parse_bookmark_html_with_limits(
    input: &[u8],
    limits: ImportLimits,
) -> Result<BookmarkImport, ImportError> {
    let text = decode_input(input, &limits)?;
    let mut tokens = Tokenizer::new(text).peekable();
    require_doctype(&mut tokens)?;

    // Feed every token to the tree builder with the line it starts on.
    let mut builder = Builder::new(limits);
    let mut lines = LineCounter::new(text);
    for (offset, token) in tokens {
        builder.token(lines.line_at(offset), token)?;
    }
    builder.finish()
}

/// Checks the size and encoding; exports are UTF-8, so other encodings are refused, not guessed.
fn decode_input<'a>(input: &'a [u8], limits: &ImportLimits) -> Result<&'a str, ImportError> {
    if input.len() > limits.max_input_bytes {
        return Err(ImportError::InputTooLarge {
            limit: limits.max_input_bytes,
            actual: input.len(),
        });
    }
    if input.starts_with(b"\xFF\xFE") || input.starts_with(b"\xFE\xFF") {
        return Err(ImportError::UnsupportedEncoding);
    }
    let body = input.strip_prefix(UTF8_BOM).unwrap_or(input);
    std::str::from_utf8(body).map_err(|error| ImportError::NotUtf8 {
        offset: error.valid_up_to() + (input.len() - body.len()),
    })
}

/// Requires the export declaration before anything but whitespace and comments.
fn require_doctype(tokens: &mut std::iter::Peekable<Tokenizer<'_>>) -> Result<(), ImportError> {
    while let Some((_, Token::Text(text))) = tokens.peek() {
        if !text.trim().is_empty() {
            return Err(ImportError::NotABookmarkFile);
        }
        tokens.next();
    }
    let Some((_, Token::Doctype(declaration))) = tokens.next() else {
        return Err(ImportError::NotABookmarkFile);
    };
    let words: Vec<&str> = declaration.split_ascii_whitespace().collect();
    let matches = words.len() == DOCTYPE.len()
        && words
            .iter()
            .zip(DOCTYPE)
            .all(|(word, expected)| word.eq_ignore_ascii_case(expected));
    if !matches {
        return Err(ImportError::NotABookmarkFile);
    }
    Ok(())
}

/// Converts increasing byte offsets to one-based line numbers in a single pass.
struct LineCounter<'a> {
    text: &'a str,
    offset: usize,
    line: usize,
}

impl<'a> LineCounter<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            offset: 0,
            line: 1,
        }
    }

    fn line_at(&mut self, offset: usize) -> usize {
        let advanced = &self.text.as_bytes()[self.offset..offset];
        self.line += advanced.iter().filter(|&&byte| byte == b'\n').count();
        self.offset = offset;
        self.line
    }
}

// Titles and addresses are the owner's data, so diagnostics show only their sizes.
impl fmt::Debug for ImportedFolder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImportedFolder")
            .field("role", &self.role)
            .field("items", &self.items)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for ImportedBookmark {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImportedBookmark")
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for SkippedEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SkippedEntry")
            .field("line", &self.line)
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for SkipReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingAddress => formatter.write_str("the bookmark has no address"),
            Self::AddressTooLong => formatter.write_str("the address is too long"),
            Self::InvalidAddress => formatter.write_str("the address is not a valid web address"),
            Self::CredentialsInAddress => {
                formatter.write_str("the address contains a user name or password")
            }
            Self::ScriptAddress => {
                formatter.write_str("bookmarklets (javascript:) are not imported")
            }
            Self::DataAddress => formatter.write_str("data: addresses are not imported"),
            Self::UnsupportedScheme(scheme) => {
                write!(formatter, "{scheme}: addresses are not imported")
            }
            Self::Separator => formatter.write_str("separators are not imported"),
        }
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Describe the file's shape without echoing its contents.
        match self {
            Self::InputTooLarge { limit, actual } => write!(
                formatter,
                "the file is {actual} bytes; the import accepts at most {limit}"
            ),
            Self::UnsupportedEncoding => {
                formatter.write_str("the file is UTF-16; export bookmarks as UTF-8 HTML")
            }
            Self::NotUtf8 { offset } => {
                write!(formatter, "the file is not valid UTF-8 at byte {offset}")
            }
            Self::NotABookmarkFile => {
                formatter.write_str("the file is not an exported HTML bookmarks file")
            }
            Self::TooDeep { line, limit } => write!(
                formatter,
                "folders nest deeper than {limit} levels at line {line}"
            ),
            Self::TooManyBareLists { line, limit } => write!(
                formatter,
                "more than {limit} lists without a folder name are open at line {line}"
            ),
            Self::TooManyEntries { line, limit } => write!(
                formatter,
                "the file has more than {limit} entries (reached at line {line})"
            ),
        }
    }
}

impl std::error::Error for ImportError {}
