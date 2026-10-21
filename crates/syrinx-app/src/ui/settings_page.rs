//! Syrinx's Settings page: every setting in its own tab, opened from the sidebar's gear and
//! Ctrl , (#91). It draws the sections Customize used to hold rather than copies of them.

use gpui::{
    Context, Div, FontWeight, InteractiveElement, ParentElement, Role, Stateful,
    StatefulInteractiveElement, Styled, Window, actions, div, px,
};
use syrinx_core::usage::Feature;
use syrinx_core::{ActivationTarget, SETTINGS_ADDRESS, TabId, TabModelSnapshot};
use syrinx_ui::Palette;
use syrinx_ui::metrics::{chrome, radius, text};

use super::BrowserView;
use super::updates::pill;
use crate::controls::control;
use crate::session::SessionCommand;
use crate::theme::{self, rgba};

actions!(settings, [OpenSettings]);

/// Settings' key, as in Chrome and Edge. A focused page hands it over as a browser shortcut.
pub(super) const SETTINGS_KEY: &str = "ctrl-,";

const SECTION_LIST_WIDTH: f32 = 220.;
const SECTION_MAX_WIDTH: f32 = 560.;

/// One part of the Settings page, listed on the left and drawn on the right.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum SettingsSection {
    #[default]
    Appearance,
    Privacy,
    UsageReport,
    Updates,
}

impl SettingsSection {
    /// The sections in list order. #91 also names About, which waits until Syrinx has
    /// something to say there beyond the version Updates already shows.
    pub(super) const ALL: [Self; 4] = [
        Self::Appearance,
        Self::Privacy,
        Self::UsageReport,
        Self::Updates,
    ];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Privacy => "Privacy",
            Self::UsageReport => "Usage report",
            Self::Updates => "Updates",
        }
    }

    fn element_id(self) -> &'static str {
        match self {
            Self::Appearance => "settings-appearance",
            Self::Privacy => "settings-privacy",
            Self::UsageReport => "settings-usage-report",
            Self::Updates => "settings-updates",
        }
    }
}

/// Which section the page shows; it outlives the tab, as a browser's settings do.
#[derive(Default)]
pub(super) struct SettingsPage {
    section: SettingsSection,
}

/// What opening Settings does to the tabs.
#[derive(Debug, Eq, PartialEq)]
enum SettingsRoute {
    /// Settings is already the active tab.
    Stay,
    /// A Settings tab is open in the background; switch to it.
    SwitchTo(TabId),
    /// No Settings tab is open; open one.
    Open,
}

/// Chooses how to show Settings, reusing an open Settings tab. One still being opened is not
/// in the model yet, so two quick presses can still open two, as with Home.
fn settings_route(model: &TabModelSnapshot) -> SettingsRoute {
    if active_is_settings(model) {
        return SettingsRoute::Stay;
    }
    match model.tabs.iter().find(|tab| tab.metadata.is_settings()) {
        Some(tab) => SettingsRoute::SwitchTo(tab.id),
        None => SettingsRoute::Open,
    }
}

/// Whether the model's active tab is a Settings tab.
fn active_is_settings(model: &TabModelSnapshot) -> bool {
    model
        .tabs
        .iter()
        .any(|tab| model.active_id == Some(tab.id) && tab.metadata.is_settings())
}

impl BrowserView {
    /// The gear and Ctrl , keep the section last shown; Customize's "All settings…" names one.
    pub(super) fn open_settings(&mut self, section: Option<SettingsSection>) {
        if let Some(section) = section {
            self.settings.section = section;
        }
        match settings_route(&self.model) {
            SettingsRoute::Stay => return,
            SettingsRoute::SwitchTo(id) => self.activate(ActivationTarget::Tab(id)),
            SettingsRoute::Open => {
                // A Settings tab is not a palette entry, so its acknowledgement opens no palette.
                let command = SessionCommand::NewTab {
                    address: SETTINGS_ADDRESS.to_owned(),
                };
                if !self.try_command_without_entry(command) {
                    return;
                }
            }
        }
        self.count(Feature::SettingsOpened);
    }

    /// Whether the active tab is the Settings page.
    pub(super) fn settings_showing(&self) -> bool {
        active_is_settings(&self.model)
    }

    /// The page card while Settings is the active tab: the sections, then the chosen one.
    pub(super) fn settings_page(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let palette = theme::current();
        let card = div()
            .size_full()
            .rounded(px(radius::HOME_CARD))
            .bg(rgba(palette.card))
            .border_1()
            .border_color(rgba(palette.card_line))
            .overflow_hidden()
            .text_color(rgba(palette.text))
            .flex()
            .child(self.section_list(&palette, cx))
            .child(self.section_panel(&palette, window, cx));
        div()
            .flex_1()
            .h_full()
            .min_w_0()
            .pr(px(chrome::CARD_INSET))
            .pb(px(chrome::CARD_INSET))
            .child(card)
    }

    /// Customize's way to everything it no longer holds.
    pub(super) fn all_settings_entry(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let open = pill(
            palette,
            control(
                div().id("customize-all-settings"),
                Role::Button,
                "All settings…",
            ),
        )
        .child("All settings…")
        .on_click(cx.listener(|this, _, _, cx| {
            this.close_customize();
            this.open_settings(Some(SettingsSection::Appearance));
            cx.notify();
        }));
        entry_row(
            palette,
            "More settings",
            "Site permissions, the usage report and updates are in Settings.",
            open,
        )
    }

    /// The left column: the page's heading and one tab per section.
    fn section_list(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let mut tabs = div()
            .id("settings-sections")
            .role(Role::TabList)
            .aria_label("Settings sections")
            .flex()
            .flex_col()
            .gap(px(2.));
        for section in SettingsSection::ALL {
            tabs = tabs.child(self.section_tab(palette, section, cx));
        }
        div()
            .w(px(SECTION_LIST_WIDTH))
            .flex_none()
            .h_full()
            .px(px(16.))
            .pt(px(24.))
            .border_r_1()
            .border_color(rgba(palette.card_line))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(heading("settings-title", 1, "Settings", text::TITLE).px(px(10.)))
            .child(tabs)
    }

    fn section_tab(
        &self,
        palette: &Palette,
        section: SettingsSection,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let selected = section == self.settings.section;
        let (background, color) = if selected {
            (palette.row_selected, palette.text)
        } else {
            (syrinx_ui::Color::rgba(0, 0, 0, 0.), palette.text_secondary)
        };
        control(div().id(section.element_id()), Role::Tab, section.name())
            .aria_selected(selected)
            .h(px(32.))
            .px(px(10.))
            .rounded(px(radius::CONTROL))
            .bg(rgba(background))
            .text_color(rgba(color))
            .flex()
            .items_center()
            .cursor_pointer()
            .child(section.name())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.settings.section = section;
                cx.notify();
            }))
    }

    /// The right column: the chosen section, scrolling when the window is short.
    fn section_panel(
        &self,
        palette: &Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let section = self.settings.section;
        div()
            .id("settings-panel")
            .role(Role::TabPanel)
            .aria_label(section.name())
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .px(px(40.))
            .pt(px(24.))
            .pb(px(32.))
            .child(
                div()
                    .max_w(px(SECTION_MAX_WIDTH))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(heading(
                        "settings-section-title",
                        2,
                        section.name(),
                        text::SUBTITLE,
                    ))
                    .child(self.section_contents(section, palette, window, cx)),
            )
    }

    /// The views each section reuses from what Customize drew before #91.
    fn section_contents(
        &self,
        section: SettingsSection,
        palette: &Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let column = div().flex().flex_col().gap(px(16.));
        match section {
            SettingsSection::Appearance => column
                .child(self.theme_choices(palette, window, cx))
                .child(self.accent_choices(palette, window, cx))
                .child(self.transparency_switch(palette, cx)),
            // Managing permissions or reading the report replaces its section's summary.
            SettingsSection::Privacy if self.home.site_permissions.is_open() => {
                self.site_permissions_view(palette, cx)
            }
            SettingsSection::Privacy => column
                .child(self.site_permissions_entry(palette, cx))
                .child(self.clear_data_entry(palette, cx)),
            SettingsSection::UsageReport if self.usage.report.is_open() => {
                self.usage_report_view(palette, cx)
            }
            SettingsSection::UsageReport => self.usage_section(palette, cx),
            SettingsSection::Updates => self.updates_section(palette, cx),
        }
    }

    /// Privacy's way to the existing clear browsing data dialog, which asks before deleting.
    fn clear_data_entry(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let open = pill(
            palette,
            control(
                div().id("settings-clear-data"),
                Role::Button,
                "Clear browsing data…",
            ),
        )
        .child("Clear…")
        .on_click(cx.listener(|this, _, _, cx| {
            this.open_clear_data();
            cx.notify();
        }));
        entry_row(
            palette,
            "Clear browsing data",
            "Choose what to clear. Nothing is deleted until you confirm.",
            open,
        )
    }
}

/// A heading screen readers can jump to.
fn heading(id: &'static str, level: usize, title: &'static str, size: f32) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Heading)
        .aria_level(level)
        .aria_label(title)
        .text_size(px(size))
        .font_weight(FontWeight::SEMIBOLD)
        .child(title)
}

/// A divided row: a title and a line of explanation, with its one action on the right.
fn entry_row(
    palette: &Palette,
    title: &'static str,
    description: &'static str,
    action: Stateful<Div>,
) -> Div {
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
                .child(div().font_weight(FontWeight::MEDIUM).child(title))
                .child(
                    div()
                        .text_size(px(text::SMALL))
                        .text_color(rgba(palette.text_muted))
                        .child(description),
                ),
        )
        .child(action)
}

#[cfg(test)]
mod tests {
    use syrinx_core::{Activation, TabKind, TabMetadata, TabModel};

    use super::*;

    fn open(model: &mut TabModel, address: &str, activation: Activation) -> TabId {
        model
            .open(
                TabKind::Web,
                TabMetadata {
                    address: address.to_owned(),
                    ..TabMetadata::default()
                },
                activation,
            )
            .expect("open synthetic tab")
    }

    #[test]
    fn with_no_settings_tab_the_gear_and_key_open_one() {
        let mut model = TabModel::new();
        assert_eq!(settings_route(&model.snapshot()), SettingsRoute::Open);
        open(&mut model, "about:blank", Activation::Foreground);
        open(
            &mut model,
            "https://example.test/settings",
            Activation::Foreground,
        );
        assert_eq!(settings_route(&model.snapshot()), SettingsRoute::Open);
    }

    #[test]
    fn an_open_settings_tab_is_reused_rather_than_duplicated() {
        let mut model = TabModel::new();
        let settings = open(&mut model, SETTINGS_ADDRESS, Activation::Foreground);
        open(&mut model, "https://example.test/", Activation::Foreground);
        assert_eq!(
            settings_route(&model.snapshot()),
            SettingsRoute::SwitchTo(settings)
        );
    }

    #[test]
    fn settings_already_showing_stays_put() {
        let mut model = TabModel::new();
        open(&mut model, "https://example.test/", Activation::Foreground);
        let first = open(&mut model, SETTINGS_ADDRESS, Activation::Background);
        let second = open(&mut model, SETTINGS_ADDRESS, Activation::Foreground);
        assert_eq!(settings_route(&model.snapshot()), SettingsRoute::Stay);
        // From elsewhere, the first Settings tab is the one reused.
        model.select(first).expect("select first Settings tab");
        assert_eq!(settings_route(&model.snapshot()), SettingsRoute::Stay);
        let page = model.snapshot().tabs[0].id;
        model.select(page).expect("select the page");
        assert_eq!(
            settings_route(&model.snapshot()),
            SettingsRoute::SwitchTo(first)
        );
        assert_ne!(first, second);
    }

    #[test]
    fn the_sections_are_the_settings_syrinx_has_in_list_order() {
        let names: Vec<_> = SettingsSection::ALL.map(SettingsSection::name).into();
        assert_eq!(names, ["Appearance", "Privacy", "Usage report", "Updates"]);
        assert_eq!(SettingsSection::default(), SettingsSection::Appearance);
        // Each section's list entry has its own element id, as GPUI's accessibility tree needs.
        let mut ids: Vec<_> = SettingsSection::ALL.map(SettingsSection::element_id).into();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), SettingsSection::ALL.len());
    }
}
