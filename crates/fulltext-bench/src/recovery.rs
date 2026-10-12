//! Kills only its own disposable writer after SQLite has spilled an uncommitted write.

use crate::{Result, corpus, store};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
};

pub fn writer(path: &Path) -> Result<()> {
    let connection = store::open(path)?;
    connection
        .execute_batch("PRAGMA cache_size=8; BEGIN IMMEDIATE; DELETE FROM pages WHERE rowid=5;")?;
    let next: i64 = connection.query_row("SELECT max(rowid)+1 FROM pages", [], |row| row.get(0))?;
    for id in [3, 1005]
        .into_iter()
        .chain(next as usize..next as usize + 64)
    {
        let mut document = corpus::document(id);
        document.body = "interruptedphantom ".repeat(8192);
        store::insert(&connection, &document)?;
    }
    println!("READY");
    std::io::stdout().flush()?;
    // Parent termination is the only successful exit from this test writer.
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Err("writer was not killed by its parent".into())
}

pub fn interrupt(path: &Path) -> Result<()> {
    let mut child = Command::new(std::env::current_exe()?)
        .arg("crash-writer")
        .arg(path)
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
    let wal_bytes = std::fs::metadata(path.with_extension("db-wal"))?.len();
    if wal_bytes <= 32 {
        return Err("no WAL frames at the crash point".into());
    }
    println!("crash_wal_bytes,{wal_bytes}");
    Ok(())
}
