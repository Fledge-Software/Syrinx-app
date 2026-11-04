//! The fixed bounds the host puts on a plugin's resources and on every value it lifts.
//!
//! The memory cap and the call deadline are provisional: #28's Windows
//! measurements (decision 3 of 2026-10-21) set the figures #135 uses.

use std::time::Duration;

/// Each store's one linear memory may grow to at most this.
pub const MEMORY_BYTES: usize = 32 * 1024 * 1024;
/// A component's core instances; Rust guests built for `wasm32-wasip2` need a handful.
pub const INSTANCES: usize = 16;
/// One memory, so the per-memory cap above is the store's whole budget.
pub const MEMORIES: usize = 1;
pub const TABLES: usize = 8;
/// Entries per table. Tables live in host memory outside the memory cap and grow
/// in host code the deadline cannot interrupt, so each is bounded too.
pub const TABLE_ELEMENTS: usize = 64 * 1024;

/// One epoch tick, and how many ticks a single export call may run.
pub const EPOCH_TICK: Duration = Duration::from_millis(10);
pub const CALL_DEADLINE_TICKS: u64 = 100;

/// Deferred calls drained after one export; a plugin that keeps queueing more is stopped.
pub const MAX_DEFERRED_PER_EXPORT: usize = 1_024;
/// Deferred calls waiting at once; an import that would queue more is refused.
pub const MAX_DEFERRED_QUEUED: usize = 1_024;

/// Views one plugin may hold open, and what may wait for each view's interface.
pub const MAX_OPEN_VIEWS: usize = 64;
pub const MAX_VIEW_EVENTS: usize = 256;
pub const MAX_VIEW_EVENT_BYTES: usize = 4 * 1024 * 1024;

/// Text the plugin hands the host.
pub const MAX_LOG_CHARS: usize = 4 * 1024;
pub const MAX_LOG_LINES: usize = 256;
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_LABEL_CHARS: usize = 200;
/// An error's or outcome's text; longer text is cut, not refused.
pub const MAX_MESSAGE_CHARS: usize = 2 * 1024;
pub const MAX_ADDRESS_BYTES: usize = 2 * 1024;
pub const MAX_JSON_BYTES: usize = 1024 * 1024;

/// A plugin's own settings.
pub const MAX_SETTINGS_KEY_BYTES: usize = 64;
pub const MAX_SETTINGS_VALUE_BYTES: usize = 64 * 1024;
pub const MAX_SETTINGS_KEYS: usize = 256;

/// Forms.
pub const MAX_FORM_FIELDS: usize = 32;
pub const MAX_CHOICE_OPTIONS: usize = 256;
pub const MAX_FIELD_TEXT_BYTES: usize = 64 * 1024;

/// Search feeds.
pub const MAX_BATCH_CHANGES: u32 = 512;
pub const MAX_ENTRY_ID_BYTES: usize = 512;
pub const MAX_DOCUMENT_BODY_BYTES: usize = 1024 * 1024;
pub const MAX_CURSOR_BYTES: usize = 1024;
/// Keywords per catalogue entry, and characters per keyword (G9, item 7).
pub const MAX_KEYWORDS: usize = 16;
pub const MAX_KEYWORD_CHARS: usize = 120;
