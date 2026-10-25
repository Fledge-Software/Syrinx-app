//! Builds disposable index folders and synthetic documents, and reads back what is stored.

use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use tantivy::TantivyDocument;

use super::super::{
    Change, Deletion, FullText, FullTextDocument, Namespace, NamespaceFilter, SearchRequest,
    schema::RowKind,
};

/// A fixed synthetic clock: 2030-03-17, far from any real capture.
pub const NOW: i64 = 1_900_000_000;
pub const DAY: i64 = 24 * 60 * 60;

static NEXT_FOLDER: AtomicU64 = AtomicU64::new(0);

/// A folder this test created exclusively; it is removed when the test ends.
pub struct Fixture {
    pub path: PathBuf,
}

/// One stored row, as the audit reads it from every live segment.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct StoredRow {
    pub key: String,
    pub tombstone: bool,
    pub revision: u64,
    pub body: String,
}

impl Fixture {
    pub fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("synthetic clock follows epoch")
            .as_nanos();
        let sequence = NEXT_FOLDER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "syrinx-fulltext-test-{}-{stamp}-{sequence}",
            std::process::id()
        ));
        // Exclusive creation proves this test owns everything it later removes.
        fs::create_dir(&path).expect("create synthetic index folder");
        Self { path }
    }

    pub fn open(&self) -> FullText {
        FullText::open(&self.path).expect("open synthetic index")
    }

    /// The ids of committed documents matching `words`, in rank order.
    pub fn ids(&self, index: &FullText, words: &str) -> Vec<String> {
        self.ids_in(index, words, NamespaceFilter::All)
    }

    pub fn ids_in(
        &self,
        index: &FullText,
        words: &str,
        namespaces: NamespaceFilter,
    ) -> Vec<String> {
        let request = SearchRequest {
            text: words.to_owned(),
            prefix_last: false,
            namespaces,
            limit: 100,
        };
        index
            .search(&request)
            .expect("search synthetic index")
            .into_iter()
            .map(|hit| hit.id)
            .collect()
    }

    /// Reads every stored row and checks no segment still carries a deleted one.
    ///
    /// Tantivy keeps deleted rows' stored text until their segment is rewritten, so this
    /// is the check that deleted and expired text left active storage (trapstoknow.md).
    pub fn audit(&self, index: &FullText) -> Vec<StoredRow> {
        let searcher = index.searcher().expect("open synthetic searcher");
        let fields = index.fields;
        let mut rows = Vec::new();
        for segment in searcher.segment_readers() {
            assert_eq!(
                segment.max_doc(),
                segment.num_docs(),
                "a segment still stores deleted rows"
            );
            let store = segment.get_store_reader(0).expect("open stored rows");
            for address in 0..segment.max_doc() {
                let row: TantivyDocument = store.get(address).expect("read stored row");
                rows.push(stored_row(&fields, &row));
            }
        }
        self.assert_no_retired_segment_files(&searcher);
        rows.sort();
        rows
    }

    /// A merged-away segment's files still hold its text, so none may remain in the folder.
    fn assert_no_retired_segment_files(&self, searcher: &tantivy::Searcher) {
        let live: BTreeSet<String> = searcher
            .segment_readers()
            .iter()
            .map(|segment| segment.segment_id().uuid_string())
            .collect();
        for entry in fs::read_dir(&self.path).expect("list the synthetic index") {
            let name = entry.expect("read an index entry").file_name();
            let name = name.to_string_lossy();
            let Some((stem, _)) = name.split_once('.') else {
                continue;
            };
            let is_segment = stem.len() == 32 && stem.bytes().all(|byte| byte.is_ascii_hexdigit());
            assert!(
                !is_segment || live.contains(stem),
                "a retired segment file remains"
            );
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Only the folder created in `new` is removed; a failure leaves it for inspection.
        if let Err(error) = fs::remove_dir_all(&self.path) {
            eprintln!("synthetic index folder kept: {}", error.kind());
        }
    }
}

fn stored_row(fields: &super::super::Fields, row: &TantivyDocument) -> StoredRow {
    let held = fields.held(row).expect("stored row identity");
    let tombstone = held.kind == RowKind::Tombstone;
    let body = if tombstone {
        // A tombstone keeps only the key it deleted, never its words.
        assert!(row.get_first(fields.title).is_none() && row.get_first(fields.body).is_none());
        String::new()
    } else {
        fields
            .text(row, fields.body)
            .expect("stored body")
            .to_owned()
    };
    StoredRow {
        key: fields.text(row, fields.key).expect("stored key").to_owned(),
        tombstone,
        revision: held.revision,
        body,
    }
}

pub fn notes() -> Namespace {
    Namespace::feed("notes").expect("valid synthetic feed name")
}

pub fn page_document(id: &str, revision: u64, captured_at: i64, word: &str) -> FullTextDocument {
    document(Namespace::page(), id, revision, captured_at, word)
}

pub fn document(
    namespace: Namespace,
    id: &str,
    revision: u64,
    captured_at: i64,
    word: &str,
) -> FullTextDocument {
    FullTextDocument {
        namespace,
        id: id.to_owned(),
        revision,
        captured_at,
        title: format!("Synthetic page {id}"),
        body: format!("A made-up paragraph about {word} for a synthetic test."),
    }
}

pub fn upsert(document: FullTextDocument) -> Change {
    Change::Upsert(document)
}

pub fn delete(namespace: Namespace, id: &str, revision: u64) -> Change {
    Change::Delete(Deletion {
        namespace,
        id: id.to_owned(),
        revision,
        deleted_at: NOW,
    })
}
