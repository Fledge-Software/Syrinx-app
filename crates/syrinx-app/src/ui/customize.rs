//! Draws the Customize panel and applies its choices live, saving them through the session (P1-29).

use std::collections::BTreeMap;

use gpui::{
    App, AppContext, Context, Div, Entity, FontWeight, InteractiveElement, ParentElement, Role,
    Stateful, StatefulInteractiveElement, Styled, Subscription, Toggled, Window, div, px,
};
use gpui_component::input::{InputEvent, InputState};
use syrinx_core::{ActivationTarget, QuickDestinations, TabId};
use syrinx_platform::backdrop::{self, Backdrop};
use syrinx_ui::metrics::{radius, text};
use syrinx_ui::theme::Material;
use syrinx_ui::{ACCENT_PRESETS, Accent, Color, Palette, ShadowRole};

use super::BrowserView;
use super::home_palette::HomePaletteIntent;
use super::main_window::subscribe_in_main;
use crate::appearance::{self, Appearance, accent_hex, parse_accent};
use crate::controls::{control, raised};
use crate::session::SessionCommand;
use crate::text_field;
use crate::theme::{self, rgba};

/// The preset names: element ids and the swatches' accessible names.
const ACCENT_NAMES: [&str; 4] = ["Coral", "Blue", "Mint", "Gold"];
const SWATCH_SIZE: f32 = 28.;

/// The hex field behind the dashed "custom accent" button.
pub(super) struct CustomAccent {
    input: Entity<InputState>,
    error: Option<&'static str>,
    _subscription: Subscription,
}

impl BrowserView {
    /// Shows the stored appearance, saved themes, sidebar width state and tiles. Only an old
    /// palette choice carried over into a saved theme is written back, once.
    pub(super) fn restore_preferences(
        &mut self,
        settings: &BTreeMap<String, String>,
        destinations: QuickDestinations,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.home.destinations = destinations;
        self.restore_sidebar_state(settings);
        let loaded = appearance::load(settings);
        if !loaded.unreadable.0.is_empty() {
            self.message =
                "A saved appearance setting could not be read, so Syrinx used its default."
                    .to_owned();
        }
        // An old Haze, Glass or Tint choice is saved once as a theme of that name.
        if !loaded.carried_over.is_empty() {
            self.command(SessionCommand::SaveSettings(loaded.carried_over));
        }
        self.home.themes = loaded.themes;
        self.show_appearance(loaded.appearance, window, cx);
    }

    /// Applies and saves a Customize choice.
    pub(super) fn choose_appearance(
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

    pub(super) fn show_appearance(
        &mut self,
        appearance: Appearance,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Every later render, in every Syrinx window, reads the new palette.
        self.home.appearance = appearance;
        theme::apply(appearance.drawn_palette(&self.home.themes));
        theme::apply_component_theme(cx);
        show_material(appearance.material(&self.home.themes), window);
        window.refresh();
    }

    pub(super) fn customize_panel(
        &self,
        palette: &Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let contents = div().flex().flex_col().gap(px(16.));
        // Managing site permissions replaces the appearance choices, so the panel stays short.
        let contents = if self.home.site_permissions.is_open() {
            contents.child(self.site_permissions_view(palette, cx))
        } else if self.usage.report.is_open() {
            // The report view replaces the choices in the same way.
            contents.child(self.usage_report_view(palette, cx))
        } else {
            contents
                .child(self.customize_title(palette, cx))
                .child(self.theme_choices(palette, window, cx))
                .child(self.accent_choices(palette, window, cx))
                .child(self.transparency_switch(palette, cx))
                .child(self.site_permissions_entry(palette, cx))
                .child(self.usage_section(palette, cx))
                .child(self.updates_section(palette, cx))
        };
        let panel = raised(div().id("customize-panel"), ShadowRole::Panel)
            .role(Role::Dialog)
            .aria_label("Customize")
            .occlude()
            .max_h_full()
            .overflow_y_scroll()
            .rounded(px(16.))
            .bg(rgba(palette.panel))
            .border_1()
            .border_color(rgba(palette.panel_line))
            .p(px(16.))
            .text_color(rgba(palette.text))
            .child(contents);
        // The frame spans the card's height, so a short window scrolls the panel instead of
        // clipping it; the frame has no handlers, so clicks beside the panel reach Home.
        div()
            .absolute()
            .top(px(60.))
            .bottom(px(16.))
            .right(px(56.))
            .w(px(420.))
            .flex()
            .flex_col()
            .child(panel)
    }

    /// The sidebar's Appearance (settings) button: the Customize panel lives on Home, so
    /// another page switches to an open Home tab, or opens one, with the panel open.
    pub(super) fn open_appearance(&mut self) {
        let home_showing = self.home_showing();
        let open_home = self.first_home_tab();
        let route = appearance_route_ending_entry(
            &mut self.home_palette,
            home_showing,
            self.home.customize_open,
            open_home,
        );
        match route {
            AppearanceRoute::Close => {
                self.close_customize();
                return;
            }
            AppearanceRoute::Show => {}
            AppearanceRoute::SwitchTo(id) => self.activate(ActivationTarget::Tab(id)),
            AppearanceRoute::OpenHome => self.open_home_without_palette(),
        }
        self.home.customize_open = true;
    }

    /// Opens a Home tab for the panel. It is not a palette entry: nobody asked to type, so
    /// its acknowledgement must not open the palette over the panel (#14).
    fn open_home_without_palette(&mut self) {
        self.try_command_without_entry(SessionCommand::NewTab {
            address: "about:blank".to_owned(),
        });
    }

    /// The first open tab shown as Home, which the Appearance button reuses.
    fn first_home_tab(&self) -> Option<TabId> {
        self.model
            .tabs
            .iter()
            .find(|tab| self.tab_is_home(tab))
            .map(|tab| tab.id)
    }

    /// Closes the panel, dropping an unfinished custom accent and the permissions view.
    pub(super) fn close_customize(&mut self) {
        self.home.customize_open = false;
        self.home.custom_accent = None;
        self.home.theme_editor = None;
        self.home.site_permissions.close();
        self.usage.report.close();
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
            .child(self.customize_close_button(palette, cx))
    }

    pub(super) fn customize_close_button(
        &self,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        control(div().id("customize-close"), Role::Button, "Close Customize")
            .size(px(28.))
            .rounded(px(7.))
            .flex()
            .items_center()
            .justify_center()
            .text_color(rgba(palette.icon_dim))
            .cursor_pointer()
            .child("×")
            .on_click(cx.listener(|this, _, _, cx| {
                this.close_customize();
                cx.notify();
            }))
    }

    fn accent_choices(&self, palette: &Palette, window: &Window, cx: &mut Context<Self>) -> Div {
        let chosen = self.home.appearance;
        let mut swatches = div().flex().items_center().gap(px(12.));
        for (index, color) in ACCENT_PRESETS.into_iter().enumerate() {
            let selected = chosen.accent.fill() == color;
            swatches = swatches.child(
                accent_swatch(
                    ACCENT_NAMES[index],
                    ACCENT_NAMES[index],
                    palette,
                    color,
                    selected,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.home.custom_accent = None;
                    let appearance = Appearance {
                        accent: Accent::new(color),
                        ..this.home.appearance
                    };
                    this.choose_appearance(appearance, window, cx);
                })),
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
            group = group.child(custom_accent_field(custom, palette, window, cx));
        }
        group
    }

    fn custom_accent_button(&self, palette: &Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let appearance = self.home.appearance;
        // A custom accent shows as a filled swatch; otherwise the dashed "+" invites one.
        let button = if appearance.uses_preset_accent() {
            control(div().id("accent-custom"), Role::Button, "Custom accent")
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
            accent_swatch(
                "accent-custom",
                "Custom accent",
                palette,
                appearance.accent.fill(),
                true,
            )
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
        let subscription = subscribe_in_main(&input, cx, |this, event, window, cx| {
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
            custom.error = Some("Enter a colour as # and six hex digits, such as #FF612E.");
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
        let switch = switch_control("reduce-transparency", "Reduce transparency", on, palette)
            .on_click(cx.listener(move |this, _, window, cx| {
                let appearance = Appearance {
                    reduce_transparency: !on,
                    ..this.home.appearance
                };
                this.choose_appearance(appearance, window, cx);
            }));
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
                            .child("Draws see-through themes opaque."),
                    ),
            )
            .child(switch)
    }
}

/// Ask Windows for the theme's material; an opaque theme keeps Mica under its solid fill.
fn show_material(material: Option<Material>, window: &Window) {
    let backdrop = match material {
        Some(Material::MicaAlt) => Backdrop::MicaAlt,
        Some(Material::Acrylic) => Backdrop::Acrylic,
        Some(Material::Mica | Material::None) | None => Backdrop::Mica,
    };
    let applied = super::native_handle(window)
        .map_err(str::to_owned)
        .and_then(|hwnd| backdrop::apply(hwnd, backdrop).map_err(|error| error.to_string()));
    if let Err(error) = applied {
        eprintln!("Syrinx could not set the {backdrop:?} backdrop: {error}");
    }
}

pub(super) fn group_heading(palette: &Palette, title: &'static str) -> Div {
    div()
        .text_size(px(text::CAPTION))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgba(palette.text_muted))
        .child(title.to_uppercase())
}

/// What one theme row shows: its element id, name, a line about it and its colours.
pub(super) struct ThemeRow<'a> {
    pub(super) id: gpui::ElementId,
    pub(super) name: &'a str,
    pub(super) description: &'a str,
    pub(super) sample: &'a Palette,
}

/// One theme choice: a miniature of that theme, its name and description.
pub(super) fn theme_row(panel: &Palette, row: ThemeRow<'_>, selected: bool) -> Stateful<Div> {
    let ThemeRow {
        id,
        name,
        description,
        sample,
    } = row;
    let (background, outline) = if selected {
        (panel.accent.soft, panel.accent.soft_line)
    } else {
        (Color::rgba(0, 0, 0, 0.), panel.panel_divider)
    };
    let mut row = control(div().id(id), Role::RadioButton, name.to_owned())
        .aria_description(description.to_owned())
        .aria_toggled(toggled(selected))
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
        .child(palette_miniature(sample))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(name.to_owned()),
                )
                .child(
                    div()
                        .text_size(px(text::SMALL))
                        .text_color(rgba(panel.text_muted))
                        .child(description.to_owned()),
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

/// A radio button's or switch's checked state for screen readers.
pub(super) fn toggled(on: bool) -> Toggled {
    if on { Toggled::True } else { Toggled::False }
}

/// A Customize on/off switch, drawn and announced as one; the caller adds its click.
pub(super) fn switch_control(
    id: &'static str,
    label: &'static str,
    on: bool,
    palette: &Palette,
) -> Stateful<Div> {
    let (track, knob) = if on {
        (palette.accent.fill, palette.accent.on_fill)
    } else {
        (palette.field, palette.icon_dim)
    };
    let switch = control(div().id(id), Role::Switch, label)
        .aria_toggled(toggled(on))
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
        .child(div().size(px(14.)).rounded_full().bg(rgba(knob)));
    // The knob sits right when on, as Windows draws its own switches.
    if on { switch.justify_end() } else { switch }
}

fn accent_swatch(
    id: impl Into<gpui::ElementId>,
    name: &'static str,
    palette: &Palette,
    color: Color,
    selected: bool,
) -> Stateful<Div> {
    let mut swatch = control(div().id(id), Role::RadioButton, name)
        .aria_toggled(toggled(selected))
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

fn custom_accent_field(custom: &CustomAccent, palette: &Palette, window: &Window, cx: &App) -> Div {
    let mut field = div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(text_field::labelled(
            text_field::CUSTOM_ACCENT,
            &custom.input,
            window,
            cx,
        ));
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

/// Where the Appearance button takes the owner.
#[derive(Debug, PartialEq, Eq)]
enum AppearanceRoute {
    /// Home already shows the panel, so a second press closes it.
    Close,
    /// Home is showing; open the panel on it.
    Show,
    /// Another page is showing and a Home tab is open; switch to it.
    SwitchTo(TabId),
    /// No Home tab is open; open one for the panel, without the palette (#14).
    OpenHome,
}

/// Chooses the Appearance button's route, reusing a Home tab already in the model. A Home
/// still being opened is not in the model yet, so two quick presses can still open two.
fn appearance_route(
    home_showing: bool,
    customize_open: bool,
    open_home: Option<TabId>,
) -> AppearanceRoute {
    if home_showing {
        return if customize_open {
            AppearanceRoute::Close
        } else {
            AppearanceRoute::Show
        };
    }
    match open_home {
        Some(id) => AppearanceRoute::SwitchTo(id),
        None => AppearanceRoute::OpenHome,
    }
}

/// Ends a pending Ctrl T and chooses the gear's route. The gear supersedes the Ctrl T, whose
/// late acknowledgement would otherwise open the palette over the panel; showing or closing
/// the panel queues no command that would end it (#78).
fn appearance_route_ending_entry(
    intent: &mut HomePaletteIntent,
    home_showing: bool,
    customize_open: bool,
    open_home: Option<TabId>,
) -> AppearanceRoute {
    intent.finish_entry();
    appearance_route(home_showing, customize_open, open_home)
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use syrinx_core::{Activation, TabKind, TabMetadata, TabModel, TabModelSnapshot};

    use super::*;

    fn tab(id: u64) -> TabId {
        TabId::new(id).expect("synthetic tab ID")
    }

    /// A model whose active tab is a freshly created blank Home.
    fn new_home_model() -> (TabModelSnapshot, TabId) {
        let mut model = TabModel::new();
        let id = model
            .open(
                TabKind::Web,
                TabMetadata {
                    address: "about:blank".into(),
                    ..TabMetadata::default()
                },
                Activation::Foreground,
            )
            .expect("open synthetic Home");
        (model.snapshot(), id)
    }

    /// Queues Ctrl T's blank tab and returns the request ID its acknowledgement carries.
    fn press_ctrl_t(intent: &mut HomePaletteIntent) -> Option<u64> {
        let (sender, receiver) = mpsc::channel();
        let command = SessionCommand::NewTab {
            address: "about:blank".into(),
        };
        intent
            .send(&sender, command)
            .expect("queue synthetic Ctrl T");
        match receiver.recv().expect("receive synthetic Ctrl T") {
            SessionCommand::HomeEntry { request_id, .. } => Some(request_id),
            _ => panic!("Ctrl T must carry its Home request identity"),
        }
    }

    #[test]
    fn the_gear_on_home_ends_a_pending_ctrl_t_so_no_palette_opens_over_the_panel() {
        let (home, id) = new_home_model();
        // Show (panel closed) and Close (panel open) queue no session command (#78).
        for (customize_open, expected) in [
            (false, AppearanceRoute::Show),
            (true, AppearanceRoute::Close),
        ] {
            let mut intent = HomePaletteIntent::default();
            let pending = press_ctrl_t(&mut intent);
            let route = appearance_route_ending_entry(&mut intent, true, customize_open, Some(id));
            assert_eq!(route, expected);
            assert!(!intent.acknowledge(&home, &[id], pending));
            // A Ctrl T pressed after the gear is an ordinary palette entry again.
            let fresh = press_ctrl_t(&mut intent);
            assert!(intent.acknowledge(&home, &[id], fresh));
        }
    }

    #[test]
    fn appearance_on_home_shows_the_panel_and_a_second_press_closes_it() {
        assert_eq!(
            appearance_route(true, false, Some(tab(1))),
            AppearanceRoute::Show
        );
        assert_eq!(
            appearance_route(true, true, Some(tab(1))),
            AppearanceRoute::Close
        );
    }

    #[test]
    fn appearance_on_a_page_reuses_an_open_home_tab_instead_of_adding_one() {
        assert_eq!(
            appearance_route(false, false, Some(tab(7))),
            AppearanceRoute::SwitchTo(tab(7))
        );
        // The panel was left open on a Home the owner has since moved away from.
        assert_eq!(
            appearance_route(false, true, Some(tab(7))),
            AppearanceRoute::SwitchTo(tab(7))
        );
    }

    #[test]
    fn appearance_with_no_home_tab_opens_one() {
        assert_eq!(
            appearance_route(false, false, None),
            AppearanceRoute::OpenHome
        );
    }
}
