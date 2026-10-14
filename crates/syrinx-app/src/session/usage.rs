//! Keeps the usage counts in `usage.json` beside the database, only while counting is on (AN-01).
//!
//! The file existing is what "on" means: switching off deletes it, so no counts linger.

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

/// Beside `syrinx.db`, so D13's synced-folder guard covers it; backups copy only the database.
pub(super) const FILE_NAME: &str = "usage.json";
/// Each write goes here first, then replaces the file in one rename.
const REPLACEMENT_NAME: &str = "usage.json.new";
/// An unreadable file is moved here, replacing any earlier one, and counting starts afresh.
const DAMAGED_NAME: &str = "usage.json.damaged";
/// The counts are written at most this often while in use, and once at a clean exit.
pub(super) const WRITE_INTERVAL: Duration = Duration::from_secs(5 * 60);

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
    Delete,
    /// The file was unreadable, so it was set aside and counting started afresh.
    Damaged,
    /// Today's date is unknown, so a period cannot start.
    Clock,
}

impl std::fmt::Display for UsageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Read => "Syrinx couldn't read its usage counts, so it isn't counting this time",
            Self::Write => "Syrinx couldn't save its usage counts; it will try again",
            Self::Delete => "Syrinx couldn't delete its usage counts",
            Self::Damaged => "Syrinx's usage counts were unreadable, so it started them afresh",
            Self::Clock => "Syrinx couldn't read today's date, so usage counting didn't change",
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
        let Some(bytes) = read_bounded(&self.path(FILE_NAME)).map_err(|_| UsageError::Read)? else {
            return Ok(());
        };
        let Some(today) = today else {
            return Err(UsageError::Clock);
        };
        match UsageFile::decode(&bytes) {
            Ok(mut file) => {
                file.start_day(today);
                self.start(file);
                self.recorder.count(Feature::AppStarted);
                Ok(())
            }
            Err(_) => self.start_afresh(today, now),
        }
    }

    /// Moves a damaged file aside and saves a new one at once, so counting stays on.
    fn start_afresh(&mut self, today: UsageDate, now: Instant) -> Result<(), UsageError> {
        fs::rename(self.path(FILE_NAME), self.path(DAMAGED_NAME)).map_err(|_| UsageError::Read)?;
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

    pub(super) fn control(
        &mut self,
        control: UsageControl,
        today: Option<UsageDate>,
        now: Instant,
    ) -> Result<UsageStatus, UsageError> {
        match control {
            UsageControl::TurnOn if self.file.is_none() => self.begin(today, now)?,
            UsageControl::TurnOff => self.turn_off()?,
            UsageControl::Reset if self.file.is_some() => self.begin(today, now)?,
            UsageControl::TurnOn | UsageControl::Reset => {}
        }
        Ok(self.status())
    }

    /// Starts one empty period today and saves it, which is what turns counting on.
    fn begin(&mut self, today: Option<UsageDate>, now: Instant) -> Result<(), UsageError> {
        let today = today.ok_or(UsageError::Clock)?;
        // Reset also deletes counts set aside from a damaged file.
        remove_if_present(&self.path(DAMAGED_NAME))?;
        self.start(UsageFile::begin(today));
        if let Err(error) = self.write(now) {
            // Without a file, counting cannot be on; leave nothing half-made.
            self.turn_off()?;
            return Err(error);
        }
        Ok(())
    }

    /// Stops counting and deletes every usage file, so nothing lingers while off.
    fn turn_off(&mut self) -> Result<(), UsageError> {
        self.recorder.switch_off();
        self.file = None;
        self.unwritten = false;
        for name in [FILE_NAME, REPLACEMENT_NAME, DAMAGED_NAME] {
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
        replace_file(
            &self.path(REPLACEMENT_NAME),
            &self.path(FILE_NAME),
            text.as_bytes(),
        )
        .map_err(|_| UsageError::Write)?;
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

/// Writes and flushes the replacement, then renames it over the file in one step.
fn replace_file(replacement: &Path, target: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create(replacement)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(replacement, target)
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
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            for name in [FILE_NAME, REPLACEMENT_NAME, DAMAGED_NAME] {
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
        assert!(!folder.0.join(FILE_NAME).exists());
    }

    #[test]
    fn writes_wait_for_a_change_and_five_minutes() {
        let folder = Folder::new();
        let recorder = Arc::new(UsageRecorder::new());
        let start = Instant::now();
        let (mut collector, _) =
            UsageCollector::load(recorder.clone(), &folder.0, Some(today()), start);
        let status = collector
            .control(UsageControl::TurnOn, Some(today()), start)
            .expect("switching on writes the file");
        assert_eq!(status.since, Some(today()));
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
        assert_eq!(collector.write_if_due(start + WRITE_INTERVAL), Ok(()));
        assert!(!collector.pending());
        let bytes = fs::read(folder.0.join(FILE_NAME)).expect("read synthetic usage file");
        let file = UsageFile::decode(&bytes).expect("valid usage file");
        assert_eq!(file.current.counts.feature(Feature::TabCreated), 1);
        assert!(!folder.0.join(REPLACEMENT_NAME).exists());
    }

    #[test]
    fn switching_on_without_a_date_changes_nothing() {
        let folder = Folder::new();
        let recorder = Arc::new(UsageRecorder::new());
        let start = Instant::now();
        let (mut collector, _) = UsageCollector::load(recorder.clone(), &folder.0, None, start);
        assert_eq!(
            collector.control(UsageControl::TurnOn, None, start),
            Err(UsageError::Clock)
        );
        assert!(!recorder.is_enabled());
        assert!(!folder.0.join(FILE_NAME).exists());
    }
}
