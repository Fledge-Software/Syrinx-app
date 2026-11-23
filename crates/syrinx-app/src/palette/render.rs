//! Draws the palette's bar, rows, scope chips and status line from `syrinx-ui`'s tokens (#265).

use gpui::{
    AnyElement, Context, Div, Entity, FontWeight, HighlightStyle, Hsla, InteractiveElement,
    IntoElement, ParentElement, Role, Stateful, StatefulInteractiveElement, Styled, StyledText,
    Toggled, div, prelude::FluentBuilder, px,
};
use gpui_component::input::{Input, InputState};
use syrinx_search::{SearchHit, Target};
use syrinx_ui::metrics::{radius, text};

use super::{
    PaletteView,
    choices::PaletteChoices,
    display::{
        Highlighted, PAGE_TEXT_LABEL, ROW_TEXT_LIMIT, Section, address_label, highlight,
        page_address, page_passage, page_title, row_title, search_label,
    },
    query::{Chip, View},
};
use crate::controls::control;
use crate::session::FoundPage;
use crate::text_field;
use crate::theme::{self, rgba};

/// The panel's `border_1`, top and bottom.
pub(super) const PANEL_BORDERS: f32 = 2.;
/// The query bar, which is all an empty palette shows.
pub(super) const BAR_HEIGHT: f32 = 52.;
/// The list's `border_t_1` under the bar.
const LIST_DIVIDER: f32 = 1.;
const ROW_HEIGHT: f32 = 36.;
/// A page found by its text: its title over two lines of the passage that matched (#54).
const PAGE_ROW_HEIGHT: f32 = 64.;
const PAGE_TITLE_LINE: f32 = 18.;
const PAGE_PASSAGE_LINE: f32 = 16.;
const ROW_GAP: f32 = 2.;
const HEADER_HEIGHT: f32 = 28.;
/// The header's `mt_1`.
const HEADER_MARGIN: f32 = 4.;
const LIST_PADDING: f32 = 6.;
/// The rows scroll past this, so a long list cannot grow the popup without limit.
const LIST_MAX_HEIGHT: f32 = 372.;
/// The fixed height of the two-line "No plugins yet" message.
const EMPTY_STATE_HEIGHT: f32 = 84.;
const CHIP_HEIGHT: f32 = 26.;
/// The padding under the chip row.
const CHIP_ROW_PADDING: f32 = 12.;
const NOTICE_HEIGHT: f32 = 30.;
const TRAILING_WIDTH: f32 = 180.;

/// The popup height that fits what is drawn for `choices` in `view`, borders and dividers
/// included; `scopes_shown` adds the chip row.
pub(super) fn height(scopes_shown: bool, choices: &PaletteChoices, view: View) -> f32 {
    let mut height = PANEL_BORDERS + BAR_HEIGHT;
    if scopes_shown {
        height += CHIP_HEIGHT + CHIP_ROW_PADDING;
    }
    let list = list_height(choices, empty_message(view).is_some());
    if list > 0. {
        height += LIST_DIVIDER + list.min(LIST_MAX_HEIGHT);
    }
    if !choices.status_line().is_empty() {
        height += NOTICE_HEIGHT;
    }
    height
}

/// How tall the rows and their section headers are together; zero when nothing is listed.
fn list_height(choices: &PaletteChoices, explains_empty: bool) -> f32 {
    let rows = choices.row_count();
    if rows == 0 {
        return if explains_empty {
            2. * LIST_PADDING + EMPTY_STATE_HEIGHT
        } else {
            0.
        };
    }
    let pages = choices.pages.len();
    let headers = section_headers(&choices.results, choices.results.len()) + usize::from(pages > 0);
    let gaps = (rows + headers - 1) as f32 * ROW_GAP;
    2. * LIST_PADDING
        + (rows - pages) as f32 * ROW_HEIGHT
        + pages as f32 * PAGE_ROW_HEIGHT
        + headers as f32 * (HEADER_HEIGHT + HEADER_MARGIN)
        + gaps
}

/// The one bar: a search mark and the query field, drawn without a frame of its own.
pub(super) fn bar(input: &Entity<InputState>) -> Div {
    let tokens = theme::current();
    div()
        .h(px(BAR_HEIGHT))
        .flex_shrink_0()
        .px_4()
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(text::SUBTITLE))
                .text_color(rgba(tokens.text_muted))
                .child("⌕"),
        )
        .child(
            div().flex_1().min_w_0().child(
                Input::new(input)
                    .appearance(false)
                    .aria_label(text_field::PALETTE_QUERY.label),
            ),
        )
}

/// The position of `row` among the list's children, which also include section headers.
pub(super) fn child_index(choices: &PaletteChoices, row: usize) -> usize {
    let lead_rows = choices.lead_rows();
    let Some(hit_index) = row.checked_sub(lead_rows) else {
        return row;
    };
    let listed = choices.results.len();
    let headers = section_headers(&choices.results, (hit_index + 1).min(listed));
    // Page rows follow every result, under their own header.
    let page_header = usize::from(hit_index >= listed);
    lead_rows + hit_index + headers + page_header
}

/// How many section headers are drawn among the first `count` hits.
fn section_headers(hits: &[SearchHit], count: usize) -> usize {
    (0..count)
        .filter(|&index| starts_section(hits, index))
        .count()
}

fn starts_section(hits: &[SearchHit], index: usize) -> bool {
    let Some(hit) = hits.get(index) else {
        return false;
    };
    index == 0 || Section::of(hits[index - 1].target) != Section::of(hit.target)
}

pub(super) fn chips(view: &PaletteView, cx: &mut Context<PaletteView>) -> Div {
    let tokens = theme::current();
    let selected_chip = view.view.chip();
    let mut row = div()
        .px_4()
        .pb(px(CHIP_ROW_PADDING))
        .flex()
        .items_center()
        .gap_2();
    for (index, chip) in Chip::ALL.into_iter().enumerate() {
        let selected = selected_chip == Some(chip);
        let count = view.choices.counts.get(chip);
        let mut element = control(
            div().id(("chip", index)),
            Role::RadioButton,
            format!("{} {count}", chip.label()),
        )
        .aria_toggled(if selected {
            Toggled::True
        } else {
            Toggled::False
        })
        .h(px(CHIP_HEIGHT))
        .px_3()
        .rounded(px(13.))
        .border_1()
        .flex()
        .items_center()
        .gap(px(7.))
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, window, cx| this.choose_chip(chip, window, cx)))
        .child(chip.label())
        .child(
            div()
                .text_color(rgba(tokens.text_muted))
                .child(count.to_string()),
        );
        element = if selected {
            element
                .bg(rgba(tokens.chip_selected))
                .border_color(rgba(tokens.chip_selected))
                .text_color(rgba(tokens.text_strong))
                .font_weight(FontWeight::MEDIUM)
        } else {
            element
                .border_color(rgba(tokens.chip_line))
                .text_color(rgba(tokens.text_secondary))
        };
        row = row.child(element);
    }
    row
}

/// The rows under the bar, or nothing when there is nothing to list or explain.
pub(super) fn list(view: &PaletteView, cx: &mut Context<PaletteView>) -> Option<Div> {
    let explanation = empty_state(view.view);
    if view.choices.row_count() == 0 && explanation.is_none() {
        return None;
    }
    let tokens = theme::current();
    let mut rows = div()
        .id("results")
        .role(Role::ListBox)
        .aria_label("Results")
        .flex_1()
        .min_h_0()
        .px(px(10.))
        .py(px(LIST_PADDING))
        .flex()
        .flex_col()
        .gap(px(ROW_GAP))
        .overflow_y_scroll()
        .track_scroll(&view.scroll);
    if let Some(address) = &view.choices.address {
        rows = rows.child(address_row(view, address, cx));
    } else if view.choices.web_search {
        rows = rows.child(search_row(view, cx));
    }
    for (index, hit) in view.choices.results.iter().enumerate() {
        if starts_section(&view.choices.results, index) {
            rows = rows.child(section_header(Section::of(hit.target)));
        }
        rows = rows.child(result_row(view, index, hit, cx));
    }
    if !view.choices.pages.is_empty() {
        rows = rows.child(header(PAGE_TEXT_LABEL));
    }
    for (index, page) in view.choices.pages.iter().enumerate() {
        rows = rows.child(page_row(view, index, page, cx));
    }
    if view.choices.row_count() == 0 {
        rows = rows.children(explanation);
    }
    Some(
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .border_t_1()
            .border_color(rgba(tokens.panel_divider))
            .child(rows),
    )
}

/// The typed row for a complete address: Enter goes there (2026-10-25).
fn address_row(view: &PaletteView, address: &str, cx: &mut Context<PaletteView>) -> AnyElement {
    let request_id = view.choices.request_id;
    let label = address_label(&view.choices.entry, address);
    let title = highlight(&label, &[], ROW_TEXT_LIMIT);
    row_frame(
        "go-to-address".into(),
        title.text.clone(),
        RowState::of(&view.choices, 0),
    )
    .on_click(cx.listener(move |this, _, window, cx| {
        if this.choices.request_id != request_id {
            return;
        }
        this.choices.select(0);
        this.submit(false, window, cx);
    }))
    .child(glyph("→"))
    .child(row_text(title))
    .into_any_element()
}

/// The typed row for plain text: Enter searches the web for it unless the owner moves down.
fn search_row(view: &PaletteView, cx: &mut Context<PaletteView>) -> AnyElement {
    let request_id = view.choices.request_id;
    let title = highlight(&search_label(&view.choices.entry), &[], ROW_TEXT_LIMIT);
    row_frame(
        "search-web".into(),
        title.text.clone(),
        RowState::of(&view.choices, 0),
    )
    .on_click(cx.listener(move |this, _, window, cx| {
        if this.choices.request_id != request_id {
            return;
        }
        this.choices.select(0);
        this.submit(false, window, cx);
    }))
    .child(glyph("⌕"))
    .child(row_text(title))
    .into_any_element()
}

fn result_row(
    view: &PaletteView,
    index: usize,
    hit: &SearchHit,
    cx: &mut Context<PaletteView>,
) -> AnyElement {
    let target = hit.target;
    let request_id = view.choices.request_id;
    let row = RowState::of(&view.choices, index + view.choices.lead_rows());
    let trailing = match view.commands.command(target) {
        Some(command) => command
            .shortcut
            .as_ref()
            .map(|shortcut| key_hint(&shortcut.label()).into_any_element()),
        None => (!hit.title.is_empty())
            .then(|| trailing_address(&hit.address, &hit.address_matches).into_any_element()),
    };
    let title = row_title(hit, ROW_TEXT_LIMIT);
    row_frame(("result", index).into(), title.text.clone(), row)
        .aria_description(format!("{} {}", target_kind(target), hit.address))
        .on_click(cx.listener(move |this, _, window, cx| {
            let Some(event) = this.choices.activation(request_id, target) else {
                return;
            };
            this.finish(event, window, cx);
        }))
        .child(glyph(target_glyph(target)))
        .child(row_text(title))
        .children(trailing)
        .into_any_element()
}

/// A page found by its kept text: its title and address, then the passage that matched. A click
/// or Enter opens it in a new tab (#54).
fn page_row(
    view: &PaletteView,
    index: usize,
    page: &FoundPage,
    cx: &mut Context<PaletteView>,
) -> AnyElement {
    let request_id = view.choices.request_id;
    let lead = view.choices.lead_rows() + view.choices.results.len();
    let title = page_title(page, ROW_TEXT_LIMIT);
    let passage = page_passage(page, ROW_TEXT_LIMIT);
    let address = page.address.clone();
    row_frame(
        ("page", index).into(),
        title.text.clone(),
        RowState::of(&view.choices, lead + index),
    )
    .aria_description(format!(
        "{PAGE_TEXT_LABEL} {} {}",
        page.address, passage.text
    ))
    .h(px(PAGE_ROW_HEIGHT))
    .on_click(cx.listener(move |this, _, window, cx| {
        let Some(event) = this.choices.page_activation(request_id, &address) else {
            return;
        };
        this.finish(event, window, cx);
    }))
    .child(glyph("¶"))
    .child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .line_height(px(PAGE_TITLE_LINE))
                    .child(row_text(title))
                    .child(trailing_text(page_address(page, ROW_TEXT_LIMIT))),
            )
            .child(
                div()
                    .text_size(px(text::SMALL))
                    .font_weight(FontWeight::NORMAL)
                    .line_height(px(PAGE_PASSAGE_LINE))
                    .line_clamp(2)
                    .child(styled(passage)),
            ),
    )
    .into_any_element()
}

/// What a screen reader learns about one row besides its name.
#[derive(Clone, Copy)]
struct RowState {
    selected: bool,
    /// The row claims the screen reader's focus, so moving the selection is announced.
    announced: bool,
    /// One-based place among `count` selectable rows, read as "2 of 7".
    position: usize,
    count: usize,
}

impl RowState {
    fn of(choices: &PaletteChoices, row: usize) -> Self {
        Self {
            selected: choices.selected == Some(row),
            announced: choices.announced_row() == Some(row),
            position: row + 1,
            count: choices.row_count(),
        }
    }
}

/// A result row, named and marked selected for screen readers. Arrow keys move the
/// selection while keys stay in the query field, so rows take no Tab stop; the announced
/// row is the active descendant of the focused query container (DW-60).
fn row_frame(id: gpui::ElementId, name: String, state: RowState) -> Stateful<Div> {
    let tokens = theme::current();
    let row = div()
        .id(id)
        .role(Role::ListBoxOption)
        .aria_label(name)
        .aria_selected(state.selected)
        .aria_position_in_set(state.position)
        .aria_size_of_set(state.count)
        .when(state.announced, |row| row.aria_active_descendant())
        .h(px(ROW_HEIGHT))
        .flex_shrink_0()
        .px(px(10.))
        .rounded(px(radius::TILE))
        .flex()
        .items_center()
        .gap_3()
        .text_size(px(text::PALETTE_ROW))
        .cursor_pointer();
    if state.selected {
        row.bg(rgba(tokens.accent.soft))
            .text_color(rgba(tokens.text_strong))
            .font_weight(FontWeight::MEDIUM)
    } else {
        row.text_color(rgba(tokens.text_secondary_bright))
    }
}

/// What kind of row a screen reader is on; the glyph alone says nothing.
fn target_kind(target: Target) -> &'static str {
    match target {
        Target::Command(_) => "Command",
        Target::Tab(_) => "Open tab",
        Target::Saved(_) => "Saved page",
        Target::History(_) => "History",
    }
}

fn target_glyph(target: Target) -> &'static str {
    match target {
        Target::Command(_) => "›",
        Target::Tab(_) => "○",
        Target::Saved(_) => "◆",
        Target::History(_) => "↺",
    }
}

fn glyph(symbol: &'static str) -> Div {
    let tokens = theme::current();
    div()
        .w(px(24.))
        .h(px(24.))
        .flex_shrink_0()
        .rounded(px(7.))
        .bg(rgba(tokens.plugins.neutral.background))
        .text_color(rgba(tokens.plugins.neutral.foreground))
        .flex()
        .items_center()
        .justify_center()
        .child(symbol)
}

/// Matched characters use the accent ink at semibold weight, as on the Palette board.
fn styled(highlighted: Highlighted) -> StyledText {
    let style = HighlightStyle {
        color: Some(Hsla::from(rgba(theme::current().accent.ink))),
        font_weight: Some(FontWeight::SEMIBOLD),
        ..HighlightStyle::default()
    };
    let ranges: Vec<_> = highlighted
        .ranges
        .into_iter()
        .map(|range| (range, style))
        .collect();
    StyledText::new(highlighted.text).with_highlights(ranges)
}

fn row_text(highlighted: Highlighted) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(styled(highlighted))
}

fn trailing_address(address: &str, matches: &[std::ops::Range<usize>]) -> Div {
    trailing_text(highlight(address, matches, ROW_TEXT_LIMIT))
}

/// Muted text after a row's name, such as its address.
fn trailing_text(text: Highlighted) -> Div {
    div()
        .max_w(px(TRAILING_WIDTH))
        .flex_shrink(1.)
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_size(px(text::SMALL))
        .text_color(rgba(theme::current().text_muted))
        .font_weight(FontWeight::NORMAL)
        .child(styled(text))
}

fn section_header(section: Section) -> Div {
    header(section.label())
}

fn header(label: &'static str) -> Div {
    div()
        .h(px(HEADER_HEIGHT))
        .flex_shrink_0()
        .mt(px(HEADER_MARGIN))
        .px(px(10.))
        .flex()
        .items_center()
        .text_size(px(text::CAPTION))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgba(theme::current().text_muted))
        .child(label.to_uppercase())
}

/// The title and detail shown in place of rows, for a view that is empty for a known reason.
fn empty_message(view: View) -> Option<(&'static str, &'static str)> {
    match view {
        View::PluginActions => Some((
            "No plugins yet",
            "Plugin actions appear here once the plugin runtime is installed.",
        )),
        View::Everything | View::Tabs | View::Commands | View::Bookmarks | View::History => None,
    }
}

fn empty_state(view: View) -> Option<Div> {
    let (title, detail) = empty_message(view)?;
    let tokens = theme::current();
    Some(
        div()
            .h(px(EMPTY_STATE_HEIGHT))
            .flex_shrink_0()
            .px(px(10.))
            .flex()
            .flex_col()
            .justify_center()
            .gap_1()
            .child(div().text_color(rgba(tokens.text_strong)).child(title))
            .child(
                div()
                    .text_size(px(text::SMALL))
                    .text_color(rgba(tokens.text_muted))
                    .child(detail),
            ),
    )
}

/// One quiet line for a problem or a wait; routine progress is not shown at all.
pub(super) fn notice(choices: &PaletteChoices) -> Option<Div> {
    let status = notice_text(choices)?;
    let tokens = theme::current();
    Some(
        div()
            .h(px(NOTICE_HEIGHT))
            .flex_shrink_0()
            .px_4()
            .flex()
            .items_center()
            .border_t_1()
            .border_color(rgba(tokens.panel_divider))
            .text_size(px(text::CAPTION))
            .text_color(rgba(tokens.text_muted))
            .overflow_hidden()
            .whitespace_nowrap()
            .child(status),
    )
}

/// The status line's text, or nothing when the line is not drawn.
fn notice_text(choices: &PaletteChoices) -> Option<String> {
    let line = choices.status_line();
    (!line.is_empty()).then_some(line)
}

fn key_hint(label: &str) -> Div {
    let tokens = theme::current();
    div()
        .flex_shrink_0()
        .px(px(6.))
        .rounded(px(radius::KEY_HINT))
        .border_1()
        .border_b_2()
        .border_color(rgba(tokens.key))
        .text_size(px(text::CAPTION))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgba(tokens.icon_dim))
        .child(label.to_owned())
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use syrinx_core::{SavedPageId, TabId};
    use syrinx_search::{ScopeCounts, SearchResults};

    use super::*;

    fn hit(target: Target) -> SearchHit {
        SearchHit {
            target,
            score: 1,
            title: "Synthetic row".to_owned(),
            address: "https://rows.example.test/".to_owned(),
            title_matches: Vec::new(),
            address_matches: Vec::new(),
        }
    }

    #[test]
    fn the_popup_is_the_bar_plus_its_chips_list_and_status_line() {
        let mut choices = PaletteChoices::default();
        let bar = PANEL_BORDERS + BAR_HEIGHT;
        assert_eq!(height(false, &choices, View::Everything), bar);
        assert_eq!(
            height(true, &choices, View::Everything),
            bar + CHIP_HEIGHT + CHIP_ROW_PADDING,
            "Tab's chip row"
        );
        assert_eq!(
            height(false, &choices, View::PluginActions),
            bar + LIST_DIVIDER + 2. * LIST_PADDING + EMPTY_STATE_HEIGHT,
            "an empty view that explains itself"
        );
        choices.status = "Synthetic problem".to_owned();
        assert_eq!(
            height(false, &choices, View::Everything),
            bar + NOTICE_HEIGHT,
            "a status line"
        );
        choices.reset(Some("Commands are unavailable: synthetic".to_owned()));
        assert_eq!(
            height(false, &choices, View::Everything),
            bar + NOTICE_HEIGHT,
            "a registry problem alone keeps the status line (#273)"
        );
    }

    #[test]
    fn the_notice_shows_a_registry_problem_on_its_own() {
        let problem = "Commands are unavailable: synthetic";
        let mut choices = PaletteChoices::default();
        assert_eq!(notice_text(&choices), None, "nothing to say, no line");
        choices.reset(Some(problem.to_owned()));
        assert_eq!(notice_text(&choices).as_deref(), Some(problem));
    }

    #[test]
    fn the_list_is_as_tall_as_its_rows_headers_and_gaps() {
        let first = Target::Tab(TabId::new(1).expect("tab"));
        let second = Target::Tab(TabId::new(2).expect("tab"));
        let saved = Target::Saved(SavedPageId::new(1).expect("saved"));
        let mut choices = PaletteChoices::default();
        assert_eq!(
            list_height(&choices, false),
            0.,
            "an empty palette is the bar alone"
        );
        assert_eq!(
            list_height(&choices, true),
            2. * LIST_PADDING + EMPTY_STATE_HEIGHT
        );
        let request = choices.query_scoped("synthetic", false);
        assert!(choices.receive(
            request,
            Ok(SearchResults {
                generation: 1,
                counts: ScopeCounts::default(),
                hits: [first, second, saved].map(hit).to_vec(),
            })
        ));
        // Children: [Open tabs] tab tab [Bookmarks] saved: three rows, two headers, four gaps.
        let expected = 2. * LIST_PADDING
            + 3. * ROW_HEIGHT
            + 2. * (HEADER_HEIGHT + HEADER_MARGIN)
            + 4. * ROW_GAP;
        assert_eq!(list_height(&choices, false), expected);
    }

    #[test]
    fn pages_found_by_their_text_add_their_header_and_taller_rows() {
        let tab = Target::Tab(TabId::new(1).expect("tab"));
        let found = |address: &str| crate::session::FoundPage {
            address: address.to_owned(),
            title: "Synthetic found page".to_owned(),
            passage: "a synthetic passage".to_owned(),
            matches: Vec::new(),
        };
        let mut choices = PaletteChoices::default();
        let request = choices.query_scoped("synthetic", true);
        assert!(choices.receive(
            request,
            Ok(SearchResults {
                generation: 1,
                counts: ScopeCounts::default(),
                hits: vec![hit(tab)],
            })
        ));
        let rows_only = list_height(&choices, false);
        assert!(choices.receive_pages(
            request,
            vec![
                found("https://one.example.test/"),
                found("https://two.example.test/")
            ]
        ));
        // Children: typed row, [Open tabs] tab, [Page text] page page.
        let expected =
            rows_only + HEADER_HEIGHT + HEADER_MARGIN + 2. * PAGE_ROW_HEIGHT + 3. * ROW_GAP;
        assert_eq!(list_height(&choices, false), expected);
        assert_eq!(child_index(&choices, 1), 2);
        assert_eq!(child_index(&choices, 2), 4);
        assert_eq!(child_index(&choices, 3), 5);
    }

    #[test]
    fn keyboard_rows_map_to_children_after_section_headers() {
        let command = Target::Command(NonZeroU64::MIN);
        let first = Target::Tab(TabId::new(1).expect("tab"));
        let second = Target::Tab(TabId::new(2).expect("tab"));
        let saved = Target::Saved(SavedPageId::new(1).expect("saved"));
        let mut choices = PaletteChoices::default();
        // A scoped view has no lead row, so the first row is the first result.
        let request = choices.query_scoped("synthetic", false);
        assert!(choices.receive(
            request,
            Ok(SearchResults {
                generation: 1,
                counts: ScopeCounts::default(),
                hits: [command, first, second, saved].map(hit).to_vec(),
            })
        ));
        // Children: [Commands] cmd [Open tabs] tab tab [Bookmarks] saved
        assert_eq!(child_index(&choices, 0), 1);
        assert_eq!(child_index(&choices, 1), 3);
        assert_eq!(child_index(&choices, 2), 4);
        assert_eq!(child_index(&choices, 3), 6);

        // With a Go to row first, every result shifts by one child.
        let request = choices.query_scoped("https://rows.example.test/", true);
        assert!(choices.receive(
            request,
            Ok(SearchResults {
                generation: 2,
                counts: ScopeCounts::default(),
                hits: [first].map(hit).to_vec(),
            })
        ));
        assert_eq!(child_index(&choices, 0), 0);
        assert_eq!(child_index(&choices, 1), 2);
    }

    #[test]
    fn a_section_with_scattered_rows_repeats_its_header_after_the_word_matches() {
        let word_tab = Target::Tab(TabId::new(1).expect("tab"));
        let scattered_tab = Target::Tab(TabId::new(2).expect("tab"));
        let history = Target::History(NonZeroU64::MIN);
        let page = |target, title: &str, address: &str| SearchHit {
            title: title.to_owned(),
            address: address.to_owned(),
            ..hit(target)
        };
        let mut choices = PaletteChoices::default();
        let request = choices.query_scoped("youtube", true);
        assert!(choices.receive_ordered(
            request,
            Ok(SearchResults {
                generation: 1,
                counts: ScopeCounts::default(),
                hits: vec![
                    page(word_tab, "YouTube Music", "https://music.example.test/"),
                    page(scattered_tab, "Studios/Terra", "https://example.test/terra"),
                    page(history, "YouTube", "https://youtube.example/"),
                ],
            })
        ));
        let order: Vec<_> = choices.results.iter().map(|hit| hit.target).collect();
        assert_eq!(order, [word_tab, history, scattered_tab]);
        // Children: typed row, [Open tabs] tab, [History] history, [Open tabs] tab
        assert_eq!(child_index(&choices, 0), 0);
        assert_eq!(child_index(&choices, 1), 2);
        assert_eq!(child_index(&choices, 2), 4);
        assert_eq!(child_index(&choices, 3), 6);
    }
}
