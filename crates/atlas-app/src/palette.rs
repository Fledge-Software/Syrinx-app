//! Presents local catalogue results in an owned window above native page views.

use atlas_platform::browser::BrowserShortcut;
use atlas_search::{SearchHit, SearchResults, Target};
use futures::channel::mpsc::UnboundedSender;
use gpui::StatefulInteractiveElement;
use gpui::{
    App, AppContext, Context, Entity, InteractiveElement, IntoElement, KeyBinding, ParentElement,
    Render, Styled, Subscription, Window, actions, div, px, rgb,
};
use gpui_component::input::{Input, InputEvent, InputState, MoveDown, MoveUp};

use crate::{navigation, session::SessionError, ui};

actions!(palette, [Dismiss, NextResult, PreviousResult, SearchWeb]);

pub enum PaletteEvent {
    Query { request_id: u64, text: String },
    Submit { text: String, force_search: bool },
    Activate(Target),
    Shortcut(BrowserShortcut),
    Closed,
}

impl PaletteEvent {
    pub fn finishes_entry(&self) -> bool {
        match self {
            Self::Submit { .. } | Self::Activate(_) | Self::Closed => true,
            Self::Query { .. } | Self::Shortcut(_) => false,
        }
    }
}

pub struct PaletteView {
    input: Entity<InputState>,
    events: UnboundedSender<PaletteEvent>,
    choices: PaletteChoices,
    _subscription: Subscription,
}

#[derive(Default)]
struct PaletteChoices {
    request_id: u64,
    address: Option<String>,
    results: Vec<SearchHit>,
    selected: Option<usize>,
    refresh_selection: Option<RefreshSelection>,
    status: String,
}

#[derive(Clone, Copy)]
enum RefreshSelection {
    Default,
    Target(Target),
    Unselected,
}

impl PaletteChoices {
    fn begin_query(&mut self) -> u64 {
        self.request_id = self
            .request_id
            .checked_add(1)
            .expect("palette request IDs exhausted");
        self.status = "Searching locally…".to_owned();
        self.results.clear();
        self.request_id
    }

    fn query(&mut self, text: &str) -> u64 {
        self.begin_query();
        // Refused input has no address row; submission still uses the existing validator.
        self.address = match navigation::recognized_address(text) {
            Ok(address) => address,
            Err(error) => {
                self.status = error.to_owned();
                None
            }
        };
        self.selected = Some(0);
        self.refresh_selection = None;
        self.request_id
    }

    fn selection(&self) -> RefreshSelection {
        if let Some(hit) = self.selected_hit() {
            return RefreshSelection::Target(hit.target);
        }
        if self.selected.is_some() {
            RefreshSelection::Default
        } else {
            RefreshSelection::Unselected
        }
    }

    fn refresh(&mut self) -> u64 {
        // Repeated model changes retain intent even after the visible rows were invalidated.
        let selection = self.refresh_selection.unwrap_or_else(|| self.selection());
        self.begin_query();
        self.selected = if matches!(selection, RefreshSelection::Default) && self.address.is_some()
        {
            Some(0)
        } else {
            None
        };
        self.refresh_selection = Some(selection);
        self.request_id
    }

    fn receive(&mut self, request_id: u64, result: Result<SearchResults, SessionError>) -> bool {
        if request_id != self.request_id {
            return false;
        }
        let selected_target = self.selected_hit().map(|hit| hit.target);
        let refresh_selection = self.refresh_selection.take();
        let succeeded = result.is_ok();
        match result {
            Ok(results) => {
                self.status = format!(
                    "{} tabs · {} saved pages · {} commands",
                    results.counts.tabs, results.counts.saved, results.counts.commands
                );
                self.results = results.hits;
            }
            Err(error) => {
                self.status = error.to_string();
                self.results.clear();
            }
        }
        // A removed local selection cannot silently activate the row that inherited its index.
        self.selected = match refresh_selection {
            Some(RefreshSelection::Target(target)) => self.target_row(target),
            Some(RefreshSelection::Unselected) => None,
            Some(RefreshSelection::Default) if !succeeded && self.address.is_none() => None,
            Some(RefreshSelection::Default) => Some(0),
            None if self.address.is_some() => selected_target
                .and_then(|target| self.target_row(target))
                .or(Some(0)),
            None => Some(0),
        };
        true
    }

    fn target_row(&self, target: Target) -> Option<usize> {
        self.results
            .iter()
            .position(|hit| hit.target == target)
            .map(|index| index + self.address_rows())
            .or_else(|| self.address.as_ref().map(|_| 0))
    }

    fn address_rows(&self) -> usize {
        usize::from(self.address.is_some())
    }

    fn selected_hit(&self) -> Option<&SearchHit> {
        self.selected
            .and_then(|selected| selected.checked_sub(self.address_rows()))
            .and_then(|index| self.results.get(index))
    }

    fn select(&mut self, selected: usize) {
        self.selected = Some(selected);
        // An explicit choice while waiting supersedes the retained pre-refresh selection.
        if self.refresh_selection.is_some() {
            self.refresh_selection = Some(self.selection());
        }
    }

    fn next(&mut self) {
        let rows = self.results.len() + self.address_rows();
        if rows == 0 {
            return;
        }
        self.select(
            self.selected
                .map_or(0, |selected| selected.saturating_add(1).min(rows - 1)),
        );
    }

    fn previous(&mut self) {
        if self.results.is_empty() && self.address.is_none() {
            return;
        }
        self.select(
            self.selected
                .map_or(0, |selected| selected.saturating_sub(1)),
        );
    }

    fn activation(&self, request_id: u64, target: Target) -> Option<PaletteEvent> {
        if request_id != self.request_id || self.refresh_selection.is_some() {
            return None;
        }
        self.results
            .iter()
            .any(|hit| hit.target == target)
            .then_some(PaletteEvent::Activate(target))
    }

    fn submission(&self, text: String, force_search: bool) -> Option<PaletteEvent> {
        if !force_search {
            self.selected?;
            if let Some(hit) = self.selected_hit() {
                return Some(PaletteEvent::Activate(hit.target));
            }
            if self.selected == Some(0)
                && let Some(address) = &self.address
            {
                return Some(PaletteEvent::Submit {
                    text: address.clone(),
                    force_search: false,
                });
            }
        }
        Some(PaletteEvent::Submit { text, force_search })
    }
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Dismiss, Some("AtlasPalette")),
        KeyBinding::new("down", NextResult, Some("AtlasPalette")),
        KeyBinding::new("up", PreviousResult, Some("AtlasPalette")),
        KeyBinding::new("alt-enter", SearchWeb, Some("AtlasPalette")),
    ]);
}

impl PaletteView {
    pub fn focus_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    pub fn new(
        initial: String,
        events: UnboundedSender<PaletteEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search tabs, saved pages, or enter an address")
        });
        input.update(cx, |input, cx| {
            input.set_value(initial, window, cx);
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::Change => this.query(cx),
                InputEvent::PressEnter { .. } => this.submit(false, window, cx),
                _ => {}
            });
        let mut view = Self {
            input,
            events,
            choices: PaletteChoices::default(),
            _subscription: subscription,
        };
        view.query(cx);
        view
    }

    fn query(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        let request_id = self.choices.query(&text);
        self.emit(PaletteEvent::Query { request_id, text });
        cx.notify();
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        let request_id = self.choices.refresh();
        self.emit(PaletteEvent::Query { request_id, text });
        cx.notify();
    }

    pub fn receive(
        &mut self,
        request_id: u64,
        result: Result<SearchResults, SessionError>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.choices.receive(request_id, result) {
            return false;
        }
        cx.notify();
        true
    }

    fn emit(&mut self, event: PaletteEvent) {
        if self.events.unbounded_send(event).is_err() {
            self.choices.status = "The browser window is no longer available.".to_owned();
        }
    }

    fn submit(&mut self, force_search: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(event) = self
            .choices
            .submission(self.input.read(cx).value().to_string(), force_search)
        else {
            return;
        };
        self.emit(event);
        window.remove_window();
    }
}

impl Render for PaletteView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows = div().flex().flex_col().gap_1();
        if let Some(address) = &self.choices.address {
            let request_id = self.choices.request_id;
            rows = rows.child(
                div()
                    .id("go-to-address")
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(if self.choices.selected == Some(0) {
                        0x343037
                    } else {
                        0x1b1c21
                    }))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.choices.request_id != request_id {
                            return;
                        }
                        this.choices.select(0);
                        this.submit(false, window, cx);
                    }))
                    .child(
                        div()
                            .text_color(rgb(0xecedf0))
                            .overflow_hidden()
                            .child(format!("Go to {address}")),
                    ),
            );
        }
        for (index, hit) in self.choices.results.iter().enumerate() {
            let target = hit.target;
            let request_id = self.choices.request_id;
            let title = if hit.title.is_empty() {
                hit.address.clone()
            } else {
                hit.title.clone()
            };
            rows = rows.child(
                div()
                    .id(("result", index))
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(
                        if Some(index + self.choices.address_rows()) == self.choices.selected {
                            0x343037
                        } else {
                            0x1b1c21
                        },
                    ))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, _| {
                        let Some(event) = this.choices.activation(request_id, target) else {
                            return;
                        };
                        this.emit(event);
                        window.remove_window();
                    }))
                    .child(div().text_color(rgb(0xecedf0)).child(title))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0xa3a6ae))
                            .overflow_hidden()
                            .child(hit.address.clone()),
                    ),
            );
        }
        div()
            .key_context("AtlasPalette")
            .size_full()
            .bg(rgb(0x1b1c21))
            .text_color(rgb(0xecedf0))
            .p_3()
            .rounded_lg()
            .flex()
            .flex_col()
            .gap_3()
            // The focused Input consumes these actions before parent key bindings can run.
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                this.choices.next();
                cx.stop_propagation();
                cx.notify();
            }))
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                this.choices.previous();
                cx.stop_propagation();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Dismiss, window, _| {
                this.emit(PaletteEvent::Closed);
                window.remove_window();
            }))
            .on_action(cx.listener(|this, _: &NextResult, _, cx| {
                this.choices.next();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PreviousResult, _, cx| {
                this.choices.previous();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &SearchWeb, window, cx| this.submit(true, window, cx)))
            // The popup queues browser actions without replacing its focused input or results.
            .on_action(cx.listener(|this, _: &ui::NewTab, _, _| {
                this.emit(PaletteEvent::Shortcut(BrowserShortcut::NewTab));
            }))
            .on_action(cx.listener(|this, _: &ui::CloseTab, _, _| {
                this.emit(PaletteEvent::Shortcut(BrowserShortcut::CloseTab));
            }))
            .on_action(cx.listener(|this, _: &ui::Address, _, _| {
                this.emit(PaletteEvent::Shortcut(BrowserShortcut::Address));
            }))
            .on_action(cx.listener(|this, _: &ui::Palette, _, _| {
                this.emit(PaletteEvent::Shortcut(BrowserShortcut::Palette));
            }))
            .on_action(cx.listener(|this, _: &ui::NextTab, _, _| {
                this.emit(PaletteEvent::Shortcut(BrowserShortcut::NextTab));
            }))
            .on_action(cx.listener(|this, _: &ui::PreviousTab, _, _| {
                this.emit(PaletteEvent::Shortcut(BrowserShortcut::PreviousTab));
            }))
            .on_action(cx.listener(|this, _: &ui::ReopenTab, _, _| {
                this.emit(PaletteEvent::Shortcut(BrowserShortcut::ReopenTab));
            }))
            .on_action(cx.listener(|this, _: &ui::SavePage, _, _| {
                this.emit(PaletteEvent::Shortcut(BrowserShortcut::SavePage));
            }))
            .on_action(cx.listener(|this, _: &ui::Back, _, _| {
                this.emit(PaletteEvent::Shortcut(BrowserShortcut::Back));
            }))
            .on_action(cx.listener(|this, _: &ui::Forward, _, _| {
                this.emit(PaletteEvent::Shortcut(BrowserShortcut::Forward));
            }))
            .on_action(cx.listener(|this, _: &ui::Reload, _, _| {
                this.emit(PaletteEvent::Shortcut(BrowserShortcut::Reload));
            }))
            .child(Input::new(&self.input))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0xb4b7bf))
                    .child(self.choices.status.clone()),
            )
            .child(div().id("results").flex_1().overflow_y_scroll().child(rows))
            .child(
                div()
                    .id("web-search")
                    .h(px(36.))
                    .px_3()
                    .flex()
                    .items_center()
                    .rounded_md()
                    .bg(rgb(0x29272a))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, window, cx| this.submit(true, window, cx)))
                    .child("Search the web")
                    .child(div().flex_1())
                    .child(div().text_xs().child("Alt Enter")),
            )
    }
}

#[cfg(test)]
mod tests {
    use atlas_core::{SavedPageId, TabId};
    use atlas_search::{ScopeCounts, SearchError};
    use futures::{FutureExt, StreamExt, channel::mpsc::unbounded};

    use super::*;

    const ADDRESS: &str = "https://palette.example.test/Case?Key=Value";

    #[test]
    fn browser_command_selection_activates_instead_of_navigating() {
        for entry in crate::commands::entries() {
            let mut choices = PaletteChoices::default();
            let request = choices.query(&entry.title);
            assert!(choices.receive(request, Ok(results(&[entry.target]))));
            assert!(matches!(
                choices.submission(entry.title.clone(), false),
                Some(PaletteEvent::Activate(target)) if target == entry.target
            ));
            assert!(matches!(
                choices.activation(request, entry.target),
                Some(PaletteEvent::Activate(target)) if target == entry.target
            ));
        }
    }

    #[test]
    fn shortcut_stream_keeps_queries_until_submit_activation_or_close() {
        let shortcuts = [
            BrowserShortcut::NewTab,
            BrowserShortcut::CloseTab,
            BrowserShortcut::Address,
            BrowserShortcut::Palette,
            BrowserShortcut::NextTab,
            BrowserShortcut::PreviousTab,
            BrowserShortcut::ReopenTab,
            BrowserShortcut::SavePage,
            BrowserShortcut::Back,
            BrowserShortcut::Forward,
            BrowserShortcut::Reload,
        ];
        for terminal in [
            PaletteEvent::Submit {
                text: ADDRESS.to_owned(),
                force_search: false,
            },
            PaletteEvent::Submit {
                text: "synthetic search".to_owned(),
                force_search: true,
            },
            PaletteEvent::Activate(targets()[0]),
            PaletteEvent::Closed,
        ] {
            let (sender, mut receiver) = unbounded();
            // Typing and further shortcuts must reach the same receiver before entry finishes.
            for (index, shortcut) in shortcuts.iter().enumerate() {
                sender
                    .unbounded_send(PaletteEvent::Shortcut(*shortcut))
                    .expect("queue synthetic shortcut");
                sender
                    .unbounded_send(PaletteEvent::Query {
                        request_id: index as u64,
                        text: ADDRESS.to_owned(),
                    })
                    .expect("queue synthetic query");
            }
            sender.unbounded_send(terminal).expect("queue completion");
            sender
                .unbounded_send(PaletteEvent::Shortcut(BrowserShortcut::NewTab))
                .expect("queue late shortcut");
            drop(sender);

            let mut received_shortcuts = Vec::new();
            let mut received_queries = Vec::new();
            while let Some(event) = receiver.next().now_or_never().expect("queued event") {
                if event.finishes_entry() {
                    break;
                }
                match event {
                    PaletteEvent::Shortcut(shortcut) => received_shortcuts.push(shortcut),
                    PaletteEvent::Query { request_id, text } => {
                        assert_eq!(text, ADDRESS);
                        received_queries.push(request_id);
                    }
                    _ => panic!("entry completion must stop the receiver"),
                }
            }
            assert_eq!(received_shortcuts, shortcuts);
            assert_eq!(received_queries, (0..11).collect::<Vec<u64>>());
            // A queued action after Enter, activation or Escape belongs to the retired popup.
            assert!(matches!(
                receiver.next().now_or_never(),
                Some(Some(PaletteEvent::Shortcut(BrowserShortcut::NewTab)))
            ));
        }
    }

    fn targets() -> [Target; 2] {
        [
            Target::Tab(TabId::new(1).expect("synthetic tab identity")),
            Target::Saved(SavedPageId::new(1).expect("synthetic saved identity")),
        ]
    }

    fn results(targets: &[Target]) -> SearchResults {
        let tabs = targets
            .iter()
            .filter(|target| matches!(target, Target::Tab(_)))
            .count();
        let saved = targets
            .iter()
            .filter(|target| matches!(target, Target::Saved(_)))
            .count();
        let commands = targets
            .iter()
            .filter(|target| matches!(target, Target::Command(_)))
            .count();
        SearchResults {
            generation: 1,
            counts: ScopeCounts {
                tabs,
                saved,
                commands,
                ..ScopeCounts::default()
            },
            hits: targets
                .iter()
                .map(|target| SearchHit {
                    target: *target,
                    score: 1,
                    title: "Synthetic palette match".to_owned(),
                    address: ADDRESS.to_owned(),
                    title_matches: Vec::new(),
                    address_matches: Vec::new(),
                })
                .collect(),
        }
    }

    fn assert_submit(choices: &PaletteChoices, input: &str, force_search: bool, expected: &str) {
        match choices.submission(input.to_owned(), force_search) {
            Some(PaletteEvent::Submit {
                text,
                force_search: actual_force,
            }) => {
                assert_eq!(text, expected);
                assert_eq!(actual_force, force_search);
            }
            _ => panic!("expected explicit address or search submission"),
        }
    }

    fn assert_target(choices: &PaletteChoices, expected: Target) {
        assert!(matches!(
            choices.submission(ADDRESS.to_owned(), false),
            Some(PaletteEvent::Activate(target)) if target == expected
        ));
    }

    #[test]
    fn literal_enter_stays_on_go_to_across_local_results_empty_and_failure() {
        let mut choices = PaletteChoices::default();
        let request = choices.query(ADDRESS);
        assert_eq!(choices.address.as_deref(), Some(ADDRESS));
        assert_submit(&choices, ADDRESS, false, ADDRESS);
        choices.next();
        assert_eq!(
            choices.selected,
            Some(0),
            "Down cannot select an unavailable local row"
        );
        assert!(choices.receive(request, Ok(results(&targets()))));
        assert_eq!(choices.selected, Some(0));
        assert_submit(&choices, ADDRESS, false, ADDRESS);
        assert!(choices.receive(request, Ok(results(&[]))));
        assert_submit(&choices, ADDRESS, false, ADDRESS);
        assert!(choices.receive(
            request,
            Err(SessionError::Search(SearchError::QueryTooLong))
        ));
        assert_submit(&choices, ADDRESS, false, ADDRESS);
    }

    #[test]
    fn deliberate_local_selection_tracks_target_and_arrows_return_to_go_to() {
        let [tab, saved] = targets();
        let mut choices = PaletteChoices::default();
        let request = choices.query(ADDRESS);
        assert!(choices.receive(request, Ok(results(&[tab, saved]))));
        choices.next();
        assert_eq!(choices.selected, Some(1));
        assert_target(&choices, tab);
        choices.previous();
        assert_submit(&choices, ADDRESS, false, ADDRESS);
        choices.previous();
        assert_eq!(choices.selected, Some(0));
        choices.next();
        choices.next();
        choices.next();
        assert_eq!(choices.selected, Some(2));
        assert_target(&choices, saved);

        // An updated ranking must not quietly change the chosen destination.
        assert!(choices.receive(request, Ok(results(&[saved, tab]))));
        assert_eq!(choices.selected, Some(1));
        assert_target(&choices, saved);
        assert!(choices.receive(request, Ok(results(&[tab]))));
        assert_eq!(choices.selected, Some(0));
        assert_submit(&choices, ADDRESS, false, ADDRESS);
    }

    #[test]
    fn input_changes_reject_stale_replies_and_reopening_resets_selection() {
        let mut choices = PaletteChoices::default();
        let old = choices.query(ADDRESS);
        assert!(choices.receive(old, Ok(results(&targets()))));
        choices.next();
        assert_target(&choices, targets()[0]);
        let next_address = "https://next-palette.example.test/";
        let next = choices.query(next_address);
        assert_ne!(next, old);
        assert_eq!(choices.selected, Some(0));
        let status = choices.status.clone();
        assert!(!choices.receive(old, Ok(results(&targets()))));
        assert!(!choices.receive(old, Err(SessionError::Search(SearchError::QueryTooLong))));
        assert_eq!(choices.status, status);
        assert!(choices.results.is_empty());
        assert_submit(&choices, next_address, false, next_address);
        assert!(choices.receive(next, Ok(results(&targets()))));
        assert_submit(&choices, next_address, false, next_address);

        // The owning UI correlates popup identities; a new popup starts fresh local state.
        let mut reopened = PaletteChoices::default();
        assert_eq!(reopened.query(ADDRESS), old);
        assert!(reopened.results.is_empty());
        assert_eq!(reopened.selected, Some(0));
        assert_submit(&reopened, ADDRESS, false, ADDRESS);
    }

    #[test]
    fn ordinary_and_refused_input_keep_existing_submission_behavior() {
        for input in [
            "ordinary search terms",
            "",
            "JAVASCRIPT:1",
            "https://synthetic@example.test/",
            "https://[invalid]",
        ] {
            let mut choices = PaletteChoices::default();
            let request = choices.query(input);
            assert_eq!(choices.address, None);
            assert_submit(&choices, input, false, input);
            if let Err(error) = navigation::recognized_address(input) {
                assert_eq!(choices.status, error);
            }
            assert!(choices.receive(request, Ok(results(&targets()))));
            assert_target(&choices, targets()[0]);
            choices.next();
            assert_target(&choices, targets()[1]);
            choices.previous();
            assert_target(&choices, targets()[0]);
            assert!(choices.receive(
                request,
                Err(SessionError::Search(SearchError::QueryTooLong))
            ));
            assert_submit(&choices, input, false, input);
        }
    }

    #[test]
    fn go_to_uses_canonical_address_but_forced_search_preserves_input() {
        let input = "  HTTPS://palette.example.test/Case?Key=Value  ";
        let mut choices = PaletteChoices::default();
        let request = choices.query(input);
        assert_eq!(choices.address.as_deref(), Some(ADDRESS));
        assert_submit(&choices, input, false, ADDRESS);
        assert_submit(&choices, input, true, input);
        assert!(choices.receive(request, Ok(results(&targets()))));
        choices.next();
        assert_target(&choices, targets()[0]);
        assert_submit(&choices, input, true, input);

        // Clicking Go to explicitly selects its row, regardless of the previous local choice.
        choices.select(0);
        assert_submit(&choices, input, false, ADDRESS);
    }

    #[test]
    fn model_refresh_replaces_counts_and_rows_without_an_input_change() {
        let [first, _] = targets();
        let second = Target::Tab(TabId::new(2).expect("second synthetic Home"));
        let mut choices = PaletteChoices::default();
        let old = choices.query("");
        assert!(choices.receive(old, Ok(results(&[first, second]))));
        assert_eq!(choices.status, "2 tabs · 0 saved pages · 0 commands");

        let refreshed = choices.refresh();
        assert!(choices.results.is_empty());
        assert_eq!(choices.status, "Searching locally…");
        assert!(choices.submission(String::new(), false).is_none());
        assert!(choices.activation(old, second).is_none());
        assert!(!choices.receive(old, Ok(results(&[first, second]))));
        assert!(choices.receive(refreshed, Ok(results(&[first]))));
        assert_eq!(choices.status, "1 tabs · 0 saved pages · 0 commands");
        assert_eq!(choices.results.len(), 1);
        assert_target(&choices, first);
        assert!(choices.activation(refreshed, second).is_none());
    }

    #[test]
    fn repeated_refreshes_preserve_target_identity_and_reject_earlier_replies() {
        let [tab, saved] = targets();
        for input in [ADDRESS, "synthetic"] {
            let mut choices = PaletteChoices::default();
            let initial = choices.query(input);
            assert!(choices.receive(initial, Ok(results(&[tab, saved]))));
            choices.select(choices.address_rows() + 1);
            assert_target(&choices, saved);

            let first_refresh = choices.refresh();
            let latest_refresh = choices.refresh();
            assert!(choices.results.is_empty());
            assert!(choices.submission(input.to_owned(), false).is_none());
            assert!(choices.activation(initial, saved).is_none());
            assert!(!choices.receive(first_refresh, Ok(results(&[tab]))));
            assert!(!choices.receive(
                initial,
                Err(SessionError::Search(SearchError::QueryTooLong))
            ));
            assert!(choices.receive(latest_refresh, Ok(results(&[saved, tab]))));
            assert_target(&choices, saved);
            assert_eq!(choices.selected, Some(choices.address_rows()));
            assert!(matches!(
                choices.activation(latest_refresh, saved),
                Some(PaletteEvent::Activate(target)) if target == saved
            ));
        }
    }

    #[test]
    fn removed_local_selection_never_activates_its_replacement_row() {
        let [tab, saved] = targets();
        for input in [ADDRESS, "synthetic"] {
            let mut choices = PaletteChoices::default();
            let request = choices.query(input);
            assert!(choices.receive(request, Ok(results(&[tab, saved]))));
            choices.select(choices.address_rows());
            let refresh = choices.refresh();
            assert!(choices.receive(refresh, Ok(results(&[saved]))));
            assert!(choices.activation(request, tab).is_none());
            assert!(choices.activation(refresh, tab).is_none());
            if input == ADDRESS {
                assert_submit(&choices, input, false, ADDRESS);
                choices.next();
            } else {
                assert_eq!(choices.selected, None);
                assert!(choices.submission(input.to_owned(), false).is_none());
                let again = choices.refresh();
                assert!(choices.receive(again, Ok(results(&[saved]))));
                assert!(choices.submission(input.to_owned(), false).is_none());
                choices.next();
            }
            assert_target(&choices, saved);
        }
    }

    #[test]
    fn refresh_failure_and_input_change_keep_submission_intent_distinct() {
        let [tab, saved] = targets();
        let mut choices = PaletteChoices::default();
        let initial = choices.query("synthetic");
        assert!(choices.receive(initial, Ok(results(&[tab, saved]))));
        let refresh = choices.refresh();
        assert!(choices.receive(
            refresh,
            Err(SessionError::Search(SearchError::QueryTooLong))
        ));
        assert!(choices.results.is_empty());
        assert!(choices.submission("synthetic".to_owned(), false).is_none());
        assert_submit(&choices, "synthetic", true, "synthetic");

        // A new input query keeps the established no-result submission behavior.
        let current = choices.query("new synthetic query");
        assert_submit(
            &choices,
            "new synthetic query",
            false,
            "new synthetic query",
        );
        assert!(!choices.receive(refresh, Ok(results(&[saved]))));
        assert!(choices.receive(current, Ok(results(&[saved]))));
        assert_target(&choices, saved);
    }

    #[test]
    fn go_to_selection_survives_refresh_and_supersedes_pending_local_intent() {
        let [tab, saved] = targets();
        let mut choices = PaletteChoices::default();
        let initial = choices.query(ADDRESS);
        assert!(choices.receive(initial, Ok(results(&[tab, saved]))));
        let refresh = choices.refresh();
        assert_submit(&choices, ADDRESS, false, ADDRESS);
        assert!(choices.receive(refresh, Ok(results(&[saved, tab]))));
        assert_submit(&choices, ADDRESS, false, ADDRESS);

        choices.next();
        assert_target(&choices, saved);
        let pending = choices.refresh();
        assert!(choices.submission(ADDRESS.to_owned(), false).is_none());
        // Only Go to is available while local rows are being refreshed.
        choices.previous();
        assert_submit(&choices, ADDRESS, false, ADDRESS);
        let latest = choices.refresh();
        assert!(!choices.receive(pending, Ok(results(&[saved, tab]))));
        assert!(choices.receive(latest, Ok(results(&[saved, tab]))));
        assert_submit(&choices, ADDRESS, false, ADDRESS);
    }

    #[test]
    fn refresh_updates_saved_counts_and_metadata_from_current_results() {
        let [tab, saved] = targets();
        let mut choices = PaletteChoices::default();
        let initial = choices.query("synthetic");
        assert!(choices.receive(initial, Ok(results(&[tab]))));
        let refresh = choices.refresh();
        let mut current = results(&[tab, saved]);
        current.hits[0].title = "Changed synthetic title".to_owned();
        current.hits[0].address = "https://changed.example.test/".to_owned();
        assert!(choices.receive(refresh, Ok(current)));
        assert_eq!(choices.status, "1 tabs · 1 saved pages · 0 commands");
        assert_eq!(choices.results[0].title, "Changed synthetic title");
        assert_eq!(choices.results[0].address, "https://changed.example.test/");
        assert_target(&choices, tab);

        let removed = choices.refresh();
        assert!(choices.receive(removed, Ok(results(&[tab]))));
        assert_eq!(choices.status, "1 tabs · 0 saved pages · 0 commands");
        assert!(choices.activation(removed, saved).is_none());
    }
}
