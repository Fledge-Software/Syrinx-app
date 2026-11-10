//! The values that cross the full-text adapter's boundary; none prints page text.

use std::fmt;

/// D40: page text expires seven days after it was captured.
pub const PAGE_RETENTION_SECONDS: i64 = 7 * 24 * 60 * 60;

/// The longest feed name and document id the adapter accepts, in bytes.
const MAX_FEED_NAME_BYTES: usize = 64;
const MAX_ID_BYTES: usize = 2048;

/// The source a document came from; each source keeps its own deletion rules.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Namespace(Source);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum Source {
    Page,
    Feed(String),
}

/// How long a namespace keeps its documents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Retention {
    /// Documents and their tombstones leave once this many seconds old (page text, D40).
    ExpiresAfter(i64),
    /// Documents stay until deleted or the namespace is reset (a plugin feed such as Notes).
    Kept,
}

impl Namespace {
    /// Captured page text, which expires under D40 and follows history clears (D47).
    pub fn page() -> Self {
        Self(Source::Page)
    }

    /// A plugin's search feed: lowercase ASCII letters, digits, `-`, `_` or `.`.
    pub fn feed(name: &str) -> Result<Self, FullTextError> {
        let valid = !name.is_empty()
            && name.len() <= MAX_FEED_NAME_BYTES
            && name.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'-' | b'_' | b'.')
            });
        if !valid {
            return Err(FullTextError::InvalidNamespace);
        }
        Ok(Self(Source::Feed(name.to_owned())))
    }

    pub fn retention(&self) -> Retention {
        match self.0 {
            Source::Page => Retention::ExpiresAfter(PAGE_RETENTION_SECONDS),
            Source::Feed(_) => Retention::Kept,
        }
    }

    /// The stored spelling; `:` cannot occur in a feed name, so spellings never collide.
    pub(super) fn stored(&self) -> String {
        match &self.0 {
            Source::Page => "page".to_owned(),
            Source::Feed(name) => format!("feed:{name}"),
        }
    }

    pub(super) fn from_stored(stored: &str) -> Result<Self, FullTextError> {
        if stored == "page" {
            return Ok(Self::page());
        }
        let name = stored.strip_prefix("feed:").ok_or(FullTextError::Damaged)?;
        Self::feed(name).map_err(|_| FullTextError::Damaged)
    }
}

/// One searchable document. `revision` must grow with every new capture of the same id.
#[derive(Clone, Eq, PartialEq)]
pub struct FullTextDocument {
    pub namespace: Namespace,
    /// Unique within its namespace; an opaque key chosen by the source.
    pub id: String,
    pub revision: u64,
    /// Unix seconds; page text expires by this time.
    pub captured_at: i64,
    pub title: String,
    pub body: String,
}

impl fmt::Debug for FullTextDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Ids, titles and bodies can be the owner's browsing; they never reach a log.
        formatter
            .debug_struct("FullTextDocument")
            .field("namespace", &self.namespace)
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

/// One change to stage. Deleting leaves a tombstone so an older revision cannot return.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Change {
    Upsert(FullTextDocument),
    Delete(Deletion),
}

/// Removes `id` up to and including `revision`; `deleted_at` dates its tombstone's expiry.
#[derive(Clone, Eq, PartialEq)]
pub struct Deletion {
    pub namespace: Namespace,
    pub id: String,
    pub revision: u64,
    pub deleted_at: i64,
}

impl fmt::Debug for Deletion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Deletion")
            .field("namespace", &self.namespace)
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

impl Change {
    pub(super) fn namespace(&self) -> &Namespace {
        match self {
            Self::Upsert(document) => &document.namespace,
            Self::Delete(deletion) => &deletion.namespace,
        }
    }

    pub(super) fn id(&self) -> &str {
        match self {
            Self::Upsert(document) => &document.id,
            Self::Delete(deletion) => &deletion.id,
        }
    }

    pub(super) fn revision(&self) -> u64 {
        match self {
            Self::Upsert(document) => document.revision,
            Self::Delete(deletion) => deletion.revision,
        }
    }

    /// The time that decides whether page retention has already passed.
    pub(super) fn dated(&self) -> i64 {
        match self {
            Self::Upsert(document) => document.captured_at,
            Self::Delete(deletion) => deletion.deleted_at,
        }
    }

    pub(super) fn validate(&self) -> Result<(), FullTextError> {
        let id = self.id();
        if id.is_empty() || id.len() > MAX_ID_BYTES {
            return Err(FullTextError::InvalidDocument);
        }
        Ok(())
    }
}

/// What staging did with one change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// Written to the pending batch; durable and visible only after `commit`.
    Staged,
    /// The index already holds exactly this revision, so replaying it changes nothing.
    Unchanged,
    /// A newer revision or a newer deletion is already held.
    Stale,
    /// Page text already past D40's seven days is refused rather than stored.
    Expired,
}

/// One record of D47's journal, which numbers every clear whatever it removed.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ClearRequest {
    pub generation: u64,
    pub scope: ClearScope,
}

/// What one journal clear means for this index.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum ClearScope {
    /// A history clear: page text and tombstones dated in `[start, end)` Unix seconds go.
    Pages { start: i64, end: i64 },
    /// A clear of data the index does not hold, such as site zoom levels (#121). It only
    /// advances the generation, so the journal's numbering stays without gaps.
    Elsewhere,
}

impl fmt::Debug for ClearScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Matches the journal: a cleared range is never printed.
        formatter.write_str(match self {
            Self::Pages { .. } => "Pages([redacted])",
            Self::Elsewhere => "Elsewhere",
        })
    }
}

impl fmt::Debug for ClearRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Matches the journal: a cleared range is never printed.
        formatter
            .debug_struct("ClearRequest")
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

/// Which namespaces a query reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NamespaceFilter {
    All,
    Only(Vec<Namespace>),
}

/// A ranked full-text query over titles and bodies.
#[derive(Clone, Eq, PartialEq)]
pub struct SearchRequest {
    pub text: String,
    /// Match the last word as a prefix, for search as the owner types.
    pub prefix_last: bool,
    pub namespaces: NamespaceFilter,
    pub limit: usize,
}

impl fmt::Debug for SearchRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SearchRequest")
            .field("prefix_last", &self.prefix_last)
            .field("namespaces", &self.namespaces)
            .field("limit", &self.limit)
            .finish_non_exhaustive()
    }
}

/// One ranked result with a bounded snippet of its body.
#[derive(Clone, PartialEq)]
pub struct FullTextHit {
    pub namespace: Namespace,
    pub id: String,
    pub revision: u64,
    pub score: f32,
    pub title: String,
    pub snippet: String,
    /// Byte ranges of `snippet` that matched the query.
    pub highlights: Vec<std::ops::Range<usize>>,
}

impl fmt::Debug for FullTextHit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FullTextHit")
            .field("namespace", &self.namespace)
            .field("revision", &self.revision)
            .field("score", &self.score)
            .finish_non_exhaustive()
    }
}

/// Structural failures; none carries a path, an id or document text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FullTextError {
    InvalidNamespace,
    InvalidDocument,
    /// A maintenance operation was asked for while staged changes await `commit`.
    StagedWorkPending,
    /// Journal clears must arrive in order, without a missing generation.
    ClearOutOfOrder,
    /// The folder holds an index with a different layout; it is a cache and can be rebuilt.
    IncompatibleIndex,
    Damaged,
    /// Another writer holds the index.
    Busy,
    /// The commit is durable, but deleted text is still on disk; the next commit retries.
    CleanupPending,
    /// A failed rollback left this handle without its writer; open the index again.
    NeedsReopen,
    Io,
    Engine,
}

impl fmt::Display for FullTextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidNamespace => "That search source name is not valid",
            Self::InvalidDocument => "That document cannot be indexed",
            Self::StagedWorkPending => "Search index changes are still waiting to be saved",
            Self::ClearOutOfOrder => "Syrinx's record of cleared history is out of order",
            Self::IncompatibleIndex => "The search index was made by another version of Syrinx",
            Self::Damaged => "The search index is damaged",
            Self::Busy => "Another Syrinx window is using the search index",
            Self::CleanupPending => {
                "Syrinx could not yet remove deleted text from its search index"
            }
            Self::NeedsReopen => "The search index must be reopened; restart Syrinx",
            Self::Io => "Syrinx could not read or write its search index",
            Self::Engine => "The search index stopped unexpectedly",
        })
    }
}

impl std::error::Error for FullTextError {}

impl From<tantivy::TantivyError> for FullTextError {
    fn from(error: tantivy::TantivyError) -> Self {
        use tantivy::{TantivyError as Tantivy, directory::error::OpenReadError};
        // Tantivy's messages can hold paths, so only the category crosses this boundary.
        match error {
            Tantivy::LockFailure(..) => Self::Busy,
            Tantivy::DataCorruption(_) | Tantivy::DeserializeError(_) => Self::Damaged,
            // A file the metadata names is gone, as a failed empty can leave it (#195): only
            // emptying again recovers. Any other I/O fault may pass, so it never wipes the index.
            Tantivy::OpenReadError(OpenReadError::FileDoesNotExist(_)) => Self::Damaged,
            Tantivy::SchemaError(_)
            | Tantivy::FieldNotFound(_)
            | Tantivy::IncompatibleIndex(_)
            | Tantivy::IndexAlreadyExists => Self::IncompatibleIndex,
            Tantivy::IoError(_)
            | Tantivy::OpenDirectoryError(_)
            | Tantivy::OpenReadError(_)
            | Tantivy::OpenWriteError(_) => Self::Io,
            _ => Self::Engine,
        }
    }
}

impl From<std::io::Error> for FullTextError {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}
