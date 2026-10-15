//! Development only: `syrinx --theme-lab` shows variants of Tint, Haze and Glass on the owner's
//! real desktop, so the owner can pick one of each before the design canvas changes.

use std::sync::Arc;

use gpui::{
    App, AppContext, Background, Bounds, Context, Div, Image, InteractiveElement, IntoElement,
    ObjectFit, ParentElement, Render, StatefulInteractiveElement, Styled, StyledImage,
    TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds, WindowOptions, div, img,
    linear_color_stop, linear_gradient, px, size,
};
use syrinx_platform::backdrop::{self, Backdrop};
use syrinx_ui::{Accent, Color, Ground, Palette, PaletteId, palette};

use crate::ground_image::gradient_image;
use crate::theme::rgba;

/// Opens the lab; the application quits when it closes.
pub(crate) fn open(cx: &mut App) -> Result<(), String> {
    cx.on_window_closed(|cx, _| cx.quit()).detach();
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1280.), px(860.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Syrinx · theme lab".into()),
                ..Default::default()
            }),
            // Mica clears GPUI's frame to transparent, so any system backdrop can show through.
            window_background: WindowBackgroundAppearance::MicaBackdrop,
            ..Default::default()
        },
        |window, cx| {
            let lab = ThemeLab::new();
            lab.apply_backdrop(window);
            cx.new(|_| lab)
        },
    )
    .map(|_| ())
    .map_err(|error| format!("Cannot open the theme lab window: {error}"))
}

/// Which palette a variant belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Glass,
    Haze,
    Tint,
}

impl Family {
    fn name(self) -> &'static str {
        match self {
            Self::Glass => "Glass",
            Self::Haze => "Haze",
            Self::Tint => "Tint",
        }
    }

    fn palette(self) -> Palette {
        let id = match self {
            Self::Glass => PaletteId::Glass,
            Self::Haze => PaletteId::Haze,
            Self::Tint => PaletteId::Tint,
        };
        palette(id, Accent::default())
    }
}

/// How the chrome's surfaces sit on a see-through ground: fill, edge ring and top highlight.
#[derive(Clone, Copy)]
struct Edges {
    surface: f32,
    ring: f32,
    highlight: f32,
}

/// What lies behind the chrome.
enum Ground2 {
    /// A system backdrop with a dark tint of this opacity painted over it.
    Material {
        backdrop: Backdrop,
        tint: (u8, u8, u8),
        opacity: f32,
    },
    /// Today's two-stop gradient, as the current build draws Tint.
    TwoStop,
    /// A generated three-stop gradient with grain, as the canvas draws Tint.
    Generated(Arc<Image>),
}

/// One choice the owner can name back, such as "G2".
struct Variant {
    code: &'static str,
    family: Family,
    title: &'static str,
    detail: &'static str,
    ground: Ground2,
    edges: Option<Edges>,
}

struct ThemeLab {
    variants: Vec<Variant>,
    selected: usize,
}

impl ThemeLab {
    fn new() -> Self {
        let variants = glass_variants()
            .into_iter()
            .chain(haze_variants())
            .chain(tint_variants())
            .collect();
        // Open on Glass as the canvas specifies it.
        Self {
            variants,
            selected: 1,
        }
    }

    fn variant(&self) -> &Variant {
        &self.variants[self.selected]
    }

    /// Ask Windows for the selected variant's backdrop; Tint covers it with an opaque ground.
    fn apply_backdrop(&self, window: &Window) {
        let backdrop = match self.variant().ground {
            Ground2::Material { backdrop, .. } => backdrop,
            Ground2::TwoStop | Ground2::Generated(_) => Backdrop::Mica,
        };
        let applied = crate::ui::native_handle(window)
            .map_err(str::to_owned)
            .and_then(|hwnd| backdrop::apply(hwnd, backdrop).map_err(|error| error.to_string()));
        if let Err(error) = applied {
            eprintln!("theme lab: could not set the {backdrop:?} backdrop: {error}");
        }
    }

    fn select(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = index;
        self.apply_backdrop(window);
        cx.notify();
    }
}

impl Render for ThemeLab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let variant = self.variant();
        let tokens = variant.family.palette();
        div()
            .size_full()
            .flex()
            .flex_col()
            .font_family("Segoe UI Variable Text")
            .text_sm()
            .child(mock_browser(variant, &tokens))
            .child(self.picker(cx))
    }
}

impl ThemeLab {
    /// The opaque strip at the bottom: one button per variant and the chosen one's notes.
    fn picker(&self, cx: &mut Context<Self>) -> Div {
        let frame = palette(PaletteId::Graphite, Accent::default());
        let mut buttons = div().flex().gap_2().items_center();
        let mut family = None;
        for (index, variant) in self.variants.iter().enumerate() {
            if family != Some(variant.family) {
                family = Some(variant.family);
                buttons = buttons.child(
                    div()
                        .pl_3()
                        .text_color(rgba(frame.text_muted))
                        .child(variant.family.name()),
                );
            }
            let chosen = index == self.selected;
            buttons = buttons.child(
                div()
                    .id(variant.code)
                    .px_3()
                    .py_1()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgba(if chosen {
                        frame.accent.fill
                    } else {
                        frame.line
                    }))
                    .bg(rgba(if chosen {
                        frame.row_selected
                    } else {
                        frame.field
                    }))
                    .text_color(rgba(frame.text))
                    .cursor_pointer()
                    .child(variant.code)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select(index, window, cx);
                    })),
            );
        }
        let variant = self.variant();
        div()
            .flex_none()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .bg(rgba(frame.ground.representative()))
            .text_color(rgba(frame.text))
            .child(buttons)
            .child(
                div()
                    .child(format!("{}: {}", variant.code, variant.title))
                    .text_color(rgba(frame.text)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgba(frame.text_secondary))
                    .child(variant.detail),
            )
    }
}

/// A browser frame in the variant's look: title bar, sidebar and a card holding a sample page.
fn mock_browser(variant: &Variant, tokens: &Palette) -> Div {
    let frame = div()
        .flex_1()
        .min_h_0()
        .relative()
        .flex()
        .flex_col()
        .text_color(rgba(tokens.text));
    let frame = match &variant.ground {
        Ground2::Material { tint, opacity, .. } => {
            frame.bg(rgba(Color::rgba(tint.0, tint.1, tint.2, *opacity)))
        }
        Ground2::TwoStop => frame.bg(two_stop(tokens)),
        Ground2::Generated(image) => frame.child(
            img(image.clone())
                .absolute()
                .size_full()
                .object_fit(ObjectFit::Fill),
        ),
    };
    frame.child(title_bar(tokens)).child(
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(sidebar(tokens, variant.edges))
            .child(page_card()),
    )
}

fn title_bar(tokens: &Palette) -> Div {
    div()
        .h(px(40.))
        .flex_none()
        .flex()
        .items_center()
        .gap_4()
        .px_4()
        .text_color(rgba(tokens.icon))
        .child("‹   ›   ⟳")
        .child(
            div()
                .text_color(rgba(tokens.text_secondary))
                .child("A sample article"),
        )
}

fn sidebar(tokens: &Palette, edges: Option<Edges>) -> Div {
    let section = |label: &'static str| {
        div()
            .pt_3()
            .pb_1()
            .px_2()
            .text_xs()
            .text_color(rgba(tokens.text_muted))
            .child(label)
    };
    let row = |label: &'static str, selected: bool| {
        let fill = if selected {
            edges.map_or(tokens.row_selected, |edges| white(edges.surface + 0.06))
        } else {
            Color::rgba(0, 0, 0, 0.)
        };
        let base = div()
            .relative()
            .h(px(32.))
            .px_2()
            .flex()
            .items_center()
            .rounded(px(8.))
            .bg(rgba(fill))
            .child(label);
        if selected {
            glass_edge(base, edges)
        } else {
            base
        }
    };
    let pill = glass_edge(
        div()
            .relative()
            .h(px(36.))
            .px_3()
            .flex()
            .items_center()
            .rounded(px(10.))
            .border_1()
            .border_color(rgba(tokens.pill_line))
            .bg(rgba(
                edges.map_or(tokens.pill, |edges| white(edges.surface + 0.04)),
            ))
            .text_color(rgba(tokens.text_muted))
            .child("Search or enter address"),
        edges,
    );
    let tiles = div()
        .flex()
        .gap_2()
        .pt_3()
        .children(["M", "C", "D", "N"].map(|letter| {
            glass_edge(
                div()
                    .relative()
                    .size(px(52.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(12.))
                    .bg(rgba(
                        edges.map_or(tokens.surface, |edges| white(edges.surface)),
                    ))
                    .border_1()
                    .border_color(rgba(tokens.surface_line))
                    .child(letter),
                edges,
            )
        }));
    div()
        .w(px(264.))
        .flex_none()
        .px_3()
        .pb_3()
        .flex()
        .flex_col()
        .child(pill)
        .child(tiles)
        .child(section("PLUGINS"))
        .child(row("Forge", false))
        .child(row("Notes", false))
        .child(row("Fledge", false))
        .child(section("OPEN TABS"))
        .child(row("A sample article", true))
        .child(row("Example Domain", false))
        .child(row("Weather forecast", false))
}

/// Give a surface the canvas's glass edge: a faint ring and a brighter line along its top.
fn glass_edge(element: Div, edges: Option<Edges>) -> Div {
    let Some(edges) = edges else {
        return element;
    };
    element
        .border_1()
        .border_color(rgba(white(edges.ring)))
        .child(
            div()
                .absolute()
                .top_0()
                .left(px(6.))
                .right(px(6.))
                .h(px(1.))
                .bg(rgba(white(edges.highlight))),
        )
}

/// The page card with a light sample page, standing in for the WebView2 page, which is opaque.
fn page_card() -> Div {
    let ink = Color::rgba(31, 35, 40, 1.);
    let grey = Color::rgba(222, 225, 230, 1.);
    let line = |width: f32| {
        div()
            .h(px(10.))
            .w(gpui::relative(width))
            .rounded(px(5.))
            .bg(rgba(grey))
    };
    div().flex_1().min_w_0().pr_2().pb_2().child(
        div()
            .size_full()
            .rounded(px(10.))
            .overflow_hidden()
            .bg(gpui::white())
            .text_color(rgba(ink))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(48.))
                    .px_6()
                    .flex()
                    .items_center()
                    .bg(rgba(Color::rgba(243, 244, 246, 1.)))
                    .child("example.test"),
            )
            .child(
                div()
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_2xl().child("A sample article"))
                    .child(line(0.92))
                    .child(line(0.88))
                    .child(line(0.95))
                    .child(line(0.6))
                    .child(
                        div()
                            .mt_3()
                            .h(px(160.))
                            .rounded(px(8.))
                            .bg(rgba(Color::rgba(196, 210, 232, 1.))),
                    )
                    .child(line(0.9))
                    .child(line(0.7)),
            ),
    )
}

fn glass_variants() -> Vec<Variant> {
    let tint = (16, 17, 24);
    let material = |backdrop, opacity| Ground2::Material {
        backdrop,
        tint,
        opacity,
    };
    vec![
        Variant {
            code: "G0",
            family: Family::Glass,
            title: "Current build",
            detail: "Mica with the canvas's 46 % dark tint and no glass edges: what Syrinx draws today.",
            ground: material(Backdrop::Mica, 0.46),
            edges: None,
        },
        Variant {
            code: "G1",
            family: Family::Glass,
            title: "The canvas as specified",
            detail: "Acrylic (Windows' frosted blur) with the canvas's 46 % tint, 5 % white surfaces, a 7 % \
                     edge ring and a 24 % top highlight.",
            ground: material(Backdrop::Acrylic, 0.46),
            edges: Some(Edges {
                surface: 0.05,
                ring: 0.07,
                highlight: 0.24,
            }),
        },
        Variant {
            code: "G2",
            family: Family::Glass,
            title: "Clearer glass",
            detail: "Acrylic with a 28 % tint, so more of what is behind shows, 8 % surfaces, a 10 % ring \
                     and a 32 % highlight.",
            ground: material(Backdrop::Acrylic, 0.28),
            edges: Some(Edges {
                surface: 0.08,
                ring: 0.10,
                highlight: 0.32,
            }),
        },
        Variant {
            code: "G3",
            family: Family::Glass,
            title: "Clearest glass",
            detail: "Acrylic with a 14 % tint, 10 % surfaces, a 12 % ring and a 36 % highlight. Text may fall \
                     below 4.5:1 over a bright wallpaper.",
            ground: material(Backdrop::Acrylic, 0.14),
            edges: Some(Edges {
                surface: 0.10,
                ring: 0.12,
                highlight: 0.36,
            }),
        },
    ]
}

fn haze_variants() -> Vec<Variant> {
    let tint = (26, 27, 32);
    let material = |backdrop, opacity| Ground2::Material {
        backdrop,
        tint,
        opacity,
    };
    vec![
        Variant {
            code: "H0",
            family: Family::Haze,
            title: "Current build",
            detail: "Mica with the canvas's 74 % dark tint: what Syrinx draws today.",
            ground: material(Backdrop::Mica, 0.74),
            edges: None,
        },
        Variant {
            code: "H1",
            family: Family::Haze,
            title: "Lighter tint over Mica",
            detail: "Mica with a 50 % tint, so the wallpaper's colour comes through more.",
            ground: material(Backdrop::Mica, 0.50),
            edges: None,
        },
        Variant {
            code: "H2",
            family: Family::Haze,
            title: "Mica Alt",
            detail: "Mica Alt, the stronger-coloured Mica Windows uses behind tabbed windows, with a 50 % tint.",
            ground: material(Backdrop::MicaAlt, 0.50),
            edges: None,
        },
        Variant {
            code: "H3",
            family: Family::Haze,
            title: "Mica Alt, light tint",
            detail: "Mica Alt with a 30 % tint: the most wallpaper colour.",
            ground: material(Backdrop::MicaAlt, 0.30),
            edges: None,
        },
    ]
}

fn tint_variants() -> Vec<Variant> {
    let accent = Accent::default();
    let canvas = tint_stops(&Family::Tint.palette());
    let richer = [
        (accent.tone(44., 18.), 0.),
        (accent.tone(32., 10.), 0.55),
        (accent.tone(26., 7.), 1.),
    ];
    let generated = |stops, grain| Ground2::Generated(gradient_image(stops, 155., grain));
    vec![
        Variant {
            code: "T0",
            family: Family::Tint,
            title: "Current build",
            detail: "Two colour stops (GPUI drops the canvas's middle one) and no grain: what Syrinx draws today.",
            ground: Ground2::TwoStop,
            edges: None,
        },
        Variant {
            code: "T1",
            family: Family::Tint,
            title: "The canvas as specified",
            detail: "The canvas's three stops at 0 %, 55 % and 100 %, angled 155°, with grain at 9 % \
                     opacity.",
            ground: generated(canvas, 0.09),
            edges: None,
        },
        Variant {
            code: "T2",
            family: Family::Tint,
            title: "Finer grain",
            detail: "The canvas's three stops with grain at 5 %.",
            ground: generated(canvas, 0.05),
            edges: None,
        },
        Variant {
            code: "T3",
            family: Family::Tint,
            title: "Richer colour",
            detail: "Stops with more of the accent's colour and a lighter top, with grain at 9 %.",
            ground: generated(richer, 0.09),
            edges: None,
        },
    ]
}

/// Tint's gradient stops from its palette.
fn tint_stops(tokens: &Palette) -> [(Color, f32); 3] {
    match tokens.ground {
        Ground::Gradient { stops, .. } => stops,
        Ground::Solid(color) | Ground::Material { tint: color, .. } => {
            [(color, 0.), (color, 0.5), (color, 1.)]
        }
    }
}

/// Today's Tint ground: the first and last stops only.
fn two_stop(tokens: &Palette) -> Background {
    let [(from, from_at), _, (to, to_at)] = tint_stops(tokens);
    linear_gradient(
        155.,
        linear_color_stop(rgba(from), from_at),
        linear_color_stop(rgba(to), to_at),
    )
}

const fn white(alpha: f32) -> Color {
    Color::rgba(255, 255, 255, alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_has_a_unique_code() {
        let lab = ThemeLab::new();
        let mut codes: Vec<_> = lab.variants.iter().map(|variant| variant.code).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), lab.variants.len());
    }
}
