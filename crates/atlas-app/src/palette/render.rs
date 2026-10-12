//! Draws the palette's chips, rows, preview and footer from `atlas-ui`'s tokens (design: Palette board).

use atlas_search::{SearchHit, Target};
use atlas_ui::metrics::{radius, text};
use gpui::{
    AnyElement, Context, Div, FontWeight, HighlightStyle, Hsla, InteractiveElement, IntoElement,
    ParentElement, Role, Stateful, StatefulInteractiveElement, Styled, StyledText, Toggled, div,
    px,
};

use super::{
    PaletteView,
    choices::PaletteChoices,
    display::{Highlighted, Preview, ROW_TEXT_LIMIT, Section, highlight, row_title},
    query::{Chip, View},
};
use crate::controls::control;
use crate::theme::{self, rgba};

const LIST_WIDTH: f32 = 440.;
const ROW_HEIGHT: f32 = 38.;
const HEADER_HEIGHT: f32 = 28.;
const TRAILING_WIDTH: f32 = 180.;

/// The position of `row` among the list's children, which also include section headers.
pub(super) fn child_index(choices: &PaletteChoices, row: usize) -> usize {
    let address_rows = choices.address_rows();
    let Some(hit_index) = row.checked_sub(address_rows) else {
        return row;
    };
    let headers = (0..=hit_index.min(choices.results.len().saturating_sub(1)))
        .filter(|&index| starts_section(&choices.results, index))
        .count();
    address_rows + hit_index + headers
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
    let mut row = div().px_4().pb_3().flex().items_center().gap_2();
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
        .h(px(26.))
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

pub(super) fn list(view: &PaletteView, wide: bool, cx: &mut Context<PaletteView>) -> Div {
    let tokens = theme::current();
    let mut rows = div()
        .id("results")
        .role(Role::ListBox)
        .aria_label("Results")
        .flex_1()
        .min_h_0()
        .px(px(10.))
        .py(px(6.))
        .flex()
        .flex_col()
        .gap(px(2.))
        .overflow_y_scroll()
        .track_scroll(&view.scroll);
    if let Some(address) = &view.choices.address {
        rows = rows.child(address_row(view, address, cx));
    }
    for (index, hit) in view.choices.results.iter().enumerate() {
        if starts_section(&view.choices.results, index) {
            rows = rows.child(section_header(Section::of(hit.target)));
        }
        rows = rows.child(result_row(view, index, hit, cx));
    }
    if view.choices.results.is_empty() && view.choices.address.is_none() {
        rows = rows.children(empty_state(view.view));
    }
    let column = div()
        .flex()
        .flex_col()
        .min_h_0()
        .child(rows)
        .child(web_search_row(cx));
    if wide {
        column
            .w(px(LIST_WIDTH))
            .flex_shrink_0()
            .border_r_1()
            .border_color(rgba(tokens.panel_divider))
    } else {
        column.flex_1()
    }
}

fn address_row(view: &PaletteView, address: &str, cx: &mut Context<PaletteView>) -> AnyElement {
    let request_id = view.choices.request_id;
    let title = highlight(&format!("Go to {address}"), &[], ROW_TEXT_LIMIT);
    row_frame(
        "go-to-address".into(),
        title.text.clone(),
        view.choices.selected == Some(0),
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

fn result_row(
    view: &PaletteView,
    index: usize,
    hit: &SearchHit,
    cx: &mut Context<PaletteView>,
) -> AnyElement {
    let target = hit.target;
    let request_id = view.choices.request_id;
    let selected = Some(index + view.choices.address_rows()) == view.choices.selected;
    let trailing = match view.commands.command(target) {
        Some(command) => command
            .shortcut
            .as_ref()
            .map(|shortcut| key_hint(&shortcut.label()).into_any_element()),
        None => (!hit.title.is_empty()).then(|| trailing_address(hit).into_any_element()),
    };
    let title = row_title(hit, ROW_TEXT_LIMIT);
    row_frame(("result", index).into(), title.text.clone(), selected)
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

/// A result row, named and marked selected for screen readers. Arrow keys move the
/// selection while focus stays in the query field, so rows take no Tab stop.
// TODO(DW-60): announce the moving selection; GPUI honours an active descendant only
// under the focused node, and the query field is the rows' sibling.
fn row_frame(id: gpui::ElementId, name: String, selected: bool) -> Stateful<Div> {
    let tokens = theme::current();
    let row = div()
        .id(id)
        .role(Role::ListBoxOption)
        .aria_label(name)
        .aria_selected(selected)
        .h(px(ROW_HEIGHT))
        .flex_shrink_0()
        .px(px(10.))
        .rounded(px(radius::TILE))
        .flex()
        .items_center()
        .gap_3()
        .text_size(px(text::PALETTE_ROW))
        .cursor_pointer();
    if selected {
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

fn trailing_address(hit: &SearchHit) -> Div {
    div()
        .max_w(px(TRAILING_WIDTH))
        .flex_shrink(1.)
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_size(px(text::SMALL))
        .text_color(rgba(theme::current().text_muted))
        .font_weight(FontWeight::NORMAL)
        .child(styled(highlight(
            &hit.address,
            &hit.address_matches,
            ROW_TEXT_LIMIT,
        )))
}

fn section_header(section: Section) -> Div {
    div()
        .h(px(HEADER_HEIGHT))
        .flex_shrink_0()
        .mt_1()
        .px(px(10.))
        .flex()
        .items_center()
        .text_size(px(text::CAPTION))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgba(theme::current().text_muted))
        .child(section.label().to_uppercase())
}

fn empty_state(view: View) -> Option<Div> {
    let (title, detail) = match view {
        View::PluginActions => (
            "No plugins yet",
            "Plugin actions appear here once the plugin runtime is installed.",
        ),
        View::Everything | View::Tabs | View::Commands | View::Bookmarks | View::History => {
            return None;
        }
    };
    let tokens = theme::current();
    Some(
        div()
            .px(px(10.))
            .py_4()
            .flex()
            .flex_col()
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

fn web_search_row(cx: &mut Context<PaletteView>) -> Div {
    let tokens = theme::current();
    div()
        .flex_shrink_0()
        .px(px(10.))
        .py(px(6.))
        .border_t_1()
        .border_color(rgba(tokens.panel_divider))
        .child(
            control(div().id("web-search"), Role::Button, "Search the web")
                .aria_keyshortcuts("Alt+Enter")
                .h(px(40.))
                .px(px(10.))
                .rounded(px(radius::TILE))
                .flex()
                .items_center()
                .gap_3()
                .text_size(px(text::PALETTE_ROW))
                .text_color(rgba(tokens.text_secondary_bright))
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| this.submit(true, window, cx)))
                .child(glyph("⌕"))
                .child(div().flex_1().child("Search the web"))
                .child(key_hint("Alt Enter")),
        )
}

pub(super) fn preview(preview: Option<Preview>) -> Div {
    let tokens = theme::current();
    let pane = div()
        .flex_1()
        .min_w_0()
        .px(px(24.))
        .py(px(20.))
        .flex()
        .flex_col()
        .gap(px(14.))
        .overflow_hidden()
        .bg(rgba(tokens.panel_side));
    let Some(preview) = preview else {
        return pane.child(
            div()
                .text_size(px(text::SMALL))
                .text_color(rgba(tokens.text_muted))
                .child("Type to search open tabs and bookmarks, or start with > for commands, @ for plugins and / for open tabs."),
        );
    };
    pane.child(
        div()
            .text_size(px(text::SMALL))
            .text_color(rgba(tokens.text_muted))
            .child(preview.kind),
    )
    .child(
        div()
            .text_size(px(text::SUBTITLE))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgba(tokens.text_strong))
            .line_clamp(3)
            .child(styled(preview.title)),
    )
    .children(preview.detail.map(|detail| {
        div()
            .text_size(px(text::SMALL))
            .text_color(rgba(tokens.icon_dim))
            .line_clamp(4)
            .child(styled(detail))
    }))
    .child(
        div()
            .text_size(px(text::SMALL))
            .text_color(rgba(tokens.text_muted))
            .child(preview.hint),
    )
}

pub(super) fn footer(status: &str) -> Div {
    let tokens = theme::current();
    div()
        .h(px(32.))
        .flex_shrink_0()
        .px_4()
        .flex()
        .items_center()
        .gap_3()
        .border_t_1()
        .border_color(rgba(tokens.panel_divider))
        .text_size(px(text::CAPTION))
        .text_color(rgba(tokens.text_muted))
        .child(
            div()
                .flex_1()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(status.to_owned()),
        )
        .child("↑↓ Move · Enter Run · Alt Enter Search the web")
        .child(prefix_hint(">", "commands"))
        .child(prefix_hint("@", "plugins"))
        .child(prefix_hint("/", "tabs"))
}

/// Only the prefix symbols are monospace (`design/README.md`).
fn prefix_hint(symbol: &'static str, label: &'static str) -> Div {
    div()
        .flex()
        .gap_1()
        .child(div().font_family(text::MONO_FONT_FAMILIES[0]).child(symbol))
        .child(label)
}

pub(super) fn key_hint(label: &str) -> Div {
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

    use atlas_core::{SavedPageId, TabId};
    use atlas_search::{ScopeCounts, SearchResults};

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
    fn keyboard_rows_map_to_children_after_section_headers() {
        let command = Target::Command(NonZeroU64::MIN);
        let first = Target::Tab(TabId::new(1).expect("tab"));
        let second = Target::Tab(TabId::new(2).expect("tab"));
        let saved = Target::Saved(SavedPageId::new(1).expect("saved"));
        let mut choices = PaletteChoices::default();
        let request = choices.query_scoped("synthetic", true);
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
}
