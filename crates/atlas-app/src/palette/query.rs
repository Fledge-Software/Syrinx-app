//! Reads the palette's prefixes and scope chips into one view, and totals the chip counts.

use atlas_search::{Scope, ScopeCounts};

/// The scope chips, in the order the palette draws them (spec, section 3).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Chip {
    #[default]
    Everything,
    Tabs,
    PluginActions,
    Bookmarks,
    History,
}

impl Chip {
    pub const ALL: [Chip; 5] = [
        Chip::Everything,
        Chip::Tabs,
        Chip::PluginActions,
        Chip::Bookmarks,
        Chip::History,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Everything => "Everything",
            Self::Tabs => "Tabs",
            Self::PluginActions => "Plugin actions",
            Self::Bookmarks => "Bookmarks",
            Self::History => "History",
        }
    }
}

/// A leading character that narrows the palette: `>` commands, `@` plugins, `/` open tabs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Prefix {
    Commands,
    Plugins,
    Tabs,
}

impl Prefix {
    fn from_char(character: char) -> Option<Self> {
        match character {
            '>' => Some(Self::Commands),
            '@' => Some(Self::Plugins),
            '/' => Some(Self::Tabs),
            _ => None,
        }
    }
}

/// What the palette is currently listing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum View {
    Everything,
    Tabs,
    Commands,
    PluginActions,
    Bookmarks,
    History,
}

impl View {
    /// The catalogue scope the session worker searches for this view.
    pub fn search_scope(self) -> Scope {
        match self {
            Self::Everything => Scope::Everything,
            Self::Tabs => Scope::Tabs,
            Self::Bookmarks => Scope::Saved,
            Self::History => Scope::History,
            // Commands are matched locally; the worker's reply still supplies chip counts.
            Self::Commands | Self::PluginActions => Scope::Commands,
        }
    }

    /// The chip drawn as selected; the `>` view has no chip of its own.
    pub fn chip(self) -> Option<Chip> {
        match self {
            Self::Everything => Some(Chip::Everything),
            Self::Tabs => Some(Chip::Tabs),
            Self::PluginActions => Some(Chip::PluginActions),
            Self::Bookmarks => Some(Chip::Bookmarks),
            Self::History => Some(Chip::History),
            Self::Commands => None,
        }
    }

    /// Whether the input may still be an address for the Go to row.
    pub fn accepts_address(self) -> bool {
        matches!(self, Self::Everything)
    }
}

/// The palette input split into its optional prefix and the text to match.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParsedQuery<'a> {
    pub prefix: Option<Prefix>,
    pub text: &'a str,
}

impl<'a> ParsedQuery<'a> {
    /// Only the very first character is a prefix, so `a/b` and ` >x` stay ordinary text.
    pub fn parse(input: &'a str) -> Self {
        let mut characters = input.chars();
        let prefix = characters.next().and_then(Prefix::from_char);
        match prefix {
            Some(prefix) => Self {
                prefix: Some(prefix),
                text: characters.as_str().trim_start(),
            },
            None => Self {
                prefix: None,
                text: input,
            },
        }
    }

    /// A typed prefix wins over the selected chip.
    pub fn view(self, chip: Chip) -> View {
        match (self.prefix, chip) {
            (Some(Prefix::Commands), _) => View::Commands,
            (Some(Prefix::Plugins), _) => View::PluginActions,
            (Some(Prefix::Tabs), _) => View::Tabs,
            (None, Chip::Everything) => View::Everything,
            (None, Chip::Tabs) => View::Tabs,
            (None, Chip::PluginActions) => View::PluginActions,
            (None, Chip::Bookmarks) => View::Bookmarks,
            (None, Chip::History) => View::History,
        }
    }
}

/// Commands matching the current text, split by who provides them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CommandCounts {
    pub browser: usize,
    pub plugin: usize,
}

/// The number on each chip for the current text.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChipCounts {
    pub everything: usize,
    pub tabs: usize,
    pub plugin_actions: usize,
    pub bookmarks: usize,
    pub history: usize,
}

impl ChipCounts {
    /// Commands join Everything only once text is typed; a blank palette lists pages, not commands.
    pub fn new(search: ScopeCounts, commands: CommandCounts, typed: bool) -> Self {
        let typed_commands = if typed {
            commands.browser.saturating_add(commands.plugin)
        } else {
            0
        };
        Self {
            everything: search
                .tabs
                .saturating_add(search.saved)
                .saturating_add(search.history)
                .saturating_add(typed_commands),
            tabs: search.tabs,
            plugin_actions: commands.plugin,
            bookmarks: search.saved,
            history: search.history,
        }
    }

    pub fn get(self, chip: Chip) -> usize {
        match chip {
            Chip::Everything => self.everything,
            Chip::Tabs => self.tabs,
            Chip::PluginActions => self.plugin_actions,
            Chip::Bookmarks => self.bookmarks,
            Chip::History => self.history,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_leading_prefix_narrows_the_palette() {
        let cases = [
            (">", Some(Prefix::Commands), ""),
            ("> reload", Some(Prefix::Commands), "reload"),
            (">new", Some(Prefix::Commands), "new"),
            ("@fledge", Some(Prefix::Plugins), "fledge"),
            ("/ docs", Some(Prefix::Tabs), "docs"),
            ("//", Some(Prefix::Tabs), "/"),
            ("é>", None, "é>"),
            (" >reload", None, " >reload"),
            ("https://example.test/a", None, "https://example.test/a"),
            ("a/b", None, "a/b"),
            ("", None, ""),
        ];
        for (input, prefix, text) in cases {
            let parsed = ParsedQuery::parse(input);
            assert_eq!(parsed.prefix, prefix, "{input}");
            assert_eq!(parsed.text, text, "{input}");
        }
    }

    #[test]
    fn prefixes_override_chips_and_choose_the_search_scope() {
        for chip in Chip::ALL {
            assert_eq!(ParsedQuery::parse(">x").view(chip), View::Commands);
            assert_eq!(ParsedQuery::parse("@x").view(chip), View::PluginActions);
            assert_eq!(ParsedQuery::parse("/x").view(chip), View::Tabs);
            assert_eq!(ParsedQuery::parse("x").view(chip).chip(), Some(chip));
        }
        assert_eq!(View::Everything.search_scope(), Scope::Everything);
        assert_eq!(View::Tabs.search_scope(), Scope::Tabs);
        assert_eq!(View::Bookmarks.search_scope(), Scope::Saved);
        assert_eq!(View::History.search_scope(), Scope::History);
        assert_eq!(View::Commands.search_scope(), Scope::Commands);
        assert_eq!(View::PluginActions.search_scope(), Scope::Commands);
        assert_eq!(View::Commands.chip(), None);
        assert!(View::Everything.accepts_address());
        assert!(!View::Tabs.accepts_address());
    }

    #[test]
    fn chip_counts_combine_search_and_command_matches() {
        let search = ScopeCounts {
            tabs: 3,
            saved: 2,
            history: 0,
            commands: 0,
        };
        let commands = CommandCounts {
            browser: 4,
            plugin: 0,
        };
        let typed = ChipCounts::new(search, commands, true);
        assert_eq!(typed.everything, 9);
        assert_eq!(typed.get(Chip::Tabs), 3);
        assert_eq!(typed.get(Chip::Bookmarks), 2);
        // History stays zero until the history slice feeds the catalogue.
        assert_eq!(typed.get(Chip::History), 0);
        assert_eq!(typed.get(Chip::PluginActions), 0);

        let blank = ChipCounts::new(search, commands, false);
        assert_eq!(blank.everything, 5);

        let plugins = ChipCounts::new(
            ScopeCounts::default(),
            CommandCounts {
                browser: 1,
                plugin: 2,
            },
            true,
        );
        assert_eq!(plugins.get(Chip::PluginActions), 2);
        assert_eq!(plugins.get(Chip::Everything), 3);
    }
}
