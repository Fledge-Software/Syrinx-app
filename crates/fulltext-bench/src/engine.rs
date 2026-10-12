//! Gives both disposable engines the same mutation and query measurement contract.

use crate::{Result, corpus::Document, store, tantivy_store::TantivyStore};
use rusqlite::Connection;
use std::path::Path;

pub enum Engine {
    Fts5(Connection),
    Tantivy(Box<TantivyStore>),
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
            _ => Err("engine must be fts5 or tantivy".into()),
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
                    transaction.execute("DELETE FROM pages WHERE rowid=?1", [*id as i64])?;
                    if tombstones {
                        transaction.execute("INSERT INTO deleted VALUES(?1)", [*id as i64])?;
                    }
                }
                for document in documents {
                    store::insert(&transaction, document)?;
                }
                transaction.commit()?;
                store::maintain(connection)
            }
            Self::Tantivy(store) => store.batch(documents, removed, tombstones),
        }
    }

    pub fn expire(&mut self) -> Result<usize> {
        match self {
            Self::Fts5(connection) => {
                let expired = store::expire(connection)?;
                store::maintain(connection)?;
                Ok(expired)
            }
            Self::Tantivy(store) => store.expire(),
        }
    }

    pub fn query(&self, query: &str) -> Result<usize> {
        match self {
            Self::Fts5(connection) => {
                let mut statement = connection.prepare("SELECT rowid,snippet(pages,2,'[',']','...',16) FROM pages WHERE pages MATCH ?1 ORDER BY rank LIMIT 20")?;
                let rows = statement.query_map([query], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?;
                let mut returned = 0;
                for row in rows {
                    std::hint::black_box(row?);
                    returned += 1;
                }
                Ok(returned)
            }
            Self::Tantivy(store) => store.query(query),
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
        }
    }

    pub fn verify(&self, expected: usize) -> Result<()> {
        match self {
            Self::Fts5(connection) => store::verify(connection, expected),
            Self::Tantivy(store) => store.verify(expected),
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
                store::maintain(connection)
            }
            Self::Tantivy(store) => store.replay(documents),
        }
    }

    pub fn close(self) -> Result<()> {
        match self {
            Self::Fts5(connection) => connection.close().map_err(|(_, error)| error.into()),
            Self::Tantivy(store) => store.close(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::{self, Corpus};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn directory(label: &str) -> Result<PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/p1-23-tests");
        fs::create_dir_all(&root)?;
        let path = root.join(format!(
            "{label}-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir(&path)?;
        Ok(path)
    }

    #[test]
    fn engines_agree_on_queries_in_both_corpora() -> Result<()> {
        for corpus in [Corpus::Overlap, Corpus::Broad] {
            let mut fts5 = Engine::open("fts5", &directory("fts5")?)?;
            let mut tantivy = Engine::open("tantivy", &directory("tantivy")?)?;
            let documents: Vec<_> = (0..200).map(|id| corpus.document(id)).collect();
            fts5.batch(&documents, &[], false)?;
            tantivy.batch(&documents, &[], false)?;
            for query in
                corpus::QUERIES
                    .into_iter()
                    .chain(["cafe", "\"meadow copper\"", "lexeme0001"])
            {
                assert_eq!(
                    fts5.hits(query)?,
                    tantivy.hits(query)?,
                    "{corpus:?} {query}"
                );
                assert_eq!(
                    fts5.query(query)?,
                    tantivy.query(query)?,
                    "{corpus:?} {query}"
                );
            }
            fts5.close()?;
            tantivy.close()?;
        }
        Ok(())
    }

    #[test]
    fn tantivy_expunges_bodies_and_rejects_stale_replay_after_reopen() -> Result<()> {
        let path = directory("retention")?;
        let mut engine = Engine::open("tantivy", &path)?;
        let documents: Vec<_> = [3, 5, 1005].into_iter().map(corpus::document).collect();
        engine.batch(&documents, &[], false)?;
        engine.batch(&[], &[1005], true)?;
        assert_eq!(engine.expire()?, 1);
        engine.verify(1)?;
        engine.close()?;
        let mut reopened = Engine::open("tantivy", &path)?;
        reopened.replay(&[corpus::document(3), corpus::document(1005)])?;
        reopened.verify(1)?;
        reopened.close()
    }
}
