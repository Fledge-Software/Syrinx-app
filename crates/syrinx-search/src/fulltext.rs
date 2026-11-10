//! Syrinx's durable full-text index (tier 2, P1-24) on Tantivy, with one namespace per source.

mod maintenance;
mod schema;
mod search;
mod staging;
mod types;

#[cfg(test)]
mod tests;

use std::{collections::HashMap, ops::Bound, path::Path};

use tantivy::{
    Index, IndexReader, IndexWriter, ReloadPolicy, Searcher, TantivyDocument,
    collector::{Count, DocSetCollector, TopDocs},
    indexer::NoMergePolicy,
    query::{BooleanQuery, Occur, Query, RangeQuery, TermQuery},
    schema::IndexRecordOption,
};

use schema::{Fields, Held, RowKind};
pub use types::{
    Change, ClearRequest, ClearScope, Deletion, FullTextDocument, FullTextError, FullTextHit,
    Namespace, NamespaceFilter, Outcome, PAGE_RETENTION_SECONDS, Retention, SearchRequest,
};

/// The writer's indexing memory; Tantivy's minimum is 15 MB.
const WRITER_MEMORY_BYTES: usize = 32 * 1024 * 1024;

/// An open index folder, held exclusively by this writer.
///
/// Writes are staged in memory and become visible and durable together at `commit`; until
/// then a crash or `discard` loses them. Expiry, namespace resets and journal clears are
/// maintenance operations that commit on their own. Every commit physically removes the
/// rows it deleted before it returns, or reports `CleanupPending` and retries at the next.
pub struct FullText {
    index: Index,
    writer: IndexWriter,
    reader: Option<IndexReader>,
    fields: Fields,
    /// What each key holds after the staged changes, which the committed reader cannot see.
    pending: HashMap<String, Held>,
    /// The last D47 journal generation applied to page text.
    cleared: u64,
    /// A failed cleanup left deleted rows on disk; every later commit retries it.
    cleanup_pending: bool,
    /// A rollback failed and released the folder's lock, so this handle refuses all work.
    broken: bool,
}

impl FullText {
    /// Opens or creates the index in the existing `directory`, finishing any interrupted
    /// cleanup. The folder must be local and outside D13's backups: the index is a cache (D40).
    pub fn open(directory: &Path) -> Result<Self, FullTextError> {
        let (schema, fields) = schema::build();
        let index = if directory.join("meta.json").try_exists()? {
            Index::open_in_dir(directory)?
        } else {
            Index::create_in_dir(directory, schema.clone())?
        };
        if index.schema() != schema {
            return Err(FullTextError::IncompatibleIndex);
        }
        schema::register_tokenizer(&index);
        let writer = index.writer_with_num_threads(1, WRITER_MEMORY_BYTES)?;
        keep_merges_explicit(&writer);
        let cleared = maintenance::cleared_generation(&index)?;
        let mut fulltext = Self {
            index,
            writer,
            reader: None,
            fields,
            pending: HashMap::new(),
            cleared,
            cleanup_pending: false,
            broken: false,
        };
        // A crash after a commit but before its cleanup leaves deleted rows on disk. If they
        // cannot be removed now, the index still opens and `cleanup_pending` reports them.
        match fulltext.tidy() {
            Ok(()) | Err(FullTextError::CleanupPending) => Ok(fulltext),
            Err(error) => Err(error),
        }
    }

    /// Stages changes in order; nothing is visible or durable until `commit`.
    ///
    /// Every change is checked before any is staged. If staging fails part-way, everything
    /// staged since the last commit is discarded, so a partial batch can never be committed.
    pub fn stage(&mut self, changes: &[Change], now: i64) -> Result<Vec<Outcome>, FullTextError> {
        self.require_usable()?;
        for change in changes {
            change.validate()?;
        }
        let mut outcomes = Vec::with_capacity(changes.len());
        for change in changes {
            match self.stage_one(change, now) {
                Ok(outcome) => outcomes.push(outcome),
                Err(error) => return Err(self.discard_after(error)),
            }
        }
        Ok(outcomes)
    }

    /// Makes every staged change durable and visible, and removes the text it replaced.
    ///
    /// `CleanupPending` means the commit is durable but replaced text is still on disk.
    pub fn commit(&mut self) -> Result<(), FullTextError> {
        self.require_usable()?;
        self.commit_with(self.cleared)
    }

    /// Drops every staged change, as a crash before `commit` would.
    pub fn discard(&mut self) -> Result<(), FullTextError> {
        self.require_usable()?;
        self.pending.clear();
        if let Err(error) = self.writer.rollback() {
            // Tantivy has already released the folder's lock, so this writer must stop.
            self.broken = true;
            return Err(error.into());
        }
        // Tantivy's rollback builds a fresh writer with its default merge policy.
        keep_merges_explicit(&self.writer);
        Ok(())
    }

    /// Whether deleted or expired text is still on disk after a failed cleanup. Not saved:
    /// after a restart it reflects only the cleanup `open` itself attempted.
    pub fn cleanup_pending(&self) -> bool {
        self.cleanup_pending
    }

    /// Retries removing deleted rows from disk without waiting for the next commit.
    pub fn retry_cleanup(&mut self) -> Result<(), FullTextError> {
        self.require_usable()?;
        self.tidy()
    }

    /// Removes page text and tombstones past D40's seven days; returns the documents removed.
    ///
    /// Search does not filter by age, so the owner of the index runs this at start-up and
    /// at least daily. `CleanupPending` means the expiry is durable but its text is still on
    /// disk; running it again then reports 0.
    pub fn expire(&mut self, now: i64) -> Result<usize, FullTextError> {
        self.require_nothing_staged()?;
        let cutoff = now.saturating_sub(PAGE_RETENTION_SECONDS);
        let expired = self.page_dated(Bound::Unbounded, Bound::Included(cutoff));
        let removed = self.count_documents(&expired)?;
        self.stage_deletion(expired)?;
        self.commit_with(self.cleared)?;
        Ok(removed)
    }

    /// Removes every document and tombstone in one namespace, leaving the others intact.
    ///
    /// A feed's tombstones never expire, so this is also how a feed starts afresh.
    pub fn reset(&mut self, namespace: &Namespace) -> Result<(), FullTextError> {
        self.require_nothing_staged()?;
        self.writer
            .delete_term(self.fields.namespace_term(namespace));
        self.commit_with(self.cleared)
    }

    /// The last D47 journal generation this index has applied.
    pub fn cleared_generation(&self) -> u64 {
        self.cleared
    }

    /// Applies D47 journal clears to page text, skipping generations already applied.
    ///
    /// Each history clear removes page documents and tombstones dated inside its range;
    /// tombstones go too because their ids can name the cleared pages. A clear of anything
    /// else only advances the generation. The new generation is committed
    /// with the deletions, so a crash repeats the clear, never skips it, and replaying the
    /// same journal twice deletes nothing twice. Returns how many clears were newly applied.
    ///
    /// `CleanupPending` means the clears and their generation are durable but the text is
    /// still on disk; a retry then reports 0. An index whose generation is ahead of the
    /// journal cannot be detected here; the caller compares the two first and calls
    /// `restart_clears` when the journal is behind.
    pub fn apply_clears(&mut self, clears: &[ClearRequest]) -> Result<usize, FullTextError> {
        self.require_nothing_staged()?;
        let unapplied: Vec<_> = clears
            .iter()
            .filter(|clear| clear.generation > self.cleared)
            .collect();
        let Some(last) = unapplied.last() else {
            return Ok(0);
        };
        let in_order = unapplied
            .iter()
            .zip(self.cleared + 1..)
            .all(|(clear, expected)| clear.generation == expected && well_formed(clear.scope));
        if !in_order {
            return Err(FullTextError::ClearOutOfOrder);
        }
        for clear in &unapplied {
            // A clear of something else only moves the generation on.
            let ClearScope::Pages { start, end } = clear.scope else {
                continue;
            };
            let range = self.page_dated(Bound::Included(start), Bound::Excluded(end));
            self.stage_deletion(range)?;
        }
        self.commit_with(last.generation)?;
        Ok(unapplied.len())
    }

    /// Removes all page text and its tombstones and counts clears from generation 0 again.
    ///
    /// For a journal recreated behind this index: its generations no longer say what was
    /// cleared, so nothing dated can be trusted to stay. Feeds are untouched. The caller
    /// then applies the journal from its start.
    pub fn restart_clears(&mut self) -> Result<(), FullTextError> {
        self.require_nothing_staged()?;
        self.writer
            .delete_term(self.fields.namespace_term(&Namespace::page()));
        self.commit_with(0)
    }

    /// Removes every page row, document or tombstone, whose id `matches` accepts, and leaves
    /// no tombstone in its place: a tombstone's id would itself keep the address. Returns how
    /// many rows went. Excluding a site erases its text this way (#54).
    pub fn remove_pages(&mut self, matches: impl Fn(&str) -> bool) -> Result<usize, FullTextError> {
        self.require_nothing_staged()?;
        let doomed: Vec<String> = self
            .page_ids()?
            .into_iter()
            .filter(|id| matches(id))
            .collect();
        if doomed.is_empty() {
            return Ok(0);
        }
        let page = Namespace::page();
        for id in &doomed {
            self.writer.delete_term(self.fields.key_term(&page, id));
        }
        self.commit_with(self.cleared)?;
        Ok(doomed.len())
    }

    /// Ranks committed documents against the owner's words.
    pub fn search(&self, request: &SearchRequest) -> Result<Vec<FullTextHit>, FullTextError> {
        self.require_usable()?;
        search::run(&self.index, &self.searcher()?, &self.fields, request)
    }

    /// Decides one change against what its key holds, and writes it if it wins.
    fn stage_one(&mut self, change: &Change, now: i64) -> Result<Outcome, FullTextError> {
        let held = self.held(change.namespace(), change.id())?;
        let outcome = staging::decide(change, held, now);
        if outcome == Outcome::Staged {
            self.write(change)?;
        }
        Ok(outcome)
    }

    /// What `key` holds once staged changes are counted.
    fn held(&self, namespace: &Namespace, id: &str) -> Result<Option<Held>, FullTextError> {
        if let Some(held) = self.pending.get(&schema::key(namespace, id)) {
            return Ok(Some(*held));
        }
        let searcher = self.searcher()?;
        let query = TermQuery::new(
            self.fields.key_term(namespace, id),
            IndexRecordOption::Basic,
        );
        let rows = searcher.search(&query, &TopDocs::with_limit(2).order_by_score())?;
        match rows.as_slice() {
            [] => Ok(None),
            [(_, address)] => Ok(Some(self.fields.held(&searcher.doc(*address)?)?)),
            _ => Err(FullTextError::Damaged),
        }
    }

    /// Replaces whatever the key held with the change's row.
    fn write(&mut self, change: &Change) -> Result<(), FullTextError> {
        let namespace = change.namespace();
        self.writer
            .delete_term(self.fields.key_term(namespace, change.id()));
        let (row, kind) = match change {
            Change::Upsert(document) => (self.fields.document(document), RowKind::Document),
            Change::Delete(deletion) => (
                self.fields.tombstone(
                    namespace,
                    &deletion.id,
                    deletion.revision,
                    deletion.deleted_at,
                ),
                RowKind::Tombstone,
            ),
        };
        self.writer.add_document(row)?;
        let held = Held {
            kind,
            revision: change.revision(),
        };
        self.pending
            .insert(schema::key(namespace, change.id()), held);
        Ok(())
    }

    /// Discards a half-written batch and reports the failure that stopped it.
    fn discard_after(&mut self, error: FullTextError) -> FullTextError {
        match self.discard() {
            Ok(()) => error,
            // Both failed: the rollback's failure is the one that leaves the writer unusable.
            Err(rollback) => rollback,
        }
    }

    fn require_usable(&self) -> Result<(), FullTextError> {
        if self.broken {
            Err(FullTextError::NeedsReopen)
        } else {
            Ok(())
        }
    }

    fn require_nothing_staged(&self) -> Result<(), FullTextError> {
        self.require_usable()?;
        if self.pending.is_empty() {
            Ok(())
        } else {
            Err(FullTextError::StagedWorkPending)
        }
    }

    /// Page rows, documents and tombstones alike, dated within the bounds.
    fn page_dated(&self, start: Bound<i64>, end: Bound<i64>) -> BooleanQuery {
        let term = |value| tantivy::Term::from_field_i64(self.fields.captured, value);
        let range = RangeQuery::new(start.map(term), end.map(term));
        let page = TermQuery::new(
            self.fields.namespace_term(&Namespace::page()),
            IndexRecordOption::Basic,
        );
        BooleanQuery::new(vec![
            (Occur::Must, Box::new(page) as Box<dyn Query>),
            (Occur::Must, Box::new(range)),
        ])
    }

    /// The id of every committed page row, documents and tombstones alike.
    fn page_ids(&self) -> Result<Vec<String>, FullTextError> {
        let searcher = self.searcher()?;
        let page = TermQuery::new(
            self.fields.namespace_term(&Namespace::page()),
            IndexRecordOption::Basic,
        );
        let rows = searcher.search(&page, &DocSetCollector)?;
        rows.into_iter()
            .map(|address| {
                let row: TantivyDocument = searcher.doc(address)?;
                Ok(self.fields.text(&row, self.fields.id)?.to_owned())
            })
            .collect()
    }

    fn count_documents(&self, rows: &BooleanQuery) -> Result<usize, FullTextError> {
        let documents = TermQuery::new(
            self.fields.kind_term(RowKind::Document),
            IndexRecordOption::Basic,
        );
        let query = BooleanQuery::new(vec![
            (Occur::Must, Box::new(rows.clone()) as Box<dyn Query>),
            (Occur::Must, Box::new(documents)),
        ]);
        Ok(self.searcher()?.search(&query, &Count)?)
    }

    /// Stages a maintenance deletion; on failure nothing it staged survives.
    fn stage_deletion(&mut self, rows: BooleanQuery) -> Result<(), FullTextError> {
        if let Err(error) = self.writer.delete_query(Box::new(rows)) {
            return Err(self.discard_after(error.into()));
        }
        Ok(())
    }

    /// Commits durably; a failed commit discards the batch rather than leave it half-known.
    fn commit_with(&mut self, cleared: u64) -> Result<(), FullTextError> {
        if let Err(error) = maintenance::commit(&mut self.writer, cleared) {
            return Err(self.discard_after(error));
        }
        self.cleared = cleared;
        self.pending.clear();
        self.tidy()
    }

    /// Removes deleted rows from disk, then always reopens a reader on the result.
    ///
    /// A failed cleanup leaves the index usable and is reported as `CleanupPending`.
    fn tidy(&mut self) -> Result<(), FullTextError> {
        // Files an open reader maps cannot be deleted on Windows, so release it first.
        self.reader = None;
        let cleaned = maintenance::expunge(&self.index, &mut self.writer);
        self.reader = Some(
            self.index
                .reader_builder()
                .reload_policy(ReloadPolicy::Manual)
                .try_into()?,
        );
        self.cleanup_pending = cleaned.is_err();
        cleaned.map_err(|error| match error {
            // Damage is not something a retry fixes, so it keeps its own category.
            FullTextError::Damaged => FullTextError::Damaged,
            _ => FullTextError::CleanupPending,
        })
    }

    fn searcher(&self) -> Result<Searcher, FullTextError> {
        let reader = self.reader.as_ref().ok_or(FullTextError::Engine)?;
        Ok(reader.searcher())
    }
}

/// A page clear must name a non-empty range; anything else carries no range.
fn well_formed(scope: ClearScope) -> bool {
    match scope {
        ClearScope::Pages { start, end } => start < end,
        ClearScope::Elsewhere => true,
    }
}

/// Merges run only when this adapter asks, so deleted text leaves on its schedule and no
/// background merge holds files that cleanup must delete.
fn keep_merges_explicit(writer: &IndexWriter) {
    writer.set_merge_policy(Box::new(NoMergePolicy));
}
