//! Proves AN-01's collection through the session worker with synthetic profiles and values.

use syrinx_core::usage::{Counts, UsageFile, report};

use super::*;

/// Private-looking synthetic values no usage file may ever contain, even in part.
const PRIVATE_ADDRESS: &str = "https://private.example.test/quokka?q=zeppelin-marmalade";
const PRIVATE_NOTE: &str = "Synthetic note body: wombat lanterns hum beneath plum orchards";
const PRIVATE_PATH: &str = r"C:\synthetic\wombat\ledger";

fn usage_path(fixture: &Fixture) -> PathBuf {
    fixture.0.join("usage.json")
}

fn read_usage(fixture: &Fixture) -> UsageFile {
    let bytes = checked_io(fs::read(usage_path(fixture)));
    UsageFile::decode(&bytes).expect("the worker wrote a valid usage file")
}

/// The next usage status, skipping the tab states other commands publish.
fn usage_status(events: &mut UnboundedReceiver<SessionEvent>) -> UsageStatus {
    loop {
        match next_any_event(events) {
            SessionEvent::Usage(result) => return result.expect("usage control succeeds"),
            SessionEvent::Preferences { .. } | SessionEvent::State(_) => continue,
            event => panic!("expected a usage status, got {event:?}"),
        }
    }
}

/// The status a worker reports as it starts, read with the first state after it.
fn started_status(events: &mut UnboundedReceiver<SessionEvent>) -> UsageStatus {
    let status = usage_status(events);
    state(events);
    status
}

/// Every tab, saved-page and folder command the worker counts, carrying private values.
fn drive_worker(handle: &SessionHandle, events: &mut UnboundedReceiver<SessionEvent>) {
    let opened = change(
        handle,
        events,
        SessionCommand::NewTab {
            address: PRIVATE_ADDRESS.to_owned(),
        },
    );
    let id = opened.created[0];
    change(
        handle,
        events,
        SessionCommand::ObserveMetadata {
            id,
            title: Some(PRIVATE_NOTE.to_owned()),
            address: Some(PRIVATE_ADDRESS.to_owned()),
        },
    );
    change(handle, events, SessionCommand::SaveActive);
    change(
        handle,
        events,
        SessionCommand::Folder(FolderCommand::Create {
            title: PRIVATE_PATH.to_owned(),
            parent: None,
        }),
    );
    change(
        handle,
        events,
        SessionCommand::NewTab {
            address: "https://second.example.test/".to_owned(),
        },
    );
    change(
        handle,
        events,
        SessionCommand::CycleActive { backwards: true },
    );
    change(
        handle,
        events,
        SessionCommand::CycleActive { backwards: false },
    );
    send(
        handle,
        SessionCommand::Query {
            request_id: 1,
            text: format!("{PRIVATE_NOTE} {PRIVATE_PATH}"),
            scope: Scope::Everything,
            policy: query_policy(),
        },
    );
    assert!(matches!(
        next_event(events),
        SessionEvent::Search { request_id: 1, .. }
    ));
    change(handle, events, SessionCommand::CloseActive);
    change(handle, events, SessionCommand::ReopenClosed);
}

/// No run of five or more characters from any private value appears in `text`.
fn assert_no_private_fragment(text: &str) {
    for private in [PRIVATE_ADDRESS, PRIVATE_NOTE, PRIVATE_PATH] {
        let characters: Vec<char> = private.chars().collect();
        for window in characters.windows(5) {
            let fragment: String = window.iter().collect();
            assert!(!text.contains(&fragment), "{fragment:?} leaked");
        }
    }
}

#[test]
fn counting_stays_off_and_writes_nothing_until_switched_on() {
    let fixture = Fixture::new();
    let usage = Arc::new(UsageRecorder::new());
    let (handle, mut events) = fixture.start_with(usage.clone());
    assert_eq!(
        started_status(&mut events),
        UsageStatus {
            counting: false,
            since: None
        }
    );
    drive_worker(&handle, &mut events);
    stop(handle, &mut events);
    assert!(!usage.is_enabled());
    assert!(!usage.has_changes());
    assert!(!usage_path(&fixture).exists());
    fixture.cleanup();
}

#[test]
fn switching_on_counts_into_the_file_and_switching_off_deletes_it() {
    let fixture = Fixture::new();
    let usage = Arc::new(UsageRecorder::new());
    let (handle, mut events) = fixture.start_with(usage.clone());
    started_status(&mut events);
    send(&handle, SessionCommand::Usage(UsageControl::TurnOn));
    let status = usage_status(&mut events);
    assert!(status.counting);
    let today = status.since.expect("a period starts today");
    assert!(
        usage_path(&fixture).exists(),
        "switching on writes the file"
    );

    drive_worker(&handle, &mut events);
    stop(handle, &mut events);
    let file = read_usage(&fixture);
    let counts = &file.current.counts;
    assert_eq!(file.current.start, today);
    assert_eq!(counts.feature(Feature::TabCreated), 2);
    assert_eq!(counts.feature(Feature::SavedSaved), 1);
    assert_eq!(counts.feature(Feature::FolderCreated), 1);
    assert_eq!(counts.feature(Feature::TabSwitchedKeyboard), 2);
    assert_eq!(counts.feature(Feature::TabClosed), 1);
    assert_eq!(counts.feature(Feature::TabReopened), 1);
    assert_eq!(counts.timing(Timing::SearchTier1).iter().sum::<u32>(), 1);
    assert!(counts.timing(Timing::StoreSave).iter().sum::<u32>() > 0);
    // Switching on is not a start of Syrinx; the next launch is.
    assert_eq!(counts.feature(Feature::AppStarted), 0);

    let (handle, mut events) = fixture.start_with(usage.clone());
    assert!(started_status(&mut events).counting);
    send(&handle, SessionCommand::Usage(UsageControl::TurnOff));
    assert_eq!(
        usage_status(&mut events),
        UsageStatus {
            counting: false,
            since: None
        }
    );
    assert!(
        !usage_path(&fixture).exists(),
        "switching off deletes the counts"
    );
    change(
        &handle,
        &mut events,
        SessionCommand::NewTab {
            address: "about:blank".to_owned(),
        },
    );
    stop(handle, &mut events);
    assert!(
        !usage_path(&fixture).exists(),
        "nothing is written while off"
    );
    assert!(!usage.has_changes());
    fixture.cleanup();
}

#[test]
fn private_values_never_reach_the_usage_file_or_its_report() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    started_status(&mut events);
    send(&handle, SessionCommand::Usage(UsageControl::TurnOn));
    usage_status(&mut events);
    drive_worker(&handle, &mut events);
    stop(handle, &mut events);

    let text = checked_io(fs::read_to_string(usage_path(&fixture)));
    assert_no_private_fragment(&text);
    let file = read_usage(&fixture);
    assert_no_private_fragment(&report(&file.current, file.current.last_counted));
    fixture.cleanup();
}

#[test]
fn reset_deletes_both_periods_and_starts_today() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    started_status(&mut events);
    stop(handle, &mut events);

    // A synthetic file with a previous period and counts in a current one begun five days ago;
    // the worker reads today's real date, so the periods are placed relative to it.
    let today = local_today().expect("Windows reports today's date");
    let mut seeded = UsageFile::begin(today.plus_days(-40));
    seeded.start_day(today.plus_days(-5));
    assert!(seeded.previous.is_some());
    let usage = UsageRecorder::new();
    usage.switch_on();
    usage.count(Feature::TabCreated);
    seeded.add(&usage.take().expect("synthetic count"));
    checked_io(fs::write(usage_path(&fixture), seeded.encode()));

    let (handle, mut events) = fixture.start();
    let status = started_status(&mut events);
    assert_eq!(status.since, Some(today.plus_days(-5)));
    send(&handle, SessionCommand::Usage(UsageControl::Reset));
    let reset = usage_status(&mut events);
    assert!(reset.counting);
    assert_eq!(reset.since, Some(today));
    stop(handle, &mut events);

    let file = read_usage(&fixture);
    assert_eq!(file.previous, None);
    assert_eq!(Some(file.current.start), reset.since);
    assert_eq!(file.current.days_used, 1);
    assert_eq!(file.current.counts, Counts::default());
    fixture.cleanup();
}

#[test]
fn a_damaged_usage_file_is_set_aside_counted_once_and_counting_continues() {
    let fixture = Fixture::new();
    let (handle, mut events) = fixture.start();
    started_status(&mut events);
    stop(handle, &mut events);
    let damaged = b"{\"format\": \"syrinx-usage\", \"schema_version\": 1, \"current\": ";
    checked_io(fs::write(usage_path(&fixture), damaged));

    let (handle, mut events) = fixture.start();
    assert!(matches!(
        next_any_event(&mut events),
        SessionEvent::Failure {
            error: SessionError::Usage(UsageError::Damaged),
            fatal: false
        }
    ));
    assert!(started_status(&mut events).counting);
    stop(handle, &mut events);
    assert_eq!(
        checked_io(fs::read(fixture.0.join("usage.json.damaged"))),
        damaged
    );
    let first = read_usage(&fixture);
    assert_eq!(first.current.counts.error(ErrorCategory::UsageFileReset), 1);

    // The next start reads the new file normally and counts no second reset.
    let (handle, mut events) = fixture.start();
    assert!(started_status(&mut events).counting);
    stop(handle, &mut events);
    let second = read_usage(&fixture);
    assert_eq!(
        second.current.counts.error(ErrorCategory::UsageFileReset),
        1
    );
    assert_eq!(second.current.counts.feature(Feature::AppStarted), 2);

    // Reset also deletes the counts set aside from the damaged file.
    let (handle, mut events) = fixture.start();
    started_status(&mut events);
    send(&handle, SessionCommand::Usage(UsageControl::Reset));
    usage_status(&mut events);
    stop(handle, &mut events);
    assert!(!fixture.0.join("usage.json.damaged").exists());
    fixture.cleanup();
}

#[test]
fn backups_never_carry_the_usage_file_and_a_restore_leaves_it_alone() {
    let fixture = Fixture::new();
    seed_recovery(&fixture);
    // Counting was on before the damage, with synthetic counts the restore must not touch.
    let today = local_today().expect("Windows reports today's date");
    let mut seeded = UsageFile::begin(today);
    let usage = UsageRecorder::new();
    usage.switch_on();
    for _ in 0..7 {
        usage.count(Feature::TabCreated);
    }
    seeded.add(&usage.take().expect("synthetic counts"));
    checked_io(fs::write(usage_path(&fixture), seeded.encode()));
    damage_recovery_primary(&fixture);

    let (handle, mut events) = fixture.start();
    let offer_id = recovery_offer(&mut events);
    send(&handle, SessionCommand::ConfirmRecovery { offer_id });
    assert!(started_status(&mut events).counting);
    stop(handle, &mut events);

    // No backup holds a usage file, and the restore replaced only database files.
    for entry in checked_io(fs::read_dir(fixture.0.join("backups"))) {
        let name = checked_io(entry).file_name();
        assert!(
            !name.to_string_lossy().contains("usage"),
            "{name:?} in backups"
        );
    }
    remove_preserved_recovery_bundle(&fixture, Some(DAMAGED_RECOVERY_PRIMARY));
    let file = read_usage(&fixture);
    let counts = &file.current.counts;
    assert_eq!(file.current.start, seeded.current.start);
    assert_eq!(counts.feature(Feature::TabCreated), 7);
    assert_eq!(counts.feature(Feature::AppStarted), 1);
    assert_eq!(counts.error(ErrorCategory::RecoveryOffered), 1);
    assert_eq!(counts.error(ErrorCategory::StorageOpen), 1);
    fixture.cleanup();
}
