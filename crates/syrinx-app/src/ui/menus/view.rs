//! The menu popup's own view: it draws the card, keeps the keyboard selection, and sends
//! the keys and clicks on its rows to the browser.

use super::*;

/// What a menu shows when it opens.
pub(super) struct MenuContent {
    pub(super) kind: MenuKind,
    pub(super) rows: Vec<MenuRow>,
    /// The page's zoom level, which the Zoom row shows and the browser keeps current.
    pub(super) zoom: f64,
    pub(super) selected: Option<usize>,
}

/// The popup's content: the rows, the keyboard selection, and the keys and clicks on them.
pub(super) struct MenuView {
    browser: WeakEntity<BrowserView>,
    generation: u64,
    kind: MenuKind,
    pub(super) rows: Vec<MenuRow>,
    pub(super) zoom: f64,
    /// The row arrows have moved to; none until the owner presses one or hovers a row.
    selected: Option<usize>,
    hover: HoverGuard,
    focus: FocusHandle,
    /// Deactivation before the popup was ever active is its hidden creation, not a click away.
    pub(super) was_active: bool,
    _activation: Subscription,
}

impl MenuView {
    pub(super) fn new(
        browser: WeakEntity<BrowserView>,
        generation: u64,
        menu: MenuContent,
        popup: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let MenuContent {
            kind,
            rows,
            zoom,
            selected,
        } = menu;
        let activation = cx.observe_window_activation(popup, |this: &mut Self, popup, cx| {
            if popup.is_window_active() {
                this.was_active = true;
                popup.focus(&this.focus, cx);
            } else if this.was_active {
                this.send(MenuAction::Deactivated, cx);
            }
        });
        Self {
            browser,
            generation,
            kind,
            rows,
            zoom,
            selected,
            hover: HoverGuard::default(),
            focus: cx.focus_handle(),
            was_active: false,
            _activation: activation,
        }
    }

    pub(super) fn send(&self, action: MenuAction, cx: &mut Context<Self>) {
        let generation = self.generation;
        let queued = self.browser.update(cx, |browser, cx| {
            browser.queue_menu_action(generation, action, cx);
        });
        if queued.is_err() {
            eprintln!("Menu input arrived after its browser was removed.");
        }
    }

    /// Arrows, Home and End move the selection; Enter and Space choose it; Escape closes.
    pub(super) fn key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if let Some(step) = MenuStep::for_key(key) {
            self.selected = step_selection(step, self.selected, &self.rows);
            cx.notify();
            return;
        }
        let selected = self.selected.and_then(|row| self.rows.get(row));
        let action = selected
            .and_then(|row| download_key(key, row))
            .or_else(|| key_action(key, selected.map(|row| row.item)));
        let Some(action) = action else {
            return;
        };
        cx.stop_propagation();
        self.send(action, cx);
    }
}

/// What a key other than the selection moves does on the selected row, if anything.
pub(super) fn key_action(key: &str, selected: Option<MenuItem>) -> Option<MenuAction> {
    match (key, selected) {
        ("escape", _) => Some(MenuAction::Dismiss),
        // On the Zoom row, Left and Right step like its − and + buttons.
        ("left", Some(MenuItem::Zoom)) => Some(MenuAction::Zoom(BrowserShortcut::ZoomOut)),
        ("right", Some(MenuItem::Zoom)) => Some(MenuAction::Zoom(BrowserShortcut::ZoomIn)),
        ("enter" | "space", Some(item)) if item.is_choosable() => Some(MenuAction::Choose(item)),
        _ => None,
    }
}

impl MenuView {
    /// A download row's Pause, Resume and Cancel buttons.
    pub(super) fn download_buttons(
        &self,
        index: usize,
        row: &MenuRow,
        cx: &mut Context<Self>,
    ) -> Vec<Stateful<Div>> {
        let MenuItem::Download(download) = row.item else {
            return Vec::new();
        };
        row.controls
            .iter()
            .map(|&control| {
                let (id, icon, verb) = control_face(control);
                let button = crate::controls::push_button(
                    div().id((id, index)),
                    format!("{verb} {}", row.label),
                )
                .h(px(22.))
                .w(px(22.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(11.))
                .cursor_pointer()
                .hover(|style| style.bg(theme::line()));
                with_icon(button, icon, 10.).on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.send(MenuAction::Download(download, control), cx);
                }))
            })
            .collect()
    }

    /// The Zoom row's − / percentage / + stepper; each press zooms the page in view.
    pub(super) fn zoom_stepper(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let menu = cx.weak_entity();
        let label = super::essentials::zoom_percent(self.zoom);
        super::essentials::zoom_stepper("menu-zoom", label, move |shortcut, _, cx| {
            let sent = menu.update(cx, |menu, cx| menu.send(MenuAction::Zoom(shortcut), cx));
            if sent.is_err() {
                eprintln!("A zoom click arrived after its menu was removed.");
            }
        })
    }
}

impl Render for MenuView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::current();
        let mut card = div()
            .id("chrome-menu")
            .role(gpui::Role::Menu)
            .aria_label(menu_name(self.kind, &self.rows))
            .track_focus(&self.focus)
            .size_full()
            .p(px(MENU_PADDING))
            .flex()
            .flex_col()
            .border_1()
            .border_color(theme::rgba(palette.panel_line))
            .bg(theme::panel())
            .font_family("Segoe UI Variable Text")
            .text_size(px(syrinx_ui::metrics::text::BODY))
            .text_color(theme::text())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                this.key_down(event, cx);
            }));
        // A history menu's pages have icons; its other rows line their text up with the titles.
        let inset = matches!(self.kind, MenuKind::History(_));
        for (index, row) in self.rows.iter().enumerate() {
            let item = row.item;
            if !item.is_selectable() {
                card = card.child(drawn_only_row(index, row, inset, &palette));
                continue;
            }
            let selected = self.selected == Some(index);
            let drawn = menu_row(index, row, selected, inset, &palette).on_mouse_move(cx.listener(
                move |this, event: &MouseMoveEvent, _, cx| {
                    if this.hover.may_select(event.position) && this.selected != Some(index) {
                        this.selected = Some(index);
                        cx.notify();
                    }
                },
            ));
            card = card.child(if item == MenuItem::Zoom {
                // Screen readers hear the level with the row, such as "Zoom, 125%".
                let name = format!(
                    "{}, {}",
                    row.label,
                    super::essentials::zoom_percent(self.zoom)
                );
                drawn.aria_label(name).child(self.zoom_stepper(cx))
            } else if !item.is_choosable() {
                // A download is heard with its state, such as "synthetic.zip, Done, 1.0 MB".
                let name = match &row.shortcut {
                    Some(detail) => format!("{}, {detail}", row.label),
                    None => row.label.clone(),
                };
                drawn
                    .aria_label(name)
                    .children(self.download_buttons(index, row, cx))
            } else {
                // A page is heard with its host and distance, such as "Intro, 2 pages back".
                drawn
                    .aria_label(history_rows::spoken_page(row))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.send(MenuAction::Choose(item), cx);
                    }))
            });
        }
        crate::controls::watch_pointer(card)
    }
}

/// The popup's accessible name: the confirmation is named by its question, such as "Close 3
/// other tabs?", so a screen reader announces what it asks.
pub(super) fn menu_name(kind: MenuKind, rows: &[MenuRow]) -> String {
    let question = rows.iter().find(|row| row.item == MenuItem::ConfirmPrompt);
    match (kind, question) {
        (MenuKind::Confirm, Some(row)) => row.label.clone(),
        _ => kind.name().to_owned(),
    }
}

/// Lets the pointer select a row only once it has really moved. Windows sends a mouse move
/// when a popup appears under a still pointer; that must not replace a keyboard selection.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct HoverGuard {
    first: Option<Point<Pixels>>,
    moved: bool,
}

impl HoverGuard {
    /// Whether a pointer move to `position` may change the selection.
    pub(super) fn may_select(&mut self, position: Point<Pixels>) -> bool {
        if self.moved {
            return true;
        }
        match self.first {
            None => self.first = Some(position),
            Some(first) => self.moved = first != position,
        }
        self.moved
    }
}

/// The selection after an arrow, Home or End, skipping rows that cannot be selected; `None`
/// when the menu has no selectable row.
pub(super) fn step_selection(
    step: MenuStep,
    selected: Option<usize>,
    rows: &[MenuRow],
) -> Option<usize> {
    // From First or Last, later tries move inwards; Next and Previous keep their direction.
    let onward = match step {
        MenuStep::First => MenuStep::Next,
        MenuStep::Last => MenuStep::Previous,
        other => other,
    };
    let mut candidate = step.apply(selected, rows.len());
    for _ in 0..rows.len() {
        if rows
            .get(candidate)
            .is_some_and(|row| row.item.is_selectable())
        {
            return Some(candidate);
        }
        candidate = onward.apply(Some(candidate), rows.len());
    }
    None
}

pub(super) fn first_selectable(rows: &[MenuRow]) -> Option<usize> {
    rows.iter().position(|row| row.item.is_selectable())
}

/// A keyboard move of the selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MenuStep {
    Next,
    Previous,
    First,
    Last,
}

impl MenuStep {
    pub(super) fn for_key(key: &str) -> Option<Self> {
        match key {
            "down" => Some(Self::Next),
            "up" => Some(Self::Previous),
            "home" => Some(Self::First),
            "end" => Some(Self::Last),
            _ => None,
        }
    }

    /// The row selected after this step; Down from nothing selects the first row, Up the
    /// last, and both wrap around the ends as Windows menus do.
    pub(super) fn apply(self, selected: Option<usize>, rows: usize) -> usize {
        let last = rows.saturating_sub(1);
        match (self, selected) {
            (Self::First, _) | (Self::Next, None) => 0,
            (Self::Last, _) | (Self::Previous, None) => last,
            (Self::Next, Some(row)) if row >= last => 0,
            (Self::Next, Some(row)) => row + 1,
            (Self::Previous, Some(0)) => last,
            (Self::Previous, Some(row)) => row - 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_selects_only_after_the_pointer_has_moved() {
        let still = point(px(40.), px(20.));
        let mut guard = HoverGuard::default();
        // The move Windows sends when the popup appears under a still pointer.
        assert!(!guard.may_select(still));
        assert!(!guard.may_select(still), "the same place again is no move");
        assert!(guard.may_select(point(px(41.), px(20.))));
        // Once moved, every later move selects, back at the first place too.
        assert!(guard.may_select(still));
    }

    #[test]
    fn left_and_right_step_the_zoom_row_and_nothing_else() {
        let zoom = Some(MenuItem::Zoom);
        assert!(matches!(
            key_action("left", zoom),
            Some(MenuAction::Zoom(BrowserShortcut::ZoomOut))
        ));
        assert!(matches!(
            key_action("right", zoom),
            Some(MenuAction::Zoom(BrowserShortcut::ZoomIn))
        ));
        assert!(
            key_action("enter", zoom).is_none(),
            "Enter does not choose the row"
        );
        assert!(key_action("left", Some(MenuItem::Find)).is_none());
        assert!(key_action("right", None).is_none());
        assert!(matches!(
            key_action("enter", Some(MenuItem::Find)),
            Some(MenuAction::Choose(MenuItem::Find))
        ));
        assert!(matches!(
            key_action("escape", zoom),
            Some(MenuAction::Dismiss)
        ));
    }

    #[test]
    fn arrows_move_the_selection_and_wrap_around_the_ends() {
        use MenuStep::*;
        assert_eq!(Next.apply(None, 8), 0, "Down selects the first row");
        assert_eq!(Previous.apply(None, 8), 7, "Up selects the last row");
        assert_eq!(Next.apply(Some(2), 8), 3);
        assert_eq!(Previous.apply(Some(2), 8), 1);
        assert_eq!(Next.apply(Some(7), 8), 0);
        assert_eq!(Previous.apply(Some(0), 8), 7);
        assert_eq!(First.apply(Some(5), 8), 0);
        assert_eq!(Last.apply(Some(1), 8), 7);
        assert_eq!(MenuStep::for_key("down"), Some(Next));
        assert_eq!(MenuStep::for_key("enter"), None);
    }
}
