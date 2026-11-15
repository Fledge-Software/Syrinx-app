//! Runs the bake-off's workload against the production index in `syrinx-search` (#53), so
//! its latency and start-up cost compare directly with #52's measurements.

use crate::{
    Result,
    corpus::{CUTOFF, Document, NOW},
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
use syrinx_search::fulltext::{
    Change, Deletion, FullText, FullTextDocument, Namespace, NamespaceFilter, Outcome,
    SearchRequest,
};
use tantivy::{
    Index, TantivyDocument,
    schema::{Field, Value},
};

/// Pages are staged on the corpus's first day, when none of them has expired yet.
const CAPTURE_TIME: i64 = 0;
/// The adapter's own cap on hits per query, so `hits` saturates there.
const MAX_HITS: usize = 1000;
/// The bake-off ranks and snippets the top 20, like the other engines.
const TOP: usize = 20;
/// The adapter's stored kind for a document; 1 is a tombstone (`fulltext/schema.rs`).
const DOCUMENT_ROW: u64 = 0;

pub struct AdapterStore {
    index: FullText,
    /// The revision each id was last staged at by this process. Reopening starts afresh, so
    /// it is valid only within one process, which is all the bake-off's run needs.
    revisions: HashMap<usize, u64>,
    /// Read again by `verify`, which scans the stored rows without the adapter.
    directory: PathBuf,
}

/// One stored row, read straight from the folder's segments.
struct StoredRow {
    kind: u64,
    id: String,
    /// The capture time of a document, or the deletion time of a tombstone.
    dated: i64,
}

impl AdapterStore {
    pub fn open(directory: &Path) -> Result<Self> {
        Ok(Self {
            index: FullText::open(directory)?,
            revisions: HashMap::new(),
            directory: directory.to_owned(),
        })
    }

    /// Stages deletions and upserts, then commits; an id in both is a newer revision.
    pub fn batch(&mut self, documents: &[Document], removed: &[usize]) -> Result<()> {
        let outcomes = self.stage(documents, removed)?;
        if outcomes.iter().any(|outcome| *outcome != Outcome::Staged) {
            return Err("the adapter refused a fresh change".into());
        }
        self.index.commit()?;
        Ok(())
    }

    /// Stages without committing; the crash writer is killed before any commit.
    pub fn stage(&mut self, documents: &[Document], removed: &[usize]) -> Result<Vec<Outcome>> {
        let replaced: HashSet<usize> = documents.iter().map(|document| document.id).collect();
        let mut changes: Vec<Change> = removed
            .iter()
            .filter(|id| !replaced.contains(id))
            .map(|id| Change::Delete(self.deletion(*id)))
            .collect();
        for document in documents {
            let revision = self.revisions.entry(document.id).or_insert(0);
            *revision += 1;
            changes.push(Change::Upsert(page(document, *revision)));
        }
        Ok(self.index.stage(&changes, CAPTURE_TIME)?)
    }

    fn deletion(&self, id: usize) -> Deletion {
        Deletion {
            namespace: Namespace::page(),
            id: id.to_string(),
            revision: self.revisions.get(&id).copied().unwrap_or(1),
            // Deleted at the run's present, so the tombstone outlives the expiry pass.
            deleted_at: NOW,
        }
    }

    /// The pass the session worker runs at start-up and daily.
    pub fn expire(&mut self) -> Result<usize> {
        Ok(self.index.expire(NOW)?)
    }

    /// Returns the top-20 ids in rank order; every hit must highlight its snippet.
    pub fn query(&self, query: &str) -> Result<Vec<u64>> {
        let request = request(query, TOP).ok_or("query syntax the adapter does not offer")?;
        let mut ids = Vec::with_capacity(TOP);
        for hit in self.index.search(&request)? {
            if hit.highlights.is_empty() {
                return Err("body query returned an unhighlighted snippet".into());
            }
            std::hint::black_box(&hit.snippet);
            ids.push(hit.id.parse()?);
        }
        Ok(ids)
    }

    /// Counts matches up to the adapter's cap of 1,000.
    pub fn hits(&self, query: &str) -> Result<usize> {
        let request = request(query, MAX_HITS).ok_or("query syntax the adapter does not offer")?;
        Ok(self.index.search(&request)?.len())
    }

    /// Scans every stored row in the folder, independently of the adapter's own reads.
    pub fn verify(&self, expected: usize) -> Result<()> {
        let rows = stored_rows(&self.directory)?;
        let deleted: HashSet<&str> = rows
            .iter()
            .filter(|row| row.kind != DOCUMENT_ROW)
            .map(|row| row.id.as_str())
            .collect();
        let mut pages = HashSet::new();
        for row in rows.iter().filter(|row| row.kind == DOCUMENT_ROW) {
            let retained = row.dated > CUTOFF && !deleted.contains(row.id.as_str());
            if !retained || !pages.insert(row.id.as_str()) {
                return Err("expired, deleted or duplicate stored text returned".into());
            }
        }
        if pages.len() != expected {
            return Err(format!("expected {expected} documents, found {}", pages.len()).into());
        }
        for (query, expected_hits) in [
            ("sigil1005", 0),
            ("sigil3", 0),
            ("sigil5", 1),
            ("interruptedphantom", 0),
        ] {
            if expected != 0 && self.hits(query)? != expected_hits {
                return Err(format!("unexpected search result for {query}").into());
            }
        }
        Ok(())
    }

    /// Replays original revisions at the run's present; each must be refused or a no-op.
    pub fn replay(&mut self, documents: &[Document]) -> Result<()> {
        let changes: Vec<_> = documents
            .iter()
            .map(|document| Change::Upsert(page(document, 1)))
            .collect();
        let outcomes = self.index.stage(&changes, NOW)?;
        if outcomes.contains(&Outcome::Staged) {
            return Err("stale replay accepted".into());
        }
        self.index.commit()?;
        Ok(())
    }
}

/// Translates a bake-off query; phrases and `OR` have no adapter equivalent.
pub fn request(query: &str, limit: usize) -> Option<SearchRequest> {
    if query.contains('"') || query.contains(" OR ") {
        return None;
    }
    let words = query.replace(" AND ", " ");
    let (text, prefix_last) = match words.strip_suffix('*') {
        Some(prefix) => (prefix.to_owned(), true),
        None => (words, false),
    };
    Some(SearchRequest {
        text,
        prefix_last,
        namespaces: NamespaceFilter::Only(vec![Namespace::page()]),
        limit,
    })
}

fn page(document: &Document, revision: u64) -> FullTextDocument {
    FullTextDocument {
        namespace: Namespace::page(),
        id: document.id.to_string(),
        revision,
        captured_at: document.captured,
        title: document.title.clone(),
        body: document.body.clone(),
    }
}

/// Every stored row in the folder, after checking no segment keeps deleted rows.
fn stored_rows(directory: &Path) -> Result<Vec<StoredRow>> {
    let index = Index::open_in_dir(directory)?;
    if !index.validate_checksum()?.is_empty() {
        return Err("Tantivy checksum mismatch".into());
    }
    let schema = index.schema();
    let fields = [
        schema.get_field("kind")?,
        schema.get_field("id")?,
        schema.get_field("captured")?,
    ];
    let searcher = index.reader()?.searcher();
    let mut rows = Vec::new();
    for segment in searcher.segment_readers() {
        if segment.max_doc() != segment.num_docs() {
            return Err("a segment still stores deleted rows".into());
        }
        let store = segment.get_store_reader(0)?;
        for address in 0..segment.max_doc() {
            let row: TantivyDocument = store.get(address)?;
            rows.push(stored_row(&row, fields)?);
        }
    }
    refuse_retired_segment_files(directory, &searcher)?;
    Ok(rows)
}

/// A merged-away segment's files still hold its text, so none may remain in the folder.
fn refuse_retired_segment_files(directory: &Path, searcher: &tantivy::Searcher) -> Result<()> {
    let live: HashSet<String> = searcher
        .segment_readers()
        .iter()
        .map(|segment| segment.segment_id().uuid_string())
        .collect();
    for entry in std::fs::read_dir(directory)? {
        let name = entry?.file_name();
        let name = name.to_string_lossy();
        let Some((stem, _)) = name.split_once('.') else {
            continue;
        };
        let is_segment = stem.len() == 32 && stem.bytes().all(|byte| byte.is_ascii_hexdigit());
        if is_segment && !live.contains(stem) {
            return Err("a retired segment file remains".into());
        }
    }
    Ok(())
}

fn stored_row(row: &TantivyDocument, [kind, id, dated]: [Field; 3]) -> Result<StoredRow> {
    Ok(StoredRow {
        kind: row
            .get_first(kind)
            .and_then(|value| value.as_u64())
            .ok_or("missing kind")?,
        id: row
            .get_first(id)
            .and_then(|value| value.as_str())
            .ok_or("missing id")?
            .to_owned(),
        dated: row
            .get_first(dated)
            .and_then(|value| value.as_i64())
            .ok_or("missing captured")?,
    })
}
