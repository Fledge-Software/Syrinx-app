//! Turns search hits into bounded, safely highlighted palette rows, in order and in sections, and previews.

use std::collections::HashSet;
use std::ops::Range;

use syrinx_core::{AddressPrefix, TabModelSnapshot, address_prefix, live_instance};
use syrinx_search::{SearchHit, SearchResults, Target};

/// Row text is cut here so one long title cannot stall layout.
pub const ROW_TEXT_LIMIT: usize = 160;
/// The preview shows more of an address, but still a bounded amount.
pub const PREVIEW_TEXT_LIMIT: usize = 600;

/// Text clipped to a character limit, with highlight ranges that are valid for it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Highlighted {
    pub text: String,
    pub ranges: Vec<Range<usize>>,
}

/// Clips `text` to `limit` characters and keeps only well-formed highlight ranges inside it.
///
/// Search already widens matcher positions to whole UTF-8 scalars (`syrinx-search`'s highlights),
/// but the renderer requires character boundaries, so anything else is dropped here rather than
/// trusted. Overlapping or unsorted ranges are merged; a valid match may have none at all.
pub fn highlight(text: &str, ranges: &[Range<usize>], limit: usize) -> Highlighted {
    let (clipped, kept) = clip(text, limit);
    let mut valid: Vec<Range<usize>> = ranges
        .iter()
        .filter(|range| is_scalar_range(text, range))
        .map(|range| range.start..range.end.min(kept))
        .filter(|range| range.start < range.end)
        .collect();
    valid.sort_unstable_by_key(|range| range.start);
    Highlighted {
        text: clipped,
        ranges: merge(valid),
    }
}

fn is_scalar_range(text: &str, range: &Range<usize>) -> bool {
    range.start < range.end
        && range.end <= text.len()
        && text.is_char_boundary(range.start)
        && text.is_char_boundary(range.end)
}

/// Returns the shown text and how many original bytes it keeps.
fn clip(text: &str, limit: usize) -> (String, usize) {
    let Some((cut, _)) = text.char_indices().nth(limit) else {
        return (text.to_owned(), text.len());
    };
    (format!("{}…", &text[..cut]), cut)
}

fn merge(ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        if let Some(previous) = merged.last_mut()
            && range.start <= previous.end
        {
            previous.end = previous.end.max(range.end);
            continue;
        }
        merged.push(range);
    }
    merged
}

/// The row's main text: its title, or its address when the title is empty.
pub fn row_title(hit: &SearchHit, limit: usize) -> Highlighted {
    if hit.title.is_empty() {
        return highlight(&hit.address, &hit.address_matches, limit);
    }
    highlight(&hit.title, &hit.title_matches, limit)
}

/// The palette's groups, in the order the design lists them.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Section {
    Commands,
    OpenTabs,
    Bookmarks,
    History,
}

impl Section {
    pub fn of(target: Target) -> Self {
        match target {
            Target::Command(_) => Self::Commands,
            Target::Tab(_) => Self::OpenTabs,
            Target::Saved(_) => Self::Bookmarks,
            Target::History(_) => Self::History,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Commands => "Commands",
            Self::OpenTabs => "Open tabs",
            Self::Bookmarks => "Bookmarks",
            Self::History => "History",
        }
    }
}

/// How closely a row matches the typed text. A row that only holds the typed letters scattered
/// through it ranks below every row holding the text as a word or an address prefix (2026-10-25).
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Strength {
    /// Each typed word starts a word of the title or address, or the text starts the address.
    Word,
    /// Only a fuzzy match: letters in order with gaps, or with a typo.
    Scattered,
}

impl Strength {
    pub fn of(hit: &SearchHit, text: &str) -> Self {
        let text = text.trim();
        if text.is_empty() || address_prefix(text, &hit.address) != AddressPrefix::No {
            return Self::Word;
        }
        let title = hit.title.to_lowercase();
        let address = hit.address.to_lowercase();
        let every_word_found = text
            .to_lowercase()
            .split_whitespace()
            .all(|word| starts_a_word(&title, word) || starts_a_word(&address, word));
        if every_word_found {
            Self::Word
        } else {
            Self::Scattered
        }
    }
}

/// Whether `word` occurs in `text` where a word begins: at the start, or after a character that
/// is not a letter or digit (a space, `.`, `/`, `-` and so on).
fn starts_a_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(start, _)| {
        text[..start]
            .chars()
            .next_back()
            .is_none_or(|previous| !previous.is_alphanumeric())
    })
}

/// Prepares worker results for display: one row per destination, in the worker's ranking.
///
/// A saved page's live instance appears as its saved row, since activating that row focuses
/// the same live page (D43); listing both would offer two rows for one destination.
pub fn arrange(mut results: SearchResults, model: &TabModelSnapshot) -> SearchResults {
    let mut seen = HashSet::new();
    let mut hits = Vec::with_capacity(results.hits.len());
    for mut hit in results.hits {
        if let Target::Tab(id) = hit.target
            && let Some(saved) = saved_page_of(model, id)
        {
            hit.target = Target::Saved(saved);
        }
        if seen.insert(hit.target) {
            hits.push(hit);
        }
    }
    results.hits = hits;
    results
}

/// Orders rows for the list, the one place that does so: word matches first, then scattered
/// ones, each grouped by section. A stable sort keeps the worker's ranking inside each group.
pub fn order(hits: &mut [SearchHit], text: &str) {
    hits.sort_by_cached_key(|hit| (Strength::of(hit, text), Section::of(hit.target)));
}

fn saved_page_of(
    model: &TabModelSnapshot,
    id: syrinx_core::TabId,
) -> Option<syrinx_core::SavedPageId> {
    let saved = model.tabs.iter().find(|tab| tab.id == id)?.saved_page_id?;
    // Only an association the current model still holds may redirect the row.
    (live_instance(model, saved) == Some(id)).then_some(saved)
}

/// The typed row's name when the text is a search, shared by the row and its preview.
pub fn search_label(entry: &str) -> String {
    format!("{entry} — Search the web")
}

/// The typed row's name when the text is an address, shared by the row and its preview.
pub fn address_label(entry: &str, address: &str) -> String {
    format!("{entry} — Go to {address}")
}

/// What the preview pane says about the selected row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Preview {
    pub kind: &'static str,
    pub title: Highlighted,
    pub detail: Option<Highlighted>,
    pub hint: String,
}

impl Preview {
    pub fn address(entry: &str, address: &str) -> Self {
        Self {
            kind: "Address",
            title: highlight(&address_label(entry, address), &[], PREVIEW_TEXT_LIMIT),
            detail: None,
            hint: "Enter opens this address in the current tab. Matches, if any, are below it."
                .to_owned(),
        }
    }

    pub fn search(entry: &str) -> Self {
        Self {
            kind: "Web search",
            title: highlight(&search_label(entry), &[], PREVIEW_TEXT_LIMIT),
            detail: None,
            hint: "Enter searches the web, with Google, in the current tab. Matches, if any, are below it."
                .to_owned(),
        }
    }

    pub fn page(hit: &SearchHit) -> Self {
        let (kind, hint) = match hit.target {
            Target::Tab(_) => ("Open tab", "Enter switches to this tab."),
            Target::Saved(_) => (
                "Saved page",
                "Enter switches to its open tab, or opens it if it is not loaded.",
            ),
            Target::History(_) => ("History", "Enter opens this page."),
            Target::Command(_) => ("Command", "Enter runs this command."),
        };
        let detail = (!hit.title.is_empty() && !hit.address.is_empty())
            .then(|| highlight(&hit.address, &hit.address_matches, PREVIEW_TEXT_LIMIT));
        Self {
            kind,
            title: row_title(hit, PREVIEW_TEXT_LIMIT),
            detail,
            hint: hint.to_owned(),
        }
    }

    pub fn command(hit: &SearchHit, plugin: Option<&str>, shortcut: Option<String>) -> Self {
        let kind = if plugin.is_some() {
            "Plugin action"
        } else {
            "Browser command"
        };
        let hint = match shortcut {
            Some(shortcut) => format!("Enter runs it here; {shortcut} also runs it."),
            None => "Enter runs it here.".to_owned(),
        };
        Self {
            kind,
            title: highlight(&hit.title, &hit.title_matches, PREVIEW_TEXT_LIMIT),
            detail: plugin.map(|plugin| highlight(plugin, &[], PREVIEW_TEXT_LIMIT)),
            hint,
        }
    }
}

#[cfg(test)]
mod tests {
    use syrinx_core::{Activation, SavedPageId, TabId, TabKind, TabMetadata, TabModel};
    use syrinx_search::ScopeCounts;

    use super::*;

    #[test]
    fn multibyte_highlights_stay_on_scalar_boundaries() {
        // "é" is two bytes, "😀" four, "東" three.
        let text = "é😀東x";
        let shown = highlight(text, &[0..2, 2..6, 6..9], 10);
        assert_eq!(shown.text, text);
        assert_eq!(shown.ranges, vec![Range { start: 0, end: 9 }]);
        for range in &shown.ranges {
            assert!(shown.text.is_char_boundary(range.start));
            assert!(shown.text.is_char_boundary(range.end));
        }
        // Ranges that split a scalar or run past the text are never handed to the renderer.
        let unsafe_ranges = highlight(text, &[1..2, 3..4, 9..20, Range { start: 4, end: 2 }], 10);
        assert!(unsafe_ranges.ranges.is_empty());
        // Unsorted and overlapping ranges are normalised.
        assert_eq!(
            highlight(text, &[6..9, 0..2, 0..6], 10).ranges,
            vec![Range { start: 0, end: 9 }]
        );
    }

    #[test]
    fn clipping_keeps_highlights_inside_the_shown_text() {
        let text = "ééééé";
        let shown = highlight(text, &[0..2, 4..10], 3);
        assert_eq!(shown.text, "ééé…");
        assert_eq!(shown.ranges, vec![0..2, 4..6]);
        assert!(shown.ranges.iter().all(|range| range.end <= "ééé".len()));
        let empty = highlight("", &[], 3);
        assert_eq!(empty.text, "");
        assert!(empty.ranges.is_empty());
        // A valid match without positions still renders, just without highlights.
        assert!(highlight("Synthetic", &[], 3).ranges.is_empty());
    }

    fn hit(target: Target, title: &str) -> SearchHit {
        SearchHit {
            target,
            score: 1,
            title: title.to_owned(),
            address: "https://arrange.example.test/".to_owned(),
            title_matches: Vec::new(),
            address_matches: vec![Range { start: 0, end: 5 }],
        }
    }

    #[test]
    fn empty_titles_fall_back_to_highlighted_addresses() {
        let untitled = hit(Target::Tab(TabId::new(1).expect("tab")), "");
        let title = row_title(&untitled, ROW_TEXT_LIMIT);
        assert_eq!(title.text, "https://arrange.example.test/");
        assert_eq!(title.ranges, vec![Range { start: 0, end: 5 }]);
        let preview = Preview::page(&untitled);
        assert_eq!(preview.detail, None);
        assert_eq!(preview.kind, "Open tab");
    }

    #[test]
    fn saved_instances_collapse_into_one_saved_row_and_sections_group() {
        let mut model = TabModel::new();
        let saved_tab = model
            .open(
                TabKind::Web,
                TabMetadata {
                    title: "Synthetic saved".to_owned(),
                    address: "https://saved.example.test/".to_owned(),
                    ..TabMetadata::default()
                },
                Activation::Foreground,
            )
            .expect("open synthetic tab");
        let saved = model.save_page(saved_tab).expect("save synthetic tab");
        let ordinary = model
            .open(TabKind::Web, TabMetadata::default(), Activation::Foreground)
            .expect("open ordinary tab");
        let unknown = TabId::new(99).expect("unknown tab");
        let other_saved = SavedPageId::new(42).expect("other saved page");
        let results = SearchResults {
            generation: 1,
            counts: ScopeCounts::default(),
            hits: vec![
                hit(Target::Saved(other_saved), "Other saved"),
                hit(Target::Tab(saved_tab), "Live instance"),
                hit(Target::Tab(ordinary), "Ordinary"),
                hit(Target::Saved(saved), "Saved row"),
                hit(Target::Tab(unknown), "Unknown"),
            ],
        };
        let mut arranged = arrange(results, &model.snapshot());
        // Collapsing keeps the worker's ranking; only the saved page with a live tab moves.
        let ranked: Vec<_> = arranged.hits.iter().map(|hit| hit.target).collect();
        assert_eq!(
            ranked,
            [
                Target::Saved(other_saved),
                Target::Saved(saved),
                Target::Tab(ordinary),
                Target::Tab(unknown),
            ]
        );
        order(&mut arranged.hits, "");
        let targets: Vec<_> = arranged.hits.iter().map(|hit| hit.target).collect();
        assert_eq!(
            targets,
            [
                Target::Tab(ordinary),
                Target::Tab(unknown),
                Target::Saved(other_saved),
                Target::Saved(saved),
            ]
        );
        // The better-ranked duplicate wins, so the live instance's metadata is shown.
        assert_eq!(arranged.hits[3].title, "Live instance");
    }

    fn page(target: Target, title: &str, address: &str) -> SearchHit {
        SearchHit {
            title: title.to_owned(),
            address: address.to_owned(),
            ..hit(target, title)
        }
    }

    fn history(key: u64) -> Target {
        Target::History(std::num::NonZeroU64::new(key).expect("synthetic history key"))
    }

    #[test]
    fn a_word_or_address_prefix_is_strong_and_scattered_letters_are_weak() {
        let tab = Target::Tab(TabId::new(1).expect("tab"));
        let strong = [
            ("youtube", "YouTube", "https://youtube.example/"),
            ("youtube", "Synthetic home", "https://www.youtube.example/"),
            (
                "youtube",
                "youtube - Synthetic search",
                "https://search.example.test/",
            ),
            (
                "YOUTUBE",
                "Watch on YouTube",
                "https://videos.example.test/",
            ),
            (
                "road",
                "Studios/Terra roadmap",
                "https://example.test/terra",
            ),
            (
                "terra road",
                "Studios/Terra roadmap",
                "https://example.test/x",
            ),
            ("example.test/te", "Synthetic", "https://example.test/terra"),
            ("", "Anything", "https://example.test/"),
        ];
        for (text, title, address) in strong {
            let row = page(tab, title, address);
            assert_eq!(
                Strength::of(&row, text),
                Strength::Word,
                "{text} in {title}"
            );
        }
        let weak = [
            // Letters in order with gaps, or mid-word, are only a fuzzy match.
            (
                "youtube",
                "Studios/Terra roadmap",
                "https://example.test/terra",
            ),
            ("tube", "YouTube", "https://youtube.example/"),
            ("ample", "Synthetic", "https://example.test/"),
            (
                "terra zzz",
                "Studios/Terra roadmap",
                "https://example.test/x",
            ),
        ];
        for (text, title, address) in weak {
            let row = page(tab, title, address);
            assert_eq!(
                Strength::of(&row, text),
                Strength::Scattered,
                "{text} in {title}"
            );
        }
    }

    #[test]
    fn a_scattered_tab_ranks_below_a_word_match_in_history() {
        let tab = Target::Tab(TabId::new(1).expect("tab"));
        let saved = Target::Saved(SavedPageId::new(1).expect("saved"));
        // The worker ranked the open tab first: its open-tab boost outweighs the history row.
        let mut hits = vec![
            page(tab, "Studios/Terra roadmap", "https://example.test/terra"),
            page(
                saved,
                "Synthetic video notes",
                "https://notes.example.test/",
            ),
            page(history(1), "YouTube", "https://youtube.example/"),
            page(
                history(2),
                "youtube - Synthetic search",
                "https://search.example.test/",
            ),
        ];
        order(&mut hits, "youtube");
        let targets: Vec<_> = hits.iter().map(|hit| hit.target).collect();
        // Word matches lead in their section; every scattered row follows, still listed.
        assert_eq!(targets, [history(1), history(2), tab, saved]);
    }

    #[test]
    fn single_letters_keep_every_match_and_rank_word_starts_first() {
        let tab = Target::Tab(TabId::new(1).expect("tab"));
        let mut hits = vec![
            page(tab, "Synthetic toy", "https://example.test/"),
            page(history(1), "Yarn", "https://yarn.example.test/"),
        ];
        order(&mut hits, "y");
        let targets: Vec<_> = hits.iter().map(|hit| hit.target).collect();
        assert_eq!(targets, [history(1), tab], "both stay listed");
    }

    #[test]
    fn the_typed_row_names_the_text_before_what_enter_does() {
        assert_eq!(search_label("youtube"), "youtube — Search the web");
        assert_eq!(
            address_label("example.com", "https://example.com/"),
            "example.com — Go to https://example.com/"
        );
        assert_eq!(
            Preview::search("youtube").title.text,
            "youtube — Search the web"
        );
    }
}
