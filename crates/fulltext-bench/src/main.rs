//! Compares disposable synthetic full-text caches; no product crate uses this harness.

mod corpus;
mod engine;
mod recovery;
mod store;
mod tantivy_store;

use corpus::Corpus;
use engine::Engine;
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
        return recovery::writer(
            arguments.get(2).ok_or("missing crash engine")?,
            Path::new(arguments.get(3).ok_or("missing crash path")?),
        );
    }
    let directory = PathBuf::from(
        arguments
            .get(1)
            .ok_or("usage: fulltext-bench NEW_DIRECTORY [COUNT] [fts5|tantivy] [overlap|broad]")?,
    );
    let count = arguments
        .get(2)
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(100_000);
    if count < 2000 {
        return Err("use at least 2000 documents".into());
    }
    let engine = arguments.get(3).map(String::as_str).unwrap_or("fts5");
    if !matches!(engine, "fts5" | "tantivy") {
        return Err("engine must be fts5 or tantivy".into());
    }
    let corpus = match arguments.get(4).map(String::as_str).unwrap_or("overlap") {
        "overlap" => Corpus::Overlap,
        "broad" => Corpus::Broad,
        _ => return Err("corpus must be overlap or broad".into()),
    };
    fs::create_dir(&directory)?;
    run(&directory, count, engine, corpus)
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

fn run(directory: &Path, count: usize, name: &str, corpus: Corpus) -> Result<()> {
    let mut engine = Engine::open(name, directory)?;
    println!(
        "engine,{name}\ncorpus,{corpus:?}\nseed,{}\ncount,{count}",
        corpus::SEED
    );
    let documents: Vec<_> = (0..count).map(|id| corpus.document(id)).collect();
    println!(
        "corpus_body_bytes,{}",
        documents
            .iter()
            .map(|document| document.body.len())
            .sum::<usize>()
    );
    timed("bulk", || engine.batch(&documents, &[], false))?;
    println!("disk_after_bulk,{}", disk_size(directory)?);
    // The only consolidation in a run; later mutations rely on each engine's deletion path.
    timed("maintenance_once", || engine.optimize())?;
    println!("disk_after_maintenance,{}", disk_size(directory)?);
    queries(&engine)?;
    timed("add_1000", || {
        let documents: Vec<_> = (count..count + 1000)
            .map(|id| corpus.document(id))
            .collect();
        engine.batch(&documents, &[], false)
    })?;
    timed("update_1000", || {
        let documents: Vec<_> = (0..1000)
            .map(|id| {
                let mut document = corpus.document(id);
                document.body.push_str(" revisedfolio");
                document
            })
            .collect();
        engine.batch(&documents, &(0..1000).collect::<Vec<_>>(), false)
    })?;
    if engine.hits("revisedfolio")? != 1000 || engine.hits(&format!("sigil{count}"))? != 1 {
        return Err("incremental update/add verification failed".into());
    }
    timed("delete_1000", || {
        engine.batch(&[], &(1000..2000).collect::<Vec<_>>(), true)
    })?;
    let expired = timed("expiry", || engine.expire())?;
    println!(
        "expired_count,{expired}\ndisk_after_expiry,{}",
        disk_size(directory)?
    );
    let expected = (0..count + 1000)
        .filter(|id| !(1000..2000).contains(id) && (*id % 10) as i64 * 86_400 > corpus::CUTOFF)
        .count();
    engine.verify(expected)?;
    engine.close()?;
    timed("interrupted_writer_until_kill", || {
        recovery::interrupt(name, directory)
    })?;
    // Time reopening apart from each engine's own, differently shaped verification.
    let recovered = timed("reopen", || Engine::open(name, directory))?;
    timed("verify_after_reopen", || recovered.verify(expected))?;
    recovered.close()?;
    let mut recovered = Engine::open(name, directory)?;
    let stale: Vec<_> = (0..2000)
        .map(|id| corpus.document(id))
        .filter(|document| {
            document.captured <= corpus::CUTOFF || (1000..2000).contains(&document.id)
        })
        .collect();
    timed("stale_replay", || recovered.replay(&stale))?;
    timed("verify_after_replay", || recovered.verify(expected))?;
    recovered.close()?;
    let final_engine = Engine::open(name, directory)?;
    final_engine.verify(expected)?;
    final_engine.close()?;
    println!("recovery_and_replay,PASS\nfinal_count,{expected}");
    Ok(())
}

fn queries(engine: &Engine) -> Result<()> {
    // Measure parsing, ranking, top-20 retrieval and bounded snippets together.
    for query in corpus::QUERIES {
        let mut samples = Vec::new();
        let mut returned = 0usize;
        for _ in 0..101 {
            let start = Instant::now();
            returned += engine.query(query)?.len();
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        let first = samples.remove(0);
        samples.sort_by(f64::total_cmp);
        println!(
            "query,{query},{:.3},{:.3},{:.3},first={first:.3},rows={returned},hits={}",
            samples[49],
            samples[94],
            samples[99],
            engine.hits(query)?
        );
        // Ranked IDs, outside timing, let separate runs compare the engines' top 20.
        let top: Vec<_> = engine.query(query)?.iter().map(u64::to_string).collect();
        println!("top,{query},{}", top.join(";"));
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
