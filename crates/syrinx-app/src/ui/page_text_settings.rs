//! Settings' page-text rows (#54): the capture switch, the owner's excluded sites and the
//! field that adds one. The switch's text never promises that every `no-store` page is
//! skipped, because WebView2 can hide a repeated header (F2).

use gpui::{
    AppContext, Context, Div, Entity, FontWeight, InteractiveElement, ParentElement, Role,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px,
};
use gpui_component::input::{InputEvent, InputState};
use syrinx_ui::Palette;
use syrinx_ui::metrics::text;

use super::BrowserView;
use super::customize::switch_control;
use super::main_window::subscribe_in_main;
use super::settings_page::entry_row;
use super::tile_editor::form_button;
use super::updates::pill;
use crate::controls::control;
use crate::text_field;
use crate::theme::rgba;

const CAPTURE_DESCRIPTION: &str = "Keeps the words on pages you open after switching this on, \
    on this computer only, and deletes them after seven days. Never reads Google's sites, \
    banks and payment sites, pages with a password or card field, or the sites you exclude. \
    Pages that ask not to be stored are skipped whenever Syrinx can see that request.";

/// The open "add a site" field, created on the owner's click like the tile form.
pub(super) struct SiteField {
    input: Entity<InputState>,
    error: Option<&'static str>,
    _subscription: Subscription,
}

impl BrowserView {
    /// The capture switch, then the excluded sites and the way to add one.
    pub(super) fn page_text_section(
        &self,
        palette: &Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut section = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(self.capture_switch_row(palette, cx));
        if self.page_text.unreadable {
            section = section.child(note(
                palette,
                "Syrinx could not read part of your excluded sites, so page text is off. \
                 Check the list below, then switch it on again.",
            ));
        }
        section
            .child(self.excluded_site_rows(palette, cx))
            .child(self.site_field_or_button(palette, window, cx))
    }

    fn capture_switch_row(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        let on = self.page_text.is_on();
        let switch = switch_control("page-text-capture", "Remember page text", on, palette)
            .on_click(cx.listener(move |this, _, _, cx| {
                let entries = this.page_text.switch(!on);
                this.page_text_setting_changed(entries);
                cx.notify();
            }));
        entry_row(palette, "Remember page text", CAPTURE_DESCRIPTION, switch)
    }

    /// One row per excluded site, each with its own Remove button.
    fn excluded_site_rows(&self, palette: &Palette, cx: &mut Context<Self>) -> Div {
        if self.page_text.excluded().is_empty() {
            return div().child(note(palette, "You have not excluded any sites."));
        }
        let mut rows = div()
            .id("page-text-excluded-sites")
            .role(Role::List)
            .aria_label("Sites page text never reads")
            .flex()
            .flex_col()
            .gap(px(4.));
        for (position, site) in self.page_text.excluded().iter().enumerate() {
            let removed = site.clone();
            let remove = pill(
                palette,
                control(
                    div().id(("page-text-remove-site", position)),
                    Role::Button,
                    format!("Stop excluding {site}"),
                ),
            )
            .child("Remove")
            .on_click(cx.listener(move |this, _, _, cx| {
                let entries = this.page_text.remove_exclusion(&removed);
                this.page_text_setting_changed(entries);
                cx.notify();
            }));
            rows = rows.child(
                div()
                    .id(("page-text-site", position))
                    .role(Role::ListItem)
                    .aria_label(site.clone())
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().flex_1().min_w_0().child(site.clone()))
                    .child(remove),
            );
        }
        div().child(rows)
    }

    fn site_field_or_button(
        &self,
        palette: &Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(field) = &self.page_text_site else {
            let add = pill(
                palette,
                control(
                    div().id("page-text-add-site"),
                    Role::Button,
                    "Exclude a site…",
                ),
            )
            .child("Exclude a site…")
            .on_click(cx.listener(|this, _, window, cx| this.start_site_field(window, cx)));
            return div().flex().child(add);
        };
        let mut form = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(text_field::labelled(
                text_field::EXCLUDED_SITE,
                &field.input,
                window,
                cx,
            ));
        if let Some(error) = field.error {
            form = form.child(note(palette, error));
        }
        form.child(
            div()
                .flex()
                .gap(px(8.))
                .child(
                    form_button("page-text-site-save", "Exclude", palette, true)
                        .on_click(cx.listener(|this, _, _, cx| this.submit_site_field(cx))),
                )
                .child(
                    form_button("page-text-site-cancel", "Cancel", palette, false).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.page_text_site = None;
                            cx.notify();
                        }),
                    ),
                ),
        )
    }

    /// Opens the field with the keys in it; Enter adds the site.
    fn start_site_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("example.com"));
        input.update(cx, |input, cx| input.focus(window, cx));
        let subscription = subscribe_in_main(
            &input,
            cx,
            |this: &mut Self, event: &InputEvent, _: &mut Window, cx: &mut Context<Self>| {
                if let InputEvent::PressEnter { .. } = event {
                    this.submit_site_field(cx);
                }
            },
        );
        self.page_text_site = Some(SiteField {
            input,
            error: None,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Adds the typed site, or shows why it was refused and keeps the field open.
    fn submit_site_field(&mut self, cx: &mut Context<Self>) {
        let Some(field) = &mut self.page_text_site else {
            return;
        };
        let entry = field.input.read(cx).value().to_string();
        match self.page_text.add_exclusion(&entry) {
            Ok(entries) => {
                self.page_text_site = None;
                self.page_text_setting_changed(entries);
            }
            Err(message) => field.error = Some(message),
        }
        cx.notify();
    }
}

/// A line of small, muted explanation.
fn note(palette: &Palette, message: &'static str) -> Div {
    div()
        .text_size(px(text::SMALL))
        .text_color(rgba(palette.text_muted))
        .font_weight(FontWeight::NORMAL)
        .child(message)
}
