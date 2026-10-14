//! Proves AN-01's schema, recorder, periods and file format with synthetic counts only.

use std::time::Duration;

use super::codec::leaves;
use super::schema::{OTHER_COMMAND, command_index};
use super::*;
use crate::{BrowserAction, CommandAction, CommandRegistry};

fn date(year: i32, month: u32, day: u32) -> UsageDate {
    UsageDate::from_ymd(year, month, day).expect("synthetic date is valid")
}

/// A recorder switched on with every counter, command, bucket and category used once.
fn every_field_recorded() -> UsageRecorder {
    let recorder = UsageRecorder::new();
    recorder.switch_on();
    for feature in Feature::ALL {
        recorder.count(feature);
    }
    for (_, command) in CommandRegistry::browser().iter() {
        let CommandAction::Browser(action) = command.action else {
            panic!("built-in commands are browser actions");
        };
        recorder.command(action);
    }
    recorder.other_command();
    for timing in Timing::ALL {
        recorder.time(timing, Duration::from_millis(3));
    }
    for category in ErrorCategory::ALL {
        recorder.error(category);
    }
    recorder
}

/// Every key path a version 1 report must have, in order, below the envelope.
fn section_paths() -> Vec<String> {
    let mut paths: Vec<String> = Feature::ALL
        .iter()
        .map(|feature| format!("counts/{}", feature.key()))
        .collect();
    paths.extend(COMMAND_KEYS.iter().map(|key| format!("commands/{key}")));
    for timing in Timing::ALL {
        for bucket in timing.scale().bucket_names() {
            paths.push(format!("timings/{}/{bucket}", timing.key()));
        }
    }
    paths.extend(
        ErrorCategory::ALL
            .iter()
            .map(|category| format!("errors/{}", category.key())),
    );
    paths
}

#[test]
fn every_enum_lists_its_fields_in_slot_order() {
    for (index, feature) in Feature::ALL.iter().enumerate() {
        assert_eq!(*feature as usize, index, "{feature:?}");
    }
    for (index, timing) in Timing::ALL.iter().enumerate() {
        assert_eq!(*timing as usize, index, "{timing:?}");
    }
    for (index, category) in ErrorCategory::ALL.iter().enumerate() {
        assert_eq!(*category as usize, index, "{category:?}");
    }
    assert_eq!(SLOT_COUNT, 35 + 21 + 5 * 12 + 3 * 10 + 13);
    // The proposal's memory budget is under 16 KiB; version 1 needs well under 1 KiB.
    assert!(std::mem::size_of::<UsageRecorder>() < 1024);
}

#[test]
fn the_frozen_command_list_matches_the_built_in_registry() {
    // A new built-in command is a schema change for the owner to approve, not a silent field.
    let registry = CommandRegistry::browser();
    assert_eq!(registry.len(), COMMAND_KEYS.len() - 1);
    for (_, command) in registry.iter() {
        let CommandAction::Browser(action) = command.action else {
            panic!("built-in commands are browser actions");
        };
        assert_eq!(
            COMMAND_KEYS[command_index(action)],
            format!("command.{}", command.id.as_str())
        );
    }
    assert_eq!(COMMAND_KEYS[OTHER_COMMAND], "command.other");
}

#[test]
fn a_report_has_exactly_the_schema_keys_and_only_numbers_outside_the_envelope() {
    let recorder = every_field_recorded();
    let mut file = UsageFile::begin(date(2026, 11, 2));
    file.add(&recorder.take().expect("synthetic counts"));
    let text = report(&file.current, date(2026, 11, 30));

    let leaves = leaves(&text).expect("the report parses");
    let mut expected = vec![
        "schema_version".to_owned(),
        "syrinx_version".to_owned(),
        "period_start".to_owned(),
        "period_end".to_owned(),
        "days_used".to_owned(),
        "about".to_owned(),
    ];
    expected.extend(section_paths());
    let paths: Vec<&String> = leaves.iter().map(|(path, _)| path).collect();
    assert_eq!(paths, expected.iter().collect::<Vec<_>>());

    // Only the four text fields are not numbers, and each is fixed or a date.
    let texts: Vec<&str> = leaves
        .iter()
        .filter(|(_, number)| number.is_none())
        .map(|(path, _)| path.as_str())
        .collect();
    assert_eq!(
        texts,
        ["syrinx_version", "period_start", "period_end", "about"]
    );
    assert!(text.contains("\"period_start\": \"2026-11-02\""));
    assert!(text.contains("\"period_end\": \"2026-11-30\""));
    assert!(text.contains(ABOUT));
    assert!(text.ends_with("}\n") && !text.contains('\r'));
}

#[test]
fn recorded_values_reach_their_own_slots() {
    let recorder = UsageRecorder::new();
    recorder.switch_on();
    recorder.count(Feature::TabCreated);
    recorder.count(Feature::TabCreated);
    recorder.command(BrowserAction::Reload);
    recorder.other_command();
    recorder.error(ErrorCategory::NavigationFailed);
    recorder.time(Timing::SearchTier1, Duration::from_micros(1_500));
    recorder.time(Timing::BackupSnapshot, Duration::from_secs(9));
    let counts = recorder.take().expect("counts changed");
    assert_eq!(counts.feature(Feature::TabCreated), 2);
    assert_eq!(counts.feature(Feature::TabClosed), 0);
    assert_eq!(counts.command(BrowserAction::Reload), 1);
    assert_eq!(counts.command(BrowserAction::Back), 0);
    assert_eq!(counts.other_command(), 1);
    assert_eq!(counts.error(ErrorCategory::NavigationFailed), 1);
    // 1.5 ms is above the 1 ms edge, so it lands in `le_2`.
    assert_eq!(counts.timing(Timing::SearchTier1)[1], 1);
    assert_eq!(counts.timing(Timing::BackupSnapshot)[9], 1);
    assert_eq!(counts.timing(Timing::SearchTier2), [0; 12]);
    // Taking leaves zeros and no pending change.
    assert!(!recorder.has_changes());
    assert_eq!(recorder.take(), None);
}

#[test]
fn bucket_edges_are_inclusive_upper_bounds() {
    let interaction = Scale::Interaction;
    assert_eq!(interaction.bucket(Duration::ZERO), 0);
    assert_eq!(interaction.bucket(Duration::from_millis(1)), 0);
    assert_eq!(
        interaction.bucket(Duration::from_millis(1) + Duration::from_nanos(1)),
        1
    );
    assert_eq!(interaction.bucket(Duration::from_millis(16)), 4);
    assert_eq!(interaction.bucket(Duration::from_millis(1000)), 10);
    assert_eq!(interaction.bucket(Duration::from_millis(1001)), 11);
    assert_eq!(interaction.bucket(Duration::MAX), 11);
    assert_eq!(Scale::Startup.bucket(Duration::from_millis(100)), 0);
    assert_eq!(Scale::Startup.bucket(Duration::from_millis(5001)), 9);
    for scale in [Scale::Interaction, Scale::Startup] {
        assert_eq!(
            scale.bucket(Duration::MAX) + 1,
            scale.bucket_names().len(),
            "{scale:?}"
        );
    }
}

#[test]
fn disabled_collection_records_nothing() {
    let recorder = UsageRecorder::new();
    for feature in Feature::ALL {
        recorder.count(feature);
    }
    recorder.command(BrowserAction::NewTab);
    recorder.other_command();
    recorder.time(Timing::StoreSave, Duration::from_millis(2));
    recorder.error(ErrorCategory::StorageWrite);
    assert!(!recorder.is_enabled());
    assert!(!recorder.has_changes());
    assert_eq!(recorder.take(), None);

    // Switching off forgets what was not yet taken, and switching on starts from zero.
    let recorder = every_field_recorded();
    recorder.switch_off();
    recorder.count(Feature::AppStarted);
    assert_eq!(recorder.take(), None);
    recorder.switch_on();
    assert_eq!(recorder.take(), None);
}

#[test]
fn counters_stop_at_the_maximum_instead_of_wrapping() {
    let recorder = UsageRecorder::new();
    recorder.switch_on();
    recorder.preset(Feature::TabCreated, u32::MAX - 1);
    recorder.count(Feature::TabCreated);
    recorder.count(Feature::TabCreated);
    let counts = recorder.take().expect("the last increment counted");
    assert_eq!(counts.feature(Feature::TabCreated), u32::MAX);

    let mut total = counts.clone();
    total.add(&counts);
    assert_eq!(total.feature(Feature::TabCreated), u32::MAX);
}

#[test]
fn a_period_closes_after_thirty_days_and_only_two_are_kept() {
    let start = date(2026, 10, 14);
    let mut file = UsageFile::begin(start);
    assert_eq!(file.current.days_used, 1);

    // Another start the same day is not another day of use; a later day is.
    file.start_day(start);
    file.start_day(start.plus_days(5));
    file.start_day(start.plus_days(5));
    assert_eq!(file.current.days_used, 2);
    assert_eq!(file.current.last_counted, start.plus_days(5));

    // A clock set back counts nothing.
    file.start_day(start.plus_days(1));
    assert_eq!(file.current.days_used, 2);

    // Day 29 still belongs to the first period; day 30 closes it.
    file.start_day(start.plus_days(29));
    assert!(file.previous.is_none());
    file.start_day(start.plus_days(PERIOD_DAYS));
    let previous = file.previous.clone().expect("the first period closed");
    assert_eq!(previous.start, start);
    assert_eq!(previous.days_used, 3);
    assert_eq!(previous.span_end(), date(2026, 11, 12));
    assert_eq!(file.current.start, start.plus_days(30));
    assert_eq!(file.current.days_used, 1);

    // A third period deletes the first: at most two exist.
    file.start_day(start.plus_days(75));
    assert_eq!(
        file.previous.as_ref().map(|period| period.start),
        Some(start.plus_days(30))
    );
    assert_eq!(file.current.start, start.plus_days(75));
}

#[test]
fn a_file_round_trips_and_stays_under_its_size_limit() {
    let recorder = every_field_recorded();
    let mut file = UsageFile::begin(date(2026, 10, 14));
    file.add(&recorder.take().expect("synthetic counts"));
    file.start_day(date(2026, 11, 20));
    file.add(&recorder_with_all_fields_at_maximum());
    let text = file.encode();
    assert_eq!(UsageFile::decode(text.as_bytes()), Ok(file.clone()));
    assert!(text.len() < MAX_FILE_BYTES, "{} bytes", text.len());
    assert!(!text.contains('\r') && text.ends_with("}\n"));

    // The largest possible counts in both periods still fit.
    let mut full = file;
    full.previous = Some(full.current.clone());
    full.current.days_used = u32::MAX;
    assert!(full.encode().len() < MAX_FILE_BYTES);
}

fn recorder_with_all_fields_at_maximum() -> Counts {
    let mut counts = Counts::default();
    let every = every_field_recorded().take().expect("synthetic counts");
    for _ in 0..3 {
        counts.add(&every);
    }
    let mut full = Counts::from_slots([u32::MAX; SLOT_COUNT]);
    full.add(&counts);
    full
}

#[test]
fn damaged_files_are_refused_without_panicking() {
    let file = UsageFile::begin(date(2026, 10, 14));
    let text = file.encode();

    // Every truncation is refused, never read as a shorter valid file.
    for length in 0..text.len() - 1 {
        assert!(
            UsageFile::decode(&text.as_bytes()[..length]).is_err(),
            "{length}"
        );
    }
    let oversized = vec![b' '; MAX_FILE_BYTES + 1];
    assert_eq!(UsageFile::decode(&oversized), Err(UsageFileError::TooLarge));
    let later = text.replace("\"schema_version\": 1", "\"schema_version\": 2");
    assert_eq!(
        UsageFile::decode(later.as_bytes()),
        Err(UsageFileError::UnknownVersion)
    );
    assert_eq!(
        UsageFile::decode(&[0xff, 0xfe]),
        Err(UsageFileError::NotUtf8)
    );
    for damaged in [
        text.replace("\"app.started\": 0", "\"app.started\": -1"),
        text.replace("\"app.started\": 0", "\"app.started\": 01"),
        text.replace("\"app.started\": 0", "\"app.started\": 4294967296"),
        text.replace("\"app.started\": 0", "\"app.started\": 1.5"),
        text.replace("\"app.started\": 0", "\"app.stArted\": 0"),
        text.replace("\"app.started\": 0", "\"app.started\": 0, \"extra\": 0"),
        text.replace("\"format\": \"syrinx-usage\"", "\"format\": \"other\""),
        text.replace("2026-10-14", "2026-02-30"),
        text.replace("\"previous\": null", "\"previous\": \"C:\\\\synthetic\""),
        text.replace("\"previous\": null", "\"previous\": {}"),
        text.replace("\"tab.created\": 0", "\"tab.created\": [0]"),
        format!("{text}{{}}"),
        "{".repeat(64),
        String::new(),
    ] {
        assert!(UsageFile::decode(damaged.as_bytes()).is_err(), "{damaged}");
    }
}

#[test]
fn dates_are_calendar_days_written_as_iso_text() {
    let mut day = date(1970, 1, 1);
    for _ in 0..(366 * 60) {
        let text = day.to_string();
        assert_eq!(UsageDate::parse(&text), Some(day), "{text}");
        day = day.plus_days(1);
    }
    assert_eq!(date(2024, 2, 29).to_string(), "2024-02-29");
    assert_eq!(date(2024, 2, 28).plus_days(2).ymd(), (2024, 3, 1));
    for refused in [
        "2023-02-29",
        "2026-13-01",
        "2026-00-10",
        "2026-1-01",
        "1969-12-31",
        "+026-01-01",
        "2026-01-01T",
    ] {
        assert_eq!(UsageDate::parse(refused), None, "{refused}");
    }
}
