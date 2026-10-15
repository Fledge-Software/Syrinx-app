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

/// The approved version 1 field names, copied literally from the proposal and the frozen
/// command list, so the test does not trust the code it checks.
const APPROVED_FEATURES: [&str; 35] = [
    "app.started",
    "tab.created",
    "tab.closed",
    "tab.reopened",
    "tab.switched_sidebar",
    "tab.switched_keyboard",
    "saved.saved",
    "saved.removed",
    "saved.opened_unloaded",
    "folder.created",
    "folder.removed",
    "import.completed",
    "palette.opened_ctrl_k",
    "palette.opened_ctrl_l",
    "palette.dismissed",
    "palette.chose_tab",
    "palette.chose_saved",
    "palette.chose_history",
    "palette.chose_page_text",
    "palette.chose_command",
    "palette.chose_address",
    "palette.chose_web_search",
    "palette.scope_changed",
    "nav.back",
    "nav.forward",
    "nav.reload",
    "sidebar.set_wide",
    "sidebar.set_compact",
    "sidebar.set_hidden",
    "sidebar.peek_shown",
    "home.tile_opened",
    "favorite.opened",
    "find.opened",
    "page_text.captured",
    "settings.opened",
];
const APPROVED_COMMANDS: [&str; 21] = [
    "command.browser.new-tab",
    "command.browser.close-tab",
    "command.browser.reopen-tab",
    "command.browser.reload",
    "command.browser.back",
    "command.browser.forward",
    "command.browser.edit-address",
    "command.browser.next-tab",
    "command.browser.previous-tab",
    "command.browser.save-page",
    "command.browser.show-history",
    "command.browser.find",
    "command.browser.downloads",
    "command.browser.print",
    "command.browser.zoom-in",
    "command.browser.zoom-out",
    "command.browser.zoom-reset",
    "command.browser.clear-data",
    "command.browser.install-update",
    "command.browser.check-updates",
    "command.other",
];
const INTERACTION_BUCKETS: [&str; 12] = [
    "le_1", "le_2", "le_5", "le_8", "le_16", "le_33", "le_50", "le_100", "le_250", "le_500",
    "le_1000", "gt_1000",
];
const STARTUP_BUCKETS: [&str; 10] = [
    "le_100", "le_250", "le_500", "le_750", "le_1000", "le_1500", "le_2000", "le_3000", "le_5000",
    "gt_5000",
];
const APPROVED_TIMINGS: [(&str, &[&str]); 8] = [
    ("startup.first_frame_ms", &STARTUP_BUCKETS),
    ("startup.session_restored_ms", &STARTUP_BUCKETS),
    ("palette.open_to_paint_ms", &INTERACTION_BUCKETS),
    ("search.tier1_ms", &INTERACTION_BUCKETS),
    ("search.tier2_ms", &INTERACTION_BUCKETS),
    ("tab.switch_to_visible_ms", &INTERACTION_BUCKETS),
    ("store.save_ms", &INTERACTION_BUCKETS),
    ("backup.snapshot_ms", &STARTUP_BUCKETS),
];
const APPROVED_ERRORS: [&str; 13] = [
    "error.storage_open",
    "error.storage_write",
    "error.storage_location_refused",
    "error.backup_failed",
    "error.recovery_offered",
    "error.unclean_shutdown",
    "error.renderer_failed",
    "error.browser_process_exited",
    "error.webview_creation",
    "error.navigation_failed",
    "error.import_refused",
    "error.page_text_capture",
    "error.usage_file_reset",
];

/// Every key path a version 1 report must have, in order, below the envelope.
fn section_paths() -> Vec<String> {
    let mut paths: Vec<String> = APPROVED_FEATURES
        .iter()
        .map(|key| format!("counts/{key}"))
        .collect();
    paths.extend(
        APPROVED_COMMANDS
            .iter()
            .map(|key| format!("commands/{key}")),
    );
    for (timing, buckets) in APPROVED_TIMINGS {
        for bucket in buckets {
            paths.push(format!("timings/{timing}/{bucket}"));
        }
    }
    paths.extend(APPROVED_ERRORS.iter().map(|key| format!("errors/{key}")));
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
    // A completed period ends on the day it actually closed (the proposal's `period_end`).
    assert_eq!(previous.closed_on, Some(start.plus_days(PERIOD_DAYS)));
    assert_eq!(file.current.closed_on, None);
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
        // The current period cannot have closed.
        text.replace("\"closed_on\": null", "\"closed_on\": \"2026-10-20\""),
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

/// The `about` text exactly as the approved proposal prints it, copied rather than imported.
const APPROVED_ABOUT: &str = "Aggregate counts from Syrinx, kept on the owner's computer. \
                              Counters are totals for the period. Timing buckets are named by \
                              their upper edge in milliseconds; gt_N counts everything above N. \
                              No addresses, searches, text, paths or identifiers are recorded.";

/// One `"key": value` line of a hand-written report, at `depth` levels of two spaces.
fn expected_line(depth: usize, key: &str, value: &str, last: bool) -> String {
    let comma = if last { "" } else { "," };
    format!("{}\"{key}\": {value}{comma}\n", "  ".repeat(depth))
}

/// A group of single counts, each 1, as `every_field_recorded` leaves them.
fn expected_count_group(name: &str, keys: &[&str], last: bool) -> String {
    let mut text = format!("  \"{name}\": {{\n");
    for (index, key) in keys.iter().enumerate() {
        text += &expected_line(2, key, "1", index + 1 == keys.len());
    }
    text += if last { "  }\n" } else { "  },\n" };
    text
}

/// Every timing recorded 3 ms once: `le_5` on the interaction scale, `le_100` at start-up.
fn expected_timings() -> String {
    let mut text = String::from("  \"timings\": {\n");
    for (index, (timing, buckets)) in APPROVED_TIMINGS.iter().enumerate() {
        text += &format!("    \"{timing}\": {{\n");
        let startup = buckets.len() == STARTUP_BUCKETS.len();
        for (position, bucket) in buckets.iter().enumerate() {
            let hit = if startup { "le_100" } else { "le_5" };
            let count = if *bucket == hit { "1" } else { "0" };
            text += &expected_line(3, bucket, count, position + 1 == buckets.len());
        }
        let last = index + 1 == APPROVED_TIMINGS.len();
        text += if last { "    }\n" } else { "    },\n" };
    }
    text += "  },\n";
    text
}

/// A second, deliberately plain rendering of the synthetic report, written from the approved
/// key lists, so AN-02's byte-exact check does not trust the renderer it checks.
fn expected_report_text(start: &str, end: &str) -> String {
    let mut text = String::from("{\n");
    text += &expected_line(1, "schema_version", "1", false);
    let version = format!("\"{}\"", env!("CARGO_PKG_VERSION"));
    text += &expected_line(1, "syrinx_version", &version, false);
    text += &expected_line(1, "period_start", &format!("\"{start}\""), false);
    text += &expected_line(1, "period_end", &format!("\"{end}\""), false);
    text += &expected_line(1, "days_used", "1", false);
    text += &expected_line(1, "about", &format!("\"{APPROVED_ABOUT}\""), false);
    text += &expected_count_group("counts", &APPROVED_FEATURES, false);
    text += &expected_count_group("commands", &APPROVED_COMMANDS, false);
    text += &expected_timings();
    text += &expected_count_group("errors", &APPROVED_ERRORS, true);
    text += "}\n";
    text
}

/// The current period's report with every field used once, from 2026-11-02 to 2026-11-30.
fn synthetic_report() -> String {
    let recorder = every_field_recorded();
    let mut file = UsageFile::begin(date(2026, 11, 2));
    file.add(&recorder.take().expect("synthetic counts"));
    file.report_text(ReportPeriod::Current, date(2026, 11, 30))
        .expect("the current period always reports")
}

#[test]
fn a_report_is_written_byte_for_byte_in_the_approved_format() {
    let text = synthetic_report();
    let bytes = text.as_bytes();

    // Compared byte for byte, normalising nothing (the line-ending trap).
    assert_eq!(text, expected_report_text("2026-11-02", "2026-11-30"));
    assert_eq!(bytes.first(), Some(&b'{'), "no byte-order mark");
    assert!(!bytes.contains(&b'\r') && !bytes.contains(&b'\t'));
    assert!(text.ends_with("}\n") && !text.ends_with("\n\n"));
    for line in text.lines() {
        let indent = line.len() - line.trim_start_matches(' ').len();
        assert_eq!(indent % 2, 0, "two-space indentation: {line:?}");
    }
}

#[test]
fn the_preview_table_holds_exactly_the_report_fields() {
    let table = read_report(&synthetic_report()).expect("a written report reads back");

    let envelope: Vec<(&str, &str)> = table
        .envelope
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    assert_eq!(
        envelope,
        [
            ("schema_version", "1"),
            ("syrinx_version", env!("CARGO_PKG_VERSION")),
            ("period_start", "2026-11-02"),
            ("period_end", "2026-11-30"),
            ("days_used", "1"),
            ("about", APPROVED_ABOUT),
        ]
    );
    // Every row of the table, as a key path, equals the approved schema and nothing more.
    let mut paths = Vec::new();
    for group in &table.groups {
        for row in &group.rows {
            match &row.value {
                ReportValue::Count(count) => {
                    assert_eq!(*count, 1);
                    paths.push(format!("{}/{}", group.key, row.key));
                }
                ReportValue::Buckets(buckets) => paths.extend(
                    buckets
                        .iter()
                        .map(|(bucket, _)| format!("{}/{}/{bucket}", group.key, row.key)),
                ),
            }
        }
    }
    assert_eq!(paths, section_paths());
}

#[test]
fn the_preview_refuses_any_text_but_the_exact_report() {
    let text = synthetic_report();
    let altered = [
        text.replace('\n', "\r\n"),
        format!("\u{feff}{text}"),
        text.trim_end().to_owned(),
        text.replacen("  \"schema_version\"", "    \"schema_version\"", 1),
        text.replacen(
            "\"days_used\": 1,",
            "\"days_used\": 1,\n  \"user\": \"x\",",
            1,
        ),
        text.replacen("    \"app.started\": 1,\n", "", 1),
        text.replacen("\"schema_version\": 1", "\"schema_version\": 2", 1),
    ];
    for (index, variant) in altered.iter().enumerate() {
        assert_ne!(variant, &text, "variant {index} changes the text");
        assert!(read_report(variant).is_err(), "variant {index} was read");
    }
}

#[test]
fn a_completed_period_reports_until_the_day_it_closed() {
    let start = date(2026, 10, 14);
    let mut file = UsageFile::begin(start);
    assert_eq!(
        file.report_text(ReportPeriod::Previous, start.plus_days(3)),
        None,
        "nothing to export before a period has closed"
    );
    file.start_day(start.plus_days(31));
    let text = file
        .report_text(ReportPeriod::Previous, start.plus_days(40))
        .expect("the first period closed");
    let table = read_report(&text).expect("a written report reads back");
    assert_eq!(table.envelope_value("period_start"), Some("2026-10-14"));
    assert_eq!(table.envelope_value("period_end"), Some("2026-11-14"));
}
