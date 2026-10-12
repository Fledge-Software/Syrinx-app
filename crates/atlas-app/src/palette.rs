//! Presents commands and local catalogue results in an owned window above native page views.

mod choices;
mod commands;
mod display;
mod query;
mod render;

use std::rc::Rc;

use atlas_core::{CommandAction, CommandContext, CommandRegistry};
use atlas_platform::browser::BrowserShortcut;
use atlas_search::{Scope, SearchResults, Target};
use futures::channel::mpsc::UnboundedSender;
use gpui::{
    App, AppContext, Context, Entity, InteractiveElement, IntoElement, KeyBinding, ParentElement,
    Render, ScrollHandle, Styled, Subscription, Window, actions, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState, MoveDown, MoveUp};

pub use display::arrange;

use choices::PaletteChoices;
use commands::{CommandMatcher, CommandMatches};
use display::Preview;
use query::{Chip, ParsedQuery, View};

use crate::{session::SessionError, theme, ui};

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

pub struct PaletteView {
    input: Entity<InputState>,
    events: UnboundedSender<PaletteEvent>,
    choices: PaletteChoices,
    commands: CommandMatcher,
    context: CommandContext,
    chip: Chip,
    view: View,
    scroll: ScrollHandle,
    _subscription: Subscription,
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
        registry: Rc<CommandRegistry>,
        context: CommandContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search tabs and bookmarks, run > commands, or enter an address")
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
        let (commands, status) = match CommandMatcher::new(registry) {
            Ok(commands) => (commands, None),
            Err(error) => (
                CommandMatcher::empty(),
                Some(format!("Commands are unavailable: {error}")),
            ),
        };
        let mut view = Self {
            input,
            events,
            choices: PaletteChoices::default(),
            commands,
            context,
            chip: Chip::default(),
            view: View::Everything,
            scroll: ScrollHandle::new(),
            _subscription: subscription,
        };
        view.query(cx);
        if let Some(status) = status {
            view.choices.status = status;
        }
        view
    }

    fn query(&mut self, cx: &mut Context<Self>) {
        let input = self.input.read(cx).value().to_string();
        let parsed = ParsedQuery::parse(&input);
        self.view = parsed.view(self.chip);
        let request_id = self
            .choices
            .query_scoped(&input, self.view.accepts_address());
        self.send_query(request_id, parsed.text, parsed.prefix.is_some());
        cx.notify();
    }

    /// Re-runs the current text after the tab model or command context changed.
    pub fn refresh(&mut self, context: CommandContext, cx: &mut Context<Self>) {
        self.context = context;
        let input = self.input.read(cx).value().to_string();
        let parsed = ParsedQuery::parse(&input);
        let request_id = self.choices.refresh();
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

    pub fn receive(
        &mut self,
        request_id: u64,
        result: Result<SearchResults, SessionError>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.choices.receive(request_id, result) {
            return false;
        }
        self.scroll_to_selection();
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
        self.finish(event, window, cx);
    }

    /// Sends the entry's final event and closes the popup; command rows become their action.
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
        window.remove_window();
    }

    /// Choosing a chip drops a typed prefix, since the prefix would otherwise override it.
    fn choose_chip(&mut self, chip: Chip, window: &mut Window, cx: &mut Context<Self>) {
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
            atlas_core::CommandScope::Plugin { plugin } => Some(plugin.as_str()),
            atlas_core::CommandScope::Browser => None,
        };
        let shortcut = command.shortcut.as_ref().map(atlas_core::Shortcut::label);
        Some(Preview::command(hit, plugin, shortcut))
    }
}

impl Render for PaletteView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = theme::tokens();
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
        div()
            .key_context("AtlasPalette")
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
            .on_action(cx.listener(|this, _: &Dismiss, window, _| {
                this.emit(PaletteEvent::Closed);
                window.remove_window();
            }))
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
            .on_action(cx.listener(|this, _: &ui::Reload, _, _| {
                this.emit(PaletteEvent::Shortcut(BrowserShortcut::Reload));
            }))
            .child(
                div()
                    .h(px(56.))
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().flex_1().child(Input::new(&self.input)))
                    .child(render::key_hint("Esc")),
            )
            .child(render::chips(self, cx))
            .child(body)
            .child(render::footer(&self.choices.status))
    }
}

#[cfg(test)]
mod tests {
    use atlas_core::{BrowserAction, TabId};
    use futures::{FutureExt, StreamExt, channel::mpsc::unbounded};

    use super::*;

    const ADDRESS: &str = "https://palette.example.test/Case?Key=Value";

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
