//! Measures an isolated synthetic FTS5 cache; no product crate uses this harness.

mod corpus;
mod recovery;
mod store;

use rusqlite::Connection;
use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments
        .get(1)
        .is_some_and(|argument| argument == "crash-writer")
    {
        return recovery::writer(Path::new(arguments.get(2).ok_or("missing crash path")?));
    }
    let directory = PathBuf::from(
        arguments
            .get(1)
            .ok_or("usage: fulltext-bench NEW_DIRECTORY [COUNT]")?,
    );
    let count = arguments
        .get(2)
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(100_000);
    if count < 2000 {
        return Err("use at least 2000 documents".into());
    }
    fs::create_dir(&directory)?;
    run(&directory, count)
}

fn timed<T>(name: &str, action: impl FnOnce() -> Result<T>) -> Result<T> {
    let start = Instant::now();
    let result = action()?;
    println!(
        "operation,{name},{:.3}",
        start.elapsed().as_secs_f64() * 1000.0
    );
    Ok(result)
}

fn run(directory: &Path, count: usize) -> Result<()> {
    let path = directory.join("synthetic-cache.db");
    let mut connection = store::open(&path)?;
    let version: String = connection.query_row("SELECT sqlite_version()", [], |row| row.get(0))?;
    if version != "3.53.4" {
        return Err(format!("unexpected SQLite {version}").into());
    }
    let fts5: bool = connection.query_row(
        "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
        [],
        |row| row.get(0),
    )?;
    if !fts5 {
        return Err("FTS5 compile option missing".into());
    }
    println!(
        "engine,sqlite,{version}\nseed,{}\ncount,{count}",
        corpus::SEED
    );
    let documents: Vec<_> = (0..count).map(corpus::document).collect();
    println!(
        "corpus_body_bytes,{}",
        documents
            .iter()
            .map(|document| document.body.len())
            .sum::<usize>()
    );
    timed("bulk", || {
        let transaction = connection.transaction()?;
        for document in &documents {
            store::insert(&transaction, document)?;
        }
        transaction.commit()?;
        store::maintain(&connection)
    })?;
    println!("disk_after_bulk,{}", disk_size(directory)?);
    queries(&connection)?;
    timed("add_1000", || {
        let transaction = connection.transaction()?;
        for id in count..count + 1000 {
            store::insert(&transaction, &corpus::document(id))?;
        }
        transaction.commit()?;
        store::maintain(&connection)
    })?;
    timed("update_1000", || {
        let transaction = connection.transaction()?;
        for id in 0..1000 {
            transaction.execute("DELETE FROM pages WHERE rowid=?1", [id as i64])?;
            let mut document = corpus::document(id);
            document.body.push_str(" revisedfolio");
            store::insert(&transaction, &document)?;
        }
        transaction.commit()?;
        store::maintain(&connection)
    })?;
    timed("delete_1000", || {
        let transaction = connection.transaction()?;
        for id in 1000..2000 {
            transaction.execute("INSERT INTO deleted VALUES(?1)", [id])?;
            transaction.execute("DELETE FROM pages WHERE rowid=?1", [id])?;
        }
        transaction.commit()?;
        store::maintain(&connection)
    })?;
    let expired = timed("expiry_and_maintenance", || {
        let expired = store::expire(&connection)?;
        store::maintain(&connection)?;
        Ok(expired)
    })?;
    println!(
        "expired_count,{expired}\ndisk_after_expiry,{}",
        disk_size(directory)?
    );
    let expected = (0..count + 1000)
        .filter(|id| !(1000..2000).contains(id) && corpus::document(*id).captured > corpus::CUTOFF)
        .count();
    store::verify(&connection, expected)?;
    connection.close().map_err(|(_, error)| error)?;
    timed("interrupted_writer_until_kill", || {
        recovery::interrupt(&path)
    })?;
    timed("reopen_and_verify", || {
        let recovered = store::open(&path)?;
        store::verify(&recovered, expected)?;
        recovered.close().map_err(|(_, error)| error)?;
        Ok(())
    })?;
    let mut recovered = store::open(&path)?;
    timed("stale_replay_and_verify", || {
        let transaction = recovered.transaction()?;
        for id in 0..2000 {
            let document = corpus::document(id);
            if (document.captured <= corpus::CUTOFF || (1000..2000).contains(&id))
                && store::replay(&transaction, &document)?
            {
                return Err("stale replay accepted".into());
            }
        }
        transaction.commit()?;
        store::maintain(&recovered)?;
        store::verify(&recovered, expected)
    })?;
    recovered.close().map_err(|(_, error)| error)?;
    let final_connection = store::open(&path)?;
    store::verify(&final_connection, expected)?;
    println!("recovery_and_replay,PASS\nfinal_count,{expected}");
    Ok(())
}

fn queries(connection: &Connection) -> Result<()> {
    // Measure preparation, ranking, top-20 retrieval and bounded snippets together.
    for query in corpus::QUERIES {
        let mut samples = Vec::new();
        let mut returned = 0usize;
        for _ in 0..101 {
            let start = Instant::now();
            let mut statement = connection.prepare("SELECT rowid,snippet(pages,2,'[',']','...',16) FROM pages WHERE pages MATCH ?1 ORDER BY rank LIMIT 20")?;
            let rows = statement.query_map([query], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                std::hint::black_box(row?);
                returned += 1;
            }
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        let cold = samples.remove(0);
        samples.sort_by(f64::total_cmp);
        println!(
            "query,{query},{:.3},{:.3},{:.3},first={cold:.3},rows={returned}",
            samples[49], samples[94], samples[99]
        );
    }
    Ok(())
}

fn disk_size(directory: &Path) -> Result<u64> {
    let mut total = 0;
    for entry in fs::read_dir(directory)? {
        total += entry?.metadata()?.len();
    }
    Ok(total)
}
