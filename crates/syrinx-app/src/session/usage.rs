//! Keeps the usage counts in `usage.json` beside the database, only while counting is on (AN-01).
//!
//! The file existing is what "on" means, unless the off marker says a switch-off is unfinished.

use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use syrinx_core::usage::{
    ErrorCategory, Feature, MAX_FILE_BYTES, UsageDate, UsageFile, UsageRecorder,
};

/// The profile folder holding the database, the deletion journal and the usage file.
pub const STATE_FOLDER: &str = "State";
/// Beside `syrinx.db`, so D13's synced-folder guard covers it; backups copy only the database.
const FILE_NAME: &str = "usage.json";
/// Each write goes here first, then replaces the file in one rename.
const REPLACEMENT_NAME: &str = "usage.json.new";
/// An unreadable file is moved here, replacing any earlier one, and counting starts afresh.
const DAMAGED_NAME: &str = "usage.json.damaged";
/// Written before switching off deletes anything, and removed last: while it exists, counting
/// stays off and the next start finishes the deletion.
const OFF_NAME: &str = "usage.off";
/// The counts are written at most this often while in use, and once at a clean exit.
pub(super) const WRITE_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Where a profile keeps its usage file; the smoke run checks that it never appears.
pub fn usage_file(profile: &Path) -> PathBuf {
    profile.join(STATE_FOLDER).join(FILE_NAME)
}

/// What the owner switches in Customize's Usage report section.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageControl {
    TurnOn,
    /// Stops counting and deletes the counts; the window asks first.
    TurnOff,
    /// Clears both periods and starts a new one today; the window asks first.
    Reset,
}

/// Whether counting is on, and since which day, for the Usage report section.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UsageStatus {
    pub counting: bool,
    pub since: Option<UsageDate>,
}

/// A usage file failure: a fixed category, never a path or the file's contents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageError {
    Read,
    Write,
    /// Counting is off, but some counts could not be deleted yet; the next start retries.
    Delete,
    /// Neither the off marker nor the deletion succeeded, so nothing on disk records the
    /// switch-off; the next start may count again.
    NotStopped,
    /// The file was unreadable, so it was set aside and counting started afresh.
    Damaged,
    /// Today's date is unknown, so a period cannot start or close.
    Clock,
}

impl std::fmt::Display for UsageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Read => "Syrinx couldn't read its usage counts, so it isn't counting this time",
            Self::Write => "Syrinx couldn't save its usage counts",
            Self::Delete => {
                "Counting is off, but Syrinx couldn't delete all its usage counts yet; it will \
                 finish when it next starts"
            }
            Self::NotStopped => {
                "Counting is off for now, but Syrinx couldn't delete its usage counts or record \
                 the switch-off, so it may count again when it next starts; switch it off again"
            }
            Self::Damaged => "Syrinx's usage counts were unreadable, so it started them afresh",
            Self::Clock => "Syrinx couldn't read today's date for its usage counts",
        })
    }
}

/// The session worker's side of usage counting: loading, periodic writes and the controls.
pub(super) struct UsageCollector {
    recorder: Arc<UsageRecorder>,
    directory: PathBuf,
    /// `None` while counting is off.
    file: Option<UsageFile>,
    last_write: Instant,
    /// The in-memory file has changes the disk does not.
    unwritten: bool,
}

impl UsageCollector {
    /// Reads `usage.json` if it exists and starts today's counting; without it, stays off.
    pub(super) fn load(
        recorder: Arc<UsageRecorder>,
        directory: &Path,
        today: Option<UsageDate>,
        now: Instant,
    ) -> (Self, Option<UsageError>) {
        recorder.switch_off();
        let mut collector = Self {
            recorder,
            directory: directory.to_path_buf(),
            file: None,
            last_write: now,
            unwritten: false,
        };
        let problem = collector.open(today, now).err();
        (collector, problem)
    }

    fn open(&mut self, today: Option<UsageDate>, now: Instant) -> Result<(), UsageError> {
        // An unfinished switch-off is finished, never undone.
        if self.path(OFF_NAME).exists() {
            return self.finish_off();
        }
        let Some(bytes) = read_bounded(&self.path(FILE_NAME)).map_err(|_| UsageError::Read)? else {
            // Off: copies a failed switch-off or reset could not delete go now.
            return remove_leftovers(&self.directory);
        };
        let decoded = UsageFile::decode(&bytes);
        match (decoded, today) {
            (Ok(mut file), Some(today)) => {
                file.start_day(today);
                self.start(file);
                self.recorder.count(Feature::AppStarted);
                Ok(())
            }
            // Without a date the file keeps counting as it is: no new day, no rotation.
            (Ok(file), None) => {
                self.start(file);
                self.recorder.count(Feature::AppStarted);
                Err(UsageError::Clock)
            }
            (Err(_), today) => self.start_afresh(today, now),
        }
    }

    /// Moves a damaged file aside and, with a date, saves a new one at once so counting
    /// stays on; without one, nothing remains that could say counting is on.
    fn start_afresh(&mut self, today: Option<UsageDate>, now: Instant) -> Result<(), UsageError> {
        fs::rename(self.path(FILE_NAME), self.path(DAMAGED_NAME)).map_err(|_| UsageError::Read)?;
        let Some(today) = today else {
            return Err(UsageError::Clock);
        };
        self.start(UsageFile::begin(today));
        self.recorder.count(Feature::AppStarted);
        self.recorder.error(ErrorCategory::UsageFileReset);
        self.write(now)?;
        Err(UsageError::Damaged)
    }

    fn start(&mut self, file: UsageFile) {
        self.file = Some(file);
        self.unwritten = true;
        self.recorder.switch_on();
    }

    pub(super) fn status(&self) -> UsageStatus {
        UsageStatus {
            counting: self.file.is_some(),
            since: self.file.as_ref().map(|file| file.current.start),
        }
    }

    /// Whether a write is waiting; nothing is waiting until a counter changes.
    pub(super) fn pending(&self) -> bool {
        self.file.is_some() && (self.unwritten || self.recorder.has_changes())
    }

    /// Writes when something changed and the last write was five or more minutes ago.
    pub(super) fn write_if_due(&mut self, now: Instant) -> Result<(), UsageError> {
        if !self.pending() || now.saturating_duration_since(self.last_write) < WRITE_INTERVAL {
            return Ok(());
        }
        self.write(now)
    }

    /// The clean-exit write of anything not yet saved.
    pub(super) fn finish(&mut self, now: Instant) -> Result<(), UsageError> {
        if !self.pending() {
            return Ok(());
        }
        self.write(now)
    }

    /// Applies a control; the status is the real one afterwards, even when it failed.
    pub(super) fn control(
        &mut self,
        control: UsageControl,
        today: Option<UsageDate>,
        now: Instant,
    ) -> (UsageStatus, Option<UsageError>) {
        let result = match control {
            UsageControl::TurnOn if self.file.is_none() => self.begin(today, now),
            UsageControl::TurnOff => self.turn_off(),
            UsageControl::Reset if self.file.is_some() => self.begin(today, now),
            UsageControl::TurnOn | UsageControl::Reset => Ok(()),
        };
        (self.status(), result.err())
    }

    /// Starts one empty period today and saves it, which is what turns counting on.
    fn begin(&mut self, today: Option<UsageDate>, now: Instant) -> Result<(), UsageError> {
        let today = today.ok_or(UsageError::Clock)?;
        // Reset deletes both periods and counts set aside from a damaged file; a leftover
        // switch-off is finished first, so nothing earlier survives into the new period.
        self.recorder.switch_off();
        self.file = None;
        if let Err(error) = self.finish_off() {
            // Old counts that cannot be deleted must not come back as "on": stay off durably.
            self.turn_off()?;
            return Err(error);
        }
        self.start(UsageFile::begin(today));
        if let Err(error) = self.write(now) {
            // Without a file, counting cannot be on; leave nothing half-made.
            self.turn_off()?;
            return Err(error);
        }
        Ok(())
    }

    /// Stops counting at once, marks the switch-off durably, then deletes every usage file.
    fn turn_off(&mut self) -> Result<(), UsageError> {
        self.recorder.switch_off();
        self.file = None;
        self.unwritten = false;
        let marked = write_durably(&self.path(OFF_NAME), b"");
        let deleted = self.finish_off();
        // Renaming the file aside is not tried: on Windows a rename needs the same delete
        // access that the failed deletion lacked.
        match (marked, deleted) {
            (_, Ok(())) => Ok(()),
            // The marker keeps the next start off while the deletion waits.
            (Ok(()), Err(error)) => Err(error),
            (Err(_), Err(_)) => Err(UsageError::NotStopped),
        }
    }

    /// Deletes the usage files, then the off marker last.
    fn finish_off(&mut self) -> Result<(), UsageError> {
        for name in [FILE_NAME, REPLACEMENT_NAME, DAMAGED_NAME, OFF_NAME] {
            remove_if_present(&self.path(name))?;
        }
        Ok(())
    }

    /// Merges the recorder's counts and replaces the file through a temporary copy.
    fn write(&mut self, now: Instant) -> Result<(), UsageError> {
        let Some(file) = &mut self.file else {
            return Ok(());
        };
        if let Some(counts) = self.recorder.take() {
            file.add(&counts);
        }
        let text = file.encode();
        // A failure is retried at the next interval, not on every command.
        self.last_write = now;
        self.unwritten = true;
        let replacement = self.path(REPLACEMENT_NAME);
        write_durably(&replacement, text.as_bytes()).map_err(|_| UsageError::Write)?;
        fs::rename(replacement, self.path(FILE_NAME)).map_err(|_| UsageError::Write)?;
        self.unwritten = false;
        Ok(())
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }
}

/// The file's bytes, `None` when it does not exist; reading stops one byte past the limit.
fn read_bounded(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

/// Creates or truncates the file and flushes it to disk.
fn write_durably(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Deletes a replacement or damaged copy left beside an absent usage file.
fn remove_leftovers(directory: &Path) -> Result<(), UsageError> {
    for name in [REPLACEMENT_NAME, DAMAGED_NAME] {
        remove_if_present(&directory.join(name))?;
    }
    Ok(())
}

fn remove_if_present(path: &Path) -> Result<(), UsageError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(UsageError::Delete),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        os::windows::fs::OpenOptionsExt,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    /// An exclusively created folder, so the test owns everything it removes.
    struct Folder(PathBuf);

    impl Folder {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("synthetic clock follows epoch")
                .as_nanos();
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "syrinx-usage-test-{}-{stamp}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("create test-owned folder");
            Self(path)
        }

        fn has(&self, name: &str) -> bool {
            self.0.join(name).exists()
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            for name in [FILE_NAME, REPLACEMENT_NAME, DAMAGED_NAME, OFF_NAME] {
                if let Err(error) = remove_if_present(&self.0.join(name)) {
                    eprintln!("synthetic usage file left behind: {error}");
                }
            }
            if let Err(error) = fs::remove_dir(&self.0) {
                eprintln!("synthetic usage folder left behind: {:?}", error.kind());
            }
        }
    }

    fn today() -> UsageDate {
        UsageDate::from_ymd(2026, 10, 14).expect("synthetic date")
    }

    fn modified(folder: &Folder) -> std::time::SystemTime {
        fs::metadata(folder.0.join(FILE_NAME))
            .and_then(|metadata| metadata.modified())
            .expect("the usage file exists")
    }

    /// A collector on `folder` with counting switched on today.
    fn counting(folder: &Folder, recorder: &Arc<UsageRecorder>) -> UsageCollector {
        let start = Instant::now();
        let (mut collector, _) =
            UsageCollector::load(recorder.clone(), &folder.0, Some(today()), start);
        let (status, problem) = collector.control(UsageControl::TurnOn, Some(today()), start);
        assert_eq!(problem, None);
        assert_eq!(status.since, Some(today()));
        collector
    }

    #[test]
    fn without_a_file_nothing_is_counted_or_armed() {
        let folder = Folder::new();
        let recorder = Arc::new(UsageRecorder::new());
        let start = Instant::now();
        let (mut collector, problem) =
            UsageCollector::load(recorder.clone(), &folder.0, Some(today()), start);
        assert_eq!(problem, None);
        recorder.count(Feature::TabCreated);
        assert!(!collector.pending(), "no write is armed while off");
        let later = start + WRITE_INTERVAL * 3;
        assert_eq!(collector.write_if_due(later), Ok(()));
        assert_eq!(collector.finish(later), Ok(()));
        assert!(!folder.has(FILE_NAME));
    }

    #[test]
    fn writes_wait_for_a_change_and_five_minutes() {
        let folder = Folder::new();
        let recorder = Arc::new(UsageRecorder::new());
        let start = Instant::now();
        let mut collector = counting(&folder, &recorder);
        let first = modified(&folder);

        // An unchanged recorder arms nothing, however long Syrinx stays idle.
        assert!(!collector.pending());
        assert_eq!(collector.write_if_due(start + WRITE_INTERVAL * 10), Ok(()));
        assert_eq!(modified(&folder), first);

        // A change waits for the interval, then is written once.
        recorder.count(Feature::TabCreated);
        assert!(collector.pending());
        assert_eq!(collector.write_if_due(start + WRITE_INTERVAL / 2), Ok(()));
        assert!(collector.pending(), "not yet due");
        assert_eq!(collector.write_if_due(start + WRITE_INTERVAL * 11), Ok(()));
        assert!(!collector.pending());
        let bytes = fs::read(folder.0.join(FILE_NAME)).expect("read synthetic usage file");
        let file = UsageFile::decode(&bytes).expect("valid usage file");
        assert_eq!(file.current.counts.feature(Feature::TabCreated), 1);
        assert!(!folder.has(REPLACEMENT_NAME));
    }

    #[test]
    fn switching_on_without_a_date_changes_nothing() {
        let folder = Folder::new();
        let recorder = Arc::new(UsageRecorder::new());
        let start = Instant::now();
        let (mut collector, _) = UsageCollector::load(recorder.clone(), &folder.0, None, start);
        let (status, problem) = collector.control(UsageControl::TurnOn, None, start);
        assert!(!status.counting);
        assert_eq!(problem, Some(UsageError::Clock));
        assert!(!recorder.is_enabled());
        assert!(!folder.has(FILE_NAME));
    }

    #[test]
    fn an_existing_file_keeps_counting_when_the_date_is_unknown() {
        let folder = Folder::new();
        let recorder = Arc::new(UsageRecorder::new());
        drop(counting(&folder, &recorder));
        let (collector, problem) =
            UsageCollector::load(recorder.clone(), &folder.0, None, Instant::now());
        assert_eq!(problem, Some(UsageError::Clock));
        assert!(collector.status().counting, "the file still says on");
        assert!(recorder.is_enabled());
    }

    #[test]
    fn a_switch_off_that_cannot_delete_stays_off_and_finishes_at_the_next_start() {
        let folder = Folder::new();
        let recorder = Arc::new(UsageRecorder::new());
        let mut collector = counting(&folder, &recorder);

        // Another program holds the file open without delete sharing.
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(folder.0.join(FILE_NAME))
            .expect("hold the synthetic usage file");
        let (status, problem) = collector.control(UsageControl::TurnOff, None, Instant::now());
        assert!(
            !status.counting,
            "counting stops even though deletion failed"
        );
        assert_eq!(problem, Some(UsageError::Delete));
        assert!(!recorder.is_enabled());
        assert!(folder.has(FILE_NAME) && folder.has(OFF_NAME));

        // While the file is still held, the next start stays off.
        let (collector, problem) =
            UsageCollector::load(recorder.clone(), &folder.0, Some(today()), Instant::now());
        assert!(!collector.status().counting);
        assert_eq!(problem, Some(UsageError::Delete));
        assert!(!recorder.is_enabled());

        // Once released, the next start finishes the deletion and removes the marker.
        drop(held);
        let (collector, problem) =
            UsageCollector::load(recorder.clone(), &folder.0, Some(today()), Instant::now());
        assert!(!collector.status().counting);
        assert_eq!(problem, None);
        assert!(!folder.has(FILE_NAME) && !folder.has(OFF_NAME));
    }

    #[test]
    fn a_failed_reset_reports_the_status_it_left() {
        let folder = Folder::new();
        let recorder = Arc::new(UsageRecorder::new());
        let mut collector = counting(&folder, &recorder);
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(folder.0.join(FILE_NAME))
            .expect("hold the synthetic usage file");
        let (status, problem) =
            collector.control(UsageControl::Reset, Some(today()), Instant::now());
        assert!(problem.is_some());
        assert!(
            !status.counting,
            "a reset that could not delete leaves counting off"
        );
        assert!(!recorder.is_enabled());
        assert!(folder.has(OFF_NAME), "the switch-off is recorded");

        // While the file is still held, the next start stays off.
        let (collector, problem) =
            UsageCollector::load(recorder.clone(), &folder.0, Some(today()), Instant::now());
        assert!(!collector.status().counting);
        assert_eq!(problem, Some(UsageError::Delete));

        // Once released, the next start finishes the deletion.
        drop(held);
        let (collector, problem) =
            UsageCollector::load(recorder.clone(), &folder.0, Some(today()), Instant::now());
        assert!(!collector.status().counting);
        assert_eq!(problem, None);
        assert!(!folder.has(FILE_NAME) && !folder.has(OFF_NAME));
    }

    #[test]
    fn a_switch_off_that_cannot_record_or_delete_says_counting_may_resume() {
        let folder = Folder::new();
        let recorder = Arc::new(UsageRecorder::new());
        let mut collector = counting(&folder, &recorder);
        // A folder where the marker belongs makes writing the marker fail.
        fs::create_dir(folder.0.join(OFF_NAME)).expect("block the synthetic marker");
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(folder.0.join(FILE_NAME))
            .expect("hold the synthetic usage file");
        let (status, problem) = collector.control(UsageControl::TurnOff, None, Instant::now());
        assert!(!status.counting);
        assert_eq!(problem, Some(UsageError::NotStopped));
        assert!(!recorder.is_enabled());
        assert!(
            !UsageError::NotStopped.to_string().contains("finish when"),
            "no retry is promised"
        );
        drop(held);
        fs::remove_dir(folder.0.join(OFF_NAME)).expect("unblock the synthetic marker");

        // As the message warned, nothing recorded the switch-off, so the file still counts.
        let (collector, _) =
            UsageCollector::load(recorder.clone(), &folder.0, Some(today()), Instant::now());
        assert!(collector.status().counting);
    }

    #[test]
    fn leftover_copies_beside_an_absent_file_go_at_start_up() {
        let folder = Folder::new();
        for name in [REPLACEMENT_NAME, DAMAGED_NAME] {
            fs::write(folder.0.join(name), b"synthetic leftover").expect("seed a leftover");
        }
        let recorder = Arc::new(UsageRecorder::new());
        let (collector, problem) =
            UsageCollector::load(recorder.clone(), &folder.0, Some(today()), Instant::now());
        assert_eq!(problem, None);
        assert!(!collector.status().counting);
        assert!(!folder.has(REPLACEMENT_NAME) && !folder.has(DAMAGED_NAME));
    }
}
