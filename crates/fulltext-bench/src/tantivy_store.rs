//! Measures Tantivy commits, physical segment cleanup and durable deletion barriers.

use crate::{
    Result,
    corpus::{CUTOFF, Document},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Bound,
    path::Path,
};
use tantivy::{
    Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term,
    collector::{Count, TopDocs},
    indexer::NoMergePolicy,
    query::{BooleanQuery, Occur, PhrasePrefixQuery, Query, QueryParser, RangeQuery, TermQuery},
    schema::{
        FAST, Field, INDEXED, IndexRecordOption, STORED, Schema, TextFieldIndexing, TextOptions,
        Value,
    },
    snippet::SnippetGenerator,
    tokenizer::{AsciiFoldingFilter, LowerCaser, SimpleTokenizer, TextAnalyzer},
};

/// Tantivy bounds snippets by characters; FTS5's 16 tokens are roughly 110 to 180 here.
const SNIPPET_CHARACTERS: usize = 120;

pub struct TantivyStore {
    index: Index,
    writer: IndexWriter,
    reader: Option<IndexReader>,
    id: Field,
    deleted: Field,
    title: Field,
    url: Field,
    body: Field,
    captured: Field,
}

impl TantivyStore {
    pub fn open(directory: &Path) -> Result<Self> {
        let mut schema = Schema::builder();
        let id = schema.add_u64_field("id", INDEXED | STORED);
        let deleted = schema.add_u64_field("deleted", INDEXED | STORED);
        let text = TextOptions::default().set_stored().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("folded")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        let title = schema.add_text_field("title", text.clone());
        let url = schema.add_text_field("url", STORED);
        let body = schema.add_text_field("body", text);
        // A fast, indexed capture time lets expiry select old pages without loading them.
        let captured = schema.add_i64_field("captured", INDEXED | STORED | FAST);
        let index = if directory.join("meta.json").exists() {
            Index::open_in_dir(directory)?
        } else {
            Index::create_in_dir(directory, schema.build())?
        };
        index.tokenizers().register(
            "folded",
            TextAnalyzer::builder(SimpleTokenizer::default())
                .filter(LowerCaser)
                .filter(AsciiFoldingFilter)
                .build(),
        );
        let writer = index.writer_with_num_threads(1, 50_000_000)?;
        writer.set_merge_policy(Box::new(NoMergePolicy));
        // Recover orphaned prepared segments before any reader can be exposed.
        let garbage = writer.garbage_collect_files().wait()?;
        if !garbage.failed_to_delete_files.is_empty() {
            return Err("Tantivy startup garbage collection failed".into());
        }
        let mut store = Self {
            index,
            writer,
            reader: None,
            id,
            deleted,
            title,
            url,
            body,
            captured,
        };
        store.reload()?;
        Ok(store)
    }

    fn reload(&mut self) -> Result<()> {
        self.reader = Some(
            self.index
                .reader_builder()
                .reload_policy(ReloadPolicy::Manual)
                .try_into()?,
        );
        Ok(())
    }

    fn searcher(&self) -> Result<tantivy::Searcher> {
        Ok(self
            .reader
            .as_ref()
            .ok_or("reader unavailable during maintenance")?
            .searcher())
    }

    pub fn insert(&self, document: &Document) -> Result<()> {
        let mut row = TantivyDocument::default();
        row.add_u64(self.id, document.id as u64);
        row.add_text(self.title, &document.title);
        row.add_text(self.url, &document.url);
        row.add_text(self.body, &document.body);
        row.add_i64(self.captured, document.captured);
        self.writer.add_document(row)?;
        Ok(())
    }

    pub fn remove(&self, id: usize) {
        self.writer
            .delete_term(Term::from_field_u64(self.id, id as u64));
    }

    pub fn batch(
        &mut self,
        documents: &[Document],
        removed: &[usize],
        tombstones: bool,
    ) -> Result<()> {
        for id in removed {
            self.remove(*id);
            if tombstones {
                let mut row = TantivyDocument::default();
                row.add_u64(self.deleted, *id as u64);
                self.writer.add_document(row)?;
            }
        }
        for document in documents {
            self.insert(document)?;
        }
        self.commit_and_expunge()
    }

    fn commit_and_expunge(&mut self) -> Result<()> {
        // Rewrite only segments holding deleted documents, so their text leaves the disk.
        self.reader = None;
        self.writer.commit()?;
        let dirty: Vec<_> = self
            .index
            .searchable_segment_metas()?
            .iter()
            .filter(|segment| segment.num_deleted_docs() > 0)
            .map(|segment| segment.id())
            .collect();
        if !dirty.is_empty() {
            self.writer.merge(&dirty).wait()?;
        }
        self.collect_garbage()?;
        self.reload()
    }

    pub fn optimize(&mut self) -> Result<()> {
        // Consolidate every segment once, timed apart from ordinary mutations.
        self.reader = None;
        let segments: Vec<_> = self
            .index
            .searchable_segment_metas()?
            .iter()
            .map(|segment| segment.id())
            .collect();
        if segments.len() > 1 {
            self.writer.merge(&segments).wait()?;
        }
        self.collect_garbage()?;
        self.reload()
    }

    fn collect_garbage(&mut self) -> Result<()> {
        let garbage = self.writer.garbage_collect_files().wait()?;
        if !garbage.failed_to_delete_files.is_empty() {
            return Err("Tantivy retention garbage collection failed".into());
        }
        Ok(())
    }

    fn rows(&self) -> Result<Vec<TantivyDocument>> {
        let searcher = self.searcher()?;
        let mut rows = Vec::new();
        for segment in searcher.segment_readers() {
            if segment.max_doc() != segment.num_docs() {
                return Err("deleted stored documents remain in an active segment".into());
            }
            let store = segment.get_store_reader(0)?;
            for id in 0..segment.max_doc() {
                rows.push(store.get(id)?);
            }
        }
        Ok(rows)
    }

    pub fn expire(&mut self) -> Result<usize> {
        // Tombstones carry no capture time, so the range selects only expired pages.
        let expired_range = || {
            RangeQuery::new(
                Bound::Unbounded,
                Bound::Included(Term::from_field_i64(self.captured, CUTOFF)),
            )
        };
        let expired = self.searcher()?.search(&expired_range(), &Count)?;
        self.writer.delete_query(Box::new(expired_range()))?;
        self.commit_and_expunge()?;
        Ok(expired)
    }

    fn parse(&self, query: &str) -> Result<Box<dyn Query>> {
        if let Some(prefix) = query.strip_suffix('*') {
            let clauses = [self.title, self.body]
                .into_iter()
                .map(|field| {
                    let mut query =
                        PhrasePrefixQuery::new(vec![Term::from_field_text(field, prefix)]);
                    query.set_max_expansions(u32::MAX);
                    (Occur::Should, Box::new(query) as Box<dyn Query>)
                })
                .collect();
            return Ok(Box::new(BooleanQuery::new(clauses)));
        }
        Ok(QueryParser::for_index(&self.index, vec![self.title, self.body]).parse_query(query)?)
    }

    pub fn query(&self, text: &str) -> Result<Vec<u64>> {
        let searcher = self.searcher()?;
        let query = self.parse(text)?;
        let results = searcher.search(&query, &TopDocs::with_limit(20).order_by_score())?;
        let mut snippets = self.snippets(&searcher, &*query, text)?;
        snippets.set_max_num_chars(SNIPPET_CHARACTERS);
        let mut ids = Vec::with_capacity(results.len());
        for (_, address) in &results {
            let row: TantivyDocument = searcher.doc(*address)?;
            let id = row
                .get_first(self.id)
                .and_then(|value| value.as_u64())
                .ok_or("missing id")?;
            let snippet = snippets.snippet_from_doc(&row).to_html();
            if snippet.is_empty() {
                return Err("body query returned an empty snippet".into());
            }
            std::hint::black_box(snippet);
            ids.push(id);
        }
        Ok(ids)
    }

    fn snippets(
        &self,
        searcher: &tantivy::Searcher,
        query: &dyn Query,
        text: &str,
    ) -> Result<SnippetGenerator> {
        let Some(prefix) = text.strip_suffix('*') else {
            return Ok(SnippetGenerator::create(searcher, query, self.body)?);
        };
        // Native prefix queries omit their suffix term from snippet extraction.
        let mut frequencies = BTreeMap::new();
        for segment in searcher.segment_readers() {
            let inverted = segment.inverted_index(self.body)?;
            let mut stream = inverted
                .terms()
                .range()
                .ge(prefix.as_bytes())
                .into_stream()?;
            while stream.advance() && stream.key().starts_with(prefix.as_bytes()) {
                let term = std::str::from_utf8(stream.key())?.to_owned();
                *frequencies.entry(term).or_insert(0u64) += u64::from(stream.value().doc_freq);
            }
        }
        let weights = frequencies
            .into_iter()
            .map(|(term, frequency)| (term, 1.0 / (1.0 + frequency as f32)))
            .collect();
        Ok(SnippetGenerator::new(
            weights,
            self.index.tokenizer_for_field(self.body)?,
            self.body,
            SNIPPET_CHARACTERS,
        ))
    }

    pub fn hits(&self, text: &str) -> Result<usize> {
        Ok(self.searcher()?.search(&self.parse(text)?, &Count)?)
    }

    pub fn verify(&self, expected: usize) -> Result<()> {
        if !self.index.validate_checksum()?.is_empty() {
            return Err("Tantivy checksum mismatch".into());
        }
        let rows = self.rows()?;
        let deleted: BTreeSet<_> = rows
            .iter()
            .filter_map(|row| row.get_first(self.deleted).and_then(|value| value.as_u64()))
            .collect();
        let mut ids = BTreeSet::new();
        for row in &rows {
            if row.get_first(self.deleted).is_some() {
                continue;
            }
            let id = row
                .get_first(self.id)
                .and_then(|value| value.as_u64())
                .ok_or("missing id")?;
            let captured = row
                .get_first(self.captured)
                .and_then(|value| value.as_i64())
                .ok_or("missing captured")?;
            if captured <= CUTOFF || deleted.contains(&id) || !ids.insert(id) {
                return Err("expired, deleted or duplicate stored text returned".into());
            }
        }
        if ids.len() != expected {
            return Err(format!("expected {expected} documents, found {}", ids.len()).into());
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

    pub fn replay(&mut self, documents: &[Document]) -> Result<()> {
        let searcher = self.searcher()?;
        for document in documents {
            let query = TermQuery::new(
                Term::from_field_u64(self.deleted, document.id as u64),
                IndexRecordOption::Basic,
            );
            let deleted = searcher.search(&query, &Count)? != 0;
            if document.captured > CUTOFF && !deleted {
                return Err("stale replay accepted".into());
            }
        }
        drop(searcher);
        self.commit_and_expunge()
    }

    pub fn crash_prepare(&mut self) -> Result<()> {
        let prepared = self.writer.prepare_commit()?;
        crate::recovery::signal_and_wait()?;
        prepared.abort()?;
        Err("writer was not killed by its parent".into())
    }

    pub fn close(self) -> Result<()> {
        drop(self.reader);
        self.writer.wait_merging_threads()?;
        Ok(())
    }
}
