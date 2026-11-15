//! Gives the disposable engines and the production adapter one measurement contract.

use crate::{
    Result, adapter_store::AdapterStore, corpus::Document, store, tantivy_store::TantivyStore,
};
use rusqlite::Connection;
use std::path::Path;

pub enum Engine {
    Fts5(Connection),
    Tantivy(Box<TantivyStore>),
    Adapter(Box<AdapterStore>),
}

impl Engine {
    pub fn open(name: &str, directory: &Path) -> Result<Self> {
        match name {
            "fts5" => {
                let connection = store::open(&directory.join("synthetic-cache.db"))?;
                let version: String =
                    connection.query_row("SELECT sqlite_version()", [], |row| row.get(0))?;
                if version != "3.53.4" {
                    return Err(format!("unexpected SQLite {version}").into());
                }
                Ok(Self::Fts5(connection))
            }
            "tantivy" => Ok(Self::Tantivy(Box::new(TantivyStore::open(directory)?))),
            "adapter" => Ok(Self::Adapter(Box::new(AdapterStore::open(directory)?))),
            _ => Err("engine must be fts5, tantivy or adapter".into()),
        }
    }

    pub fn batch(
        &mut self,
        documents: &[Document],
        removed: &[usize],
        tombstones: bool,
    ) -> Result<()> {
        match self {
            Self::Fts5(connection) => {
                let transaction = connection.transaction()?;
                for id in removed {
                    store::remove(&transaction, *id)?;
                    if tombstones {
                        transaction.execute("INSERT INTO deleted VALUES(?1)", [*id as i64])?;
                    }
                }
                for document in documents {
                    store::insert(&transaction, document)?;
                }
                transaction.commit()?;
                store::checkpoint(connection)
            }
            Self::Tantivy(store) => store.batch(documents, removed, tombstones),
            // The adapter always leaves a tombstone for a deletion.
            Self::Adapter(store) => store.batch(documents, removed),
        }
    }

    /// Consolidates the index once; mutations alone already remove deleted text.
    pub fn optimize(&mut self) -> Result<()> {
        match self {
            Self::Fts5(connection) => store::optimize(connection),
            Self::Tantivy(store) => store.optimize(),
            // The adapter merges at every commit and offers no separate pass.
            Self::Adapter(_) => Ok(()),
        }
    }

    pub fn expire(&mut self) -> Result<usize> {
        match self {
            Self::Fts5(connection) => {
                let expired = store::expire(connection)?;
                store::checkpoint(connection)?;
                Ok(expired)
            }
            Self::Tantivy(store) => store.expire(),
            Self::Adapter(store) => store.expire(),
        }
    }

    /// Whether this engine has the query's syntax; the adapter has no phrases or `OR`.
    pub fn supports(&self, query: &str) -> bool {
        match self {
            Self::Adapter(_) => crate::adapter_store::request(query, 1).is_some(),
            Self::Fts5(_) | Self::Tantivy(_) => true,
        }
    }

    /// Returns the top-20 IDs in rank order after building each one's snippet.
    pub fn query(&self, query: &str) -> Result<Vec<u64>> {
        match self {
            Self::Fts5(connection) => {
                let mut statement = connection.prepare("SELECT rowid,snippet(pages,2,'[',']','...',16) FROM pages WHERE pages MATCH ?1 ORDER BY rank LIMIT 20")?;
                let rows = statement.query_map([query], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?;
                let mut ids = Vec::with_capacity(20);
                for row in rows {
                    let (id, snippet) = row?;
                    std::hint::black_box(snippet);
                    ids.push(u64::try_from(id)?);
                }
                Ok(ids)
            }
            Self::Tantivy(store) => store.query(query),
            Self::Adapter(store) => store.query(query),
        }
    }

    pub fn hits(&self, query: &str) -> Result<usize> {
        match self {
            Self::Fts5(connection) => Ok(connection.query_row(
                "SELECT count(*) FROM pages WHERE pages MATCH ?1",
                [query],
                |row| row.get::<_, i64>(0),
            )? as usize),
            Self::Tantivy(store) => store.hits(query),
            Self::Adapter(store) => store.hits(query),
        }
    }

    pub fn verify(&self, expected: usize) -> Result<()> {
        match self {
            Self::Fts5(connection) => store::verify(connection, expected),
            Self::Tantivy(store) => store.verify(expected),
            Self::Adapter(store) => store.verify(expected),
        }
    }

    pub fn replay(&mut self, documents: &[Document]) -> Result<()> {
        match self {
            Self::Fts5(connection) => {
                let transaction = connection.transaction()?;
                for document in documents {
                    if store::replay(&transaction, document)? {
                        return Err("stale replay accepted".into());
                    }
                }
                transaction.commit()?;
                store::checkpoint(connection)
            }
            Self::Tantivy(store) => store.replay(documents),
            Self::Adapter(store) => store.replay(documents),
        }
    }

    pub fn close(self) -> Result<()> {
        match self {
            Self::Fts5(connection) => connection.close().map_err(|(_, error)| error.into()),
            Self::Tantivy(store) => store.close(),
            Self::Adapter(store) => {
                drop(store);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::{self, Corpus};
    use std::{
        collections::BTreeSet,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    /// Share of the broad corpus's top 20 both engines must agree on when more than 20 match.
    /// At 1,000 pages the lowest observed share is 14 of 20, for `lexeme000*`.
    const MINIMUM_OVERLAP_PERCENT: usize = 60;

    /// A fresh directory under the system temporary folder, removed when the test ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Result<Self> {
            let path = std::env::temp_dir().join(format!(
                "syrinx-p1-23-{label}-{}-{}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
            ));
            fs::create_dir(&path)?;
            Ok(Self(path))
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            // Report a leftover directory instead of hiding it; Drop cannot return the error.
            if let Err(error) = fs::remove_dir_all(&self.0) {
                eprintln!("could not remove {}: {error}", self.0.display());
            }
        }
    }

    #[test]
    fn engines_agree_on_queries_in_both_corpora() -> Result<()> {
        for corpus in [Corpus::Overlap, Corpus::Broad] {
            let (fts5_path, tantivy_path) = (Scratch::new("fts5")?, Scratch::new("tantivy")?);
            let mut fts5 = Engine::open("fts5", &fts5_path.0)?;
            let mut tantivy = Engine::open("tantivy", &tantivy_path.0)?;
            let documents: Vec<_> = (0..1000).map(|id| corpus.document(id)).collect();
            fts5.batch(&documents, &[], false)?;
            tantivy.batch(&documents, &[], false)?;
            for query in
                corpus::QUERIES
                    .into_iter()
                    .chain(["cafe", "\"meadow copper\"", "lexeme0001"])
            {
                let hits = fts5.hits(query)?;
                assert_eq!(hits, tantivy.hits(query)?, "{corpus:?} {query}");
                let (fts5_top, tantivy_top) = (fts5.query(query)?, tantivy.query(query)?);
                assert_eq!(fts5_top.len(), tantivy_top.len(), "{corpus:?} {query}");
                assert_top_overlap(corpus, query, hits, &fts5_top, &tantivy_top);
            }
            fts5.close()?;
            tantivy.close()?;
        }
        Ok(())
    }

    // Complete small result sets must match; broad-corpus top 20s must mostly agree.
    // The overlap corpus puts every term in every page, so its ranking is near-ties.
    fn assert_top_overlap(corpus: Corpus, query: &str, hits: usize, fts5: &[u64], tantivy: &[u64]) {
        let fts5: BTreeSet<_> = fts5.iter().collect();
        let tantivy: BTreeSet<_> = tantivy.iter().collect();
        if hits <= fts5.len() {
            assert_eq!(
                fts5, tantivy,
                "{corpus:?} {query}: complete result sets differ"
            );
            return;
        }
        let shared = fts5.intersection(&tantivy).count();
        println!("top_overlap,{corpus:?},{query},{shared},{}", fts5.len());
        if corpus == Corpus::Overlap {
            return;
        }
        assert!(
            shared * 100 >= fts5.len() * MINIMUM_OVERLAP_PERCENT,
            "{corpus:?} {query}: only {shared} of {} top results shared",
            fts5.len()
        );
    }

    /// Queries both corpora express without phrases or `OR`, so both engines can run them.
    const SHARED_QUERIES: [&str; 6] = [
        "lantern",
        "orb*",
        "lantern AND violet",
        "sigil999",
        "lexeme0001",
        "lexeme000*",
    ];

    #[test]
    fn the_adapter_matches_what_the_tantivy_engine_matches() -> Result<()> {
        for corpus in [Corpus::Overlap, Corpus::Broad] {
            let (tantivy_path, adapter_path) = (Scratch::new("tantivy")?, Scratch::new("adapter")?);
            let mut tantivy = Engine::open("tantivy", &tantivy_path.0)?;
            let mut adapter = Engine::open("adapter", &adapter_path.0)?;
            let documents: Vec<_> = (0..1000).map(|id| corpus.document(id)).collect();
            tantivy.batch(&documents, &[], false)?;
            adapter.batch(&documents, &[], false)?;
            for query in SHARED_QUERIES {
                // The adapter stops counting at 1,000 hits, which the overlap corpus reaches.
                let hits = tantivy.hits(query)?.min(1000);
                assert_eq!(hits, adapter.hits(query)?, "{corpus:?} {query}");
                assert_eq!(
                    tantivy.query(query)?.len(),
                    adapter.query(query)?.len(),
                    "{corpus:?} {query}"
                );
            }
            tantivy.close()?;
            adapter.close()?;
        }
        Ok(())
    }

    #[test]
    fn the_adapter_reports_phrases_and_or_as_unsupported() -> Result<()> {
        let scratch = Scratch::new("adapter-syntax")?;
        let adapter = Engine::open("adapter", &scratch.0)?;
        let unsupported: Vec<_> = corpus::QUERIES
            .into_iter()
            .filter(|query| !adapter.supports(query))
            .collect();
        assert_eq!(unsupported, ["\"copper meadow\"", "copper OR orchard"]);
        adapter.close()
    }

    #[test]
    fn the_adapter_expunges_bodies_and_rejects_stale_replay_after_reopen() -> Result<()> {
        assert_retention_and_replay("adapter")
    }

    #[test]
    fn tantivy_expunges_bodies_and_rejects_stale_replay_after_reopen() -> Result<()> {
        assert_retention_and_replay("tantivy")
    }

    fn assert_retention_and_replay(name: &str) -> Result<()> {
        let scratch = Scratch::new("retention")?;
        let path = scratch.0.as_path();
        let mut engine = Engine::open(name, path)?;
        let documents: Vec<_> = [3, 5, 1005].into_iter().map(corpus::document).collect();
        engine.batch(&documents, &[], false)?;
        engine.batch(&[], &[1005], true)?;
        assert_eq!(engine.expire()?, 1);
        engine.verify(1)?;
        engine.close()?;
        let mut reopened = Engine::open(name, path)?;
        reopened.replay(&[corpus::document(3), corpus::document(1005)])?;
        reopened.verify(1)?;
        reopened.close()
    }
}
