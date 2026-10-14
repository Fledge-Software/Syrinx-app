//! Matches registered commands on the palette's thread, because enablement depends on live browser state.
//!
//! The registry is small and bounded (built-in commands now, plugin commands with Plan 2), so this
//! ranks a few dozen rows per keystroke; tab, saved-page and history search stays on the worker.

use std::{num::NonZeroU64, rc::Rc};

use syrinx_core::{Command, CommandContext, CommandRegistry};
use syrinx_search::{
    BlankQuery, BoostWeights, Catalogue, Entry, Scope, SearchError, SearchHit, SearchPolicy,
    Target, Usage,
};

use super::query::{CommandCounts, View};

/// Everything lists at most this many commands above the page results.
const EVERYTHING_COMMAND_LIMIT: usize = 4;

pub struct CommandMatcher {
    registry: Rc<CommandRegistry>,
    catalogue: Catalogue,
}

/// The command rows for one query, and how many matched before the view's limit.
#[derive(Default)]
pub struct CommandMatches {
    pub hits: Vec<SearchHit>,
    /// The first `strong` hits may rank above pages; the rest follow them.
    pub strong: usize,
    pub counts: CommandCounts,
}

impl CommandMatcher {
    pub fn new(registry: Rc<CommandRegistry>) -> Result<Self, SearchError> {
        let mut catalogue = Catalogue::new();
        for (key, command) in registry.iter() {
            catalogue.upsert(entry(key, command))?;
        }
        Ok(Self {
            registry,
            catalogue,
        })
    }

    /// A matcher with no commands, used when the registry cannot be indexed.
    pub fn empty() -> Self {
        Self {
            registry: Rc::new(CommandRegistry::new()),
            catalogue: Catalogue::new(),
        }
    }

    pub fn command(&self, target: Target) -> Option<&Command> {
        let Target::Command(key) = target else {
            return None;
        };
        self.registry.get(key)
    }

    /// Ranks enabled commands for `text` and keeps the rows `view` shows.
    // TODO(DW-5): bound this before plugins register commands: cap the registry or rank off the UI thread.
    pub fn matches(
        &mut self,
        text: &str,
        view: View,
        context: CommandContext,
    ) -> Result<CommandMatches, SearchError> {
        let policy = policy(self.registry.len());
        let results = self.catalogue.query(text, Scope::Commands, 0, policy)?;
        let enabled: Vec<SearchHit> = results
            .hits
            .into_iter()
            .filter(|hit| {
                self.command(hit.target)
                    .is_some_and(|command| command.is_enabled(context))
            })
            .collect();
        let counts = self.count(&enabled);
        let mut hits = match view {
            View::Everything if text.is_empty() => Vec::new(),
            View::Everything => enabled.into_iter().take(EVERYTHING_COMMAND_LIMIT).collect(),
            View::Commands => enabled,
            View::PluginActions => enabled
                .into_iter()
                .filter(|hit| {
                    self.command(hit.target)
                        .is_some_and(Command::is_plugin_action)
                })
                .collect(),
            View::Tabs | View::Bookmarks | View::History => Vec::new(),
        };
        let strong = match view {
            View::Everything => strong_first(&mut hits, text),
            // `>` and `@` list commands only, so they lead by definition.
            _ => hits.len(),
        };
        Ok(CommandMatches {
            hits,
            strong,
            counts,
        })
    }

    fn count(&self, hits: &[SearchHit]) -> CommandCounts {
        let plugin = hits
            .iter()
            .filter(|hit| {
                self.command(hit.target)
                    .is_some_and(Command::is_plugin_action)
            })
            .count();
        CommandCounts {
            browser: hits.len() - plugin,
            plugin,
        }
    }
}

/// Moves exact title matches first and returns how many there are.
///
/// In Everything a command outranks pages only when the text names it outright ("close tab"), so
/// typing a word such as "close" and pressing Enter opens a page rather than closing the tab.
fn strong_first(hits: &mut [SearchHit], text: &str) -> usize {
    let wanted = text.trim().to_lowercase();
    let is_strong = |hit: &SearchHit| !wanted.is_empty() && hit.title.to_lowercase() == wanted;
    hits.sort_by_key(|hit| !is_strong(hit));
    hits.iter().take_while(|hit| is_strong(hit)).count()
}

fn entry(key: NonZeroU64, command: &Command) -> Entry {
    // Keywords sit in the second field so "refresh" finds Reload without showing as an address.
    Entry {
        target: Target::Command(key),
        title: command.title.clone(),
        address: command.keywords.clone(),
        usage: Usage {
            visits: 0,
            last_used: None,
        },
        open: false,
    }
}

/// Commands are few and named exactly, so typos would only add noise.
fn policy(command_count: usize) -> SearchPolicy {
    SearchPolicy {
        limit: command_count.max(1),
        max_typos: 0,
        blank: BlankQuery::All,
        boosts: BoostWeights {
            open: 0,
            frequency: 0,
            recency: 0,
            recency_window_secs: NonZeroU64::MIN,
        },
    }
}

#[cfg(test)]
mod tests {
    use syrinx_core::{CommandAction, CommandId, CommandScope, Enablement, Shortcut};

    use super::*;

    const PAGE: CommandContext = CommandContext {
        active_tab: true,
        active_page: true,
        tab_count: 2,
    };

    fn titles(matcher: &CommandMatcher, hits: &[SearchHit]) -> Vec<String> {
        hits.iter()
            .map(|hit| {
                matcher
                    .command(hit.target)
                    .expect("registered")
                    .title
                    .clone()
            })
            .collect()
    }

    fn matcher_with_plugin() -> CommandMatcher {
        let mut registry = CommandRegistry::browser();
        registry
            .register(Command {
                id: CommandId::new("synthetic.new-record").expect("synthetic identifier"),
                title: "New synthetic record".to_owned(),
                keywords: String::new(),
                scope: CommandScope::Plugin {
                    plugin: "synthetic".to_owned(),
                },
                enablement: Enablement::Always,
                action: CommandAction::Plugin {
                    plugin: "synthetic".to_owned(),
                    action: "new-record".to_owned(),
                },
                shortcut: Some(Shortcut::new("ctrl-alt-n").expect("synthetic shortcut")),
            })
            .expect("register synthetic plugin command");
        CommandMatcher::new(Rc::new(registry)).expect("index commands")
    }

    #[test]
    fn command_prefix_lists_every_enabled_command_and_keywords_match() {
        let mut matcher =
            CommandMatcher::new(Rc::new(CommandRegistry::browser())).expect("index commands");
        let all = matcher
            .matches("", View::Commands, PAGE)
            .expect("blank command query");
        // Eleven navigation and history commands, P1-13's six page essentials, P1-27's clear
        // data, P1-39b's install update from file and P1-39c's check for updates.
        assert_eq!(all.hits.len(), 20);
        assert_eq!(
            all.counts,
            CommandCounts {
                browser: 20,
                plugin: 0
            }
        );

        let refresh = matcher
            .matches("refresh", View::Commands, PAGE)
            .expect("keyword query");
        assert_eq!(titles(&matcher, &refresh.hits), ["Reload page"]);
        let magnify = matcher
            .matches("magnify", View::Commands, PAGE)
            .expect("keyword query");
        assert_eq!(titles(&matcher, &magnify.hits), ["Zoom in"]);

        // Home has no page, so page commands neither show nor count.
        let home = CommandContext {
            active_page: false,
            tab_count: 1,
            ..PAGE
        };
        let reload = matcher
            .matches("reload", View::Commands, home)
            .expect("disabled query");
        assert!(reload.hits.is_empty());
        assert_eq!(reload.counts, CommandCounts::default());
        for page_only in ["find in page", "print", "downloads", "reset zoom"] {
            let hidden = matcher
                .matches(page_only, View::Commands, home)
                .expect("disabled query");
            assert!(hidden.hits.is_empty(), "{page_only} needs a page");
        }
    }

    #[test]
    fn everything_shows_commands_only_for_typed_text_and_bounds_them() {
        let mut matcher =
            CommandMatcher::new(Rc::new(CommandRegistry::browser())).expect("index commands");
        let blank = matcher
            .matches("", View::Everything, PAGE)
            .expect("blank query");
        assert!(blank.hits.is_empty());
        assert_eq!(blank.counts.browser, 20);

        let tab = matcher
            .matches("tab", View::Everything, PAGE)
            .expect("typed query");
        assert!(!tab.hits.is_empty());
        assert!(tab.hits.len() <= EVERYTHING_COMMAND_LIMIT);
        assert!(tab.counts.browser >= tab.hits.len());
        for view in [View::Tabs, View::Bookmarks, View::History] {
            assert!(
                matcher
                    .matches("tab", view, PAGE)
                    .expect("query")
                    .hits
                    .is_empty()
            );
        }
    }

    #[test]
    fn only_exact_titles_may_lead_pages_in_everything() {
        let mut matcher =
            CommandMatcher::new(Rc::new(CommandRegistry::browser())).expect("index commands");
        let partial = matcher
            .matches("close", View::Everything, PAGE)
            .expect("partial query");
        assert!(!partial.hits.is_empty());
        assert_eq!(partial.strong, 0, "a partial word must not outrank pages");
        let exact = matcher
            .matches("close tab", View::Everything, PAGE)
            .expect("exact query");
        assert_eq!(exact.strong, 1);
        assert_eq!(titles(&matcher, &exact.hits[..1]), ["Close tab"]);
        let prefixed = matcher
            .matches("close", View::Commands, PAGE)
            .expect("command query");
        assert_eq!(prefixed.strong, prefixed.hits.len());
    }

    #[test]
    fn plugin_view_lists_only_plugin_actions_and_counts_them_apart() {
        let mut matcher =
            CommandMatcher::new(Rc::new(CommandRegistry::browser())).expect("index commands");
        let none = matcher
            .matches("", View::PluginActions, PAGE)
            .expect("plugin query");
        assert!(none.hits.is_empty(), "no plugins are registered yet");
        assert_eq!(none.counts.plugin, 0);

        let mut matcher = matcher_with_plugin();
        let plugins = matcher
            .matches("new", View::PluginActions, PAGE)
            .expect("plugin query");
        assert_eq!(titles(&matcher, &plugins.hits), ["New synthetic record"]);
        assert_eq!(plugins.counts.plugin, 1);
        assert!(plugins.counts.browser >= 1, "New tab also matches");
        assert!(
            matcher
                .command(Target::Tab(syrinx_core::TabId::new(1).expect("tab")))
                .is_none()
        );
    }
}
