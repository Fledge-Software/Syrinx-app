//! Gives chrome and focused pages the same native browser controls and visible zoom level,
//! and keeps each site's remembered zoom level on its pages (#90).

use super::*;
use syrinx_platform::browser::PageAction;

// Find, Downloads and Print take their keys from the command registry (`ui::bind_keys`).
actions!(
    essentials,
    [Find, Downloads, Print, ZoomIn, ZoomOut, ZoomReset]
);

/// Zoom in's chrome keys; the registry's shortcut syntax cannot spell `=` or `+`.
const ZOOM_IN_KEYS: [&str; 2] = ["ctrl-=", "ctrl-+"];
/// Zoom out's chrome key; the registry's shortcut syntax cannot spell `-`.
const ZOOM_OUT_KEY: &str = "ctrl--";

/// Binds the zoom keys the registry cannot hold, while Syrinx chrome has focus. Reset zoom's
/// Ctrl 0 comes from the registry. A focused page sends them through its accelerator handler
/// instead (owner, 2026-10-18).
pub(super) fn bind_zoom_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(ZOOM_IN_KEYS[0], ZoomIn, None),
        KeyBinding::new(ZOOM_IN_KEYS[1], ZoomIn, None),
        KeyBinding::new(ZOOM_OUT_KEY, ZoomOut, None),
    ]);
}

impl BrowserView {
    pub(super) fn essential_shortcut(&mut self, shortcut: BrowserShortcut) {
        if self.closing {
            return;
        }
        let Some(command) = page_action(self.shown.map(TabId::get), shortcut) else {
            // Off a web page there is no flyout to open; say so instead of doing nothing.
            if let Some(message) = off_page_message(shortcut) {
                self.message = message.to_owned();
            }
            return;
        };
        self.page_command(command);
    }

    pub(super) fn essential_actions(&self, element: Div, cx: &mut Context<Self>) -> Div {
        element
            .on_action(cx.listener(|this, _: &Find, window, cx| {
                this.open_find(window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Downloads, _, cx| {
                this.essential_shortcut(BrowserShortcut::Downloads);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Print, _, cx| {
                this.essential_shortcut(BrowserShortcut::Print);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ZoomIn, _, _| {
                this.essential_shortcut(BrowserShortcut::ZoomIn)
            }))
            .on_action(cx.listener(|this, _: &ZoomOut, _, _| {
                this.essential_shortcut(BrowserShortcut::ZoomOut)
            }))
            .on_action(cx.listener(|this, _: &ZoomReset, _, _| {
                this.essential_shortcut(BrowserShortcut::ZoomReset)
            }))
    }

    /// The zoom level of the page in view; 100% when no page is shown.
    pub(super) fn shown_zoom(&self) -> f64 {
        self.shown.map_or(1., |id| self.site_zoom.factor(id.get()))
    }

    /// Page `id` moved to `address`: it takes that site's remembered level.
    pub(super) fn page_site_changed(&mut self, id: u64, address: &str) {
        if let Some(level) = self.site_zoom.page_address(id, address) {
            self.set_page_zoom(id, level);
        }
    }

    /// Page `id` reports a zoom level: the owner's change is saved for its site and copied
    /// to the site's other open pages, and an open page menu shows the new level.
    pub(super) fn page_zoom_reported(&mut self, id: u64, factor: f64, cx: &mut Context<Self>) {
        let choice = self.site_zoom.page_reported(id, factor);
        for (page, level) in choice.follow {
            self.set_page_zoom(page, level);
        }
        if let Some((host, factor)) = choice.save {
            self.command(SessionCommand::SiteZoom { host, factor });
        }
        if self.shown.is_some_and(|shown| shown.get() == id) {
            self.menu_zoom_changed(factor, cx);
        }
    }

    /// Asks page `id` to take `level`; when the request cannot be sent, the page's later
    /// reports count as the owner's again.
    fn set_page_zoom(&mut self, id: u64, level: f64) {
        let sent = self.page_command(BrowserCommand::PageAction {
            id,
            action: PageAction::SetZoom(level),
        });
        if !sent {
            self.site_zoom.abandon_apply(id);
        }
    }

    /// A Clear browsing data request removed zoom levels: open pages of cleared sites return
    /// to 100%, and the dialog learns the outcome.
    pub(super) fn site_zooms_cleared(
        &mut self,
        request: u64,
        result: Result<crate::session::ClearedSiteZooms, crate::session::SessionError>,
    ) {
        let result = result.map(|cleared| {
            for (page, level) in self.site_zoom.replace_saved(cleared.remaining.0) {
                self.set_page_zoom(page, level);
            }
            cleared.sites
        });
        self.site_zooms_cleared_for_dialog(request, result);
    }

    /// The title bar's corner keeps only Downloads in view; Find, Print and zoom live in the
    /// page menu and on their keys. Away from 100% the zoom stepper shows, as in Chrome:
    /// − and + step the level, and a click on the percentage resets it.
    pub(super) fn essential_controls(&self, cx: &mut Context<Self>) -> Div {
        let factor = self.shown_zoom();
        let downloads = crate::controls::icon_button(
            div().id("downloads"),
            crate::controls::glyph::DOWNLOADS,
            "Downloads",
        )
        .aria_keyshortcuts("Control+J")
        .occlude()
        .on_click(cx.listener(|this, _, _, cx| {
            this.essential_shortcut(BrowserShortcut::Downloads);
            cx.notify();
        }));
        div()
            .flex()
            .items_center()
            .gap_1()
            .children(zoom_label(factor).map(|label| {
                let browser = cx.weak_entity();
                zoom_stepper("title-zoom", label, move |shortcut, _, cx| {
                    let stepped = browser.update(cx, |view, _| view.essential_shortcut(shortcut));
                    if stepped.is_err() {
                        eprintln!("A zoom click arrived after its browser was removed.");
                    }
                })
                .occlude()
                .bg(theme::line())
            }))
            .child(downloads)
    }
}

/// The zoom stepper the title bar and the page menu share: − , the percentage, +.
/// `step` receives Zoom out, Reset zoom (the percentage) or Zoom in.
pub(super) fn zoom_stepper(
    id: &'static str,
    label: String,
    step: impl Fn(BrowserShortcut, &mut Window, &mut App) + Clone + 'static,
) -> Stateful<Div> {
    // Each button's id is the stepper's id and its position: 0 −, 1 the percentage, 2 +.
    let part = |index: usize, name: &'static str, keys: &'static str| {
        crate::controls::push_button(div().id((id, index)), name)
            .aria_keyshortcuts(keys)
            .h(px(22.))
            .min_w(px(22.))
            .px_1()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(11.))
            .cursor_pointer()
            .hover(|style| style.bg(theme::line()))
    };
    let on = |shortcut: BrowserShortcut| {
        let step = step.clone();
        move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
            cx.stop_propagation();
            step(shortcut, window, cx);
        }
    };
    div()
        .id(id)
        .role(gpui::Role::Group)
        .aria_label("Zoom")
        .h(px(22.))
        .flex()
        .items_center()
        .rounded(px(11.))
        .text_size(px(syrinx_ui::metrics::text::SMALL))
        .child(
            part(0, "Zoom out", "Control+-")
                .font_family(crate::controls::glyph::FONT)
                .text_size(px(10.))
                .child(crate::controls::glyph::REMOVE)
                .on_click(on(BrowserShortcut::ZoomOut)),
        )
        .child(
            part(1, "Reset zoom", "Control+0")
                .aria_value(label.clone())
                .child(label)
                .on_click(on(BrowserShortcut::ZoomReset)),
        )
        .child(
            part(2, "Zoom in", "Control+=")
                .font_family(crate::controls::glyph::FONT)
                .text_size(px(10.))
                .child(crate::controls::glyph::ADD)
                .on_click(on(BrowserShortcut::ZoomIn)),
        )
}

/// The zoom level as a percentage, or `None` at 100%, where nothing needs showing.
fn zoom_label(factor: f64) -> Option<String> {
    let percent = zoom_percent(factor);
    (percent != "100%").then_some(percent)
}

/// The zoom level as the page menu always shows it, such as "110%".
pub(super) fn zoom_percent(factor: f64) -> String {
    format!("{:.0}%", (factor * 100.).round())
}

/// What the status line says when a shortcut needs a web page and none is shown.
fn off_page_message(shortcut: BrowserShortcut) -> Option<&'static str> {
    match shortcut {
        BrowserShortcut::Downloads => Some(DOWNLOADS_OFF_PAGE),
        _ => None,
    }
}

/// WebView2's downloads flyout belongs to a web page, so Home, History and Clear data have none.
const DOWNLOADS_OFF_PAGE: &str =
    "Downloads open over a web page. Switch to a web page tab to see them.";

fn page_action(id: Option<u64>, shortcut: BrowserShortcut) -> Option<BrowserCommand> {
    let id = id?;
    let action = match shortcut {
        BrowserShortcut::Downloads => PageAction::ToggleDownloads,
        BrowserShortcut::Print => PageAction::Print,
        BrowserShortcut::ZoomIn => PageAction::ZoomIn,
        BrowserShortcut::ZoomOut => PageAction::ZoomOut,
        BrowserShortcut::ZoomReset => PageAction::ZoomReset,
        _ => return None,
    };
    Some(BrowserCommand::PageAction { id, action })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_zoom_level_shows_only_away_from_one_hundred_percent() {
        assert_eq!(zoom_label(1.), None);
        assert_eq!(zoom_label(1.000_4), None, "rounding noise");
        assert_eq!(zoom_label(1.1), Some("110%".to_owned()));
        assert_eq!(zoom_label(0.5), Some("50%".to_owned()));
    }

    #[test]
    fn chrome_only_zoom_keys_never_shadow_a_registry_shortcut() {
        let registry = CommandRegistry::browser();
        let registered: Vec<&str> = registry
            .iter()
            .filter_map(|(_, command)| command.shortcut.as_ref())
            .map(syrinx_core::Shortcut::keystroke)
            .collect();
        for key in ZOOM_IN_KEYS.into_iter().chain([ZOOM_OUT_KEY]) {
            assert!(!registered.contains(&key), "{key} is also a registry key");
        }
        assert!(
            registered.contains(&"ctrl-0"),
            "Reset zoom uses the registry"
        );
    }

    #[test]
    fn page_focused_actions_target_only_the_shown_live_view() {
        for shortcut in [
            BrowserShortcut::Downloads,
            BrowserShortcut::Print,
            BrowserShortcut::ZoomIn,
            BrowserShortcut::ZoomOut,
            BrowserShortcut::ZoomReset,
        ] {
            assert!(matches!(
                page_action(Some(7), shortcut),
                Some(BrowserCommand::PageAction { id: 7, .. })
            ));
            assert!(matches!(
                page_action(Some(8), shortcut),
                Some(BrowserCommand::PageAction { id: 8, .. })
            ));
            assert!(page_action(None, shortcut).is_none());
        }
        assert!(matches!(
            page_action(Some(7), BrowserShortcut::Downloads),
            Some(BrowserCommand::PageAction {
                action: PageAction::ToggleDownloads,
                ..
            })
        ));
        assert!(page_action(Some(7), BrowserShortcut::Find).is_none());
    }

    #[test]
    fn downloads_off_a_web_page_explain_themselves_instead_of_failing_silently() {
        assert!(page_action(None, BrowserShortcut::Downloads).is_none());
        assert_eq!(
            off_page_message(BrowserShortcut::Downloads),
            Some(DOWNLOADS_OFF_PAGE)
        );
        assert!(DOWNLOADS_OFF_PAGE.contains("web page"));
        for quiet in [
            BrowserShortcut::Print,
            BrowserShortcut::ZoomIn,
            BrowserShortcut::ZoomReset,
        ] {
            assert_eq!(off_page_message(quiet), None);
        }
    }

    /// Where the title bar puts ↓, measured from the page card's right edge: past the three
    /// caption buttons, the title bar's right padding, ☰ and the gap before it, less the card
    /// inset.
    fn downloads_button_from_card_right() -> f32 {
        use syrinx_ui::metrics::chrome::{CARD_INSET, ICON_BUTTON, TITLE_BAR_HEIGHT};
        /// `title_contents`' `pr_1`.
        const TITLE_BAR_RIGHT_PADDING: f32 = 4.;
        /// The corner's `gap_1` between ↓ and ☰.
        const CORNER_GAP: f32 = 4.;
        let caption_buttons = 3. * TITLE_BAR_HEIGHT;
        caption_buttons + TITLE_BAR_RIGHT_PADDING + ICON_BUTTON + CORNER_GAP - CARD_INSET
    }

    #[test]
    fn the_flyout_anchor_matches_where_the_title_bar_draws_the_downloads_button() {
        assert_eq!(
            downloads_button_from_card_right(),
            syrinx_platform::browser::DOWNLOADS_BUTTON_FROM_CARD_RIGHT
        );
    }
}
