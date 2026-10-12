//! Draws the Customize panel and applies its choices live, saving them through the session (P1-29).

use std::collections::BTreeMap;

use atlas_core::QuickDestinations;
use atlas_ui::metrics::{radius, text};
use atlas_ui::{ACCENT_PRESETS, Accent, Color, Palette, PaletteId, palette};
use gpui::{
    App, AppContext, Context, Div, Entity, FontWeight, InteractiveElement, ParentElement, Stateful,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};

use super::BrowserView;
use crate::appearance::{Appearance, accent_hex, parse_accent};
use crate::session::SessionCommand;
use crate::theme::{self, rgba};

/// The preset names, used as element ids until P1-36 adds accessible names.
const ACCENT_NAMES: [&str; 4] = ["Coral", "Blue", "Mint", "Gold"];
const SWATCH_SIZE: f32 = 28.;

/// The hex field behind the dashed "custom accent" button.
pub(super) struct CustomAccent {
    input: Entity<InputState>,
    error: Option<&'static str>,
    _subscription: Subscription,
}

impl BrowserView {
    /// Shows the stored appearance and tiles; this restores, so nothing is saved back.
    pub(super) fn restore_preferences(
        &mut self,
        settings: &BTreeMap<String, String>,
        destinations: QuickDestinations,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.home.destinations = destinations;
        let (appearance, unreadable) = Appearance::from_settings(settings);
        if !unreadable.0.is_empty() {
            self.message =
                "A saved appearance setting could not be read, so Atlas used its default."
                    .to_owned();
        }
        self.show_appearance(appearance, window, cx);
    }

    /// Applies and saves a Customize choice.
    fn choose_appearance(
        &mut self,
        appearance: Appearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if appearance == self.home.appearance {
            return;
        }
        self.show_appearance(appearance, window, cx);
        self.command(SessionCommand::SaveSettings(appearance.to_settings()));
        cx.notify();
    }

    fn show_appearance(&mut self, appearance: Appearance, window: &mut Window, cx: &mut App) {
        // Every later render, in every Atlas window, reads the new palette.
        self.home.appearance = appearance;
        theme::apply(appearance.drawn_palette());
        theme::apply_component_theme(cx);
        window.set_background_appearance(theme::window_material());
        window.refresh();
    }

    pub(super) fn customize_panel(
        &self,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id("customize-panel")
            .occlude()
            .absolute()
            .top(px(60.))
            .right(px(56.))
            .w(px(420.))
            .rounded(px(16.))
            .bg(rgba(palette.panel))
            .border_1()
            .border_color(rgba(palette.panel_line))
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .text_color(rgba(palette.text))
            .child(self.customize_title(palette, cx))
            .child(self.palette_choices(palette, cx))
            .child(self.accent_choices(palette, cx))
            .child(self.transparency_switch(palette, cx))
    }

    fn customize_title(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .text_size(px(text::SUBTITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Customize"),
            )
            .child(
                div()
                    .id("customize-close")
                    .size(px(28.))
                    .rounded(px(7.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgba(palette.icon_dim))
                    .cursor_pointer()
                    .child("×")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.home.customize_open = false;
                        this.home.custom_accent = None;
                        cx.notify();
                    })),
            )
    }

    fn palette_choices(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let chosen = self.home.appearance;
        let mut rows = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(group_heading(palette, "Palette"));
        for id in PaletteId::ALL {
            let selected = chosen.palette == id;
            let row = palette_row(palette, id, chosen.accent, selected).on_click(cx.listener(
                move |this, _, window, cx| {
                    let appearance = Appearance {
                        palette: id,
                        ..this.home.appearance
                    };
                    this.choose_appearance(appearance, window, cx);
                },
            ));
            rows = rows.child(row);
        }
        rows.child(
            div()
                .text_size(px(text::SMALL))
                .text_color(rgba(palette.text_muted))
                .child(
                    "Haze and Glass let your desktop show through. They switch to Graphite when \
                     Reduce transparency is on.",
                ),
        )
    }

    fn accent_choices(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let chosen = self.home.appearance;
        let mut swatches = div().flex().items_center().gap(px(12.));
        for (index, color) in ACCENT_PRESETS.into_iter().enumerate() {
            let selected = chosen.accent.fill() == color;
            swatches = swatches.child(
                accent_swatch(ACCENT_NAMES[index], palette, color, selected).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.home.custom_accent = None;
                        let appearance = Appearance {
                            accent: Accent::new(color),
                            ..this.home.appearance
                        };
                        this.choose_appearance(appearance, window, cx);
                    },
                )),
            );
        }
        swatches = swatches
            .child(self.custom_accent_button(palette, cx))
            .child(div().flex_1())
            .child(
                div()
                    .font_family(text::MONO_FONT_FAMILIES[0])
                    .text_size(px(text::SMALL))
                    .text_color(rgba(palette.text_muted))
                    .child(accent_hex(chosen.accent)),
            );
        let mut group = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(group_heading(palette, "Accent"))
            .child(swatches);
        if let Some(custom) = &self.home.custom_accent {
            group = group.child(custom_accent_field(custom, palette));
        }
        group
    }

    fn custom_accent_button(&self, palette: &Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let appearance = self.home.appearance;
        // A custom accent shows as a filled swatch; otherwise the dashed "+" invites one.
        let button = if appearance.uses_preset_accent() {
            div()
                .id("accent-custom")
                .size(px(SWATCH_SIZE))
                .flex_none()
                .rounded_full()
                .border(px(1.5))
                .border_dashed()
                .border_color(rgba(palette.dash))
                .text_color(rgba(palette.text_muted))
                .flex()
                .items_center()
                .justify_center()
                .child("+")
        } else {
            accent_swatch("accent-custom", palette, appearance.accent.fill(), true)
        };
        button
            .cursor_pointer()
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_custom_accent(window, cx);
            }))
    }

    fn open_custom_accent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = accent_hex(self.home.appearance.accent);
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("#RRGGBB"));
        input.update(cx, |input, cx| {
            input.set_value(current, window, cx);
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        let subscription = cx.subscribe_in(&input, window, |this, _, event, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.apply_custom_accent(window, cx);
            }
        });
        self.home.custom_accent = Some(CustomAccent {
            input,
            error: None,
            _subscription: subscription,
        });
        cx.notify();
    }

    fn apply_custom_accent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(custom) = &mut self.home.custom_accent else {
            return;
        };
        let text = custom.input.read(cx).value().to_string();
        let Some(accent) = parse_accent(&text) else {
            custom.error = Some("Enter a colour as # and six hex digits, such as #FF8A65.");
            cx.notify();
            return;
        };
        self.home.custom_accent = None;
        let appearance = Appearance {
            accent,
            ..self.home.appearance
        };
        self.choose_appearance(appearance, window, cx);
        cx.notify();
    }

    fn transparency_switch(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let on = self.home.appearance.reduce_transparency;
        let (track, knob) = if on {
            (palette.accent.fill, palette.accent.on_fill)
        } else {
            (palette.field, palette.icon_dim)
        };
        let mut switch = div()
            .id("reduce-transparency")
            .w(px(36.))
            .h(px(20.))
            .flex_none()
            .rounded(px(10.))
            .border_1()
            .border_color(rgba(palette.key))
            .bg(rgba(track))
            .p(px(2.))
            .flex()
            .items_center()
            .cursor_pointer()
            .child(div().size(px(14.)).rounded_full().bg(rgba(knob)))
            .on_click(cx.listener(move |this, _, window, cx| {
                let appearance = Appearance {
                    reduce_transparency: !on,
                    ..this.home.appearance
                };
                this.choose_appearance(appearance, window, cx);
            }));
        if on {
            switch = switch.justify_end();
        }
        div()
            .pt(px(14.))
            .border_t_1()
            .border_color(rgba(palette.panel_divider))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Reduce transparency"),
                    )
                    .child(
                        div()
                            .text_size(px(text::SMALL))
                            .text_color(rgba(palette.text_muted))
                            .child("Draws Haze and Glass as solid Graphite."),
                    ),
            )
            .child(switch)
    }
}

fn group_heading(palette: &Palette, title: &'static str) -> Div {
    div()
        .text_size(px(text::CAPTION))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgba(palette.text_muted))
        .child(title.to_uppercase())
}

fn palette_description(id: PaletteId) -> &'static str {
    match id {
        PaletteId::Graphite => "Solid dark. The default.",
        PaletteId::Haze => "Dark, softly coloured by your wallpaper.",
        PaletteId::Glass => "See-through, with light edges.",
        PaletteId::Tint => "Dark, in your accent colour.",
        PaletteId::Paper => "Light.",
    }
}

/// One palette choice: a miniature of that palette, its name and description.
fn palette_row(panel: &Palette, id: PaletteId, accent: Accent, selected: bool) -> Stateful<Div> {
    let sample = palette(id, accent);
    let (background, outline) = if selected {
        (panel.accent.soft, panel.accent.soft_line)
    } else {
        (Color::rgba(0, 0, 0, 0.), panel.panel_divider)
    };
    let mut row = div()
        .id(id.id())
        .h(px(56.))
        .w_full()
        .flex_none()
        .rounded(px(12.))
        .border_1()
        .border_color(rgba(outline))
        .bg(rgba(background))
        .pl(px(8.))
        .pr(px(14.))
        .flex()
        .items_center()
        .gap(px(12.))
        .cursor_pointer()
        .child(palette_miniature(&sample))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(id.name()))
                .child(
                    div()
                        .text_size(px(text::SMALL))
                        .text_color(rgba(panel.text_muted))
                        .child(palette_description(id)),
                ),
        );
    if selected {
        row = row.child(div().text_color(rgba(panel.accent.ink)).child("✓"));
    }
    row
}

fn palette_miniature(sample: &Palette) -> Div {
    div()
        .relative()
        .w(px(64.))
        .h(px(40.))
        .flex_none()
        .rounded(px(radius::CONTROL))
        .overflow_hidden()
        .border_1()
        .border_color(rgba(sample.line))
        .bg(rgba(sample.ground.representative()))
        .child(
            div()
                .absolute()
                .left(px(6.))
                .top(px(7.))
                .w(px(12.))
                .h(px(4.))
                .rounded(px(2.))
                .bg(rgba(sample.accent.fill)),
        )
        .child(
            div()
                .absolute()
                .left(px(6.))
                .top(px(15.))
                .w(px(14.))
                .h(px(3.))
                .rounded(px(2.))
                .bg(rgba(sample.text_muted)),
        )
        .child(
            div()
                .absolute()
                .left(px(25.))
                .top(px(5.))
                .right(px(4.))
                .bottom(px(4.))
                .rounded(px(radius::XS))
                .bg(rgba(sample.card))
                .border_1()
                .border_color(rgba(sample.card_line)),
        )
}

fn accent_swatch(
    id: impl Into<gpui::ElementId>,
    palette: &Palette,
    color: Color,
    selected: bool,
) -> Stateful<Div> {
    let mut swatch = div()
        .id(id)
        .size(px(SWATCH_SIZE))
        .flex_none()
        .rounded_full()
        .bg(rgba(color))
        .cursor_pointer();
    // The ring uses text colour so it reads on every palette, like the canvas's selection ring.
    if selected {
        swatch = swatch.border_2().border_color(rgba(palette.text));
    }
    swatch
}

fn custom_accent_field(custom: &CustomAccent, palette: &Palette) -> Div {
    let mut field = div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(Input::new(&custom.input));
    if let Some(error) = custom.error {
        field = field.child(
            div()
                .text_size(px(text::SMALL))
                .text_color(rgba(palette.text_secondary))
                .child(error),
        );
    }
    field
}
