//! Keeps find text in Syrinx chrome while WebView2 searches and highlights the document.

use super::*;
use gpui::{Focusable, Subscription, Toggled};
use gpui_component::input::{InputEvent, InputState};
use syrinx_platform::browser::PageAction;

use crate::text_field;

actions!(
    find,
    [
        DismissFind,
        FindNextMatch,
        FindPreviousMatch,
        ToggleMatchCase
    ]
);

/// The status line's answer when find is asked for where there is no web page to search.
const FIND_NEEDS_PAGE: &str = "Find works on web pages";

pub(super) struct FindBar {
    id: u64,
    request: u64,
    input: Entity<InputState>,
    count: Option<i32>,
    /// WebView2's `ActiveMatchIndex` for the current search, -1 for none.
    active: i32,
    case_sensitive: bool,
    _subscription: Subscription,
}

/// What F3 or Ctrl G does: open a closed bar, or step through an open one's matches.
#[derive(Debug, PartialEq, Eq)]
enum NextKey {
    Open,
    Step,
}

/// Keys that work while the find field has focus; a focused page sends F3, Shift F3 and
/// Ctrl G through its accelerator handler instead (#82).
pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", DismissFind, Some("SyrinxFind")),
        KeyBinding::new("f3", FindNextMatch, Some("SyrinxFind")),
        KeyBinding::new("ctrl-g", FindNextMatch, Some("SyrinxFind")),
        KeyBinding::new("shift-f3", FindPreviousMatch, Some("SyrinxFind")),
        KeyBinding::new("alt-c", ToggleMatchCase, Some("SyrinxFind")),
    ]);
}

impl BrowserView {
    pub(super) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        let id = match find_target(self.shown) {
            Ok(id) => id,
            Err(refusal) => {
                self.message = refusal.into();
                return;
            }
        };
        if let Err(error) = syrinx_platform::browser::focus_chrome(self.parent_hwnd) {
            self.message = error.to_string();
            return;
        }
        if let Some(bar) = &self.find {
            bar.input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
            return;
        }
        self.count(Feature::FindOpened);
        self.find_sequence += 1;
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in page"));
        input.update(cx, |input, cx| input.focus(window, cx));
        let subscription = cx.subscribe_in(&input, window, |this, _, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.find_changed(cx);
            }
            if let Some(direction) = enter_step(event) {
                this.find_step(direction == Direction::Backward);
            }
            cx.notify();
        });
        self.find = Some(FindBar {
            id,
            request: self.find_sequence,
            input,
            count: Some(0),
            active: -1,
            case_sensitive: false,
            _subscription: subscription,
        });
        self.resize_pages(window);
        cx.notify();
    }

    /// F3, Shift F3 or Ctrl G: opens a closed bar; on an open one, steps without moving focus,
    /// so the keys stay wherever they were, the page included.
    pub(super) fn find_next_key(
        &mut self,
        backwards: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match next_key(self.find.is_some()) {
            NextKey::Open => self.open_find(window, cx),
            NextKey::Step => self.find_step(backwards),
        }
    }

    /// The open find field's focus handle, if the bar is open.
    pub(super) fn find_field_focus(&self, cx: &App) -> Option<FocusHandle> {
        let bar = self.find.as_ref()?;
        Some(bar.input.read(cx).focus_handle(cx))
    }

    /// Sends the field's text as a new search, so results for the old one are dropped.
    fn find_changed(&mut self, cx: &Context<Self>) {
        let Some(bar) = &mut self.find else {
            return;
        };
        self.find_sequence += 1;
        bar.request = self.find_sequence;
        bar.count = None;
        bar.active = -1;
        let command = BrowserCommand::PageAction {
            id: bar.id,
            action: PageAction::Find {
                request: bar.request,
                text: bar.input.read(cx).value().to_string(),
                case_sensitive: bar.case_sensitive,
            },
        };
        self.page_command(command);
    }

    /// Alt C or the Aa button: matches case, or stops matching it, and searches again.
    fn toggle_match_case(&mut self, cx: &Context<Self>) {
        let Some(bar) = &mut self.find else {
            return;
        };
        bar.case_sensitive = !bar.case_sensitive;
        self.find_changed(cx);
    }

    pub(super) fn find_completed(&mut self, id: u64, request: u64, count: i32, active: i32) {
        if let Some(bar) = &mut self.find
            && matches_search(bar.id, bar.request, id, request)
        {
            bar.count = Some(count);
            bar.active = active;
        }
    }

    fn find_step(&mut self, backwards: bool) {
        let Some(bar) = &self.find else {
            return;
        };
        if !bar.count.is_some_and(|count| count > 0) {
            return;
        }
        self.page_command(BrowserCommand::PageAction {
            id: bar.id,
            action: if backwards {
                PageAction::FindPrevious
            } else {
                PageAction::FindNext
            },
        });
    }

    pub(super) fn forget_find(&mut self, id: u64, stop: bool) {
        if !self.find.as_ref().is_some_and(|bar| bar.id == id) {
            return;
        }
        self.find = None;
        if stop {
            self.page_command(BrowserCommand::PageAction {
                id,
                action: PageAction::StopFind,
            });
        }
    }

    pub(super) fn dismiss_find(&mut self, window: &mut Window) {
        let Some(id) = self.find.as_ref().map(|bar| bar.id) else {
            return;
        };
        self.forget_find(id, true);
        self.resize_pages(window);
        self.page_command(BrowserCommand::Focus { id });
    }

    /// The open find field's focus handle, for the acceptance run's focus checks.
    pub(super) fn find_field_handle(&self, cx: &App) -> Option<FocusHandle> {
        Some(self.find.as_ref()?.input.read(cx).focus_handle(cx))
    }

    pub(super) fn find_height(&self) -> f32 {
        if self.find.is_some() { 48. } else { 0. }
    }

    pub(super) fn find_bar(&self, window: &Window, cx: &mut Context<Self>) -> Option<Div> {
        let bar = self.find.as_ref()?;
        let label = match_label(bar.input.read(cx).value().is_empty(), bar.count, bar.active);
        let bar = div()
            .key_context("SyrinxFind")
            .h(px(48.))
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .on_action(cx.listener(|this, _: &DismissFind, window, cx| {
                this.dismiss_find(window);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &FindNextMatch, _, _| this.find_step(false)))
            .on_action(cx.listener(|this, _: &FindPreviousMatch, _, _| this.find_step(true)))
            .on_action(cx.listener(|this, _: &ToggleMatchCase, _, cx| {
                this.toggle_match_case(cx);
                cx.notify();
            }))
            .child(
                text_field::labelled(text_field::FIND_IN_PAGE, &bar.input, window, cx).w(px(280.)),
            )
            .child(label)
            .child(case_toggle(bar.case_sensitive, cx))
            .child(
                button("find-previous", "Previous")
                    .aria_keyshortcuts("Shift+Enter")
                    .on_click(cx.listener(|this, _, _, _| this.find_step(true))),
            )
            .child(
                button("find-next", "Next")
                    .aria_keyshortcuts("Enter")
                    .on_click(cx.listener(|this, _, _, _| this.find_step(false))),
            )
            .child(
                button("find-close", "Close")
                    .aria_label("Close find")
                    .aria_keyshortcuts("Escape")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.dismiss_find(window);
                        cx.notify();
                    })),
            );
        Some(bar)
    }
}

/// The Aa button, announced as a pressed or unpressed "Match case" toggle.
fn case_toggle(on: bool, cx: &mut Context<BrowserView>) -> Stateful<Div> {
    let toggle = button("find-case", "Aa")
        .aria_label("Match case")
        .aria_keyshortcuts("Alt+C")
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .on_click(cx.listener(|this, _, _, cx| {
            this.toggle_match_case(cx);
            cx.notify();
        }));
    if on { toggle.bg(theme::line()) } else { toggle }
}

/// The tab find searches, or why there is none.
fn find_target(shown: Option<TabId>) -> Result<u64, &'static str> {
    shown.map(TabId::get).ok_or(FIND_NEEDS_PAGE)
}

fn next_key(bar_open: bool) -> NextKey {
    if bar_open {
        NextKey::Step
    } else {
        NextKey::Open
    }
}

/// Enter steps to the next match and Shift Enter to the previous one.
fn enter_step(event: &InputEvent) -> Option<Direction> {
    let InputEvent::PressEnter { shift, .. } = event else {
        return None;
    };
    Some(if *shift {
        Direction::Backward
    } else {
        Direction::Forward
    })
}

/// The bar's result text: nothing before a term is typed, then "3 of 12", or only the total
/// while no match is active.
fn match_label(term_empty: bool, count: Option<i32>, active: i32) -> String {
    if term_empty {
        return String::new();
    }
    match count {
        None => "Searching…".into(),
        Some(0) => "No matches".into(),
        Some(count) if (1..=count).contains(&active) => format!("{active} of {count}"),
        Some(1) => "1 match".into(),
        Some(count) => format!("{count} matches"),
    }
}

fn matches_search(active_id: u64, active_request: u64, id: u64, request: u64) -> bool {
    active_id == id && active_request == request
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_find_results_cannot_replace_the_current_query_or_tab() {
        assert!(matches_search(7, 12, 7, 12));
        assert!(!matches_search(7, 12, 7, 11));
        assert!(!matches_search(8, 12, 7, 12));
        assert!(!matches_search(7, 14, 7, 12));
        // A reopened empty bar has a new request even before its first edit.
        assert!(!matches_search(7, 13, 7, 12));
    }

    #[test]
    fn the_match_label_shows_position_count_or_nothing() {
        assert_eq!(match_label(true, Some(0), -1), "");
        assert_eq!(match_label(true, None, -1), "");
        assert_eq!(match_label(false, None, -1), "Searching…");
        assert_eq!(match_label(false, Some(0), -1), "No matches");
        assert_eq!(match_label(false, Some(12), 3), "3 of 12");
        assert_eq!(match_label(false, Some(1), 1), "1 of 1");
        // Without an active match in range, only the total, singular for one.
        assert_eq!(match_label(false, Some(12), -1), "12 matches");
        assert_eq!(match_label(false, Some(12), 0), "12 matches");
        assert_eq!(match_label(false, Some(12), 13), "12 matches");
        assert_eq!(match_label(false, Some(1), -1), "1 match");
    }

    #[test]
    fn enter_steps_forward_and_shift_enter_back() {
        assert_eq!(
            enter_step(&InputEvent::PressEnter {
                secondary: false,
                shift: false
            }),
            Some(Direction::Forward)
        );
        assert_eq!(
            enter_step(&InputEvent::PressEnter {
                secondary: false,
                shift: true
            }),
            Some(Direction::Backward)
        );
        assert_eq!(enter_step(&InputEvent::Change), None);
    }

    #[test]
    fn find_refuses_surfaces_without_a_web_page() {
        assert_eq!(find_target(None), Err(FIND_NEEDS_PAGE));
        assert_eq!(FIND_NEEDS_PAGE, "Find works on web pages");
        assert_eq!(find_target(TabId::new(7).ok()), Ok(7));
    }

    #[test]
    fn find_next_opens_a_closed_bar_and_steps_an_open_one() {
        assert_eq!(next_key(false), NextKey::Open);
        assert_eq!(next_key(true), NextKey::Step);
    }
}
