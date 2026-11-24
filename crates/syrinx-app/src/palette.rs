//! Presents commands and local catalogue results in an owned window above native page views.

mod choices;
mod commands;
mod display;
mod query;
mod render;

use std::{rc::Rc, sync::Arc, time::Instant};

use futures::channel::mpsc::UnboundedSender;
use gpui::{
    App, AppContext, Context, Div, Entity, InteractiveElement, IntoElement, KeyBinding,
    ParentElement, Render, ScrollHandle, Stateful, StatefulInteractiveElement, Styled,
    Subscription, Window, actions, div,
};
use gpui_component::input::{InputEvent, InputState, MoveDown, MoveUp};
use syrinx_core::usage::{Feature, Timing, UsageRecorder};
use syrinx_core::{CommandAction, CommandContext, CommandRegistry};
use syrinx_platform::browser::BrowserShortcut;
use syrinx_search::{Scope, SearchResults, Target};

pub use display::arrange;

/// The popup's height with nothing under the bar: the bar and the panel's borders (#265).
pub const BAR_ONLY_HEIGHT: f32 = render::PANEL_BORDERS + render::BAR_HEIGHT;

use choices::PaletteChoices;
use commands::{CommandMatcher, CommandMatches};
use query::{Chip, ParsedQuery, View};

use crate::{acceptance::PresentProbe, session::SessionError, text_field, theme, ui};

actions!(palette, [Dismiss, NextResult, PreviousResult, SearchWeb]);

/// The status line once the browser has stopped listening to the entry's channel.
const BROWSER_GONE: &str = "The browser window is no longer available.";

pub enum PaletteEvent {
    Query {
        request_id: u64,
        text: String,
        scope: Scope,
    },
    Submit {
        text: String,
        force_search: bool,
    },
    Activate(Target),
    Run(CommandAction),
    Shortcut(BrowserShortcut),
    /// What the popup lists changed its height, so the browser refits the popup (#265).
    Resized,
    Closed,
    /// A press in Syrinx's chrome ended the entry; focus stays where that press put it (#172).
    ClickedAway,
}

impl PaletteEvent {
    pub fn finishes_entry(&self) -> bool {
        match self {
            Self::Submit { .. }
            | Self::Activate(_)
            | Self::Run(_)
            | Self::Closed
            | Self::ClickedAway => true,
            Self::Query { .. } | Self::Shortcut(_) | Self::Resized => false,
        }
    }
}

/// The kept popup's native window and the main window that owns it (DW-90).
#[derive(Clone, Copy)]
pub struct PaletteNative {
    pub popup: isize,
    pub owner: isize,
}

/// What one opening of the palette starts with.
pub struct PaletteEntry {
    pub initial: String,
    pub events: UnboundedSender<PaletteEvent>,
    pub context: CommandContext,
    /// When the key or click that opened this entry was handled (AN-01).
    pub opened: Instant,
}

/// The palette's view. Its popup is created once, hidden between uses and reset on each
/// opening (DW-90), so each opening is an entry with its own event channel.
pub struct PaletteView {
    /// Replaced at each close: a focused input's caret blink re-arms a 500 ms timer that
    /// only a blur stops, and a hidden window never draws the frame that delivers the blur.
    input: Entity<InputState>,
    /// The open entry's channel; `None` while the popup is hidden.
    events: Option<UnboundedSender<PaletteEvent>>,
    choices: PaletteChoices,
    commands: CommandMatcher,
    /// Why commands are unavailable, shown again at each opening.
    command_problem: Option<String>,
    context: CommandContext,
    chip: Chip,
    /// Tab in the query shows the scope chips for the open entry; they start hidden (#265).
    scopes_shown: bool,
    /// The height the browser was last told to fit, so only a change is reported.
    fitted: FittedHeight,
    view: View,
    scroll: ScrollHandle,
    native: PaletteNative,
    /// Present-time stamps for P1-33's acceptance run; `None` in every other run.
    probe: Option<PresentProbe>,
    /// The shared usage counters (AN-01), and when the open entry began until it is painted.
    usage: Arc<UsageRecorder>,
    opening: Option<Instant>,
    _input_events: Subscription,
    /// Counts the input's notifications for P1-33's acceptance run; `None` otherwise.
    _input_count: Option<Subscription>,
    /// Whether the popup has been active during the open entry. GPUI reports activation from
    /// a task, so a deactivation queued by the previous entry's close must not end this one.
    active_this_entry: bool,
    /// Ends the open entry when the popup loses activation: a click outside it, or Alt Tab.
    _activation: Subscription,
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Dismiss, Some("SyrinxPalette")),
        KeyBinding::new("down", NextResult, Some("SyrinxPalette")),
        KeyBinding::new("up", PreviousResult, Some("SyrinxPalette")),
        KeyBinding::new("alt-enter", SearchWeb, Some("SyrinxPalette")),
    ]);
}

impl PaletteView {
    pub fn focus_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    /// Builds the hidden palette; nothing is sent until `begin` opens an entry.
    pub fn new(
        registry: Rc<CommandRegistry>,
        native: PaletteNative,
        probe: Option<PresentProbe>,
        usage: Arc<UsageRecorder>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (input, input_events, input_count) = new_input(probe.as_ref(), window, cx);
        let (commands, command_problem) = match CommandMatcher::new(registry) {
            Ok(commands) => (commands, None),
            Err(error) => (
                CommandMatcher::empty(),
                Some(format!("Commands are unavailable: {error}")),
            ),
        };
        Self {
            input,
            events: None,
            choices: PaletteChoices::default(),
            commands,
            command_problem,
            context: CommandContext::default(),
            chip: Chip::default(),
            scopes_shown: false,
            fitted: FittedHeight::default(),
            view: View::Everything,
            scroll: ScrollHandle::new(),
            native,
            probe,
            usage,
            opening: None,
            _input_events: input_events,
            _input_count: input_count,
            active_this_entry: false,
            _activation: cx.observe_window_activation(window, |this, popup, cx| {
                this.activation_changed(popup.is_window_active(), popup, cx);
            }),
        }
    }

    /// Opens an entry: the text, rows, selection, scope chip and any waiting Enter start
    /// fresh, and the first query goes out on the entry's own channel.
    pub fn begin(&mut self, entry: PaletteEntry, window: &mut Window, cx: &mut Context<Self>) {
        self.events = Some(entry.events);
        self.active_this_entry = false;
        self.context = entry.context;
        self.opening = Some(entry.opened);
        // A registry problem stands for the whole entry, so the first height includes it.
        self.choices.reset(self.command_problem.clone());
        self.chip = Chip::default();
        self.scopes_shown = false;
        self.view = View::Everything;
        self.scroll = ScrollHandle::new();
        self.input.update(cx, |input, cx| {
            input.set_value(entry.initial, window, cx);
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        // `set_value` does not emit a change event, so query explicitly.
        self.query(cx);
        // The browser shows the popup at this height, so only later changes are reported.
        self.fitted.reset(self.wanted_height());
    }

    /// The popup height that fits what is drawn now: the bar alone, or the bar and its rows.
    pub fn wanted_height(&self) -> f32 {
        render::height(self.scopes_shown, &self.choices, self.view)
    }

    /// Show a plugin command's outcome on this entry's status line while it is open (#29);
    /// see `PaletteChoices::show_outcome`.
    pub fn show_outcome(&mut self, outcome: &str, cx: &mut Context<Self>) {
        self.choices.show_outcome(outcome);
        self.redraw(cx);
    }

    /// Redraws after a change to what is drawn, asking the browser to refit the popup first
    /// when its height changed. Every such change goes through here (#265), except `query`,
    /// `refresh` and `choose_chip`: they only notify, because their reply refits the popup.
    fn redraw(&mut self, cx: &mut Context<Self>) {
        self.announce_height();
        cx.notify();
    }

    /// Asks the browser to refit the popup when what is drawn changed its height.
    fn announce_height(&mut self) {
        if self.fitted.changed_to(self.wanted_height()) && !self.emit(PaletteEvent::Resized) {
            self.choices.status = BROWSER_GONE.to_owned();
        }
    }

    /// Shows the scope chips for the open entry; whether this call was the one that did.
    fn show_scopes(&mut self, cx: &mut Context<Self>) -> bool {
        if self.scopes_shown {
            return false;
        }
        self.scopes_shown = true;
        self.redraw(cx);
        true
    }

    /// Whether an entry is open, so its keys and replies still mean something.
    pub fn is_open(&self) -> bool {
        self.events.is_some()
    }

    /// Whether the popup has been active since the open entry began.
    pub fn was_active_this_entry(&self) -> bool {
        self.active_this_entry
    }

    /// Ends the entry as if Esc were pressed; the browser then returns focus to the page.
    /// The popup is hidden even without an entry, so a stray visible popup can always close.
    pub fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.end_entry(PaletteEvent::Closed, window, cx);
    }

    /// Ends the entry for a press in the main window, without sending the keys to the page:
    /// the press chooses where focus goes, as it does for the menus (#172).
    pub fn dismiss_for_chrome_press(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.end_entry(PaletteEvent::ClickedAway, window, cx);
    }

    /// Reports how an open entry ended, then hides the popup.
    fn end_entry(&mut self, event: PaletteEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_open() {
            self.send(event, cx);
        }
        self.close(window, cx);
    }

    /// A click outside the open popup, or switching away, ends the entry as Esc does.
    fn activation_changed(&mut self, active: bool, window: &mut Window, cx: &mut Context<Self>) {
        if active {
            self.active_this_entry = true;
            return;
        }
        if dismisses_on_deactivation(self.is_open(), self.active_this_entry) {
            self.dismiss(window, cx);
        }
    }

    /// Hides the popup and drops the entry's channel, which ends the browser's receiver.
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.events = None;
        self.replace_input(window, cx);
        cx.notify();
        let hidden = syrinx_platform::browser::hide_overlay(self.native.popup, self.native.owner);
        if let Err(error) = hidden {
            // A popup that cannot hide is removed; the browser builds a new one next time.
            eprintln!("Cannot hide the command palette ({error}); closing it instead.");
            window.remove_window();
        }
    }

    /// Drops the used input, which cancels its caret blink, for a fresh unfocused one.
    fn replace_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.blur(cx);
        let (input, input_events, input_count) = new_input(self.probe.as_ref(), window, cx);
        self.input = input;
        self._input_events = input_events;
        self._input_count = input_count;
    }

    fn query(&mut self, cx: &mut Context<Self>) {
        if !self.is_open() {
            return;
        }
        let input = self.input.read(cx).value().to_string();
        let parsed = ParsedQuery::parse(&input);
        self.view = parsed.view(self.chip);
        let request_id = self
            .choices
            .query_scoped(&input, self.view.accepts_address());
        self.probe_query(request_id);
        self.send_query(request_id, parsed.text, parsed.prefix.is_some(), cx);
        cx.notify();
    }

    /// Re-runs the current text after the tab model or command context changed.
    pub fn refresh(&mut self, context: CommandContext, cx: &mut Context<Self>) {
        if !self.is_open() {
            return;
        }
        self.context = context;
        let input = self.input.read(cx).value().to_string();
        let parsed = ParsedQuery::parse(&input);
        let request_id = self.choices.refresh();
        self.probe_query(request_id);
        self.send_query(request_id, parsed.text, parsed.prefix.is_some(), cx);
        cx.notify();
    }

    /// Stages this request's commands, then asks the worker for its page rows and counts.
    fn send_query(&mut self, request_id: u64, text: &str, prefixed: bool, cx: &mut Context<Self>) {
        let matches = match self.commands.matches(text, self.view, self.context) {
            Ok(matches) => matches,
            Err(error) => {
                // The worker reports the same refusal for its rows; commands simply show none.
                self.choices.status = error.to_string();
                CommandMatches::default()
            }
        };
        self.choices
            .stage_commands(matches, !text.is_empty(), prefixed);
        let query = PaletteEvent::Query {
            request_id,
            text: text.to_owned(),
            scope: self.view.search_scope(),
        };
        self.send(query, cx);
    }

    /// Shows the current request's rows, then applies an Enter that was waiting for them.
    pub fn receive(
        &mut self,
        request_id: u64,
        result: Result<SearchResults, SessionError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.is_open() || !self.choices.receive_ordered(request_id, result) {
            return false;
        }
        if let Some(probe) = &self.probe {
            probe.replied(request_id, Instant::now());
        }
        if let Some(event) = self.choices.take_deferred() {
            self.finish(event, window, cx);
            return true;
        }
        self.scroll_to_selection();
        self.redraw(cx);
        true
    }

    fn probe_query(&self, request_id: u64) {
        if let Some(probe) = &self.probe {
            probe.queried(request_id);
        }
    }

    /// The row Enter would open or run, for P1-33's acceptance run.
    pub fn selected_target(&self) -> Option<Target> {
        self.choices.selected_hit().map(|hit| hit.target)
    }

    /// Whether the typed row leads and is Enter's choice, for P1-33's acceptance run.
    pub fn typed_row_selected(&self) -> bool {
        self.choices.lead_rows() > 0 && self.choices.selected == Some(0)
    }

    /// Moves the selection down as the Down key does, for P1-33's acceptance run.
    pub fn press_down(&mut self, cx: &mut Context<Self>) {
        self.move_selection(true, cx);
    }

    /// The typed text, for P1-33's acceptance run.
    pub fn input_text(&self, cx: &App) -> String {
        self.input.read(cx).value().to_string()
    }

    /// The newest request identity, for P1-33's acceptance run.
    pub fn current_request(&self) -> u64 {
        self.choices.request_id
    }

    /// Whether the newest request's rows are shown, for P1-33's acceptance run.
    pub fn answered(&self) -> bool {
        self.choices.answered()
    }

    /// Sends `event` on the open entry's channel; false only when the browser stopped listening.
    fn emit(&self, event: PaletteEvent) -> bool {
        let Some(events) = &self.events else {
            return true;
        };
        events.unbounded_send(event).is_ok()
    }

    /// Sends `event`, and says so under the rows when the browser stopped listening.
    fn send(&mut self, event: PaletteEvent, cx: &mut Context<Self>) {
        if self.emit(event) {
            return;
        }
        self.choices.status = BROWSER_GONE.to_owned();
        self.redraw(cx);
    }

    fn submit(&mut self, force_search: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_open() {
            return;
        }
        let text = self.input.read(cx).value().to_string();
        let Some(event) = self.choices.submission(text, force_search) else {
            // A waiting Enter shows its status until the reply applies it.
            self.redraw(cx);
            return;
        };
        self.finish(event, window, cx);
    }

    /// Sends the entry's final event and hides the popup; command rows become their action.
    fn finish(&mut self, event: PaletteEvent, window: &mut Window, cx: &mut Context<Self>) {
        let event = match event {
            PaletteEvent::Activate(target @ Target::Command(_)) => {
                let Some(command) = self.commands.command(target) else {
                    self.choices.status = "This command is no longer available.".to_owned();
                    self.redraw(cx);
                    return;
                };
                PaletteEvent::Run(command.action.clone())
            }
            event => event,
        };
        self.send(event, cx);
        self.close(window, cx);
    }

    /// Choosing a chip drops a typed prefix, since the prefix would otherwise override it.
    fn choose_chip(&mut self, chip: Chip, window: &mut Window, cx: &mut Context<Self>) {
        if chip != self.chip {
            self.usage.count(Feature::PaletteScopeChanged);
        }
        self.chip = chip;
        let input = self.input.read(cx).value().to_string();
        let parsed = ParsedQuery::parse(&input);
        if parsed.prefix.is_some() {
            let text = parsed.text.to_owned();
            self.input
                .update(cx, |input, cx| input.set_value(text, window, cx));
        }
        self.focus_input(window, cx);
        // `set_value` does not emit a change event, so query explicitly.
        self.query(cx);
    }

    fn move_selection(&mut self, forwards: bool, cx: &mut Context<Self>) {
        if forwards {
            self.choices.next();
        } else {
            self.choices.previous();
        }
        self.scroll_to_selection();
        cx.notify();
    }

    fn scroll_to_selection(&self) {
        let Some(row) = self.choices.selected else {
            return;
        };
        let child = render::child_index(&self.choices, row);
        self.scroll.scroll_to_item(child);
    }
}

/// The height the browser was last told to fit the popup to.
#[derive(Default)]
struct FittedHeight(f32);

impl FittedHeight {
    /// Records the height the popup is shown at, without reporting it.
    fn reset(&mut self, shown: f32) {
        self.0 = shown;
    }

    /// Whether `wanted` differs from the last reported height; if so it becomes that height.
    fn changed_to(&mut self, wanted: f32) -> bool {
        if (wanted - self.0).abs() < 0.5 {
            return false;
        }
        self.0 = wanted;
        true
    }
}

/// Only an open entry whose popup was active ends on deactivation; a stale report from the
/// previous entry's close arrives before this one's popup was ever active.
fn dismisses_on_deactivation(open: bool, active_this_entry: bool) -> bool {
    open && active_this_entry
}

/// The palette's search field, with its Change and Enter handlers.
fn new_input(
    probe: Option<&PresentProbe>,
    window: &mut Window,
    cx: &mut Context<PaletteView>,
) -> (Entity<InputState>, Subscription, Option<Subscription>) {
    let input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("Search tabs and bookmarks, run > commands, or enter an address")
    });
    // `subscribe_in` is safe here, unlike in src/ui (#81): only the palette popup draws this view.
    let events = cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
        InputEvent::Change => this.query(cx),
        InputEvent::PressEnter { .. } => this.submit(false, window, cx),
        _ => {}
    });
    let count = probe
        .cloned()
        .map(|probe| cx.observe(&input, move |_, _, _| probe.input_notified()));
    (input, events, count)
}

impl Render for PaletteView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Deferred work runs after this frame's present, so the stamp follows it.
        if let Some(probe) = &self.probe
            && probe.rendered(Instant::now())
        {
            let probe = probe.clone();
            cx.defer(move |_| probe.presented(Instant::now()));
        }
        // The open entry's first frame: timed after its present, as the probe is.
        if self.is_open()
            && let Some(opened) = self.opening.take()
        {
            let usage = self.usage.clone();
            cx.defer(move |_| usage.time(Timing::PaletteOpenToPaint, opened.elapsed()));
        }
        let tokens = theme::current();
        // One bar; the scope chips, rows and status line appear only when there are any (#265).
        let bar = render::bar(&self.input);
        let chips = self.scopes_shown.then(|| render::chips(self, cx));
        let list = render::list(self, cx);
        let notice = render::notice(&self.choices);
        crate::controls::watch_pointer(div())
            .id("palette")
            .role(gpui::Role::Dialog)
            .aria_label("Command palette")
            .key_context("SyrinxPalette")
            .size_full()
            .bg(theme::panel())
            .border_1()
            .border_color(theme::rgba(tokens.panel_line))
            .text_color(theme::rgba(tokens.text))
            .flex()
            .flex_col()
            // The focused Input consumes these actions before parent key bindings can run.
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                this.move_selection(true, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                this.move_selection(false, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| this.dismiss(window, cx)))
            .on_action(cx.listener(|this, _: &NextResult, _, cx| this.move_selection(true, cx)))
            .on_action(
                cx.listener(|this, _: &PreviousResult, _, cx| this.move_selection(false, cx)),
            )
            .on_action(cx.listener(|this, _: &SearchWeb, window, cx| this.submit(true, window, cx)))
            // The popup queues browser actions without replacing its focused input or results.
            .on_action(cx.listener(|this, _: &ui::NewTab, _, cx| {
                this.send(PaletteEvent::Shortcut(BrowserShortcut::NewTab), cx);
            }))
            .on_action(cx.listener(|this, _: &ui::CloseTab, _, cx| {
                this.send(PaletteEvent::Shortcut(BrowserShortcut::CloseTab), cx);
            }))
            .on_action(cx.listener(|this, _: &ui::Address, _, cx| {
                this.send(PaletteEvent::Shortcut(BrowserShortcut::Address), cx);
            }))
            .on_action(cx.listener(|this, _: &ui::Palette, _, cx| {
                this.send(PaletteEvent::Shortcut(BrowserShortcut::Palette), cx);
            }))
            .on_action(cx.listener(|this, _: &ui::NextTab, _, cx| {
                this.send(PaletteEvent::Shortcut(BrowserShortcut::NextTab), cx);
            }))
            .on_action(cx.listener(|this, _: &ui::PreviousTab, _, cx| {
                this.send(PaletteEvent::Shortcut(BrowserShortcut::PreviousTab), cx);
            }))
            .on_action(cx.listener(|this, _: &ui::ReopenTab, _, cx| {
                this.send(PaletteEvent::Shortcut(BrowserShortcut::ReopenTab), cx);
            }))
            .on_action(cx.listener(|this, _: &ui::SavePage, _, cx| {
                this.send(PaletteEvent::Shortcut(BrowserShortcut::SavePage), cx);
            }))
            .on_action(cx.listener(|this, _: &ui::Back, _, cx| {
                this.send(PaletteEvent::Shortcut(BrowserShortcut::Back), cx);
            }))
            .on_action(cx.listener(|this, _: &ui::Forward, _, cx| {
                this.send(PaletteEvent::Shortcut(BrowserShortcut::Forward), cx);
            }))
            .on_action(cx.listener(|this, _: &ui::ToggleSidebar, _, cx| {
                this.send(PaletteEvent::Shortcut(BrowserShortcut::ToggleSidebar), cx);
            }))
            .on_action(cx.listener(|this, _: &ui::Reload, _, cx| {
                this.send(PaletteEvent::Shortcut(BrowserShortcut::Reload), cx);
            }))
            // The first Tab shows the scope chips; the next one moves to them.
            .on_action(cx.listener(|this, _: &ui::FocusNextControl, window, cx| {
                if !this.show_scopes(cx) {
                    window.focus_next(cx);
                }
            }))
            .on_action(|_: &ui::FocusPreviousControl, window, cx| window.focus_prev(cx))
            .child(
                self.query_container(window, cx)
                    .child(bar)
                    .children(chips)
                    .children(list),
            )
            .children(notice)
    }
}

impl PaletteView {
    /// The combo box that holds the query field and the rows (DW-60). It tracks the query
    /// field's own focus handle, so while the owner types it is the node a screen reader
    /// hears as focused, and the announced row below it can claim that focus.
    fn query_container(&self, window: &Window, cx: &App) -> Stateful<Div> {
        text_field::container(text_field::PALETTE_QUERY, &self.input, window, cx)
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
    }
}

#[cfg(test)]
mod tests {
    use futures::{FutureExt, StreamExt, channel::mpsc::unbounded};
    use syrinx_core::{BrowserAction, TabId};

    use super::*;

    const ADDRESS: &str = "https://palette.example.test/Case?Key=Value";

    fn tab_rows(count: u64) -> SearchResults {
        SearchResults {
            generation: 1,
            counts: syrinx_search::ScopeCounts::default(),
            hits: (1..=count)
                .map(|id| syrinx_search::SearchHit {
                    target: Target::Tab(TabId::new(id).expect("synthetic tab")),
                    score: 1,
                    title: "Synthetic tab".to_owned(),
                    address: "https://fit.example.test/".to_owned(),
                    title_matches: Vec::new(),
                    address_matches: Vec::new(),
                })
                .collect(),
        }
    }

    /// The popup's height for `choices`, as `wanted_height` measures it with the chips hidden.
    fn drawn(choices: &PaletteChoices) -> f32 {
        render::height(false, choices, View::Everything)
    }

    #[test]
    fn only_a_reply_that_changes_the_drawn_height_asks_for_a_refit() {
        let mut choices = PaletteChoices::default();
        let mut fitted = FittedHeight::default();
        // An entry starts at the bar's height, which the browser shows without a report.
        let first = choices.query_scoped("", true);
        fitted.reset(drawn(&choices));
        assert!(!fitted.changed_to(drawn(&choices)), "nothing changed yet");

        assert!(choices.receive(first, Ok(tab_rows(2))));
        assert!(
            fitted.changed_to(drawn(&choices)),
            "two rows grow the popup"
        );
        assert!(!fitted.changed_to(drawn(&choices)), "reported once only");

        let second = choices.refresh();
        assert!(choices.receive(second, Ok(tab_rows(2))));
        assert!(!fitted.changed_to(drawn(&choices)), "the same rows again");

        let third = choices.refresh();
        assert!(choices.receive(third, Ok(tab_rows(1))));
        assert!(fitted.changed_to(drawn(&choices)), "one row shrinks it");

        // A new entry forgets the old height: the popup is shown afresh at the bar's.
        choices.reset(None);
        choices.query_scoped("", true);
        fitted.reset(drawn(&choices));
        assert!(!fitted.changed_to(drawn(&choices)));
    }

    #[test]
    fn showing_the_scope_chips_changes_the_drawn_height() {
        let choices = PaletteChoices::default();
        let mut fitted = FittedHeight::default();
        fitted.reset(render::height(false, &choices, View::Everything));
        assert!(fitted.changed_to(render::height(true, &choices, View::Everything)));
    }

    #[test]
    fn only_an_entry_that_was_active_closes_when_focus_leaves_it() {
        assert!(dismisses_on_deactivation(true, true), "a click outside");
        assert!(
            !dismisses_on_deactivation(true, false),
            "the previous entry's queued deactivation"
        );
        assert!(!dismisses_on_deactivation(false, true), "already closed");
    }

    #[test]
    fn shortcut_stream_keeps_queries_until_submit_activation_run_or_close() {
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
            PaletteEvent::Activate(Target::Tab(TabId::new(1).expect("synthetic tab"))),
            PaletteEvent::Run(CommandAction::Browser(BrowserAction::Reload)),
            PaletteEvent::Closed,
            PaletteEvent::ClickedAway,
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
                        scope: Scope::Everything,
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
                    PaletteEvent::Query {
                        request_id, text, ..
                    } => {
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
}
