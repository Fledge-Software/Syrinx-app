//! Kills only its own disposable writer after uncommitted data reaches disk.

use crate::{Result, corpus, store};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
};

pub fn writer(engine: &str, directory: &Path) -> Result<()> {
    if engine == "tantivy" {
        let mut store = crate::tantivy_store::TantivyStore::open(directory)?;
        store.remove(5);
        for id in [3, 1005].into_iter().chain(1_000_000..1_000_064) {
            let mut document = corpus::document(id);
            document.body = "interruptedphantom ".repeat(8192);
            store.insert(&document)?;
        }
        return store.crash_prepare();
    }
    let path = &directory.join("synthetic-cache.db");
    let connection = store::open(path)?;
    connection.execute_batch("PRAGMA cache_size=8; BEGIN IMMEDIATE;")?;
    store::remove(&connection, 5)?;
    for id in [3, 1005].into_iter().chain(1_000_000..1_000_064) {
        let mut document = corpus::document(id);
        document.body = "interruptedphantom ".repeat(8192);
        store::insert(&connection, &document)?;
    }
    signal_and_wait()?;
    Err("writer was not killed by its parent".into())
}

pub fn signal_and_wait() -> Result<()> {
    println!("READY");
    std::io::stdout().flush()?;
    // Parent termination is the only successful exit from this test writer.
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(())
}

pub fn interrupt(engine: &str, directory: &Path) -> Result<()> {
    let before = crate::disk_size(directory)?;
    let mut child = Command::new(std::env::current_exe()?)
        .arg("crash-writer")
        .arg(engine)
        .arg(directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let output = child.stdout.take().ok_or("missing child output")?;
    let reader = std::thread::spawn(move || {
        let mut signal = String::new();
        BufReader::new(output).read_line(&mut signal)?;
        Ok::<_, std::io::Error>(signal)
    });
    let start = std::time::Instant::now();
    while !reader.is_finished() && start.elapsed().as_secs() < 60 {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let timed_out = !reader.is_finished();
    // Always reap this child, including timeout and malformed-signal paths.
    if child.try_wait()?.is_none() {
        child.kill()?;
    }
    let status = child.wait()?;
    let signal = reader.join().map_err(|_| "signal reader panicked")??;
    if timed_out {
        return Err("writer did not reach crash point within 60 seconds".into());
    }
    if signal.trim() != "READY" {
        return Err(format!("writer failed before crash point: {status}").into());
    }
    if status.success() {
        return Err("writer exited successfully instead of being killed".into());
    }
    if engine == "tantivy" {
        let after = crate::disk_size(directory)?;
        if after <= before {
            return Err("no prepared segment bytes at the crash point".into());
        }
        println!("crash_prepared_extra_bytes,{}", after - before);
        return Ok(());
    }
    let wal_bytes = std::fs::metadata(directory.join("synthetic-cache.db-wal"))?.len();
    if wal_bytes <= 32 {
        return Err("no WAL frames at the crash point".into());
    }
    println!("crash_wal_bytes,{wal_bytes}");
    Ok(())
}
