//! The pages the palette finds by their kept text, and the rules for what it may show (#54).

use std::{fmt, ops::Range, time::Instant};

use futures::channel::mpsc::UnboundedSender;
use syrinx_core::{PageTextPolicy, usage::Timing};
use syrinx_search::{Scope, fulltext::FullTextHit};

use super::{PageTextSetting, SessionError, SessionEvent, Worker, emit, utc_seconds};

/// The most pages one reply lists, under the catalogue's own rows.
pub(super) const PAGES: usize = 5;

/// Fewer typed characters than this match too much of every page to be worth a lookup.
const MIN_CHARACTERS: usize = 3;

/// What a lookup was doing when it failed.
pub(super) const SEARCHING: &str = "searching page text";

/// A page whose kept text holds the typed words.
#[derive(Clone, PartialEq, Eq)]
pub struct FoundPage {
    pub address: String,
    pub title: String,
    /// The bounded passage of its text that matched.
    pub passage: String,
    /// Ascending, disjoint UTF-8 byte ranges of `passage` that matched.
    pub matches: Vec<Range<usize>>,
}

impl fmt::Debug for FoundPage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Addresses, titles and page text are the owner's browsing; they never reach a log.
        formatter.write_str("FoundPage { contents: [redacted] }")
    }
}

impl From<FullTextHit> for FoundPage {
    fn from(hit: FullTextHit) -> Self {
        Self {
            address: hit.id,
            title: hit.title,
            passage: hit.snippet,
            matches: hit.highlights,
        }
    }
}

/// An answered query whose pages are still to be looked up.
pub(super) struct PageLookup {
    request_id: u64,
    text: String,
}

impl PageLookup {
    /// The lookup a query asks for, or `None` in a view that lists no visited pages.
    pub(super) fn wanted(request_id: u64, text: &str, scope: Scope) -> Option<Self> {
        let lists_visits = matches!(scope, Scope::Everything | Scope::History);
        if !lists_visits || text.trim().chars().count() < MIN_CHARACTERS {
            return None;
        }
        Some(Self {
            request_id,
            text: text.to_owned(),
        })
    }
}

/// The rules a page must pass to be shown now, or `None` while nothing may be. A site list the
/// rules refuse shows nothing rather than everything, as capture itself fails closed on it.
pub(super) fn showable(setting: &PageTextSetting) -> Option<PageTextPolicy> {
    if !setting.on {
        return None;
    }
    PageTextPolicy::new(&setting.excluded).ok()
}

impl Worker {
    /// Answers the waiting lookup under the setting as it is now. It runs after the catalogue's
    /// rows went out, so the full-text index never delays them (D48). Nothing is sent while
    /// capture is off.
    pub(super) fn look_up_pages(
        &mut self,
        events: &UnboundedSender<SessionEvent>,
    ) -> Result<(), SessionError> {
        let Some(lookup) = self.page_lookup.take() else {
            return Ok(());
        };
        let Some(policy) = showable(self.page_text_setting.setting()) else {
            return Ok(());
        };
        let now = utc_seconds()?;
        let started = self.usage.is_enabled().then(Instant::now);
        let pages = self.page_text.search(&lookup.text, &policy, now);
        if let Some(started) = started {
            self.usage.time(Timing::SearchTier2, started.elapsed());
        }
        emit(
            events,
            SessionEvent::FoundPages {
                request_id: lookup.request_id,
                pages,
            },
        )
    }
}
