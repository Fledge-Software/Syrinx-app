//! Turns one pulled batch into what each tier keeps, and checks all of it against both tiers'
//! rules before anything is written (#141).

use syrinx_plugin_host::bindings::exports::syrinx::plugin::search_feed::{
    Batch, CatalogueEntry, Change, Document,
};
use syrinx_plugin_host::bindings::syrinx::plugin::types::Icon;
use syrinx_search::fulltext::{self, Deletion, FullTextDocument, Namespace};
use syrinx_search::{Entry, Target, Usage};
use syrinx_store::{FeedRowChange, PluginFeedCommit, PluginFeedEntry};

use super::FeedError;
use super::sinks::FeedIndex;

/// The part of a batch that is kept: everything after its last `reset`, which erases what
/// came before it, and whether the feed's rows and text are cleared first.
pub(super) struct Kept<'a> {
    pub(super) changes: &'a [Change],
    pub(super) clear_first: bool,
}

pub(super) fn kept(batch: &Batch, from_start: bool) -> Kept<'_> {
    let reset = batch
        .changes
        .iter()
        .rposition(|change| matches!(change, Change::Reset));
    Kept {
        changes: reset.map_or(&batch.changes[..], |last| &batch.changes[last + 1..]),
        clear_first: from_start || reset.is_some(),
    }
}

/// Checks the whole batch against the store's and the index's rules, so a value either tier
/// would refuse stops the batch before either is written. The host refuses these first
/// (`validate::batch`); this is the backstop.
pub(super) fn check(
    commit: &PluginFeedCommit,
    namespace: &Namespace,
    changes: &[Change],
) -> Result<(), FeedError> {
    syrinx_store::validate_plugin_feed_commit(commit).map_err(FeedError::Store)?;
    for change in changes {
        let checked = match change {
            Change::UpsertDocument(document) => {
                fulltext::Change::Upsert(full_text_document(namespace, document, 0))
            }
            Change::Delete(id) => fulltext::Change::Delete(deletion(namespace, id, 0, 0)),
            Change::UpsertEntry(_) | Change::Reset => continue,
        };
        checked.validate().map_err(FeedError::Index)?;
    }
    Ok(())
}

/// The index's form of one change; `None` for what tier 2 does not hold.
pub(super) fn document_change(
    index: &dyn FeedIndex,
    namespace: &Namespace,
    change: &Change,
    now: i64,
) -> Result<Option<fulltext::Change>, FeedError> {
    match change {
        Change::UpsertDocument(document) => Ok(Some(fulltext::Change::Upsert(full_text_document(
            namespace, document, now,
        )))),
        Change::Delete(id) => {
            // The plugin names no revision; the tombstone covers the one held, if any.
            let held = index
                .held_revision(namespace, id)
                .map_err(FeedError::Index)?;
            Ok(held
                .map(|revision| fulltext::Change::Delete(deletion(namespace, id, revision, now))))
        }
        Change::UpsertEntry(_) | Change::Reset => Ok(None),
    }
}

fn deletion(namespace: &Namespace, id: &str, revision: u64, now: i64) -> Deletion {
    Deletion {
        namespace: namespace.clone(),
        id: id.to_owned(),
        revision,
        deleted_at: now,
    }
}

fn full_text_document(namespace: &Namespace, document: &Document, now: i64) -> FullTextDocument {
    FullTextDocument {
        namespace: namespace.clone(),
        id: document.id.clone(),
        revision: document.revision,
        captured_at: now,
        title: document.title.clone(),
        body: document.body.clone(),
    }
}

/// The batch's tier 1 changes; a deletion reaches both tiers.
pub(super) fn row_changes(changes: &[Change]) -> Vec<FeedRowChange> {
    changes
        .iter()
        .filter_map(|change| match change {
            Change::UpsertEntry(entry) => Some(FeedRowChange::Upsert(stored_entry(entry))),
            Change::Delete(id) => Some(FeedRowChange::Delete(id.clone())),
            Change::UpsertDocument(_) | Change::Reset => None,
        })
        .collect()
}

fn stored_entry(entry: &CatalogueEntry) -> PluginFeedEntry {
    PluginFeedEntry {
        entry_id: entry.id.clone(),
        revision: entry.revision,
        title: entry.title.clone(),
        subtitle: entry.subtitle.clone(),
        icon: icon_name(entry.icon).to_owned(),
        last_used: entry.last_used,
        keywords: entry.keywords.clone(),
    }
}

/// The contract's icon name, as `wit/v0_1/plugin.wit` spells it.
fn icon_name(icon: Icon) -> &'static str {
    match icon {
        Icon::Plugin => "plugin",
        Icon::Document => "document",
        Icon::Person => "person",
        Icon::Folder => "folder",
        Icon::Terminal => "terminal",
        Icon::Search => "search",
        Icon::Star => "star",
        Icon::Warning => "warning",
        Icon::File => "file",
        Icon::Code => "code",
        Icon::Git => "git",
        Icon::Diff => "diff",
        Icon::Symbol => "symbol",
        Icon::Link => "link",
        Icon::Tag => "tag",
        Icon::Calendar => "calendar",
        Icon::Mail => "mail",
        Icon::List => "list",
        Icon::Image => "image",
        Icon::Settings => "settings",
        Icon::Error => "error",
    }
}

/// A plugin row as the catalogue matches it: the title, then the subtitle and keywords in
/// the address field. How a plugin row looks is the owner's call (#265, question 1).
pub(super) fn catalogue_entry(target: Target, entry: &PluginFeedEntry) -> Entry {
    let address = std::iter::once(entry.subtitle.as_str())
        .chain(entry.keywords.iter().map(String::as_str))
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Entry {
        target,
        title: entry.title.clone(),
        address,
        usage: Usage {
            visits: 0,
            last_used: entry.last_used.map(|millis| millis.div_euclid(1000)),
        },
        open: false,
    }
}
