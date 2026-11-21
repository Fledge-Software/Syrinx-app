//! Kills only its own disposable writer after uncommitted data reaches disk.

use crate::{Result, corpus, store};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use syrinx_search::fulltext::Outcome;

pub fn writer(engine: &str, directory: &Path) -> Result<()> {
    if engine == "adapter" {
        return adapter_writer(directory);
    }
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

/// Phantom pages the adapter's writer stages: enough distinct words to pass its 32 MB
/// indexing budget, so Tantivy finishes an uncommitted segment on disk before the kill.
const ADAPTER_PHANTOMS: usize = 256;
/// How long the writer waits for that segment; the parent gives up at 60 seconds.
const SEGMENT_DEADLINE: Duration = Duration::from_secs(45);

/// Stages interrupted work through the production adapter, which commits nothing.
///
/// Like the tantivy engine's writer it deletes 5, overwrites 3 and resurrects deleted 1005.
/// It cannot prepare a commit from outside, so it waits until the budget flush has finished
/// an orphaned segment for the reopen to collect.
fn adapter_writer(directory: &Path) -> Result<()> {
    let mut store = crate::adapter_store::AdapterStore::open(directory)?;
    // The run committed 3 and 5 at revision 2 (`update_1000`) and deleted 1005 at revision 1.
    store.assume_committed(3, 2);
    store.assume_committed(5, 2);
    store.assume_committed(1005, 1);
    let phantoms: Vec<_> = [3, 1005]
        .into_iter()
        .chain(1_000_000..1_000_000 + ADAPTER_PHANTOMS)
        .map(|id| {
            let mut document = corpus::document(id);
            document.body = (0..8192)
                .map(|word| format!("interruptedphantom phantom{id}x{word} "))
                .collect();
            document
        })
        .collect();
    let mut outcomes = store.stage(&[], &[5])?;
    outcomes.extend(store.stage(&phantoms, &[])?);
    if outcomes.iter().any(|outcome| *outcome != Outcome::Staged) {
        return Err("the crash writer's changes were refused, so they would test nothing".into());
    }
    wait_for_orphaned_segment(directory)?;
    signal_and_wait()?;
    Err("writer was not killed by its parent".into())
}

/// Waits until a segment that `meta.json` does not list has a non-empty term dictionary,
/// which Tantivy writes only when it finishes the segment.
fn wait_for_orphaned_segment(directory: &Path) -> Result<()> {
    let start = Instant::now();
    while start.elapsed() < SEGMENT_DEADLINE {
        if has_orphaned_segment(directory)? {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err("the adapter's writer finished no uncommitted segment".into())
}

fn has_orphaned_segment(directory: &Path) -> Result<bool> {
    // `meta.json` spells segment ids with hyphens; file names leave them out.
    let committed = std::fs::read_to_string(directory.join("meta.json"))?.replace('-', "");
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(stem) = name
            .to_string_lossy()
            .strip_suffix(".term")
            .map(str::to_owned)
        else {
            continue;
        };
        if !committed.contains(&stem) && entry.metadata()?.len() > 0 {
            return Ok(true);
        }
    }
    Ok(false)
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
    if engine == "tantivy" || engine == "adapter" {
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
