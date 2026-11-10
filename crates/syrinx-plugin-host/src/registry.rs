//! Registers a checked manifest's commands, tab types and feeds without starting the plugin,
//! so the palette lists a plugin's commands before any of its code has run (P2-02).

use std::collections::BTreeMap;
use std::fmt;
use std::num::NonZeroU64;

use syrinx_core::{
    Command, CommandAction, CommandError, CommandId, CommandRegistry, CommandScope, Enablement,
};

use crate::identity::{self, IdentityMismatch, SignedIdentity};
use crate::manifest::{ContributedCommand, PluginManifest, SearchFeed, TabType};

/// Ids no plugin may take, because Syrinx's own commands and names use them.
const RESERVED_IDS: [&str; 2] = ["browser", "syrinx"];

/// Why a plugin was not registered. Nothing it declared becomes visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistrationError {
    /// The package's signed lines name another plugin, version or world than its manifest.
    Identity(IdentityMismatch),
    /// Another registered plugin already has this id.
    DuplicatePlugin(String),
    /// The id belongs to Syrinx itself.
    ReservedId(String),
    /// A command's action is not one of its own plugin's.
    ForeignCommand { plugin: String, command: String },
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
            Self::Identity(mismatch) => write!(formatter, "{mismatch}"),
            Self::DuplicatePlugin(id) => {
                write!(formatter, "a plugin named {id} is already installed")
            }
            Self::ReservedId(id) => write!(formatter, "the plugin id {id} belongs to Syrinx"),
            Self::ForeignCommand { plugin, command } => {
                write!(
                    formatter,
                    "{plugin} declares {command}, which is not its own"
                )
            }
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

/// A registered plugin: its manifest and its commands' palette keys. It has not been started.
#[derive(Debug, Clone)]
pub struct RegisteredPlugin {
    manifest: PluginManifest,
    command_keys: Vec<NonZeroU64>,
}

impl RegisteredPlugin {
    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    /// The keys of this plugin's commands in the palette's registry.
    pub fn command_keys(&self) -> &[NonZeroU64] {
        &self.command_keys
    }

    pub fn tab_types(&self) -> &[TabType] {
        self.manifest.tab_types()
    }

    /// The feeds the search slice (2e) pulls from once the plugin runs.
    pub fn search_feeds(&self) -> &[SearchFeed] {
        self.manifest.search_feeds()
    }
}

/// Every registered plugin, by id.
#[derive(Debug, Clone, Default)]
pub struct PluginRegistry {
    plugins: BTreeMap<String, RegisteredPlugin>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register every command the manifest declares, or none, and record its tab types and
    /// feeds. The package's signed lines must name the same plugin first, so a package signed
    /// for one slot cannot list commands under another plugin's id. Reads only the manifest:
    /// nothing is compiled or instantiated.
    pub fn register(
        &mut self,
        signed: &SignedIdentity,
        manifest: PluginManifest,
        commands: &mut CommandRegistry,
    ) -> Result<&RegisteredPlugin, RegistrationError> {
        identity::check_signed(signed, &manifest).map_err(RegistrationError::Identity)?;
        let id = manifest.id().to_owned();
        self.check_id(&id)?;
        let declared = manifest
            .commands()
            .iter()
            .map(|command| plugin_command(&id, command))
            .collect::<Result<Vec<_>, _>>()?;
        let command_keys = commands.register_all(declared).map_err(|(index, error)| {
            RegistrationError::Command {
                command: manifest.commands()[index].id.clone(),
                error,
            }
        })?;
        let registered = RegisteredPlugin {
            manifest,
            command_keys,
        };
        Ok(self.plugins.entry(id).or_insert(registered))
    }

    pub fn get(&self, id: &str) -> Option<&RegisteredPlugin> {
        self.plugins.get(id)
    }

    pub fn len(&self) -> usize {
        self.plugins.len()
    }

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

/// The palette command for a declared one; its action is the command id, which must be the
/// plugin's own.
fn plugin_command(
    plugin: &str,
    declared: &ContributedCommand,
) -> Result<Command, RegistrationError> {
    let foreign = || RegistrationError::ForeignCommand {
        plugin: plugin.to_owned(),
        command: declared.id.clone(),
    };
    if !declared.id.starts_with(&format!("{plugin}.")) {
        return Err(foreign());
    }
    let id = CommandId::new(declared.id.as_str()).map_err(|_| foreign())?;
    Ok(Command {
        id,
        title: declared.title.clone(),
        keywords: declared.keywords.join(" "),
        scope: CommandScope::Plugin {
            plugin: plugin.to_owned(),
        },
        enablement: Enablement::Always,
        action: CommandAction::Plugin {
            plugin: plugin.to_owned(),
            action: declared.id.clone(),
        },
        shortcut: None,
    })
}
