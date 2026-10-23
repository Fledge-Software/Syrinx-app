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
    Subscription, Window, actions, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState, MoveDown, MoveUp};
use syrinx_core::usage::{Feature, Timing, UsageRecorder};
use syrinx_core::{CommandAction, CommandContext, CommandRegistry};
use syrinx_platform::browser::BrowserShortcut;
use syrinx_search::{Scope, SearchResults, Target};

pub use display::arrange;

use choices::PaletteChoices;
use commands::{CommandMatcher, CommandMatches};
use display::Preview;
use query::{Chip, ParsedQuery, View};

use crate::{acceptance::PresentProbe, session::SessionError, text_field, theme, ui};

actions!(palette, [Dismiss, NextResult, PreviousResult, SearchWeb]);

/// The preview pane is drawn only when the popup is at least this wide.
const PREVIEW_MIN_WIDTH: f32 = 720.;

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
    Closed,
}

impl PaletteEvent {
    pub fn finishes_entry(&self) -> bool {
        match self {
            Self::Submit { .. } | Self::Activate(_) | Self::Run(_) | Self::Closed => true,
            Self::Query { .. } | Self::Shortcut(_) => false,
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
        self.choices.reset();
        self.chip = Chip::default();
        self.view = View::Everything;
        self.scroll = ScrollHandle::new();
        self.input.update(cx, |input, cx| {
            input.set_value(entry.initial, window, cx);
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        // `set_value` does not emit a change event, so query explicitly.
        self.query(cx);
        if let Some(problem) = &self.command_problem {
            self.choices.status = problem.clone();
        }
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
        if self.is_open() {
            self.emit(PaletteEvent::Closed);
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
        self.send_query(request_id, parsed.text, parsed.prefix.is_some());
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
        self.send_query(request_id, parsed.text, parsed.prefix.is_some());
        cx.notify();
    }

    /// Stages this request's commands, then asks the worker for its page rows and counts.
    fn send_query(&mut self, request_id: u64, text: &str, prefixed: bool) {
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
        self.emit(PaletteEvent::Query {
            request_id,
            text: text.to_owned(),
            scope: self.view.search_scope(),
        });
    }

    /// Shows the current request's rows, then applies an Enter that was waiting for them.
    pub fn receive(
        &mut self,
        request_id: u64,
        result: Result<SearchResults, SessionError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.is_open() || !self.choices.receive(request_id, result) {
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
        cx.notify();
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

    fn emit(&mut self, event: PaletteEvent) {
        let Some(events) = &self.events else {
            return;
        };
        if events.unbounded_send(event).is_err() {
            self.choices.status = "The browser window is no longer available.".to_owned();
        }
    }

    fn submit(&mut self, force_search: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_open() {
            return;
        }
        let text = self.input.read(cx).value().to_string();
        let Some(event) = self.choices.submission(text, force_search) else {
            // A waiting Enter shows its status until the reply applies it.
            cx.notify();
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
                    cx.notify();
                    return;
                };
                PaletteEvent::Run(command.action.clone())
            }
            event => event,
        };
        self.emit(event);
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

    fn preview(&self) -> Option<Preview> {
        let selected = self.choices.selected?;
        if selected == 0
            && let Some(address) = &self.choices.address
        {
            return Some(Preview::address(address));
        }
        let hit = self.choices.selected_hit()?;
        let Some(command) = self.commands.command(hit.target) else {
            return Some(Preview::page(hit));
        };
        let plugin = match &command.scope {
            syrinx_core::CommandScope::Plugin { plugin } => Some(plugin.as_str()),
            syrinx_core::CommandScope::Browser => None,
        };
        let shortcut = command.shortcut.as_ref().map(syrinx_core::Shortcut::label);
        Some(Preview::command(hit, plugin, shortcut))
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
        let wide = f32::from(window.viewport_size().width) >= PREVIEW_MIN_WIDTH;
        let list = render::list(self, wide, cx);
        let mut body = div()
            .flex()
            .flex_1()
            .min_h_0()
            .border_t_1()
            .border_color(theme::rgba(tokens.panel_divider))
            .child(list);
        if wide {
            body = body.child(render::preview(self.preview()));
        }
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
            .on_action(cx.listener(|this, _: &ui::ToggleSidebar, _, _| {
                this.emit(PaletteEvent::Shortcut(BrowserShortcut::ToggleSidebar));
            }))
            .on_action(cx.listener(|this, _: &ui::Reload, _, _| {
                this.emit(PaletteEvent::Shortcut(BrowserShortcut::Reload));
            }))
            // Tab leaves the query for the scope chips and the web search button.
            .on_action(|_: &ui::FocusNextControl, window, cx| window.focus_next(cx))
            .on_action(|_: &ui::FocusPreviousControl, window, cx| window.focus_prev(cx))
            .child(
                self.query_container(window, cx)
                    .child(
                        div()
                            .h(px(56.))
                            .px_4()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(div().flex_1().child(
                                Input::new(&self.input).aria_label(text_field::PALETTE_QUERY.label),
                            ))
                            .child(render::key_hint("Esc")),
                    )
                    .child(render::chips(self, cx))
                    .child(body),
            )
            .child(render::footer(&self.choices.status))
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
