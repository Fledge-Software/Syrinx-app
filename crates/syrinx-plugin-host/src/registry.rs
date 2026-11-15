//! Registers a signed manifest's commands, tab types and feeds without compiling or starting the
//! plugin, so the palette lists a plugin's commands before any of its code has run (P2-02).

use std::collections::BTreeMap;
use std::fmt;
use std::num::NonZeroU64;

use syrinx_core::{
    Command, CommandAction, CommandError, CommandRegistry, CommandScope, Enablement,
};

use crate::identity::SignedManifest;
use crate::manifest::{ContributedCommand, PluginManifest, SearchFeed, TabType};

/// Ids no plugin may take, because Syrinx's own commands and names use them.
const RESERVED_IDS: [&str; 2] = ["browser", "syrinx"];

/// Why a plugin was not registered. Nothing it declared becomes visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistrationError {
    /// Another registered plugin already has this id.
    DuplicatePlugin(String),
    /// The id belongs to Syrinx itself.
    ReservedId(String),
    /// The palette's registry refused a command (a duplicate, a full registry, the quota).
    Command {
        command: String,
        error: CommandError,
    },
    /// A view request named a tab type its plugin does not declare.
    UnknownTabType { plugin: String, tab_type: String },
    /// No plugin with this id is registered.
    UnknownPlugin(String),
}

impl fmt::Display for RegistrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicatePlugin(id) => {
                write!(formatter, "a plugin named {id} is already installed")
            }
            Self::ReservedId(id) => write!(formatter, "the plugin id {id} belongs to Syrinx"),
            Self::Command { command, error } => {
                write!(formatter, "command {command} was refused: {error}")
            }
            Self::UnknownTabType { plugin, tab_type } => {
                write!(formatter, "{plugin} declares no tab type {tab_type}")
            }
            Self::UnknownPlugin(id) => write!(formatter, "no plugin named {id} is registered"),
        }
    }
}

impl std::error::Error for RegistrationError {}

/// A registered plugin: its manifest and its commands' palette keys. Nothing of it has been
/// compiled or started.
#[derive(Debug)]
pub struct RegisteredPlugin {
    manifest: PluginManifest,
    command_keys: Vec<NonZeroU64>,
}

impl RegisteredPlugin {
    /// The checked manifest.
    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    /// The keys of this plugin's commands in the palette's registry.
    pub fn command_keys(&self) -> &[NonZeroU64] {
        &self.command_keys
    }

    /// The tab types its views may be opened as.
    pub fn tab_types(&self) -> &[TabType] {
        self.manifest.tab_types()
    }

    /// The feeds the search slice (2e) pulls from once the plugin runs.
    pub fn search_feeds(&self) -> &[SearchFeed] {
        self.manifest.search_feeds()
    }
}

/// Every registered plugin, by id.
#[derive(Debug, Default)]
pub struct PluginRegistry {
    plugins: BTreeMap<String, RegisteredPlugin>,
}

impl PluginRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register every command a signed manifest declares, or none, and record its tab types
    /// and feeds. Only `check_signed` makes a `SignedManifest`, so the package's signed lines
    /// already name this plugin. Reads only the manifest: nothing is compiled or instantiated.
    pub fn register(
        &mut self,
        signed: SignedManifest,
        commands: &mut CommandRegistry,
    ) -> Result<&RegisteredPlugin, RegistrationError> {
        let manifest = signed.into_manifest();
        let id = manifest.id().to_owned();
        self.check_id(&id)?;
        let declared = manifest
            .commands()
            .iter()
            .map(|command| plugin_command(&id, command))
            .collect();
        let command_keys = commands.register_all(declared).map_err(|(index, error)| {
            RegistrationError::Command {
                command: manifest.commands()[index].id.as_str().to_owned(),
                error,
            }
        })?;
        let registered = RegisteredPlugin {
            manifest,
            command_keys,
        };
        Ok(self.plugins.entry(id).or_insert(registered))
    }

    /// The registered plugin with this id.
    pub fn get(&self, id: &str) -> Option<&RegisteredPlugin> {
        self.plugins.get(id)
    }

    /// How many plugins are registered.
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    /// Whether no plugin is registered.
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// The tab type a view request names, refusing one its plugin did not declare.
    pub fn tab_type(&self, plugin: &str, tab_type: &str) -> Result<&TabType, RegistrationError> {
        let registered = self
            .get(plugin)
            .ok_or_else(|| RegistrationError::UnknownPlugin(plugin.to_owned()))?;
        registered
            .tab_types()
            .iter()
            .find(|declared| declared.id == tab_type)
            .ok_or_else(|| RegistrationError::UnknownTabType {
                plugin: plugin.to_owned(),
                tab_type: tab_type.to_owned(),
            })
    }

    /// Refuse an id Syrinx reserves or another plugin already has.
    fn check_id(&self, id: &str) -> Result<(), RegistrationError> {
        if RESERVED_IDS.contains(&id) {
            return Err(RegistrationError::ReservedId(id.to_owned()));
        }
        if self.plugins.contains_key(id) {
            return Err(RegistrationError::DuplicatePlugin(id.to_owned()));
        }
        Ok(())
    }
}

/// The palette command for a declared one. The manifest already proved its id carries the
/// plugin's prefix, and the core registry refuses an action owned by another plugin.
fn plugin_command(plugin: &str, declared: &ContributedCommand) -> Command {
    Command {
        id: declared.id.clone(),
        title: declared.title.clone(),
        keywords: declared.keywords.join(" "),
        scope: CommandScope::Plugin {
            plugin: plugin.to_owned(),
        },
        enablement: Enablement::Always,
        action: CommandAction::Plugin {
            plugin: plugin.to_owned(),
            action: declared.id.as_str().to_owned(),
        },
        shortcut: None,
    }
}
