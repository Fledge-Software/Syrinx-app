//! Kills a real process inside `expire` and `apply_clears` and checks each is all or nothing.

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use syrinx_search::fulltext::{
    Change, ClearRequest, ClearScope, FullText, FullTextDocument, Namespace, NamespaceFilter,
    PAGE_RETENTION_SECONDS, SearchRequest,
};

/// Set only in a child this file starts, naming the index folder it works on.
const CHILD_FOLDER: &str = "SYRINX_FULLTEXT_MAINTENANCE_CHILD";
/// A fixed synthetic clock, far from any real capture.
const NOW: i64 = 1_900_000_000;
/// Pages per age; below the 1,000-hit search cap so a search can count them all.
const PAGES: usize = 900;
const NOTES: usize = 40;
/// How long after the child starts the operation it is killed. On this machine both took
/// 18 to 55 ms: zero landed before the commit, 15 ms between the commit and its cleanup, and
/// 60 ms or more after the end. The spread keeps all three likely on a faster or slower one.
const KILL_DELAYS_MS: [u64; 7] = [0, 3, 8, 15, 25, 40, 250];

/// The operation a child runs before waiting to be killed.
#[derive(Clone, Copy)]
enum Operation {
    Expire,
    Clear,
}

impl Operation {
    fn test_name(self) -> &'static str {
        match self {
            Self::Expire => "a_killed_expiry_is_all_or_nothing",
            Self::Clear => "a_killed_journal_clear_is_all_or_nothing",
        }
    }

    fn run(self, index: &mut FullText) {
        match self {
            Self::Expire => {
                index.expire(NOW).expect("expire synthetic pages");
            }
            Self::Clear => {
                index
                    .apply_clears(&[all_time_clear()])
                    .expect("apply synthetic clear");
            }
        }
    }
}

#[test]
fn a_killed_expiry_is_all_or_nothing() {
    run_child_if_asked(Operation::Expire);
    for delay in KILL_DELAYS_MS {
        let folder = synthetic_folder();
        // Old pages expire; recent pages and notes must survive wherever the kill lands.
        seed(&folder, NOW - PAGE_RETENTION_SECONDS - 60);
        let finished = kill_during(Operation::Expire, &folder, delay);

        let mut index = FullText::open(&folder).expect("reopen after the kill");
        let old = hits(&index, "oldsigil");
        assert!(
            old == 0 || old == PAGES,
            "expiry was partial: {old} of {PAGES} left"
        );
        assert!(!finished || old == 0, "a finished expiry was lost");
        assert_eq!(hits(&index, "recentsigil"), PAGES);
        assert_eq!(hits(&index, "notesigil"), NOTES);
        assert!(!index.cleanup_pending());

        // Running it again finishes the job and changes nothing else.
        index.expire(NOW).expect("expire after the kill");
        assert_eq!(hits(&index, "oldsigil"), 0);
        assert_eq!(hits(&index, "recentsigil"), PAGES);
        assert_eq!(hits(&index, "notesigil"), NOTES);
        drop(index);
        fs::remove_dir_all(&folder).expect("remove the synthetic index folder");
    }
}

#[test]
fn a_killed_journal_clear_is_all_or_nothing() {
    run_child_if_asked(Operation::Clear);
    for delay in KILL_DELAYS_MS {
        let folder = synthetic_folder();
        seed(&folder, NOW - 120);
        let finished = kill_during(Operation::Clear, &folder, delay);

        // The generation commits with the deletions, so the two always agree.
        let mut index = FullText::open(&folder).expect("reopen after the kill");
        let generation = index.cleared_generation();
        let pages = hits(&index, "oldsigil") + hits(&index, "recentsigil");
        match generation {
            0 => assert_eq!(pages, 2 * PAGES, "an unapplied clear removed pages"),
            1 => assert_eq!(pages, 0, "an applied clear left pages"),
            other => panic!("unexpected generation {other}"),
        }
        assert!(!finished || generation == 1, "a finished clear was lost");
        assert_eq!(hits(&index, "notesigil"), NOTES);
        assert!(!index.cleanup_pending());

        // Replaying the journal applies the clear exactly once.
        let replayed = index
            .apply_clears(&[all_time_clear()])
            .expect("replay the clear");
        assert_eq!(replayed, usize::from(generation == 0));
        assert_eq!(hits(&index, "oldsigil") + hits(&index, "recentsigil"), 0);
        assert_eq!(hits(&index, "notesigil"), NOTES);
        assert_eq!(index.cleared_generation(), 1);
        drop(index);
        fs::remove_dir_all(&folder).expect("remove the synthetic index folder");
    }
}

fn all_time_clear() -> ClearRequest {
    ClearRequest {
        generation: 1,
        scope: ClearScope::Pages {
            start: 0,
            end: i64::MAX,
        },
    }
}

/// In a child, runs the operation, reports, then waits to be killed.
fn run_child_if_asked(operation: Operation) {
    let Some(folder) = std::env::var_os(CHILD_FOLDER) else {
        return;
    };
    let mut index = FullText::open(Path::new(&folder)).expect("open the synthetic index");
    let mut stdout = std::io::stdout();
    report(&mut stdout, "BEGIN");
    operation.run(&mut index);
    report(&mut stdout, "DONE");
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

fn report(stdout: &mut std::io::Stdout, line: &str) {
    writeln!(stdout, "{line}").expect("report to the parent");
    stdout.flush().expect("flush the report");
}

/// Commits old and recent pages in four segments' worth of batches, then notes dated `old`.
fn seed(folder: &Path, old: i64) {
    let mut index = FullText::open(folder).expect("open the synthetic index");
    for batch in 0..4 {
        let changes: Vec<_> = (batch..PAGES)
            .step_by(4)
            .flat_map(|number| {
                [
                    page(format!("old-{number}"), old, "oldsigil"),
                    page(format!("recent-{number}"), NOW - 60, "recentsigil"),
                ]
            })
            .collect();
        // Staged as of the old capture, so the old pages are not refused as expired.
        index.stage(&changes, old).expect("stage synthetic pages");
        index.commit().expect("commit synthetic pages");
    }
    let notes: Vec<_> = (0..NOTES)
        .map(|number| {
            Change::Upsert(FullTextDocument {
                namespace: Namespace::feed("notes").expect("valid feed name"),
                id: format!("note-{number}"),
                revision: 1,
                captured_at: old,
                title: format!("Synthetic note {number}"),
                body: "notesigil".to_owned(),
            })
        })
        .collect();
    index.stage(&notes, old).expect("stage synthetic notes");
    index.commit().expect("commit synthetic notes");
}

fn page(id: String, captured_at: i64, sigil: &str) -> Change {
    Change::Upsert(FullTextDocument {
        namespace: Namespace::page(),
        id,
        revision: 1,
        captured_at,
        title: "Synthetic folio".to_owned(),
        // Long bodies make the merge that removes deleted rows take measurable time.
        body: format!("{sigil} {}", "filler words ".repeat(400)),
    })
}

/// Starts the child and kills it `delay` ms after it reports starting; true if it finished.
fn kill_during(operation: Operation, folder: &Path, delay: u64) -> bool {
    let mut child = Command::new(std::env::current_exe().expect("find this test binary"))
        .args([
            operation.test_name(),
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_FOLDER, folder)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the synthetic child");
    let reports = read_reports(&mut child);
    await_report(&mut child, &reports, "BEGIN");
    std::thread::sleep(Duration::from_millis(delay));
    // TerminateProcess stops the child wherever it is, as a crash or power cut would.
    child.kill().expect("kill the synthetic child");
    let status = child.wait().expect("reap the synthetic child");
    assert!(!status.success(), "the child exited on its own");
    reports.try_iter().any(|line| line.ends_with("DONE"))
}

fn read_reports(child: &mut Child) -> mpsc::Receiver<String> {
    let output = child.stdout.take().expect("child output");
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else { break };
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    receiver
}

/// Waits up to two minutes for `expected`; kills the child before failing.
fn await_report(child: &mut Child, reports: &mpsc::Receiver<String>, expected: &str) {
    loop {
        match reports.recv_timeout(Duration::from_secs(120)) {
            // libtest prints the test's name on the same line before the first report.
            Ok(line) if line.ends_with(expected) => return,
            Ok(_) => continue,
            Err(error) => {
                let killed = child.kill();
                panic!("the child never reported {expected}: {error:?}, kill: {killed:?}");
            }
        }
    }
}

fn hits(index: &FullText, word: &str) -> usize {
    let request = SearchRequest {
        text: word.to_owned(),
        prefix_last: false,
        namespaces: NamespaceFilter::All,
        limit: 1_000,
    };
    index
        .search(&request)
        .expect("search the synthetic index")
        .len()
}

fn synthetic_folder() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("synthetic clock follows epoch")
        .as_nanos();
    let folder = std::env::temp_dir().join(format!(
        "syrinx-fulltext-maintenance-crash-{}-{stamp}",
        std::process::id()
    ));
    // Exclusive creation proves this test owns what it later removes.
    fs::create_dir(&folder).expect("create the synthetic index folder");
    folder
}
