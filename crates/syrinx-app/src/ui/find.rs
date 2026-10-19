//! Keeps find text in Syrinx chrome while WebView2 searches and highlights the document.

use super::*;
use gpui::{Focusable, Subscription};
use gpui_component::input::{InputEvent, InputState};
use syrinx_platform::browser::PageAction;

use crate::text_field;

actions!(find, [DismissFind]);

pub(super) struct FindBar {
    id: u64,
    request: u64,
    input: Entity<InputState>,
    count: Option<i32>,
    _subscription: Subscription,
}

pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", DismissFind, Some("SyrinxFind"))]);
}

impl BrowserView {
    pub(super) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        let Some(id) = self.shown.map(TabId::get) else {
            return;
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
            match event {
                InputEvent::Change => this.find_changed(cx),
                InputEvent::PressEnter { .. } => this.find_step(false),
                _ => {}
            }
            cx.notify();
        });
        self.find = Some(FindBar {
            id,
            request: self.find_sequence,
            input,
            count: Some(0),
            _subscription: subscription,
        });
        self.resize_pages(window);
        cx.notify();
    }

    /// The open find field's focus handle, if the bar is open.
    pub(super) fn find_field_focus(&self, cx: &App) -> Option<FocusHandle> {
        let bar = self.find.as_ref()?;
        Some(bar.input.read(cx).focus_handle(cx))
    }

    fn find_changed(&mut self, cx: &Context<Self>) {
        let Some(bar) = &mut self.find else {
            return;
        };
        self.find_sequence += 1;
        bar.request = self.find_sequence;
        bar.count = None;
        let command = BrowserCommand::PageAction {
            id: bar.id,
            action: PageAction::Find {
                request: bar.request,
                text: bar.input.read(cx).value().to_string(),
            },
        };
        self.page_command(command);
    }

    pub(super) fn find_completed(&mut self, id: u64, request: u64, count: i32) {
        if let Some(bar) = &mut self.find
            && matches_search(bar.id, bar.request, id, request)
        {
            bar.count = Some(count);
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

    fn dismiss_find(&mut self, window: &mut Window) {
        let Some(id) = self.find.as_ref().map(|bar| bar.id) else {
            return;
        };
        self.forget_find(id, true);
        self.resize_pages(window);
        self.page_command(BrowserCommand::Focus { id });
    }

    pub(super) fn find_height(&self) -> f32 {
        if self.find.is_some() { 48. } else { 0. }
    }

    pub(super) fn find_bar(&self, window: &Window, cx: &mut Context<Self>) -> Option<Div> {
        let bar = self.find.as_ref()?;
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
            .child(
                text_field::labelled(text_field::FIND_IN_PAGE, &bar.input, window, cx).w(px(280.)),
            )
            .child(match bar.count {
                Some(count) => format!("{count} matches"),
                None => "Searching…".into(),
            })
            .child(
                button("find-previous", "Previous")
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
}
