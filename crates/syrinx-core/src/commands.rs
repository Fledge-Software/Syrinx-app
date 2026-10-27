//! Lists every command the palette and shortcuts can run, so built-in and plugin commands share one source.

use std::{fmt, num::NonZeroU64};

const MAX_ID_LENGTH: usize = 64;
const MAX_TITLE_LENGTH: usize = 120;

/// A stable command name such as `browser.new-tab`; plugins will prefix theirs with the plugin name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CommandId(String);

impl CommandId {
    pub fn new(value: impl Into<String>) -> Result<Self, CommandError> {
        let value = value.into();
        if !is_valid_id(&value) {
            return Err(CommandError::InvalidId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Dot-separated lowercase ASCII segments keep identifiers stable and safe to log.
fn is_valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID_LENGTH
        && value.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

/// Who provides a command; the palette's Plugin actions chip counts only plugin commands.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandScope {
    Browser,
    Plugin { plugin: String },
}

/// The browser state a command needs before it can run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Enablement {
    Always,
    /// Any selected tab, including Home.
    ActiveTab,
    /// A loaded web page is showing.
    ActivePage,
    /// More than one tab is open, so switching means something.
    SeveralTabs,
}

/// Facts about the browser that decide which commands are enabled.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CommandContext {
    pub active_tab: bool,
    pub active_page: bool,
    pub tab_count: usize,
}

impl Enablement {
    pub fn allows(self, context: CommandContext) -> bool {
        match self {
            Self::Always => true,
            Self::ActiveTab => context.active_tab,
            Self::ActivePage => context.active_page,
            Self::SeveralTabs => context.tab_count > 1,
        }
    }
}

/// A built-in browser action; the application maps each to its handler and key binding.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BrowserAction {
    NewTab,
    CloseTab,
    ReopenTab,
    Reload,
    Back,
    Forward,
    EditAddress,
    NextTab,
    PreviousTab,
    SavePage,
    ShowHistory,
    FindInPage,
    ShowDownloads,
    Print,
    ZoomIn,
    ZoomOut,
    ResetZoom,
    /// Opens P1-27's clear browsing data dialog; the dialog itself asks before deleting.
    ClearBrowsingData,
    /// Asks for a signed update package folder and stages it (P1-39b); nothing installs until
    /// the owner restarts or quits.
    InstallUpdateFromFile,
    /// Asks the update host what the newest release is (P1-39c); downloads only after the owner
    /// confirms the version and size.
    CheckForUpdates,
}

/// What running a command does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandAction {
    Browser(BrowserAction),
    /// Handled by the owning plugin through the plugin host (Plan 2); no host exists yet.
    Plugin {
        plugin: String,
        action: String,
    },
}

/// Modifiers in the one order a normalized shortcut spells them; `win` also covers `cmd`/`super`.
const MODIFIERS: [&str; 5] = ["ctrl", "alt", "shift", "win", "fn"];

/// Keys the palette itself owns while it is open, so no command may claim them.
const PALETTE_KEYS: [&str; 8] = [
    "ctrl-k",
    "escape",
    "up",
    "down",
    "enter",
    "alt-enter",
    "ctrl-enter",
    "tab",
];

/// Opens the address entry; only the built-in Edit address command may use it.
const ADDRESS_KEY: &str = "ctrl-l";

/// A keyboard shortcut in GPUI keystroke syntax, normalized so that equal keys compare equal:
/// `Shift-Ctrl-T` and `ctrl-shift-t` are the same shortcut.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Shortcut(String);

impl Shortcut {
    pub fn new(keystroke: impl Into<String>) -> Result<Self, CommandError> {
        let keystroke = keystroke.into().to_ascii_lowercase();
        let parts: Vec<&str> = keystroke.split('-').collect();
        let Some((key, modifiers)) = parts.split_last() else {
            return Err(CommandError::InvalidShortcut);
        };
        if !is_key_name(key) || modifier_index(key).is_some() {
            return Err(CommandError::InvalidShortcut);
        }
        let mut present = [false; MODIFIERS.len()];
        for modifier in modifiers {
            let index = modifier_index(modifier).ok_or(CommandError::InvalidShortcut)?;
            // A repeated modifier is a typo, not a different shortcut.
            if std::mem::replace(&mut present[index], true) {
                return Err(CommandError::InvalidShortcut);
            }
        }
        let mut normalized: Vec<&str> = MODIFIERS
            .iter()
            .zip(present)
            .filter_map(|(name, on)| on.then_some(*name))
            .collect();
        normalized.push(key);
        Ok(Self(normalized.join("-")))
    }

    /// Whether only the palette (or, for Ctrl L, Edit address) may use this key.
    fn is_reserved_for(&self, action: &CommandAction) -> bool {
        if PALETTE_KEYS.contains(&self.0.as_str()) {
            return true;
        }
        self.0 == ADDRESS_KEY && *action != CommandAction::Browser(BrowserAction::EditAddress)
    }

    /// The keystroke as GPUI's key binding parser expects it.
    pub fn keystroke(&self) -> &str {
        &self.0
    }

    /// The label the palette shows, such as `Ctrl Shift T`.
    pub fn label(&self) -> String {
        self.0
            .split('-')
            .map(capitalize)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn is_key_name(key: &str) -> bool {
    !key.is_empty() && key.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn modifier_index(modifier: &str) -> Option<usize> {
    let canonical = match modifier {
        "cmd" | "super" => "win",
        other => other,
    };
    MODIFIERS.iter().position(|name| *name == canonical)
}

fn capitalize(key: &str) -> String {
    let mut characters = key.chars();
    let Some(first) = characters.next() else {
        return String::new();
    };
    first.to_ascii_uppercase().to_string() + characters.as_str()
}

/// One runnable command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Command {
    pub id: CommandId,
    pub title: String,
    /// Extra words the palette also matches, such as "refresh" for Reload.
    pub keywords: String,
    pub scope: CommandScope,
    pub enablement: Enablement,
    pub action: CommandAction,
    pub shortcut: Option<Shortcut>,
}

impl Command {
    pub fn is_enabled(&self, context: CommandContext) -> bool {
        self.enablement.allows(context)
    }

    pub fn is_plugin_action(&self) -> bool {
        matches!(self.scope, CommandScope::Plugin { .. })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandError {
    InvalidId,
    InvalidShortcut,
    InvalidTitle,
    DuplicateId,
    DuplicateShortcut,
    ReservedShortcut,
    TooManyCommands,
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidId => "command identifiers are dotted lowercase ASCII names",
            Self::InvalidShortcut => "command shortcuts use GPUI keystroke syntax",
            Self::InvalidTitle => "command titles must be non-empty and short",
            Self::DuplicateId => "a command with this identifier is already registered",
            Self::DuplicateShortcut => "another command already uses this shortcut",
            Self::ReservedShortcut => "this shortcut belongs to the command palette",
            Self::TooManyCommands => {
                // Name the cap from its constant so the message cannot drift from it.
                return write!(
                    formatter,
                    "the command registry is full ({} commands)",
                    CommandRegistry::MAX_COMMANDS
                );
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for CommandError {}

/// The registered commands in registration order; each gets a stable numeric key for search.
#[derive(Clone, Debug, Default)]
pub struct CommandRegistry {
    commands: Vec<Command>,
}

impl CommandRegistry {
    /// The most commands the registry holds, so the palette's per-keystroke ranking stays bounded.
    pub const MAX_COMMANDS: usize = 1_024;

    pub fn new() -> Self {
        Self::default()
    }

    /// The browser's own commands, with the shortcuts Syrinx binds.
    pub fn browser() -> Self {
        let mut registry = Self::new();
        for command in browser_commands() {
            registry
                .register(command)
                .expect("built-in browser commands are valid and distinct");
        }
        registry
    }

    /// Adds a command and returns its key; an invalid or conflicting command changes nothing.
    pub fn register(&mut self, command: Command) -> Result<NonZeroU64, CommandError> {
        // Refuse a command past the cap before anything else is checked or stored.
        if self.commands.len() >= Self::MAX_COMMANDS {
            return Err(CommandError::TooManyCommands);
        }
        self.validate(&command)?;
        let key = u64::try_from(self.commands.len() + 1)
            .ok()
            .and_then(NonZeroU64::new)
            .ok_or(CommandError::TooManyCommands)?;
        self.commands.push(command);
        Ok(key)
    }

    fn validate(&self, command: &Command) -> Result<(), CommandError> {
        let title = command.title.trim();
        if title.is_empty() || command.title.chars().count() > MAX_TITLE_LENGTH {
            return Err(CommandError::InvalidTitle);
        }
        if self
            .commands
            .iter()
            .any(|existing| existing.id == command.id)
        {
            return Err(CommandError::DuplicateId);
        }
        let Some(shortcut) = &command.shortcut else {
            return Ok(());
        };
        if shortcut.is_reserved_for(&command.action) {
            return Err(CommandError::ReservedShortcut);
        }
        // One keystroke must not run two commands depending on registration order.
        if self
            .commands
            .iter()
            .any(|existing| existing.shortcut.as_ref() == Some(shortcut))
        {
            return Err(CommandError::DuplicateShortcut);
        }
        Ok(())
    }

    pub fn get(&self, key: NonZeroU64) -> Option<&Command> {
        let index = usize::try_from(key.get() - 1).ok()?;
        self.commands.get(index)
    }

    pub fn find(&self, id: &str) -> Option<(NonZeroU64, &Command)> {
        self.iter().find(|(_, command)| command.id.as_str() == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (NonZeroU64, &Command)> {
        self.commands.iter().enumerate().map(|(index, command)| {
            let key = NonZeroU64::new(index as u64 + 1).expect("index plus one is non-zero");
            (key, command)
        })
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// The built-in commands, in the order the palette lists them for `>`.
///
/// Zoom in and Zoom out have no registry shortcut: `ctrl-=` and `ctrl--` use punctuation keys
/// that `Shortcut` does not spell, so the application binds them beside the registry bindings.
fn browser_commands() -> Vec<Command> {
    use BrowserAction::*;
    use Enablement::*;
    [
        (
            "browser.new-tab",
            "New tab",
            "open blank home",
            Always,
            NewTab,
            Some("ctrl-t"),
        ),
        (
            "browser.close-tab",
            "Close tab",
            "unload",
            ActiveTab,
            CloseTab,
            Some("ctrl-w"),
        ),
        (
            "browser.reopen-tab",
            "Reopen closed tab",
            "restore undo",
            Always,
            ReopenTab,
            Some("ctrl-shift-t"),
        ),
        (
            "browser.reload",
            "Reload page",
            "refresh",
            ActivePage,
            Reload,
            Some("ctrl-r"),
        ),
        (
            "browser.back",
            "Go back",
            "previous page history",
            ActivePage,
            Back,
            Some("alt-left"),
        ),
        (
            "browser.forward",
            "Go forward",
            "next page history",
            ActivePage,
            Forward,
            Some("alt-right"),
        ),
        (
            "browser.edit-address",
            "Edit address",
            "url location navigate",
            Always,
            EditAddress,
            Some("ctrl-l"),
        ),
        (
            "browser.next-tab",
            "Next tab",
            "switch",
            SeveralTabs,
            NextTab,
            Some("ctrl-tab"),
        ),
        (
            "browser.previous-tab",
            "Previous tab",
            "switch",
            SeveralTabs,
            PreviousTab,
            Some("ctrl-shift-tab"),
        ),
        (
            "browser.save-page",
            "Save page",
            "pin bookmark",
            ActiveTab,
            SavePage,
            Some("ctrl-d"),
        ),
        (
            "browser.show-history",
            "Open history",
            "visited past pages recent",
            Always,
            ShowHistory,
            Some("ctrl-h"),
        ),
        (
            "browser.find",
            "Find in page",
            "search text",
            ActivePage,
            FindInPage,
            Some("ctrl-f"),
        ),
        (
            "browser.downloads",
            "Downloads",
            "files saved",
            ActivePage,
            ShowDownloads,
            Some("ctrl-j"),
        ),
        (
            "browser.print",
            "Print",
            "paper pdf",
            ActivePage,
            Print,
            Some("ctrl-p"),
        ),
        (
            "browser.zoom-in",
            "Zoom in",
            "larger bigger magnify",
            ActivePage,
            ZoomIn,
            None,
        ),
        (
            "browser.zoom-out",
            "Zoom out",
            "smaller",
            ActivePage,
            ZoomOut,
            None,
        ),
        (
            "browser.zoom-reset",
            "Reset zoom",
            "actual size 100",
            ActivePage,
            ResetZoom,
            Some("ctrl-0"),
        ),
        (
            "browser.clear-data",
            "Clear browsing data…",
            "delete history cookies cache privacy erase",
            Always,
            ClearBrowsingData,
            None,
        ),
        (
            "browser.install-update",
            "Install update from file…",
            "upgrade new version package",
            Always,
            InstallUpdateFromFile,
            None,
        ),
        (
            "browser.check-updates",
            "Check for updates",
            "latest upgrade new version",
            Always,
            CheckForUpdates,
            None,
        ),
    ]
    .into_iter()
    .map(
        |(id, title, keywords, enablement, action, shortcut)| Command {
            id: CommandId::new(id).expect("built-in identifier"),
            title: title.to_owned(),
            keywords: keywords.to_owned(),
            scope: CommandScope::Browser,
            enablement,
            action: CommandAction::Browser(action),
            shortcut: shortcut.map(|keys| Shortcut::new(keys).expect("built-in shortcut")),
        },
    )
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin_command(id: &str, shortcut: Option<&str>) -> Command {
        Command {
            id: CommandId::new(id).expect("synthetic identifier"),
            title: "Synthetic plugin action".to_owned(),
            keywords: String::new(),
            scope: CommandScope::Plugin {
                plugin: "synthetic".to_owned(),
            },
            enablement: Enablement::Always,
            action: CommandAction::Plugin {
                plugin: "synthetic".to_owned(),
                action: "create".to_owned(),
            },
            shortcut: shortcut.map(|keys| Shortcut::new(keys).expect("synthetic shortcut")),
        }
    }

    #[test]
    fn browser_registry_keeps_existing_shortcuts_and_stable_keys() {
        let registry = CommandRegistry::browser();
        let bindings: Vec<_> = registry
            .iter()
            .map(|(_, command)| {
                (
                    command.action.clone(),
                    command.shortcut.as_ref().map(Shortcut::keystroke),
                )
            })
            .collect();
        let browser = |action| CommandAction::Browser(action);
        assert_eq!(
            bindings,
            [
                (browser(BrowserAction::NewTab), Some("ctrl-t")),
                (browser(BrowserAction::CloseTab), Some("ctrl-w")),
                (browser(BrowserAction::ReopenTab), Some("ctrl-shift-t")),
                (browser(BrowserAction::Reload), Some("ctrl-r")),
                (browser(BrowserAction::Back), Some("alt-left")),
                (browser(BrowserAction::Forward), Some("alt-right")),
                (browser(BrowserAction::EditAddress), Some("ctrl-l")),
                (browser(BrowserAction::NextTab), Some("ctrl-tab")),
                (browser(BrowserAction::PreviousTab), Some("ctrl-shift-tab")),
                (browser(BrowserAction::SavePage), Some("ctrl-d")),
                (browser(BrowserAction::ShowHistory), Some("ctrl-h")),
                (browser(BrowserAction::FindInPage), Some("ctrl-f")),
                (browser(BrowserAction::ShowDownloads), Some("ctrl-j")),
                (browser(BrowserAction::Print), Some("ctrl-p")),
                (browser(BrowserAction::ZoomIn), None),
                (browser(BrowserAction::ZoomOut), None),
                (browser(BrowserAction::ResetZoom), Some("ctrl-0")),
                (browser(BrowserAction::ClearBrowsingData), None),
                (browser(BrowserAction::InstallUpdateFromFile), None),
                (browser(BrowserAction::CheckForUpdates), None),
            ]
        );
        // Keys follow registration order, so both the palette and search agree on them.
        let (key, command) = registry.find("browser.reload").expect("reload command");
        assert_eq!(key.get(), 4);
        assert_eq!(registry.get(key), Some(command));
        assert!(
            registry
                .get(NonZeroU64::new(99).expect("synthetic key"))
                .is_none()
        );
        assert!(
            registry
                .iter()
                .all(|(_, command)| !command.is_plugin_action())
        );
    }

    #[test]
    fn registration_rejects_invalid_or_conflicting_commands_without_changes() {
        let mut registry = CommandRegistry::browser();
        let before = registry.len();
        let mut duplicate = plugin_command("browser.new-tab", None);
        assert_eq!(
            registry.register(duplicate.clone()),
            Err(CommandError::DuplicateId)
        );
        duplicate.id = CommandId::new("synthetic.create").expect("synthetic identifier");
        duplicate.shortcut = Some(Shortcut::new("ctrl-t").expect("synthetic shortcut"));
        assert_eq!(
            registry.register(duplicate.clone()),
            Err(CommandError::DuplicateShortcut)
        );
        duplicate.shortcut = None;
        duplicate.title = "   ".to_owned();
        assert_eq!(
            registry.register(duplicate),
            Err(CommandError::InvalidTitle)
        );
        assert_eq!(registry.len(), before);

        let key = registry
            .register(plugin_command("synthetic.create", Some("ctrl-alt-n")))
            .expect("plugin command registers");
        assert_eq!(key.get(), before as u64 + 1);
        assert!(registry.get(key).is_some_and(Command::is_plugin_action));
    }

    #[test]
    fn registry_holds_exactly_its_cap_and_refuses_the_next_command_whole() {
        // Fill the browser registry to the cap with synthetic plugin commands.
        let mut registry = CommandRegistry::browser();
        for index in registry.len()..CommandRegistry::MAX_COMMANDS {
            let id = format!("synthetic.action-{index}");
            registry
                .register(plugin_command(&id, None))
                .expect("commands up to the cap register");
        }
        assert_eq!(registry.len(), CommandRegistry::MAX_COMMANDS);
        let (last, _) = registry
            .find(&format!(
                "synthetic.action-{}",
                CommandRegistry::MAX_COMMANDS - 1
            ))
            .expect("the last command fits");
        assert_eq!(last.get(), CommandRegistry::MAX_COMMANDS as u64);

        // One more is refused by name and leaves no trace, even a valid, distinct one.
        let extra = plugin_command("synthetic.one-too-many", Some("ctrl-alt-n"));
        assert_eq!(registry.register(extra), Err(CommandError::TooManyCommands));
        assert_eq!(registry.len(), CommandRegistry::MAX_COMMANDS);
        assert!(registry.find("synthetic.one-too-many").is_none());
        let refused_shortcut = Shortcut::new("ctrl-alt-n").expect("synthetic shortcut");
        assert!(
            registry
                .iter()
                .all(|(_, command)| command.shortcut.as_ref() != Some(&refused_shortcut))
        );
        assert_eq!(
            CommandError::TooManyCommands.to_string(),
            "the command registry is full (1024 commands)"
        );
    }

    #[test]
    fn identifiers_and_shortcuts_are_validated() {
        for valid in ["browser.new-tab", "synthetic", "plugin2.action-3"] {
            assert!(CommandId::new(valid).is_ok(), "{valid}");
        }
        for invalid in [
            "",
            "Browser.New",
            "browser..tab",
            "browser.",
            "tab name",
            "é.tab",
        ] {
            assert_eq!(
                CommandId::new(invalid),
                Err(CommandError::InvalidId),
                "{invalid}"
            );
        }
        assert_eq!(CommandId::new("a".repeat(65)), Err(CommandError::InvalidId));
        for invalid in [
            "",
            "ctrl--t",
            "ctrl-",
            "ctrl t",
            "hyper-t",
            "ctrl-ctrl-t",
            "ctrl-shift",
        ] {
            assert_eq!(
                Shortcut::new(invalid),
                Err(CommandError::InvalidShortcut),
                "{invalid}"
            );
        }
        let shortcut = Shortcut::new("ctrl-shift-tab").expect("synthetic shortcut");
        assert_eq!(shortcut.label(), "Ctrl Shift Tab");
        assert_eq!(
            Shortcut::new("alt-left").expect("shortcut").label(),
            "Alt Left"
        );
        // Modifier order and case do not make a different shortcut.
        assert_eq!(Shortcut::new("Shift-Ctrl-T"), Shortcut::new("ctrl-shift-t"));
        assert_eq!(
            Shortcut::new("cmd-k").expect("shortcut").keystroke(),
            "win-k"
        );
    }

    #[test]
    fn reordered_or_reserved_shortcuts_cannot_be_registered() {
        let mut registry = CommandRegistry::browser();
        let before = registry.len();
        for (keys, error) in [
            ("shift-ctrl-t", CommandError::DuplicateShortcut),
            ("ctrl-k", CommandError::ReservedShortcut),
            ("ctrl-l", CommandError::ReservedShortcut),
            ("escape", CommandError::ReservedShortcut),
            ("enter", CommandError::ReservedShortcut),
            ("alt-enter", CommandError::ReservedShortcut),
            ("down", CommandError::ReservedShortcut),
        ] {
            assert_eq!(
                registry.register(plugin_command("synthetic.keys", Some(keys))),
                Err(error),
                "{keys}"
            );
        }
        assert_eq!(registry.len(), before);
    }

    #[test]
    fn enablement_follows_the_browser_context() {
        let home = CommandContext {
            active_tab: true,
            active_page: false,
            tab_count: 1,
        };
        let page = CommandContext {
            active_page: true,
            tab_count: 2,
            ..home
        };
        let empty = CommandContext::default();
        assert!(Enablement::Always.allows(empty));
        assert!(!Enablement::ActiveTab.allows(empty));
        assert!(Enablement::ActiveTab.allows(home));
        assert!(!Enablement::ActivePage.allows(home));
        assert!(Enablement::ActivePage.allows(page));
        assert!(!Enablement::SeveralTabs.allows(home));
        assert!(Enablement::SeveralTabs.allows(page));

        let registry = CommandRegistry::browser();
        let enabled = |context| {
            registry
                .iter()
                .filter(|(_, command)| command.is_enabled(context))
                .count()
        };
        // Always: new tab, reopen, edit address, history, clear browsing data and both updates.
        assert_eq!(enabled(empty), 7);
        assert_eq!(enabled(home), 9);
        assert_eq!(enabled(page), registry.len());
    }
}
