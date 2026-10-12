//! Draws the five palettes and D44's sidebar width states so the owner can review the tokens.
//!
//! Development only: `atlas-app --palette-sample` opens this window instead of the browser.

use atlas_ui::{
    Accent, Color, Palette, PaletteId, SidebarWidthState,
    contrast::{self, ContrastCheck},
    metrics::{chrome, radius, space, text},
    palette, sidebar,
};
use gpui::{
    App, AppContext, Bounds, Context, Div, FontWeight, IntoElement, ParentElement, Render, Styled,
    TitlebarOptions, Window, WindowBounds, WindowOptions, div, px, size,
};

use crate::theme::rgba;

/// Opens the review window; the application quits when it closes.
pub(crate) fn open(cx: &mut App) -> Result<(), String> {
    cx.on_window_closed(|cx, _| cx.quit()).detach();
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1520.), px(980.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Atlas · design token review (P1-07)".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        |_, cx| cx.new(|_| PaletteSample),
    )
    .map(|_| ())
    .map_err(|error| format!("Cannot open the palette sample window: {error}"))
}

struct PaletteSample;

impl Render for PaletteSample {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        // The review frame itself uses Graphite, the default palette.
        let frame = palette(PaletteId::Graphite, Accent::default());
        div()
            .size_full()
            .p(px(space::XXL))
            .flex()
            .flex_col()
            .gap(px(space::XXL))
            .bg(rgba(frame.ground.representative()))
            .text_color(rgba(frame.text))
            .font_family(text::UI_FONT_FAMILIES[0])
            .text_size(px(text::BODY))
            .child(heading("Base palettes with the default accent (#FF8A65)"))
            .child(
                div().flex().gap(px(space::XL)).children(
                    PaletteId::ALL.map(|id| palette_column(&palette(id, Accent::default()))),
                ),
            )
            .child(transparency_fallback(&frame))
            .child(heading(
                "Sidebar width states (D44): PROPOSED values for review",
            ))
            .child(width_states(&frame))
    }
}

fn heading(label: &'static str) -> Div {
    div()
        .text_size(px(text::SUBTITLE))
        .font_weight(FontWeight(f32::from(text::WEIGHT_SEMIBOLD)))
        .child(label)
}

fn palette_column(palette: &Palette) -> Div {
    // One column per palette: ground, sidebar rows, card, popup and contrast summary.
    let ground = palette.ground.representative();
    div()
        .w(px(282.))
        .p(px(space::XL))
        .flex()
        .flex_col()
        .gap(px(space::M))
        .rounded(px(radius::TILE))
        .bg(rgba(ground))
        .text_color(rgba(palette.text))
        .child(
            div()
                .font_weight(FontWeight(f32::from(text::WEIGHT_SEMIBOLD)))
                .child(format!("{} ({})", palette.id.name(), ground_note(palette))),
        )
        .child(sidebar_rows(palette))
        .child(card_sample(palette))
        .child(panel_sample(palette))
        .child(tile_strip(palette))
        .child(contrast_summary(palette))
}

fn ground_note(palette: &Palette) -> &'static str {
    match palette.ground {
        atlas_ui::Ground::Solid(_) => "solid",
        atlas_ui::Ground::Gradient { .. } => "gradient, first stop",
        atlas_ui::Ground::Material { .. } => "material over canvas stand-in",
    }
}

fn sidebar_rows(palette: &Palette) -> Div {
    // A normal row, the selected row with a muted subtitle, a section label and a disabled control.
    div()
        .flex()
        .flex_col()
        .gap(px(space::XXS))
        .child(
            div()
                .text_size(px(text::CAPTION))
                .text_color(rgba(palette.text_muted))
                .child("OPEN TABS"),
        )
        .child(row(palette, "example.com", None, false))
        .child(row(palette, "Forge", Some(" · project.test"), true))
        .child(
            div()
                .px(px(space::M))
                .flex()
                .gap(px(space::M))
                .child(div().text_color(rgba(palette.icon)).child("‹ ›"))
                .child(div().text_color(rgba(palette.icon_dim)).child("↻"))
                .child(div().text_color(rgba(palette.disabled)).child("disabled")),
        )
}

fn row(
    palette: &Palette,
    title: &'static str,
    subtitle: Option<&'static str>,
    selected: bool,
) -> Div {
    let base = div()
        .h(px(30.))
        .px(px(space::M))
        .flex()
        .items_center()
        .rounded(px(radius::CONTROL))
        .text_color(rgba(if selected {
            palette.text_strong
        } else {
            palette.text_secondary_bright
        }))
        .child(title);
    let base = match subtitle {
        Some(subtitle) => base.child(div().text_color(rgba(palette.text_muted)).child(subtitle)),
        None => base,
    };
    if !selected {
        return base;
    }
    base.bg(rgba(palette.row_selected))
        .child(div().flex_1())
        .child(badge(palette, "2"))
}

fn badge(palette: &Palette, count: &'static str) -> Div {
    div()
        .px(px(space::S))
        .rounded(px(radius::TILE))
        .text_size(px(text::CAPTION))
        .font_weight(FontWeight(f32::from(text::WEIGHT_BOLD)))
        .bg(rgba(palette.accent.fill))
        .text_color(rgba(palette.accent.on_fill))
        .child(count)
}

fn card_sample(palette: &Palette) -> Div {
    // The page card around Atlas's own pages, with the search pill and a surface on it.
    div()
        .p(px(space::L))
        .flex()
        .flex_col()
        .gap(px(space::S))
        .rounded(px(radius::HOME_CARD))
        .border_1()
        .border_color(rgba(palette.card_line))
        .bg(rgba(palette.card))
        .child(
            div()
                .h(px(30.))
                .px(px(space::L))
                .flex()
                .items_center()
                .rounded(px(radius::HOME_CARD))
                .border_1()
                .border_color(rgba(palette.pill_line))
                .bg(rgba(palette.pill))
                .text_color(rgba(palette.text_secondary))
                .child("Search or type > @ /"),
        )
        .child(
            div()
                .p(px(space::M))
                .rounded(px(radius::TILE))
                .border_1()
                .border_color(rgba(palette.surface_line))
                .bg(rgba(palette.surface))
                .child(div().child("Surface text"))
                .child(
                    div()
                        .text_color(rgba(palette.text_muted))
                        .child("Muted hint"),
                ),
        )
}

fn panel_sample(palette: &Palette) -> Div {
    // The command palette: near-opaque panel, input and the accent-marked default row.
    div()
        .p(px(space::M))
        .flex()
        .flex_col()
        .gap(px(space::XS))
        .rounded(px(radius::PAGE_CARD))
        .border_1()
        .border_color(rgba(palette.panel_line))
        .bg(rgba(palette.panel))
        .child(
            div()
                .h(px(26.))
                .px(px(space::M))
                .flex()
                .items_center()
                .rounded(px(radius::CONTROL))
                .border_1()
                .border_color(rgba(palette.input_line))
                .bg(rgba(palette.input))
                .child("fled"),
        )
        .child(
            div()
                .h(px(26.))
                .px(px(space::M))
                .flex()
                .items_center()
                .rounded(px(radius::CONTROL))
                .border_1()
                .border_color(rgba(palette.accent.soft_line))
                .bg(rgba(palette.accent.soft))
                .text_color(rgba(palette.accent.ink))
                .child("New lead · Enter"),
        )
}

fn tile_strip(palette: &Palette) -> Div {
    div()
        .flex()
        .gap(px(space::XS))
        .children(palette.tiles.map(|tile| {
            div()
                .size(px(26.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(radius::CONTROL))
                .bg(rgba(tile.background))
                .text_color(rgba(tile.foreground))
                .font_weight(FontWeight(f32::from(text::WEIGHT_SEMIBOLD)))
                .child("A")
        }))
}

fn contrast_summary(palette: &Palette) -> Div {
    // Pairs below WCAG minimums with this accent, measured as the unit tests measure them.
    let failures: Vec<ContrastCheck> = contrast::audit(palette)
        .into_iter()
        .filter(|check| !check.passes())
        .collect();
    let summary = if failures.is_empty() {
        "All audited pairs pass".to_owned()
    } else {
        format!("{} pairs below minimum:", failures.len())
    };
    div()
        .text_size(px(text::CAPTION))
        .text_color(rgba(palette.text_secondary))
        .child(summary)
        .children(failures.iter().map(|check| {
            format!(
                "{} on {}: {:.2} < {}",
                check.foreground, check.background, check.ratio, check.minimum
            )
        }))
}

fn transparency_fallback(frame: &Palette) -> Div {
    // Reduce transparency resolves each choice to the palette actually drawn.
    let lines = PaletteId::ALL.map(|id| format!("{} → {}", id.name(), id.resolve(true).name()));
    div()
        .flex()
        .gap(px(space::XL))
        .text_color(rgba(frame.text_secondary))
        .child(
            div()
                .font_weight(FontWeight(f32::from(text::WEIGHT_SEMIBOLD)))
                .child("Reduce transparency on:"),
        )
        .children(lines)
}

fn width_states(frame: &Palette) -> Div {
    // A miniature window per state, at true pixel widths, plus the peek overlay.
    let states = SidebarWidthState::ALL
        .map(|state| window_sample(frame, state.name(), state.reserved_width(), false));
    div()
        .flex()
        .flex_col()
        .gap(px(space::M))
        .child(
            div()
                .flex()
                .gap(px(space::XL))
                .children(states)
                .child(window_sample(frame, "Hidden + hover peek", sidebar::PEEK_WIDTH, true)),
        )
        .child(div().text_color(rgba(frame.text_secondary)).child(format!(
            "Wide {} px · favicon-only {} px · hidden {} px · peek {} px overlay · peek edge zone {} px",
            sidebar::WIDE_WIDTH,
            sidebar::COMPACT_WIDTH,
            sidebar::HIDDEN_WIDTH,
            sidebar::PEEK_WIDTH,
            sidebar::PEEK_EDGE_ZONE,
        )))
        .child(div().text_color(rgba(frame.text_secondary)).child(format!(
            "Drag release: ≥ {} px settles wide (labels shown) · ≥ {} px settles favicon-only · narrower hides",
            sidebar::WIDE_SNAP_THRESHOLD,
            sidebar::COMPACT_SNAP_THRESHOLD,
        )))
}

fn window_sample(frame: &Palette, label: &'static str, sidebar_width: f32, peek: bool) -> Div {
    let reserved = if peek { 0. } else { sidebar_width };
    let page = div()
        .absolute()
        .left(px(reserved))
        .top(px(0.))
        .w(px(SAMPLE_WIDTH - reserved))
        .h(px(SAMPLE_HEIGHT))
        .p(px(chrome::CARD_INSET))
        .child(
            div()
                .size_full()
                .rounded(px(radius::PAGE_CARD))
                .bg(rgba(Color::hex(0xFFFFFF)))
                .text_color(rgba(Color::hex(0x1C1D21)))
                .p(px(space::M))
                .child("page card"),
        );
    let sidebar_panel = (sidebar_width > 0.).then(|| {
        div()
            .absolute()
            .left(px(0.))
            .top(px(0.))
            .w(px(sidebar_width))
            .h(px(SAMPLE_HEIGHT))
            .overflow_hidden()
            .bg(rgba(frame.panel_side))
            .border_r_1()
            .border_color(rgba(frame.panel_line))
            .p(px(space::M))
            .text_color(rgba(frame.text_secondary_bright))
            .child(if sidebar::labels_visible(sidebar_width) {
                "◆ Tab title"
            } else {
                "◆"
            })
    });
    div()
        .flex()
        .flex_col()
        .gap(px(space::XS))
        .child(label)
        .child(
            div()
                .relative()
                .w(px(SAMPLE_WIDTH))
                .h(px(SAMPLE_HEIGHT))
                .overflow_hidden()
                .rounded(px(radius::CONTROL))
                .bg(rgba(frame.ground.representative()))
                .border_1()
                .border_color(rgba(frame.line))
                .child(page)
                .children(sidebar_panel),
        )
}

const SAMPLE_WIDTH: f32 = 330.;
const SAMPLE_HEIGHT: f32 = 140.;
