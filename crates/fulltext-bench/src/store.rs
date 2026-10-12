//! Keeps the synthetic FTS5 cache separate from all product data and backups.

use crate::Result;
use crate::corpus::{CUTOFF, Document};
use rusqlite::{Connection, params};
use std::path::Path;

pub fn open(path: &Path) -> Result<Connection> {
    let connection = Connection::open(path)?;
    connection.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
        PRAGMA secure_delete=ON; PRAGMA cache_size=-8192;
        CREATE VIRTUAL TABLE IF NOT EXISTS pages USING fts5(
            title, url UNINDEXED, body, captured UNINDEXED,
            tokenize='unicode61 remove_diacritics 2', prefix='2 3');
        INSERT INTO pages(pages,rank) VALUES('secure-delete',1);
        CREATE TABLE IF NOT EXISTS deleted(id INTEGER PRIMARY KEY);",
    )?;
    Ok(connection)
}

pub fn insert(connection: &Connection, document: &Document) -> Result<()> {
    connection.execute(
        "INSERT INTO pages(rowid,title,url,body,captured) VALUES(?1,?2,?3,?4,?5)",
        params![
            document.id as i64,
            document.title,
            document.url,
            document.body,
            document.captured
        ],
    )?;
    Ok(())
}

pub fn maintain(connection: &Connection) -> Result<()> {
    // Include segment consolidation and WAL truncation in retention measurements.
    connection.execute("INSERT INTO pages(pages) VALUES('optimize')", [])?;
    let busy: i64 =
        connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))?;
    if busy != 0 {
        return Err("checkpoint remained busy".into());
    }
    Ok(())
}

pub fn expire(connection: &Connection) -> Result<usize> {
    Ok(connection.execute(
        "DELETE FROM pages WHERE CAST(captured AS INTEGER) <= ?1",
        [CUTOFF],
    )?)
}

pub fn replay(connection: &Connection, document: &Document) -> Result<bool> {
    // Replayed input must pass the same age and explicit-deletion barriers.
    if document.captured <= CUTOFF {
        return Ok(false);
    }
    let deleted: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM deleted WHERE id=?1)",
        [document.id as i64],
        |row| row.get(0),
    )?;
    if deleted {
        return Ok(false);
    }
    connection.execute("DELETE FROM pages WHERE rowid=?1", [document.id as i64])?;
    insert(connection, document)?;
    Ok(true)
}

pub fn verify(connection: &Connection, expected: usize) -> Result<()> {
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(format!("integrity check: {integrity}").into());
    }
    connection.execute("INSERT INTO pages(pages) VALUES('integrity-check')", [])?;
    let count: i64 = connection.query_row("SELECT count(*) FROM pages", [], |row| row.get(0))?;
    if count != expected as i64 {
        return Err(format!("expected {expected} documents, found {count}").into());
    }
    let forbidden: i64 = connection.query_row(
        "SELECT count(*) FROM pages WHERE CAST(captured AS INTEGER)<=?1 OR rowid IN (SELECT id FROM deleted)",
        [CUTOFF], |row| row.get(0))?;
    if forbidden != 0 {
        return Err("expired or deleted content returned".into());
    }
    let interrupted: i64 = connection.query_row(
        "SELECT count(*) FROM pages WHERE pages MATCH 'interruptedphantom'",
        [],
        |row| row.get(0),
    )?;
    // Check actual full-text lookups, including a retained document, after replay.
    for (query, expected_hits) in [("sigil1005", 0), ("sigil3", 0), ("sigil5", 1)] {
        let hits: i64 = connection.query_row(
            "SELECT count(*) FROM pages WHERE pages MATCH ?1",
            [query],
            |row| row.get(0),
        )?;
        if hits != expected_hits && expected != 0 {
            return Err(format!("unexpected search result for {query}: {hits}").into());
        }
    }
    if interrupted != 0 {
        return Err("interrupted text returned".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_boundary_and_explicit_delete_block_replay() -> Result<()> {
        let connection = open(Path::new(":memory:"))?;
        let mut boundary = crate::corpus::document(1);
        boundary.captured = CUTOFF;
        let mut live = crate::corpus::document(2);
        live.captured = CUTOFF + 1;
        insert(&connection, &boundary)?;
        insert(&connection, &live)?;
        assert_eq!(expire(&connection)?, 1);
        assert!(!replay(&connection, &boundary)?);
        assert!(replay(&connection, &live)?);
        connection.execute("INSERT INTO deleted VALUES(2)", [])?;
        connection.execute("DELETE FROM pages WHERE rowid=2", [])?;
        assert!(!replay(&connection, &live)?);
        verify(&connection, 0)
    }

    #[test]
    fn queries_preserve_accents_prefixes_and_multilingual_text() -> Result<()> {
        let connection = open(Path::new(":memory:"))?;
        let mut document = crate::corpus::document(5);
        document.body = "café naïve 星砂 сад copper meadow".to_owned();
        insert(&connection, &document)?;
        for query in ["cafe", "naive", "星砂", "сад", "cop*", "\"copper meadow\""] {
            let count: i64 = connection.query_row(
                "SELECT count(*) FROM pages WHERE pages MATCH ?1",
                [query],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1, "{query}");
        }
        connection.execute("DELETE FROM pages WHERE rowid=5", [])?;
        let count: i64 = connection.query_row(
            "SELECT count(*) FROM pages WHERE pages MATCH 'cafe'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 0);
        Ok(())
    }
}
