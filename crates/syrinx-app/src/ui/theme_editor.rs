//! The Theme section of Customize and Settings: Graphite and Paper, the owner's saved themes,
//! and the editor that changes one of them live (`docs/theme-editor.md`).

use gpui::{
    AppContext, Context, Div, Entity, FontWeight, InteractiveElement, ParentElement, Role,
    Stateful, StatefulInteractiveElement, Styled, Subscription, Window, div, px,
};
use gpui_component::input::{InputEvent, InputState};
use syrinx_ui::metrics::text;
use syrinx_ui::theme::{
    self, ANGLE, EDGES, Fill, GRAIN, Material, Mode, Range, TRANSPARENCY, ThemeSpec,
};
use syrinx_ui::{Accent, Color, Palette};

use super::BrowserView;
use super::customize::{ThemeRow, group_heading, theme_row, toggled};
use super::main_window::subscribe_in_main;
use crate::appearance::{Appearance, MAX_THEMES, PRESETS, ThemeChoice, shown_name};
use crate::controls::control;
use crate::session::SessionCommand;
use crate::text_field;
use crate::theme::rgba;

/// One stepped choice: its label, range, and the theme field it changes.
#[derive(Clone, Copy)]
struct Stepped {
    label: &'static str,
    range: Range,
    field: fn(&mut ThemeSpec) -> &mut u16,
}

const TRANSPARENCY_STEPS: Stepped = Stepped {
    label: "Transparency",
    range: TRANSPARENCY,
    field: |theme| &mut theme.transparency,
};
const EDGE_STEPS: Stepped = Stepped {
    label: "Glass edges",
    range: EDGES,
    field: |theme| &mut theme.edges,
};
const GRAIN_STEPS: Stepped = Stepped {
    label: "Grain",
    range: GRAIN,
    field: |theme| &mut theme.grain,
};

/// The name a new theme starts with; a number is added while it is taken.
const NEW_THEME_NAME: &str = "My theme";

/// The theme being edited and its three text fields.
pub(super) struct ThemeEditor {
    slot: usize,
    name: Entity<InputState>,
    background: Entity<InputState>,
    second: Entity<InputState>,
    error: Option<&'static str>,
    _subscriptions: Vec<Subscription>,
}

impl BrowserView {
    /// The presets, the saved themes, "New theme" and, while one is edited, its controls.
    pub(super) fn theme_choices(
        &self,
        palette: &Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut rows = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(group_heading(palette, "Theme"));
        for (choice, name, description) in PRESETS {
            rows = rows.child(self.preset_row(palette, choice, (name, description), cx));
        }
        for (slot, theme) in self.home.themes.iter() {
            rows = rows.child(self.custom_row(palette, slot, theme, cx));
            let editing = self.home.theme_editor.as_ref();
            if let Some(editor) = editing.filter(|editor| editor.slot == slot) {
                rows = rows.child(self.editor_view(editor, theme, palette, window, cx));
            }
        }
        let full = self.home.themes.first_free().is_none();
        let new_theme = new_theme_button(palette, full);
        rows = rows.child(if full {
            new_theme
        } else {
            new_theme.on_click(cx.listener(|this, _, window, cx| {
                this.new_theme(window, cx);
            }))
        });
        if full {
            rows = rows.child(note(
                palette,
                &format!(
                    "{MAX_THEMES} themes is the most Syrinx keeps; delete one to add another."
                ),
            ));
        }
        rows
    }

    /// One preset's row, its sample drawn exactly as choosing it would draw the window.
    fn preset_row(
        &self,
        panel: &Palette,
        choice: ThemeChoice,
        (name, description): (&'static str, &'static str),
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let sample = Appearance {
            theme: choice,
            ..self.home.appearance
        }
        .drawn_palette(&self.home.themes);
        let row = ThemeRow {
            id: name.to_lowercase().into(),
            name,
            description,
            sample: &sample,
        };
        theme_row(panel, row, self.home.appearance.theme == choice).on_click(cx.listener(
            move |this, _, window, cx| {
                this.choose_theme(choice, window, cx);
            },
        ))
    }

    /// A saved theme's row and its Edit button.
    fn custom_row(
        &self,
        panel: &Palette,
        slot: usize,
        theme: &ThemeSpec,
        cx: &mut Context<Self>,
    ) -> Div {
        let appearance = self.home.appearance;
        let sample = theme.palette(appearance.accent, appearance.reduce_transparency);
        let description = describe(theme);
        // Shown, never stored: a saved "Glass" reads "Glass (saved)" beside the preset.
        let name = shown_name(theme);
        let row = ThemeRow {
            id: ("theme-row", slot).into(),
            name: &name,
            description: &description,
            sample: &sample,
        };
        let selected = appearance.theme == ThemeChoice::Custom(slot);
        let editing = self
            .home
            .theme_editor
            .as_ref()
            .is_some_and(|editor| editor.slot == slot);
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(theme_row(panel, row, selected).on_click(cx.listener(
                        move |this, _, window, cx| {
                            this.choose_theme(ThemeChoice::Custom(slot), window, cx);
                        },
                    ))),
            )
            .child(
                small_button(
                    ("theme-edit", slot),
                    if editing { "Done" } else { "Edit" },
                    panel,
                )
                .aria_label(format!(
                    "{} {}",
                    if editing { "Done editing" } else { "Edit" },
                    name
                ))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if editing {
                        this.finish_theme_edit(window, cx);
                    } else {
                        this.open_theme_editor(slot, window, cx);
                    }
                })),
            )
    }

    fn choose_theme(&mut self, theme: ThemeChoice, window: &mut Window, cx: &mut Context<Self>) {
        let appearance = Appearance {
            theme,
            ..self.home.appearance
        };
        self.choose_appearance(appearance, window, cx);
    }

    /// Save a new theme, starting from Graphite's look, choose it and open its editor.
    fn new_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.home.themes.first_free() else {
            return;
        };
        let name = self.home.themes.unused_name(NEW_THEME_NAME);
        let row = self.home.themes.set(slot, ThemeSpec::new(&name));
        self.command(SessionCommand::SaveSettings(vec![row]));
        self.choose_theme(ThemeChoice::Custom(slot), window, cx);
        self.open_theme_editor(slot, window, cx);
    }

    fn open_theme_editor(&mut self, slot: usize, window: &mut Window, cx: &mut Context<Self>) {
        // Keep what was typed in the editor this one replaces; a bad colour keeps it open.
        if !self.apply_theme_text(window, cx) {
            return;
        }
        let Some(theme) = self.home.themes.get(slot).cloned() else {
            return;
        };
        let second = match theme.fill {
            Fill::Gradient { second, .. } => second,
            Fill::Solid => theme.background,
        };
        let name = self.field(&theme.name, "Theme name", window, cx);
        let background = self.field(&theme::hex(theme.background), "#RRGGBB", window, cx);
        let second = self.field(&theme::hex(second), "#RRGGBB", window, cx);
        // Enter in any field applies all three, as the custom accent field does.
        let subscriptions = [&name, &background, &second]
            .into_iter()
            .map(|input| {
                subscribe_in_main(input, cx, |this, event, window, cx| {
                    if let InputEvent::PressEnter { .. } = event {
                        this.apply_theme_text(window, cx);
                    }
                })
            })
            .collect();
        self.home.theme_editor = Some(ThemeEditor {
            slot,
            name,
            background,
            second,
            error: None,
            _subscriptions: subscriptions,
        });
        self.choose_theme(ThemeChoice::Custom(slot), window, cx);
        cx.notify();
    }

    fn field(
        &self,
        value: &str,
        placeholder: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let value = value.to_owned();
        input.update(cx, |input, cx| input.set_value(value, window, cx));
        input
    }

    /// Apply the text fields, then close the editor if they were valid.
    fn finish_theme_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.apply_theme_text(window, cx) {
            self.home.theme_editor = None;
            cx.notify();
        }
    }

    /// Read the name and colours; `false`, with a message, when a colour is not `#RRGGBB`.
    fn apply_theme_text(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(editor) = &mut self.home.theme_editor else {
            return true;
        };
        let read = |input: &Entity<InputState>| input.read(cx).value().to_string();
        let name = theme::clean_name(&read(&editor.name));
        let background = theme::parse_hex(&read(&editor.background));
        // The second colour is read only while its field shows: a gradient with no material.
        let shown = self
            .home
            .themes
            .get(editor.slot)
            .is_some_and(|theme| theme.material == Material::None && theme.fill != Fill::Solid);
        // A hidden field keeps the stored second colour (`None`); a shown one must parse.
        let second = if shown {
            theme::parse_hex(&read(&editor.second)).map(Some)
        } else {
            Some(None)
        };
        let (Some(background), Some(second)) = (background, second) else {
            editor.error = Some("Enter each colour as # and six hex digits, such as #141519.");
            cx.notify();
            return false;
        };
        editor.error = None;
        let slot = editor.slot;
        self.change_theme(
            slot,
            move |theme| apply_typed(theme, name, background, second),
            window,
            cx,
        );
        true
    }

    /// Change a saved theme, save it, and redraw the window if it is the one shown.
    fn change_theme(
        &mut self,
        slot: usize,
        change: impl FnOnce(&mut ThemeSpec),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut theme) = self.home.themes.get(slot).cloned() else {
            return;
        };
        let before = theme.clone();
        change(&mut theme);
        if theme == before {
            return;
        }
        let row = self.home.themes.set(slot, theme);
        self.command(SessionCommand::SaveSettings(vec![row]));
        if self.home.appearance.theme == ThemeChoice::Custom(slot) {
            self.show_appearance(self.home.appearance, window, cx);
        }
        cx.notify();
    }

    fn duplicate_theme(&mut self, slot: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(theme), Some(copy_slot)) = (
            self.home.themes.get(slot).cloned(),
            self.home.themes.first_free(),
        ) else {
            return;
        };
        let name = self
            .home
            .themes
            .unused_name(&format!("{} copy", theme.name));
        let copy = ThemeSpec {
            name: theme::clean_name(&name),
            ..theme
        };
        let row = self.home.themes.set(copy_slot, copy);
        self.command(SessionCommand::SaveSettings(vec![row]));
        self.open_theme_editor(copy_slot, window, cx);
    }

    /// Delete a saved theme; if it was shown, Graphite takes its place.
    fn delete_theme(&mut self, slot: usize, window: &mut Window, cx: &mut Context<Self>) {
        let row = self.home.themes.remove(slot);
        self.command(SessionCommand::SaveSettings(vec![row]));
        self.home.theme_editor = None;
        if self.home.appearance.theme == ThemeChoice::Custom(slot) {
            self.choose_theme(ThemeChoice::Graphite, window, cx);
        }
        cx.notify();
    }

    fn editor_view(
        &self,
        editor: &ThemeEditor,
        theme: &ThemeSpec,
        panel: &Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let slot = editor.slot;
        let material_on = theme.material != Material::None;
        let mut view = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(12.))
            .rounded(px(12.))
            .border_1()
            .border_color(rgba(panel.panel_divider))
            .child(labelled(
                panel,
                "Name",
                text_field::labelled(text_field::THEME_NAME, &editor.name, window, cx),
            ))
            .child(labelled(
                panel,
                "Mode",
                self.mode_choice(slot, theme, panel, cx),
            ))
            .child(labelled(
                panel,
                "Background",
                text_field::labelled(text_field::THEME_BACKGROUND, &editor.background, window, cx),
            ))
            .child(labelled(
                panel,
                "Material",
                self.material_choice(slot, theme, panel, cx),
            ));
        if material_on {
            view = view
                .child(self.stepper(slot, TRANSPARENCY_STEPS, theme, panel, cx))
                .child(labelled(
                    panel,
                    "Home",
                    self.home_choice(slot, theme, panel, cx),
                ));
        } else {
            view = view.child(labelled(
                panel,
                "Fill",
                self.fill_choice(slot, theme, panel, cx),
            ));
            if let Fill::Gradient { angle, .. } = theme.fill {
                view = view
                    .child(labelled(
                        panel,
                        "Second colour",
                        text_field::labelled(text_field::THEME_SECOND, &editor.second, window, cx),
                    ))
                    .child(self.angle_stepper(slot, angle, panel, cx));
            }
        }
        view = view.child(self.stepper(slot, EDGE_STEPS, theme, panel, cx));
        // Grain is drawn into an opaque image, which would cover a material, so it is offered
        // only on solid and gradient backgrounds.
        if !material_on {
            view = view.child(self.stepper(slot, GRAIN_STEPS, theme, panel, cx));
        }
        if let Some(error) = editor.error {
            view = view.child(note(panel, error));
        }
        if let Some(warning) = contrast_note(theme, self.home.appearance.accent) {
            view = view.child(note(panel, warning));
        }
        if material_on {
            view = view.child(note(
                panel,
                "Pages and the command palette stay opaque. Reduce transparency draws this theme \
                 opaque.",
            ));
        }
        view.child(self.editor_actions(slot, panel, cx))
    }

    fn mode_choice(
        &self,
        slot: usize,
        theme: &ThemeSpec,
        panel: &Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let options = [(Mode::Dark, "Dark"), (Mode::Light, "Light")];
        let mut group = segmented_group();
        for (mode, label) in options {
            group = group.child(
                segment(
                    ("theme-mode", slot * 10 + mode as usize),
                    label,
                    theme.mode == mode,
                    panel,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.change_theme(slot, |theme| theme.mode = mode, window, cx);
                })),
            );
        }
        group
    }

    fn material_choice(
        &self,
        slot: usize,
        theme: &ThemeSpec,
        panel: &Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut group = segmented_group();
        for (index, material) in Material::ALL.into_iter().enumerate() {
            let id = ("theme-material", slot * 10 + index);
            group = group.child(
                segment(id, material.name(), theme.material == material, panel).on_click(
                    cx.listener(move |this, _, window, cx| {
                        this.change_theme(slot, |theme| theme.material = material, window, cx);
                    }),
                ),
            );
        }
        group
    }

    /// Whether Home's card stays solid or follows Transparency like the rest of the chrome.
    fn home_choice(
        &self,
        slot: usize,
        theme: &ThemeSpec,
        panel: &Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let options = [(false, "Solid"), (true, "See-through")];
        let mut group = segmented_group();
        for (see_through, label) in options {
            let id = ("theme-home", slot * 10 + usize::from(see_through));
            group = group.child(
                segment(id, label, theme.see_through_home == see_through, panel).on_click(
                    cx.listener(move |this, _, window, cx| {
                        this.change_theme(
                            slot,
                            |theme| theme.see_through_home = see_through,
                            window,
                            cx,
                        );
                    }),
                ),
            );
        }
        group
    }

    fn fill_choice(
        &self,
        slot: usize,
        theme: &ThemeSpec,
        panel: &Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let gradient = matches!(theme.fill, Fill::Gradient { .. });
        let solid = segment(("theme-fill", slot * 10), "Solid", !gradient, panel).on_click(
            cx.listener(move |this, _, window, cx| {
                this.change_theme(slot, |theme| theme.fill = Fill::Solid, window, cx);
            }),
        );
        let blend = segment(("theme-fill", slot * 10 + 1), "Gradient", gradient, panel).on_click(
            cx.listener(move |this, _, window, cx| {
                this.change_theme(
                    slot,
                    |theme| {
                        if !matches!(theme.fill, Fill::Gradient { .. }) {
                            theme.fill = Fill::Gradient {
                                second: Color::hex(0x2A1838),
                                angle: 155,
                            };
                        }
                    },
                    window,
                    cx,
                );
                this.reset_second_field(window, cx);
            }),
        );
        segmented_group().child(solid).child(blend)
    }

    /// After Gradient is chosen, the second colour field shows the colour the theme now uses.
    fn reset_second_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &self.home.theme_editor else {
            return;
        };
        let Some(Fill::Gradient { second, .. }) =
            self.home.themes.get(editor.slot).map(|theme| theme.fill)
        else {
            return;
        };
        let value = theme::hex(second);
        editor
            .second
            .update(cx, |input, cx| input.set_value(value, window, cx));
    }

    fn stepper(
        &self,
        slot: usize,
        stepped: Stepped,
        theme: &ThemeSpec,
        panel: &Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let Stepped {
            label,
            range,
            field,
        } = stepped;
        let mut copy = theme.clone();
        let value = *field(&mut copy);
        let (lower, raise) = (value > range.min, value < range.max);
        let lower_click = step_listener(slot, stepped, -1, cx);
        let raise_click = step_listener(slot, stepped, 1, cx);
        stepper_row(panel, label, &format!("{value} %")).child(stepper_buttons(
            label,
            slot,
            panel,
            (lower, raise),
            lower_click,
            raise_click,
        ))
    }

    fn angle_stepper(
        &self,
        slot: usize,
        angle: u16,
        panel: &Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let left = turn_listener(slot, -1, cx);
        let right = turn_listener(slot, 1, cx);
        stepper_row(panel, "Angle", &format!("{angle}°")).child(stepper_buttons(
            "Angle",
            slot,
            panel,
            (true, true),
            left,
            right,
        ))
    }

    fn editor_actions(&self, slot: usize, panel: &Palette, cx: &mut Context<Self>) -> Div {
        let full = self.home.themes.first_free().is_none();
        let mut actions = div().flex().gap(px(8.));
        if !full {
            actions = actions.child(
                small_button(("theme-duplicate", slot), "Duplicate", panel).on_click(
                    cx.listener(move |this, _, window, cx| this.duplicate_theme(slot, window, cx)),
                ),
            );
        }
        actions
            .child(
                small_button(("theme-delete", slot), "Delete", panel).on_click(
                    cx.listener(move |this, _, window, cx| this.delete_theme(slot, window, cx)),
                ),
            )
            .child(div().flex_1())
            .child(
                small_button(("theme-done", slot), "Done", panel).on_click(
                    cx.listener(|this, _, window, cx| this.finish_theme_edit(window, cx)),
                ),
            )
    }
}

/// Put the editor's typed name and colours into `theme`. `second` is `None` while its field
/// is hidden (a material is on), so a gradient keeps its stored second colour.
fn apply_typed(theme: &mut ThemeSpec, name: String, background: Color, second: Option<Color>) {
    if !name.is_empty() {
        theme.name = name;
    }
    theme.background = background;
    if let (Some(second), Fill::Gradient { angle, .. }) = (second, theme.fill) {
        theme.fill = Fill::Gradient { second, angle };
    }
}

/// A click that moves a stepped choice by `steps`.
fn step_listener(
    slot: usize,
    stepped: Stepped,
    steps: i32,
    cx: &mut Context<BrowserView>,
) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static {
    cx.listener(move |this, _, window, cx| {
        this.change_theme(
            slot,
            |theme| {
                let current = (stepped.field)(theme);
                *current = stepped.range.stepped(*current, steps);
            },
            window,
            cx,
        );
    })
}

/// A click that turns the gradient by 15° per step.
fn turn_listener(
    slot: usize,
    steps: i32,
    cx: &mut Context<BrowserView>,
) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static {
    cx.listener(move |this, _, window, cx| {
        this.change_theme(
            slot,
            |theme| {
                if let Fill::Gradient { second, angle } = theme.fill {
                    theme.fill = Fill::Gradient {
                        second,
                        angle: ANGLE.wrapped(angle, steps * 3),
                    };
                }
            },
            window,
            cx,
        );
    })
}

/// What the editor says about a theme's contrast: whether text itself still fails, or only
/// marks and tile letters do while text was adjusted to pass.
fn contrast_note(theme: &ThemeSpec, accent: Accent) -> Option<&'static str> {
    let problems = theme.contrast_problems(accent);
    if problems
        .iter()
        .any(|check| theme::is_readable_role(check.foreground))
    {
        return Some(
            "Some text falls below the contrast minimum on this background, even after it was \
             lightened or darkened. Lower Transparency or choose another background colour.",
        );
    }
    if problems.is_empty() {
        return None;
    }
    Some(
        "Some marks or tile letters fall below the contrast minimum on this background. Text \
         has been lightened or darkened to stay readable.",
    )
}

/// A short line about a saved theme for its row.
fn describe(theme: &ThemeSpec) -> String {
    let mode = match theme.mode {
        Mode::Dark => "Dark",
        Mode::Light => "Light",
    };
    let look = match (theme.material, theme.fill) {
        (Material::None, Fill::Solid) => "solid".to_owned(),
        (Material::None, Fill::Gradient { .. }) => "gradient".to_owned(),
        (material, _) => format!("{}, {} % see-through", material.name(), theme.transparency),
    };
    let home = if theme.see_through_home && theme.material != Material::None {
        ", Home too"
    } else {
        ""
    };
    format!("{mode}, {look}{home}.")
}

fn labelled(panel: &Palette, label: &'static str, control: impl gpui::IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(text::SMALL))
                .text_color(rgba(panel.text_secondary))
                .child(label),
        )
        .child(control)
}

fn note(panel: &Palette, message: &str) -> Div {
    div()
        .text_size(px(text::SMALL))
        .text_color(rgba(panel.text_muted))
        .child(message.to_owned())
}

fn segmented_group() -> Div {
    div().flex().flex_wrap().gap(px(6.))
}

fn segment(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    selected: bool,
    panel: &Palette,
) -> Stateful<Div> {
    let (background, outline) = if selected {
        (panel.accent.soft, panel.accent.soft_line)
    } else {
        (panel.field, panel.panel_divider)
    };
    control(div().id(id), Role::RadioButton, label)
        .aria_toggled(toggled(selected))
        .h(px(28.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(8.))
        .border_1()
        .border_color(rgba(outline))
        .bg(rgba(background))
        .cursor_pointer()
        .child(label)
}

fn small_button(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    panel: &Palette,
) -> Stateful<Div> {
    control(div().id(id), Role::Button, label)
        .h(px(28.))
        .px(px(10.))
        .flex()
        .flex_none()
        .items_center()
        .rounded(px(8.))
        .border_1()
        .border_color(rgba(panel.panel_divider))
        .bg(rgba(panel.field))
        .cursor_pointer()
        .child(label)
}

fn new_theme_button(panel: &Palette, full: bool) -> Stateful<Div> {
    let button = control(div().id("theme-new"), Role::Button, "New theme")
        .h(px(40.))
        .w_full()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(12.))
        .border(px(1.5))
        .border_dashed()
        .border_color(rgba(panel.dash))
        .text_color(rgba(panel.text_secondary))
        .child("+ New theme");
    if full {
        button.opacity(0.5)
    } else {
        button.cursor_pointer()
    }
}

/// A stepper's label and value; the caller adds its buttons.
fn stepper_row(panel: &Palette, label: &'static str, value: &str) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .flex_1()
                .text_size(px(text::SMALL))
                .text_color(rgba(panel.text_secondary))
                .child(label),
        )
        .child(
            div()
                .min_w(px(48.))
                .font_weight(FontWeight::MEDIUM)
                .child(value.to_owned()),
        )
}

fn stepper_buttons(
    label: &'static str,
    slot: usize,
    panel: &Palette,
    (can_lower, can_raise): (bool, bool),
    lower: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    raise: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> Div {
    let button = |id: (&'static str, usize), glyph: &'static str, enabled: bool| {
        let element = control(div().id(id), Role::Button, format!("{glyph} {label}"))
            .size(px(28.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .border_1()
            .border_color(rgba(panel.panel_divider))
            .bg(rgba(panel.field))
            .child(glyph);
        if enabled {
            element.cursor_pointer()
        } else {
            element.opacity(0.4)
        }
    };
    let lower_button = button((label, slot * 2), "−", can_lower);
    let raise_button = button((label, slot * 2 + 1), "+", can_raise);
    div()
        .flex()
        .gap(px(4.))
        .child(if can_lower {
            lower_button.on_click(lower)
        } else {
            lower_button
        })
        .child(if can_raise {
            raise_button.on_click(raise)
        } else {
            raise_button
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hidden_second_colour_field_keeps_the_gradient_colour() {
        let second = Color::hex(0x2A1838);
        let mut theme = ThemeSpec {
            fill: Fill::Gradient { second, angle: 155 },
            material: Material::Mica,
            ..ThemeSpec::new("Gradient under Mica")
        };
        apply_typed(&mut theme, "Renamed".to_owned(), Color::hex(0x101010), None);
        assert_eq!(theme.name, "Renamed");
        assert_eq!(theme.background, Color::hex(0x101010));
        assert_eq!(theme.fill, Fill::Gradient { second, angle: 155 });
    }

    #[test]
    fn a_shown_second_colour_field_replaces_the_gradient_colour() {
        let mut theme = ThemeSpec {
            fill: Fill::Gradient {
                second: Color::hex(0x2A1838),
                angle: 155,
            },
            ..ThemeSpec::new("Gradient")
        };
        let (typed, background) = (Color::hex(0x334455), theme.background);
        apply_typed(&mut theme, String::new(), background, Some(typed));
        assert_eq!(theme.name, "Gradient");
        assert_eq!(
            theme.fill,
            Fill::Gradient {
                second: typed,
                angle: 155
            }
        );
    }
}
