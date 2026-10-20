//! Writes `usage.json` and the report in one fixed JSON shape, and reads back only that shape.

use std::fmt;

use super::period::{Period, UsageDate, UsageFile};
use super::recorder::Counts;
use super::report::{ReportGroup, ReportPeriod, ReportRow, ReportTable, ReportValue};
use super::schema::{COMMAND_KEYS, ErrorCategory, Feature, SCHEMA_VERSION, SLOT_COUNT, Timing};

/// A larger usage file is refused unread; version 1's file is about 15 KiB.
pub const MAX_FILE_BYTES: usize = 32 * 1024;
const FILE_FORMAT: &str = "syrinx-usage";
const SYRINX_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Deepest nesting the format uses is four; anything deeper is not a usage file.
const MAX_DEPTH: usize = 6;

/// Identical in every report; tells the reader or model what the numbers mean.
pub const ABOUT: &str = "Aggregate counts from Syrinx, kept on the owner's computer. Counters \
                         are totals for the period. Timing buckets are named by their upper edge \
                         in milliseconds; gt_N counts everything above N. No addresses, \
                         searches, text, paths or identifiers are recorded.";

/// Why a usage file was refused; a fixed category, never the file's contents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageFileError {
    TooLarge,
    NotUtf8,
    Malformed,
    UnknownVersion,
}

impl fmt::Display for UsageFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TooLarge => "the usage file is larger than any Syrinx writes",
            Self::NotUtf8 => "the usage file is not UTF-8 text",
            Self::Malformed => "the usage file is damaged",
            Self::UnknownVersion => "the usage file is from a different schema version",
        })
    }
}

impl std::error::Error for UsageFileError {}

/// The JSON values the format uses: no arrays, booleans, signs or escapes.
#[derive(Debug, PartialEq)]
enum Value {
    Object(Vec<(String, Value)>),
    Text(String),
    Number(u64),
    Null,
}

impl UsageFile {
    /// The file's exact text: UTF-8, LF line endings, two-space indentation, final newline.
    pub fn encode(&self) -> String {
        let previous = self.previous.as_ref().map_or(Value::Null, period_value);
        let file = object(vec![
            ("format", Value::Text(FILE_FORMAT.to_owned())),
            ("schema_version", Value::Number(SCHEMA_VERSION.into())),
            ("current", period_value(&self.current)),
            ("previous", previous),
        ]);
        render(&file)
    }

    /// Reads a file written by `encode`; any other shape, key or value is refused.
    pub fn decode(bytes: &[u8]) -> Result<Self, UsageFileError> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(UsageFileError::TooLarge);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| UsageFileError::NotUtf8)?;
        let value = Parser::new(text).document()?;
        file_from_value(value)
    }
}

/// One period as the report the owner exports (AN-02 shows and saves these exact bytes).
pub fn report(period: &Period, period_end: UsageDate) -> String {
    let mut fields = vec![
        ("schema_version", Value::Number(SCHEMA_VERSION.into())),
        ("syrinx_version", Value::Text(SYRINX_VERSION.to_owned())),
        ("period_start", Value::Text(period.start.to_string())),
        ("period_end", Value::Text(period_end.to_string())),
        ("days_used", Value::Number(period.days_used.into())),
        ("about", Value::Text(ABOUT.to_owned())),
    ];
    fields.extend(sections(&period.counts));
    render(&object(fields))
}

impl UsageFile {
    /// One period's exact report text, `None` when there is no previous period yet. The
    /// current period ends on `today`, or on the last day it counted if the clock was set
    /// back; a completed one ends on the day it closed.
    pub fn report_text(&self, period: ReportPeriod, today: UsageDate) -> Option<String> {
        match period {
            ReportPeriod::Current => {
                // A clock set back must not end the period before a day it already counted.
                let end = today.max(self.current.last_counted);
                Some(report(&self.current, end))
            }
            ReportPeriod::Previous => {
                let previous = self.previous.as_ref()?;
                Some(report(previous, previous.closed_on?))
            }
        }
    }
}

/// A report's top-level keys, in the order `report` writes them.
const REPORT_KEYS: [&str; 10] = [
    "schema_version",
    "syrinx_version",
    "period_start",
    "period_end",
    "days_used",
    "about",
    "counts",
    "commands",
    "timings",
    "errors",
];

/// Reads a report written by `report` back into its fields for the preview's table; any
/// other shape, key or value is refused, so the table can show only what the text holds.
pub fn read_report(text: &str) -> Result<ReportTable, UsageFileError> {
    if text.len() > MAX_FILE_BYTES {
        return Err(UsageFileError::TooLarge);
    }
    let value = Parser::new(text).document()?;
    // Only the exact bytes `report` writes are read: no BOM, CR, other spacing or order.
    if render(&value) != text {
        return Err(UsageFileError::Malformed);
    }
    check_version(&value)?;
    let [
        version,
        syrinx,
        start,
        end,
        days,
        about,
        counts,
        commands,
        timings,
        errors,
    ] = expect_fields(value, &REPORT_KEYS)?;
    let envelope = vec![
        (REPORT_KEYS[0], expect_u32(version)?.to_string()),
        (REPORT_KEYS[1], expect_text(syrinx)?),
        (REPORT_KEYS[2], expect_date(start)?.to_string()),
        (REPORT_KEYS[3], expect_date(end)?.to_string()),
        (REPORT_KEYS[4], expect_u32(days)?.to_string()),
        (REPORT_KEYS[5], expect_text(about)?),
    ];
    let feature_keys: Vec<&str> = Feature::ALL.iter().map(|feature| feature.key()).collect();
    let error_keys: Vec<&str> = ErrorCategory::ALL.iter().map(|error| error.key()).collect();
    let groups = vec![
        count_group(REPORT_KEYS[6], counts, &feature_keys)?,
        count_group(REPORT_KEYS[7], commands, &COMMAND_KEYS)?,
        timing_group(REPORT_KEYS[8], timings)?,
        count_group(REPORT_KEYS[9], errors, &error_keys)?,
    ];
    Ok(ReportTable {
        envelope: envelope
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
        groups,
    })
}

/// A group of single counts with exactly `keys`, in order.
fn count_group(group: &str, value: Value, keys: &[&str]) -> Result<ReportGroup, UsageFileError> {
    let rows = keys
        .iter()
        .zip(expect_counts(value, keys)?)
        .map(|(key, count)| ReportRow {
            key: (*key).to_owned(),
            value: ReportValue::Count(count.into()),
        })
        .collect();
    Ok(ReportGroup {
        key: group.to_owned(),
        rows,
    })
}

/// The timings group: every histogram with exactly its scale's buckets.
fn timing_group(group: &str, value: Value) -> Result<ReportGroup, UsageFileError> {
    let timing_keys: Vec<&str> = Timing::ALL.iter().map(|timing| timing.key()).collect();
    let mut rows = Vec::with_capacity(Timing::ALL.len());
    for (timing, buckets) in Timing::ALL.iter().zip(expect_objects(value, &timing_keys)?) {
        let names = timing.scale().bucket_names();
        let buckets = names
            .iter()
            .zip(expect_counts(buckets, names)?)
            .map(|(name, count)| ((*name).to_owned(), u64::from(count)))
            .collect();
        rows.push(ReportRow {
            key: timing.key().to_owned(),
            value: ReportValue::Buckets(buckets),
        });
    }
    Ok(ReportGroup {
        key: group.to_owned(),
        rows,
    })
}

fn object(fields: Vec<(&str, Value)>) -> Value {
    Value::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn period_value(period: &Period) -> Value {
    let mut fields = vec![
        ("period_start", Value::Text(period.start.to_string())),
        ("last_counted", Value::Text(period.last_counted.to_string())),
        (
            "closed_on",
            period
                .closed_on
                .map_or(Value::Null, |closed| Value::Text(closed.to_string())),
        ),
        ("days_used", Value::Number(period.days_used.into())),
    ];
    fields.extend(sections(&period.counts));
    object(fields)
}

/// The four groups of counters, every field present, zeros included.
fn sections(counts: &Counts) -> Vec<(&'static str, Value)> {
    let numbers = |pairs: Vec<(&str, u32)>| {
        object(
            pairs
                .into_iter()
                .map(|(key, count)| (key, Value::Number(count.into())))
                .collect(),
        )
    };
    let features = Feature::ALL
        .iter()
        .map(|feature| (feature.key(), counts.feature(*feature)))
        .collect();
    let slots = counts.slots();
    let command_base = super::schema::COMMAND_BASE;
    let commands = COMMAND_KEYS
        .iter()
        .enumerate()
        .map(|(index, key)| (*key, slots[command_base + index]))
        .collect();
    let timings = Timing::ALL
        .iter()
        .map(|timing| {
            let buckets = timing.scale().bucket_names().iter().copied();
            (*timing, buckets.zip(counts.timing(*timing).iter().copied()))
        })
        .map(|(timing, buckets)| (timing.key(), numbers(buckets.collect())))
        .collect();
    let errors = ErrorCategory::ALL
        .iter()
        .map(|category| (category.key(), counts.error(*category)))
        .collect();
    vec![
        ("counts", numbers(features)),
        ("commands", numbers(commands)),
        ("timings", object(timings)),
        ("errors", numbers(errors)),
    ]
}

fn render(value: &Value) -> String {
    let mut text = String::with_capacity(16 * 1024);
    render_value(value, 0, &mut text);
    text.push('\n');
    text
}

fn render_value(value: &Value, depth: usize, text: &mut String) {
    match value {
        Value::Null => text.push_str("null"),
        Value::Number(number) => text.push_str(&number.to_string()),
        // Every string Syrinx writes is fixed text or a date, with nothing to escape.
        Value::Text(content) => {
            text.push('"');
            text.push_str(content);
            text.push('"');
        }
        Value::Object(fields) => render_object(fields, depth, text),
    }
}

fn render_object(fields: &[(String, Value)], depth: usize, text: &mut String) {
    text.push('{');
    for (index, (key, value)) in fields.iter().enumerate() {
        text.push('\n');
        text.push_str(&"  ".repeat(depth + 1));
        text.push('"');
        text.push_str(key);
        text.push_str("\": ");
        render_value(value, depth + 1, text);
        if index + 1 < fields.len() {
            text.push(',');
        }
    }
    text.push('\n');
    text.push_str(&"  ".repeat(depth));
    text.push('}');
}

/// Takes the top level apart once its version is known to be this schema's.
fn file_from_value(value: Value) -> Result<UsageFile, UsageFileError> {
    check_version(&value)?;
    let [format, _, current, previous] =
        expect_fields(value, &["format", "schema_version", "current", "previous"])?;
    if expect_text(format)? != FILE_FORMAT {
        return Err(UsageFileError::Malformed);
    }
    let previous = match previous {
        Value::Null => None,
        period => Some(period_from_value(period)?),
    };
    let current = period_from_value(current)?;
    // Only a completed period has closed, and it closed before the current one began.
    let closed = previous.as_ref().map(|previous| previous.closed_on);
    match closed {
        None | Some(Some(_)) if current.closed_on.is_none() => {}
        _ => return Err(UsageFileError::Malformed),
    }
    if closed
        .flatten()
        .is_some_and(|closed| closed > current.start)
    {
        return Err(UsageFileError::Malformed);
    }
    Ok(UsageFile { current, previous })
}

/// Another version may have other fields, so its number is read before the shape is checked.
fn check_version(value: &Value) -> Result<(), UsageFileError> {
    let Value::Object(fields) = value else {
        return Err(UsageFileError::Malformed);
    };
    let version = fields
        .iter()
        .find(|(key, _)| key == "schema_version")
        .map(|(_, version)| version);
    match version {
        Some(Value::Number(version)) if *version == u64::from(SCHEMA_VERSION) => Ok(()),
        Some(Value::Number(_)) => Err(UsageFileError::UnknownVersion),
        _ => Err(UsageFileError::Malformed),
    }
}

fn period_from_value(value: Value) -> Result<Period, UsageFileError> {
    let [start, last, closed, days, counts, commands, timings, errors] = expect_fields(
        value,
        &[
            "period_start",
            "last_counted",
            "closed_on",
            "days_used",
            "counts",
            "commands",
            "timings",
            "errors",
        ],
    )?;
    let start = expect_date(start)?;
    let last_counted = expect_date(last)?;
    if last_counted < start {
        return Err(UsageFileError::Malformed);
    }
    let closed_on = match closed {
        Value::Null => None,
        date => Some(expect_date(date)?),
    };
    if closed_on.is_some_and(|closed| closed < last_counted) {
        return Err(UsageFileError::Malformed);
    }
    let mut slots = [0; SLOT_COUNT];
    let mut next = 0;
    let mut fill = |values: Vec<u32>| {
        for count in values {
            slots[next] = count;
            next += 1;
        }
    };
    let feature_keys: Vec<&str> = Feature::ALL.iter().map(|feature| feature.key()).collect();
    fill(expect_counts(counts, &feature_keys)?);
    fill(expect_counts(commands, &COMMAND_KEYS)?);
    let timing_keys: Vec<&str> = Timing::ALL.iter().map(|timing| timing.key()).collect();
    for (timing, buckets) in Timing::ALL
        .iter()
        .zip(expect_objects(timings, &timing_keys)?)
    {
        fill(expect_counts(buckets, timing.scale().bucket_names())?);
    }
    let error_keys: Vec<&str> = ErrorCategory::ALL.iter().map(|error| error.key()).collect();
    fill(expect_counts(errors, &error_keys)?);
    Ok(Period {
        start,
        last_counted,
        closed_on,
        days_used: expect_u32(days)?,
        counts: Counts::from_slots(slots),
    })
}

/// The object's values, if its keys are exactly `keys` in that order.
fn expect_fields<const N: usize>(
    value: Value,
    keys: &[&str; N],
) -> Result<[Value; N], UsageFileError> {
    let values = expect_objects(value, keys)?;
    values.try_into().map_err(|_| UsageFileError::Malformed)
}

fn expect_objects(value: Value, keys: &[&str]) -> Result<Vec<Value>, UsageFileError> {
    let Value::Object(fields) = value else {
        return Err(UsageFileError::Malformed);
    };
    if fields.len() != keys.len() || fields.iter().zip(keys).any(|((key, _), want)| key != want) {
        return Err(UsageFileError::Malformed);
    }
    Ok(fields.into_iter().map(|(_, value)| value).collect())
}

fn expect_counts(value: Value, keys: &[&str]) -> Result<Vec<u32>, UsageFileError> {
    expect_objects(value, keys)?
        .into_iter()
        .map(expect_u32)
        .collect()
}

fn expect_u32(value: Value) -> Result<u32, UsageFileError> {
    let Value::Number(number) = value else {
        return Err(UsageFileError::Malformed);
    };
    u32::try_from(number).map_err(|_| UsageFileError::Malformed)
}

fn expect_text(value: Value) -> Result<String, UsageFileError> {
    match value {
        Value::Text(text) => Ok(text),
        _ => Err(UsageFileError::Malformed),
    }
}

fn expect_date(value: Value) -> Result<UsageDate, UsageFileError> {
    UsageDate::parse(&expect_text(value)?).ok_or(UsageFileError::Malformed)
}

/// Every leaf of a document as a `/`-joined key path and its number (`None` for text or null).
#[cfg(test)]
pub(super) fn leaves(text: &str) -> Result<Vec<(String, Option<u64>)>, UsageFileError> {
    fn walk(prefix: &str, value: &Value, leaves: &mut Vec<(String, Option<u64>)>) {
        let Value::Object(fields) = value else {
            let number = match value {
                Value::Number(number) => Some(*number),
                _ => None,
            };
            leaves.push((prefix.to_owned(), number));
            return;
        };
        for (key, value) in fields {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}/{key}")
            };
            walk(&path, value, leaves);
        }
    }
    let mut leaves = Vec::new();
    walk("", &Parser::new(text).document()?, &mut leaves);
    Ok(leaves)
}

/// A strict reader for the subset of JSON `render` writes.
struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            bytes: text.as_bytes(),
            position: 0,
        }
    }

    /// One value and nothing after it but white space.
    fn document(mut self) -> Result<Value, UsageFileError> {
        let value = self.value(0)?;
        self.skip_space();
        if self.position != self.bytes.len() {
            return Err(UsageFileError::Malformed);
        }
        Ok(value)
    }

    fn value(&mut self, depth: usize) -> Result<Value, UsageFileError> {
        if depth > MAX_DEPTH {
            return Err(UsageFileError::Malformed);
        }
        self.skip_space();
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'"') => self.text().map(Value::Text),
            Some(b'0'..=b'9') => self.number().map(Value::Number),
            Some(b'n') => self.literal(b"null").map(|()| Value::Null),
            _ => Err(UsageFileError::Malformed),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, UsageFileError> {
        self.position += 1;
        let mut fields = Vec::new();
        self.skip_space();
        if self.peek() == Some(b'}') {
            self.position += 1;
            return Ok(Value::Object(fields));
        }
        loop {
            self.skip_space();
            let key = self.text()?;
            self.skip_space();
            self.literal(b":")?;
            fields.push((key, self.value(depth + 1)?));
            self.skip_space();
            match self.next() {
                Some(b',') => continue,
                Some(b'}') => return Ok(Value::Object(fields)),
                _ => return Err(UsageFileError::Malformed),
            }
        }
    }

    /// A quoted string without escapes or control characters, as `render` writes them.
    fn text(&mut self) -> Result<String, UsageFileError> {
        self.literal(b"\"")?;
        let start = self.position;
        loop {
            match self.next() {
                Some(b'"') => break,
                Some(b'\\') | None => return Err(UsageFileError::Malformed),
                Some(byte) if byte < 0x20 => return Err(UsageFileError::Malformed),
                Some(_) => {}
            }
        }
        let content = &self.bytes[start..self.position - 1];
        // The input is already UTF-8 and quotes are ASCII, so the slice is too.
        String::from_utf8(content.to_vec()).map_err(|_| UsageFileError::Malformed)
    }

    /// Digits only, without a leading zero, a sign, a fraction or an exponent.
    fn number(&mut self) -> Result<u64, UsageFileError> {
        let start = self.position;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.position += 1;
        }
        let digits = &self.bytes[start..self.position];
        if digits.len() > 1 && digits[0] == b'0' {
            return Err(UsageFileError::Malformed);
        }
        if matches!(self.peek(), Some(b'.' | b'e' | b'E' | b'-' | b'+')) {
            return Err(UsageFileError::Malformed);
        }
        // Accumulated digit by digit; a value past u64 is refused, never wrapped.
        digits.iter().try_fold(0u64, |value, digit| {
            value
                .checked_mul(10)
                .and_then(|value| value.checked_add(u64::from(digit - b'0')))
                .ok_or(UsageFileError::Malformed)
        })
    }

    fn literal(&mut self, expected: &[u8]) -> Result<(), UsageFileError> {
        if !self.bytes[self.position..].starts_with(expected) {
            return Err(UsageFileError::Malformed);
        }
        self.position += expected.len();
        Ok(())
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.position += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.position += 1;
        Some(byte)
    }
}
