//! Keeps each plugin search feed's cursor and its tier 1 rows (#141), committed together so the
//! cursor never moves past rows that were not kept, and the palette has them before any
//! plugin runs again.

use std::fmt;

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::{EncodingField, StoreError, StoreOperation, error::database_error};

/// The feed's cursor as its plugin last returned it; written only with the rows it covers.
pub(crate) const CURSOR_TABLE: &str = "CREATE TABLE plugin_feed_cursor (
    plugin_id TEXT NOT NULL REFERENCES plugin_install(plugin_id) ON DELETE CASCADE,
    feed TEXT NOT NULL CHECK(length(feed) BETWEEN 1 AND 64),
    cursor TEXT NOT NULL CHECK(length(cursor) <= 1024),
    PRIMARY KEY(plugin_id, feed)
) STRICT";

/// One tier 1 row per feed entry. Keywords are each ended by U+001F, which none may hold. A
/// deleted entry stays as a tombstone (`deleted` 1, its text emptied) holding its revision,
/// so tier 1 refuses a re-create at the same or a lower revision, as the full-text index's
/// tombstone does in tier 2.
pub(crate) const ENTRY_TABLE: &str = "CREATE TABLE plugin_feed_entry (
    plugin_id TEXT NOT NULL REFERENCES plugin_install(plugin_id) ON DELETE CASCADE,
    feed TEXT NOT NULL CHECK(length(feed) BETWEEN 1 AND 64),
    entry_id TEXT NOT NULL CHECK(length(entry_id) BETWEEN 1 AND 512),
    revision INTEGER NOT NULL,
    deleted INTEGER NOT NULL CHECK(deleted IN (0, 1)),
    title TEXT NOT NULL CHECK(length(title) <= 1024),
    subtitle TEXT NOT NULL CHECK(length(subtitle) <= 1024),
    icon TEXT NOT NULL CHECK(length(icon) BETWEEN 1 AND 32),
    last_used INTEGER,
    keywords TEXT NOT NULL CHECK(length(keywords) <= 2048),
    PRIMARY KEY(plugin_id, feed, entry_id)
) STRICT";

/// The plugin contract's bounds (`syrinx_plugin_host::limits`), which the host checks equal.
pub const MAX_PLUGIN_FEED_CURSOR_BYTES: usize = 1024;
pub const MAX_PLUGIN_FEED_ENTRY_ID_BYTES: usize = 512;
pub const MAX_PLUGIN_FEED_KEYWORDS: usize = 16;
/// Characters of a title or subtitle, and of one keyword; the host allows 200 and 120.
const MAX_TEXT_CHARS: usize = 1024;
const MAX_KEYWORD_CHARS: usize = 120;
/// Ends each stored keyword, so no list of keywords reads back as another; no keyword may
/// hold it (the plugin host refuses one that does).
pub const PLUGIN_FEED_KEYWORD_END: char = '\u{1f}';

/// One tier 1 row as a plugin's feed sent it.
#[derive(Clone, Eq, PartialEq)]
pub struct PluginFeedEntry {
    pub entry_id: String,
    pub revision: u64,
    pub title: String,
    pub subtitle: String,
    /// The plugin contract's `icon` name, such as `document`.
    pub icon: String,
    /// Unix milliseconds, as the plugin contract's `timestamp`.
    pub last_used: Option<i64>,
    pub keywords: Vec<String>,
}

impl fmt::Debug for PluginFeedEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Entry ids and text can be the owner's notes; they never reach a log.
        formatter
            .debug_struct("PluginFeedEntry")
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

/// A tier 1 row with the plugin and feed it belongs to, as start-up reads it.
#[derive(Clone, Eq, PartialEq)]
pub struct StoredFeedEntry {
    pub plugin_id: String,
    pub feed: String,
    pub entry: PluginFeedEntry,
}

impl fmt::Debug for StoredFeedEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StoredFeedEntry")
            .field("plugin_id", &self.plugin_id)
            .field("feed", &self.feed)
            .field("entry", &self.entry)
            .finish()
    }
}

/// One change to a feed's tier 1 rows.
#[derive(Clone, Eq, PartialEq)]
pub enum FeedRowChange {
    Upsert(PluginFeedEntry),
    /// Removes the row with this entry id, keeping its revision as a tombstone.
    Delete(String),
}

impl fmt::Debug for FeedRowChange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Upsert(entry) => formatter.debug_tuple("Upsert").field(entry).finish(),
            Self::Delete(_) => formatter.write_str("Delete(..)"),
        }
    }
}

/// One pulled batch's tier 1 rows and the cursor after it, committed in one transaction.
#[derive(Clone, Eq, PartialEq)]
pub struct PluginFeedCommit {
    pub plugin_id: String,
    pub feed: String,
    /// Removes every row of the feed first: a `reset`, or a pull from the feed's start.
    pub clear_first: bool,
    /// Applied in order.
    pub changes: Vec<FeedRowChange>,
    pub next_cursor: String,
}

impl fmt::Debug for PluginFeedCommit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The cursor is the plugin's own and may name the owner's data.
        formatter
            .debug_struct("PluginFeedCommit")
            .field("plugin_id", &self.plugin_id)
            .field("feed", &self.feed)
            .field("clear_first", &self.clear_first)
            .field("changes", &self.changes)
            .finish_non_exhaustive()
    }
}

/// What a commit did with one change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeedRowOutcome {
    /// Written or removed.
    Applied,
    /// The same revision is already held: a replay, and nothing changed.
    Unchanged,
    /// A newer revision is held, so the change was refused.
    Stale,
    /// A delete of an id the feed does not hold, or holds only as a tombstone.
    Absent,
}

fn invalid() -> StoreError {
    StoreError::InvalidEncoding(EncodingField::Plugin)
}

fn read_error(error: rusqlite::Error) -> StoreError {
    database_error(StoreOperation::Read, error)
}

fn write_error(error: rusqlite::Error) -> StoreError {
    database_error(StoreOperation::Write, error)
}

/// Checks a whole commit before anything is written, so a refused value never reaches SQLite;
/// the application also checks a pulled batch with it before writing to either search tier.
pub fn validate_plugin_feed_commit(commit: &PluginFeedCommit) -> Result<(), StoreError> {
    crate::plugins::validate_plugin_id(&commit.plugin_id)?;
    crate::plugins::validate_plugin_id(&commit.feed)?;
    if commit.next_cursor.len() > MAX_PLUGIN_FEED_CURSOR_BYTES {
        return Err(invalid());
    }
    commit.changes.iter().try_for_each(|change| match change {
        FeedRowChange::Upsert(entry) => validate_entry(entry),
        FeedRowChange::Delete(entry_id) => validate_entry_id(entry_id),
    })
}

fn validate_entry_id(entry_id: &str) -> Result<(), StoreError> {
    if entry_id.is_empty() || entry_id.len() > MAX_PLUGIN_FEED_ENTRY_ID_BYTES {
        return Err(invalid());
    }
    Ok(())
}

fn validate_entry(entry: &PluginFeedEntry) -> Result<(), StoreError> {
    validate_entry_id(&entry.entry_id)?;
    let within = |text: &str, most: usize| text.chars().count() <= most;
    let texts_fit = within(&entry.title, MAX_TEXT_CHARS) && within(&entry.subtitle, MAX_TEXT_CHARS);
    let icon_fits = !entry.icon.is_empty() && within(&entry.icon, 32);
    let keywords_fit = entry.keywords.len() <= MAX_PLUGIN_FEED_KEYWORDS
        && entry.keywords.iter().all(|keyword| {
            within(keyword, MAX_KEYWORD_CHARS) && !keyword.contains(PLUGIN_FEED_KEYWORD_END)
        });
    if !texts_fit || !icon_fits || !keywords_fit {
        return Err(invalid());
    }
    Ok(())
}

/// Applies one checked commit in its own transaction: all of it is kept, or none of it.
pub(crate) fn commit(
    connection: &mut Connection,
    commit: &PluginFeedCommit,
) -> Result<Vec<FeedRowOutcome>, StoreError> {
    validate_plugin_feed_commit(commit)?;
    let transaction = Transaction::new(connection, TransactionBehavior::Immediate)
        .map_err(|error| database_error(StoreOperation::BeginWrite, error))?;
    let applied = apply(&transaction, commit);
    crate::records::finish_transaction(transaction, applied)
}

/// The commit's statements, in order: the optional clear, each change, then the cursor.
fn apply(
    transaction: &Transaction<'_>,
    commit: &PluginFeedCommit,
) -> Result<Vec<FeedRowOutcome>, StoreError> {
    let feed = (commit.plugin_id.as_str(), commit.feed.as_str());
    if commit.clear_first {
        transaction
            .execute(
                "DELETE FROM plugin_feed_entry WHERE plugin_id = ?1 AND feed = ?2",
                params![feed.0, feed.1],
            )
            .map_err(write_error)?;
    }
    let outcomes = commit
        .changes
        .iter()
        .map(|change| match change {
            FeedRowChange::Upsert(entry) => upsert(transaction, feed, entry),
            FeedRowChange::Delete(entry_id) => delete(transaction, feed, entry_id),
        })
        .collect::<Result<Vec<_>, _>>()?;
    transaction
        .execute(
            "INSERT INTO plugin_feed_cursor(plugin_id, feed, cursor) VALUES (?1, ?2, ?3)
             ON CONFLICT(plugin_id, feed) DO UPDATE SET cursor = excluded.cursor",
            params![feed.0, feed.1, commit.next_cursor],
        )
        .map_err(write_error)?;
    Ok(outcomes)
}

/// Writes `entry` unless the feed holds the same or a newer revision of it, a tombstone's
/// included.
fn upsert(
    connection: &Connection,
    (plugin_id, feed): (&str, &str),
    entry: &PluginFeedEntry,
) -> Result<FeedRowOutcome, StoreError> {
    let held = held_revision(connection, plugin_id, feed, &entry.entry_id)?;
    match held {
        Some(held) if held == entry.revision => return Ok(FeedRowOutcome::Unchanged),
        Some(held) if held > entry.revision => return Ok(FeedRowOutcome::Stale),
        _ => {}
    }
    connection
        .execute(
            "INSERT OR REPLACE INTO plugin_feed_entry(plugin_id, feed, entry_id, revision, deleted,
                 title, subtitle, icon, last_used, keywords)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, ?8, ?9)",
            params![
                plugin_id,
                feed,
                entry.entry_id,
                entry.revision.cast_signed(),
                entry.title,
                entry.subtitle,
                entry.icon,
                entry.last_used,
                encode_keywords(&entry.keywords),
            ],
        )
        .map_err(write_error)?;
    Ok(FeedRowOutcome::Applied)
}

/// Turns the held row into a tombstone at its revision; its text is not kept.
fn delete(
    connection: &Connection,
    (plugin_id, feed): (&str, &str),
    entry_id: &str,
) -> Result<FeedRowOutcome, StoreError> {
    let removed = connection
        .execute(
            "UPDATE plugin_feed_entry
             SET deleted = 1, title = '', subtitle = '', last_used = NULL, keywords = ''
             WHERE plugin_id = ?1 AND feed = ?2 AND entry_id = ?3 AND deleted = 0",
            params![plugin_id, feed, entry_id],
        )
        .map_err(write_error)?;
    Ok(if removed > 0 {
        FeedRowOutcome::Applied
    } else {
        FeedRowOutcome::Absent
    })
}

/// Revisions are unsigned; SQLite keeps their bits in a signed integer and Rust compares them.
fn held_revision(
    connection: &Connection,
    plugin_id: &str,
    feed: &str,
    entry_id: &str,
) -> Result<Option<u64>, StoreError> {
    connection
        .query_row(
            "SELECT revision FROM plugin_feed_entry
             WHERE plugin_id = ?1 AND feed = ?2 AND entry_id = ?3",
            params![plugin_id, feed, entry_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map(|held| held.map(i64::cast_unsigned))
        .map_err(write_error)
}

fn encode_keywords(keywords: &[String]) -> String {
    let mut stored = String::new();
    for keyword in keywords {
        stored.push_str(keyword);
        stored.push(PLUGIN_FEED_KEYWORD_END);
    }
    stored
}

fn decode_keywords(stored: &str) -> Vec<String> {
    stored
        .split_terminator(PLUGIN_FEED_KEYWORD_END)
        .map(str::to_owned)
        .collect()
}

pub(crate) fn cursor(
    connection: &Connection,
    plugin_id: &str,
    feed: &str,
) -> Result<Option<String>, StoreError> {
    connection
        .query_row(
            "SELECT cursor FROM plugin_feed_cursor WHERE plugin_id = ?1 AND feed = ?2",
            params![plugin_id, feed],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_error)
}

/// Every stored row of every feed, tombstones left out, in a stable order.
pub(crate) fn entries(connection: &Connection) -> Result<Vec<StoredFeedEntry>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT plugin_id, feed, entry_id, revision, title, subtitle, icon, last_used, keywords
             FROM plugin_feed_entry WHERE deleted = 0 ORDER BY plugin_id, feed, entry_id",
        )
        .map_err(read_error)?;
    let rows = statement
        .query_map([], |row| {
            let keywords: String = row.get(8)?;
            let entry = PluginFeedEntry {
                entry_id: row.get(2)?,
                revision: row.get::<_, i64>(3)?.cast_unsigned(),
                title: row.get(4)?,
                subtitle: row.get(5)?,
                icon: row.get(6)?,
                last_used: row.get(7)?,
                keywords: decode_keywords(&keywords),
            };
            Ok(StoredFeedEntry {
                plugin_id: row.get(0)?,
                feed: row.get(1)?,
                entry,
            })
        })
        .map_err(read_error)?;
    rows.collect::<Result<_, _>>().map_err(read_error)
}

/// Rows in both tables, which a migration must leave empty.
pub(crate) fn count(connection: &Connection) -> Result<i64, StoreError> {
    connection
        .query_row(
            "SELECT (SELECT count(*) FROM plugin_feed_cursor)
                  + (SELECT count(*) FROM plugin_feed_entry)",
            [],
            |row| row.get(0),
        )
        .map_err(read_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_read_back_as_written_whatever_their_number_or_emptiness() {
        let lists: [&[&str]; 4] = [&[], &[""], &["a", ""], &["synthetic", "made-up words"]];
        for list in lists {
            let keywords: Vec<String> = list.iter().map(|keyword| (*keyword).to_owned()).collect();
            assert_eq!(decode_keywords(&encode_keywords(&keywords)), keywords);
        }
    }
}
