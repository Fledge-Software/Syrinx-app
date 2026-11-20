//! Kills a real writer process mid-run and checks the index holds only whole commits.

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use syrinx_search::fulltext::{
    Change, Deletion, FullText, FullTextDocument, Namespace, NamespaceFilter, SearchRequest,
};

const TEST_NAME: &str = "a_killed_writer_leaves_only_whole_committed_batches";
/// Set only in the child this test starts, naming the index folder it writes.
const CHILD_FOLDER: &str = "SYRINX_FULLTEXT_CRASH_CHILD";
const DOCUMENTS_PER_BATCH: usize = 200;
const BATCHES_BEFORE_KILL: u64 = 3;
const NOW: i64 = 1_900_000_000;

#[test]
fn a_killed_writer_leaves_only_whole_committed_batches() {
    if let Some(folder) = std::env::var_os(CHILD_FOLDER) {
        write_until_killed(Path::new(&folder));
    }
    let folder = synthetic_folder();
    let mut child = spawn_writer(&folder);
    let committed = wait_for_commits(&mut child);
    // TerminateProcess stops the writer wherever it is, as a crash or power cut would.
    child.kill().expect("kill the synthetic writer");
    let status = child.wait().expect("reap the synthetic writer");
    assert!(!status.success(), "the writer exited on its own");
    assert!(committed >= BATCHES_BEFORE_KILL);

    let mut index = FullText::open(&folder).expect("reopen after the kill");
    let last = last_whole_batch(&index);
    assert!(last >= committed, "a reported commit was lost");
    for batch in 1..=last {
        let expected = if batch == last {
            DOCUMENTS_PER_BATCH
        } else {
            DOCUMENTS_PER_BATCH / 2
        };
        assert_eq!(hits(&index, batch), expected, "batch {batch} is not whole");
    }
    assert_eq!(hits(&index, last + 1), 0);

    // The killed writer's lock does not outlive it.
    index
        .stage(&batch_changes(last + 1), NOW)
        .expect("stage after the kill");
    index.commit().expect("commit after the kill");
    drop(index);
    fs::remove_dir_all(&folder).expect("remove the synthetic index folder");
}

/// The child: commits numbered batches, each deleting half the previous one, forever.
fn write_until_killed(folder: &Path) -> ! {
    let mut index = FullText::open(folder).expect("open the synthetic index");
    let mut stdout = std::io::stdout();
    for batch in 1.. {
        index
            .stage(&batch_changes(batch), NOW)
            .expect("stage a synthetic batch");
        index.commit().expect("commit a synthetic batch");
        writeln!(stdout, "COMMITTED {batch}").expect("report a commit");
        stdout.flush().expect("flush the report");
    }
    unreachable!("the batch counter is unbounded")
}

/// Batch `n` adds its own documents and deletes the odd half of batch `n - 1`.
fn batch_changes(batch: u64) -> Vec<Change> {
    let mut changes: Vec<_> = (0..DOCUMENTS_PER_BATCH)
        .map(|number| {
            Change::Upsert(FullTextDocument {
                namespace: Namespace::page(),
                id: format!("{batch}-{number}"),
                revision: 1,
                captured_at: NOW,
                title: format!("Synthetic folio {number}"),
                // Long bodies keep each commit busy long enough to be interrupted.
                body: format!("batchmark{batch} {}", "filler words ".repeat(400)),
            })
        })
        .collect();
    if batch > 1 {
        changes.extend((1..DOCUMENTS_PER_BATCH).step_by(2).map(|number| {
            Change::Delete(Deletion {
                namespace: Namespace::page(),
                id: format!("{}-{number}", batch - 1),
                revision: 1,
                deleted_at: NOW,
            })
        }));
    }
    changes
}

fn hits(index: &FullText, batch: u64) -> usize {
    let request = SearchRequest {
        text: format!("batchmark{batch}"),
        prefix_last: false,
        namespaces: NamespaceFilter::All,
        limit: DOCUMENTS_PER_BATCH * 2,
    };
    index.search(&request).expect("search a batch").len()
}

fn last_whole_batch(index: &FullText) -> u64 {
    (1..)
        .take_while(|batch| hits(index, *batch) > 0)
        .last()
        .unwrap_or(0)
}

fn spawn_writer(folder: &Path) -> Child {
    Command::new(std::env::current_exe().expect("find this test binary"))
        .args([TEST_NAME, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD_FOLDER, folder)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the synthetic writer")
}

/// Reads the child's reports until enough batches committed; panics after two minutes.
fn wait_for_commits(child: &mut Child) -> u64 {
    let output = child.stdout.take().expect("child output");
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else { break };
            if let Some(batch) = line.strip_prefix("COMMITTED ")
                && sender.send(batch.parse::<u64>().ok()).is_err()
            {
                break;
            }
        }
    });
    loop {
        match receiver.recv_timeout(Duration::from_secs(120)) {
            Ok(Some(batch)) if batch >= BATCHES_BEFORE_KILL => return batch,
            Ok(Some(_)) => continue,
            other => {
                // Reap the child before failing, so no writer outlives the test.
                let killed = child.kill();
                panic!("the writer stopped reporting commits: {other:?}, kill: {killed:?}");
            }
        }
    }
}

fn synthetic_folder() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("synthetic clock follows epoch")
        .as_nanos();
    let folder = std::env::temp_dir().join(format!(
        "syrinx-fulltext-crash-{}-{stamp}",
        std::process::id()
    ));
    // Exclusive creation proves this test owns what it later removes.
    fs::create_dir(&folder).expect("create the synthetic index folder");
    folder
}
