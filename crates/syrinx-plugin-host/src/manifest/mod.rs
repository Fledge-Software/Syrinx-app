//! Reads and checks `syrinx-plugin.toml`, the static facts Syrinx needs before any plugin code
//! runs (note 10, section 3). Parsing never compiles or instantiates anything.

mod capability;
mod error;
mod raw;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use syrinx_core::{CommandId, CommandRegistry};

pub use capability::CapabilityKind;
pub use error::ManifestError;
use raw::{RawCapability, RawEntry, RawInterface, RawManifest, RawPlugin};

use crate::checks::{CONTRACT_PACKAGE, CONTRACT_VERSION};
use crate::limits;

/// The manifest's file name inside a package.
pub const MANIFEST_FILE: &str = "syrinx-plugin.toml";
/// The one manifest format this host reads.
pub const SCHEMA: i64 = 1;
/// Largest manifest accepted, in bytes.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

const MAX_PLUGIN_ID_BYTES: usize = 32;
const MAX_DESCRIPTION_CHARS: usize = 1_024;
const MAX_AUTHORS: usize = 16;
const MAX_UI_METHODS: usize = 64;
/// Folders one `folder` request may ask for when it gives a number rather than `"many"`.
const MAX_FOLDER_COUNT: i64 = 64;
const ACTIVATIONS: [&str; 3] = ["on-command", "on-view", "on-search"];
const TIERS: [&str; 2] = ["catalogue", "full-text"];
const FOLDER_ACCESS: [&str; 2] = ["read", "read-write"];
/// The one `@unstable` feature the contract has, and the only one a manifest may name.
const HOME_WIDGETS: &str = "home-widgets";

/// The worlds this host runs, as a manifest spells them.
pub fn supported_worlds() -> Vec<String> {
    vec![format!("{CONTRACT_PACKAGE}/plugin@{CONTRACT_VERSION}")]
}

/// What discovery knows about where a manifest came from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiscoveryOptions {
    /// The owner's unsigned developer mode (2d); off for every installed or bundled plugin.
    pub developer_mode: bool,
}

/// The `[plugin]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    pub repository: String,
    pub licence: String,
    pub min_syrinx: String,
}

/// The `[entry]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub module: String,
    pub ui: Option<String>,
    pub activation: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributedCommand {
    pub id: String,
    pub title: String,
    pub keywords: Vec<String>,
    /// Choosing it shows a declarative form instead of running it.
    pub form: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabType {
    pub id: String,
    pub title: String,
    /// The only view-message methods the host relays to this kind of tab.
    pub ui_methods: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchFeed {
    pub id: String,
    pub title: String,
    pub tiers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Widget {
    pub id: String,
    pub title: String,
}

/// One `[[capabilities]]` request, with its kind's scope fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRequest {
    pub kind: CapabilityKind,
    pub reason: String,
    pub required: bool,
    pub scope: Scope,
}

/// The scope fields a kind takes; every other kind takes none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    None,
    Folder {
        writable: bool,
        /// `None` for `count = "many"`.
        count: Option<u32>,
        watch: bool,
    },
    Index {
        feeds: Vec<String>,
    },
}

/// A manifest that passed every rule. Only `PluginManifest::parse` makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginManifest {
    plugin: PluginInfo,
    world: String,
    unstable: Vec<String>,
    entry: Entry,
    commands: Vec<ContributedCommand>,
    tab_types: Vec<TabType>,
    search_feeds: Vec<SearchFeed>,
    widgets: Vec<Widget>,
    capabilities: Vec<CapabilityRequest>,
}

impl PluginManifest {
    /// Check `syrinx-plugin.toml`'s bytes: schema and world first, then every other rule.
    pub fn parse(bytes: &[u8], options: DiscoveryOptions) -> Result<Self, ManifestError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(ManifestError::TooLarge);
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|_| ManifestError::Syntax("the manifest is not UTF-8".into()))?;
        let mut table: toml::Table = toml::from_str(text).map_err(syntax)?;
        check_schema(&table)?;
        check_world(&table)?;
        table.remove("schema");
        let raw: RawManifest = table.try_into().map_err(syntax)?;
        Self::from_raw(raw, options)
    }

    fn from_raw(raw: RawManifest, options: DiscoveryOptions) -> Result<Self, ManifestError> {
        let plugin = plugin_info(raw.plugin)?;
        let world = raw.interface.world.clone();
        let unstable = unstable_features(raw.interface, options)?;
        let entry = entry(raw.entry)?;
        let mut contributions = Contributions::new(&plugin.id);
        let commands = contributions.commands(raw.contributes.commands)?;
        let tab_types = contributions.tab_types(raw.contributes.tab_types)?;
        let search_feeds = contributions.search_feeds(raw.contributes.search_feeds)?;
        let widgets = widgets(&mut contributions, raw.contributes.widgets, &unstable)?;
        let capabilities = capabilities(raw.capabilities, &search_feeds)?;
        Ok(Self {
            plugin,
            world,
            unstable,
            entry,
            commands,
            tab_types,
            search_feeds,
            widgets,
            capabilities,
        })
    }

    pub fn id(&self) -> &str {
        &self.plugin.id
    }

    pub fn version(&self) -> &str {
        &self.plugin.version
    }

    pub fn world(&self) -> &str {
        &self.world
    }

    pub fn plugin(&self) -> &PluginInfo {
        &self.plugin
    }

    pub fn unstable_features(&self) -> &[String] {
        &self.unstable
    }

    pub fn entry(&self) -> &Entry {
        &self.entry
    }

    pub fn commands(&self) -> &[ContributedCommand] {
        &self.commands
    }

    pub fn tab_types(&self) -> &[TabType] {
        &self.tab_types
    }

    pub fn search_feeds(&self) -> &[SearchFeed] {
        &self.search_feeds
    }

    pub fn widgets(&self) -> &[Widget] {
        &self.widgets
    }

    pub fn capabilities(&self) -> &[CapabilityRequest] {
        &self.capabilities
    }

    /// Whether the manifest requests `kind`, granted or not.
    pub fn requests(&self, kind: CapabilityKind) -> bool {
        self.capabilities.iter().any(|request| request.kind == kind)
    }
}

fn syntax(error: toml::de::Error) -> ManifestError {
    ManifestError::Syntax(error.message().to_owned())
}

fn invalid(field: impl Into<String>, reason: impl Into<String>) -> ManifestError {
    ManifestError::InvalidField {
        field: field.into(),
        reason: reason.into(),
    }
}

/// Refuse any `schema` but 1 before reading anything else.
fn check_schema(table: &toml::Table) -> Result<(), ManifestError> {
    match table.get("schema") {
        Some(toml::Value::Integer(SCHEMA)) => Ok(()),
        Some(toml::Value::Integer(other)) => {
            Err(ManifestError::UnsupportedSchema(other.to_string()))
        }
        Some(other) => Err(ManifestError::UnsupportedSchema(format!(
            "a {}",
            other.type_str()
        ))),
        None => Err(ManifestError::UnsupportedSchema("(missing)".into())),
    }
}

/// Refuse a world this host does not run, before the rest of the manifest is read.
fn check_world(table: &toml::Table) -> Result<(), ManifestError> {
    let world = table
        .get("interface")
        .and_then(toml::Value::as_table)
        .and_then(|interface| interface.get("world"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| ManifestError::UnsupportedWorld("(missing)".into()))?;
    if supported_worlds()
        .iter()
        .any(|supported| supported == world)
    {
        return Ok(());
    }
    Err(ManifestError::UnsupportedWorld(world.to_owned()))
}

fn plugin_info(raw: RawPlugin) -> Result<PluginInfo, ManifestError> {
    check_plugin_id(&raw.id)?;
    text("plugin.name", &raw.name, limits::MAX_TITLE_CHARS)?;
    canonical_version("plugin.version", &raw.version)?;
    text(
        "plugin.description",
        &raw.description,
        MAX_DESCRIPTION_CHARS,
    )?;
    bounded_list("plugin.authors", &raw.authors, MAX_AUTHORS)?;
    for author in &raw.authors {
        text("plugin.authors", author, limits::MAX_LABEL_CHARS)?;
    }
    text(
        "plugin.repository",
        &raw.repository,
        limits::MAX_ADDRESS_BYTES,
    )?;
    text("plugin.licence", &raw.licence, limits::MAX_LABEL_CHARS)?;
    canonical_version("plugin.min-syrinx", &raw.min_syrinx)?;
    Ok(PluginInfo {
        id: raw.id,
        name: raw.name,
        version: raw.version,
        description: raw.description,
        authors: raw.authors,
        repository: raw.repository,
        licence: raw.licence,
        min_syrinx: raw.min_syrinx,
    })
}

/// A plugin id is one segment of lowercase letters, digits and hyphens.
fn check_plugin_id(id: &str) -> Result<(), ManifestError> {
    let one_segment = !id.contains('.') && id.len() <= MAX_PLUGIN_ID_BYTES;
    if one_segment && CommandId::new(id).is_ok() {
        return Ok(());
    }
    Err(invalid(
        "plugin.id",
        format!(
            "{id:?} is not lowercase letters, digits and hyphens of at most {MAX_PLUGIN_ID_BYTES} bytes"
        ),
    ))
}

/// Non-blank text of at most `max` characters.
fn text(field: &str, value: &str, max: usize) -> Result<(), ManifestError> {
    if value.trim().is_empty() {
        return Err(invalid(field, "is empty"));
    }
    if value.chars().count() > max {
        return Err(invalid(field, format!("is longer than {max} characters")));
    }
    Ok(())
}

fn bounded_list<T>(field: &str, values: &[T], max: usize) -> Result<(), ManifestError> {
    if values.len() > max {
        return Err(invalid(field, format!("lists more than {max} entries")));
    }
    Ok(())
}

/// `major.minor.patch` in canonical decimals, the form `docs/update-format.md` signs.
fn canonical_version(field: &str, value: &str) -> Result<(), ManifestError> {
    let parts: Vec<&str> = value.split('.').collect();
    let canonical = |part: &&str| {
        !part.is_empty()
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && (*part == "0" || !part.starts_with('0'))
            && part.parse::<u64>().is_ok()
    };
    if parts.len() == 3 && parts.iter().all(canonical) {
        return Ok(());
    }
    Err(invalid(
        field,
        format!("{value:?} is not a version such as 1.2.3"),
    ))
}

/// Unstable features are named only by a developer-mode plugin, and only `home-widgets`.
fn unstable_features(
    raw: RawInterface,
    options: DiscoveryOptions,
) -> Result<Vec<String>, ManifestError> {
    if let Some(unknown) = raw.unstable.iter().find(|name| *name != HOME_WIDGETS) {
        return Err(invalid(
            "interface.unstable",
            format!("{unknown:?} is not an unstable feature of the contract"),
        ));
    }
    if !raw.unstable.is_empty() && !options.developer_mode {
        return Err(invalid(
            "interface.unstable",
            "only a developer-mode plugin may use unstable features",
        ));
    }
    Ok(raw.unstable)
}

fn entry(raw: RawEntry) -> Result<Entry, ManifestError> {
    if !is_module_name(&raw.module) {
        return Err(invalid(
            "entry.module",
            format!(
                "{:?} is not a .wasm file at the package's top level",
                raw.module
            ),
        ));
    }
    if let Some(ui) = &raw.ui
        && !is_ui_folder(ui)
    {
        return Err(invalid(
            "entry.ui",
            format!("{ui:?} is not a relative folder such as \"ui/\""),
        ));
    }
    unique_choices("entry.activation", &raw.activation, &ACTIVATIONS)?;
    Ok(Entry {
        module: raw.module,
        ui: raw.ui,
        activation: raw.activation,
    })
}

/// A plain file name ending `.wasm`: never a path, and never precompiled `.cwasm`.
fn is_module_name(module: &str) -> bool {
    let Some(stem) = module.strip_suffix(".wasm") else {
        return false;
    };
    !stem.is_empty() && stem.bytes().all(is_name_byte)
}

/// A relative folder of plain names, ending with `/`.
fn is_ui_folder(ui: &str) -> bool {
    let Some(folder) = ui.strip_suffix('/') else {
        return false;
    };
    !folder.is_empty()
        && folder.split('/').all(|part| {
            !part.is_empty() && part != "." && part != ".." && part.bytes().all(is_name_byte)
        })
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
}

/// Each value one of `allowed`, none repeated.
fn unique_choices(field: &str, values: &[String], allowed: &[&str]) -> Result<(), ManifestError> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !allowed.contains(&value.as_str()) {
            return Err(invalid(
                field,
                format!("{value:?} is not one of {}", allowed.join(", ")),
            ));
        }
        if !seen.insert(value) {
            return Err(invalid(field, format!("{value:?} is listed twice")));
        }
    }
    Ok(())
}

/// Checks contributed ids: each starts with the plugin's prefix and none repeats across kinds.
struct Contributions {
    prefix: String,
    seen: BTreeSet<String>,
}

impl Contributions {
    fn new(plugin: &str) -> Self {
        Self {
            prefix: format!("{plugin}."),
            seen: BTreeSet::new(),
        }
    }

    fn id(&mut self, field: &str, id: &str) -> Result<(), ManifestError> {
        if CommandId::new(id).is_err() {
            return Err(invalid(
                field,
                format!("{id:?} is not dotted lowercase ASCII of at most 64 bytes"),
            ));
        }
        if !id.starts_with(&self.prefix) {
            return Err(ManifestError::MissingPrefix { id: id.to_owned() });
        }
        if !self.seen.insert(id.to_owned()) {
            return Err(ManifestError::DuplicateContribution { id: id.to_owned() });
        }
        Ok(())
    }

    fn commands(
        &mut self,
        raw: Vec<raw::RawCommand>,
    ) -> Result<Vec<ContributedCommand>, ManifestError> {
        let limit = CommandRegistry::MAX_COMMANDS_PER_PLUGIN;
        if raw.len() > limit {
            return Err(ManifestError::TooManyCommands {
                declared: raw.len(),
                limit,
            });
        }
        raw.into_iter()
            .map(|command| {
                self.id("contributes.commands.id", &command.id)?;
                text(&command.id, &command.title, limits::MAX_TITLE_CHARS)?;
                keywords(&command.id, &command.keywords)?;
                Ok(ContributedCommand {
                    id: command.id,
                    title: command.title,
                    keywords: command.keywords,
                    form: command.form,
                })
            })
            .collect()
    }

    fn tab_types(&mut self, raw: Vec<raw::RawTabType>) -> Result<Vec<TabType>, ManifestError> {
        raw.into_iter()
            .map(|tab_type| {
                self.id("contributes.tab-types.id", &tab_type.id)?;
                text(&tab_type.id, &tab_type.title, limits::MAX_TITLE_CHARS)?;
                ui_methods(&tab_type.id, &tab_type.ui_methods)?;
                Ok(TabType {
                    id: tab_type.id,
                    title: tab_type.title,
                    ui_methods: tab_type.ui_methods,
                })
            })
            .collect()
    }

    fn search_feeds(
        &mut self,
        raw: Vec<raw::RawSearchFeed>,
    ) -> Result<Vec<SearchFeed>, ManifestError> {
        raw.into_iter()
            .map(|feed| {
                self.id("contributes.search-feeds.id", &feed.id)?;
                text(&feed.id, &feed.title, limits::MAX_TITLE_CHARS)?;
                if feed.tiers.is_empty() {
                    return Err(invalid(&feed.id, "names no search tier"));
                }
                unique_choices(&feed.id, &feed.tiers, &TIERS)?;
                Ok(SearchFeed {
                    id: feed.id,
                    title: feed.title,
                    tiers: feed.tiers,
                })
            })
            .collect()
    }
}

fn keywords(field: &str, keywords: &[String]) -> Result<(), ManifestError> {
    bounded_list(field, keywords, limits::MAX_KEYWORDS)?;
    for keyword in keywords {
        text(field, keyword, limits::MAX_KEYWORD_CHARS)?;
    }
    Ok(())
}

/// Each method is one id segment, such as `open-note`, listed once.
fn ui_methods(field: &str, methods: &[String]) -> Result<(), ManifestError> {
    bounded_list(field, methods, MAX_UI_METHODS)?;
    let mut seen = BTreeSet::new();
    for method in methods {
        if method.contains('.') || CommandId::new(method.as_str()).is_err() {
            return Err(invalid(field, format!("{method:?} is not a method name")));
        }
        if !seen.insert(method) {
            return Err(invalid(field, format!("{method:?} is listed twice")));
        }
    }
    Ok(())
}

/// Widgets stay behind `@unstable(feature = home-widgets)` until #31.
fn widgets(
    contributions: &mut Contributions,
    raw: Vec<raw::RawWidget>,
    unstable: &[String],
) -> Result<Vec<Widget>, ManifestError> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    if !unstable.iter().any(|feature| feature == HOME_WIDGETS) {
        return Err(ManifestError::WidgetsNotAllowed);
    }
    raw.into_iter()
        .map(|widget| {
            contributions.id("contributes.widgets.id", &widget.id)?;
            text(&widget.id, &widget.title, limits::MAX_TITLE_CHARS)?;
            Ok(Widget {
                id: widget.id,
                title: widget.title,
            })
        })
        .collect()
}

fn capabilities(
    raw: Vec<RawCapability>,
    feeds: &[SearchFeed],
) -> Result<Vec<CapabilityRequest>, ManifestError> {
    let mut seen = BTreeSet::new();
    raw.into_iter()
        .map(|request| {
            let request = capability(request, feeds)?;
            if !seen.insert(request.kind) {
                return Err(ManifestError::DuplicateCapability(request.kind));
            }
            Ok(request)
        })
        .collect()
}

/// Kind, reason and trust first, then the kind's own scope fields.
fn capability(
    raw: RawCapability,
    feeds: &[SearchFeed],
) -> Result<CapabilityRequest, ManifestError> {
    let kind = CapabilityKind::from_name(&raw.kind)
        .ok_or_else(|| ManifestError::UnknownCapability(raw.kind.clone()))?;
    let reason = raw
        .reason
        .clone()
        .filter(|reason| !reason.trim().is_empty())
        .ok_or(ManifestError::MissingReason(kind))?;
    if reason.chars().count() > limits::MAX_MESSAGE_CHARS {
        return Err(ManifestError::InvalidScope {
            kind,
            reason: format!(
                "its reason is longer than {} characters",
                limits::MAX_MESSAGE_CHARS
            ),
        });
    }
    check_trust(kind, raw.trust.as_deref())?;
    let scope = scope(kind, &raw, feeds)?;
    Ok(CapabilityRequest {
        kind,
        reason,
        required: raw.required,
        scope,
    })
}

/// The host decides trust: a full-trust kind must say so, and nothing else may.
fn check_trust(kind: CapabilityKind, trust: Option<&str>) -> Result<(), ManifestError> {
    match (kind.is_full_trust(), trust) {
        (true, Some("full")) | (false, None) => Ok(()),
        (true, _) => Err(ManifestError::FullTrustNotDeclared(kind)),
        (false, Some(_)) => Err(ManifestError::TrustMismatch(kind)),
    }
}

fn scope(
    kind: CapabilityKind,
    raw: &RawCapability,
    feeds: &[SearchFeed],
) -> Result<Scope, ManifestError> {
    match kind {
        CapabilityKind::Folder => folder_scope(raw),
        CapabilityKind::Index => index_scope(raw, feeds),
        _ => no_scope(kind, raw),
    }
}

fn scope_error(kind: CapabilityKind, reason: impl Into<String>) -> ManifestError {
    ManifestError::InvalidScope {
        kind,
        reason: reason.into(),
    }
}

/// `access` is required; `count` is a small number or `"many"`; `watch` asks for change events.
fn folder_scope(raw: &RawCapability) -> Result<Scope, ManifestError> {
    let kind = CapabilityKind::Folder;
    if raw.feeds.is_some() {
        return Err(scope_error(kind, "takes no feeds"));
    }
    let writable = match raw.access.as_deref() {
        Some("read") => false,
        Some("read-write") => true,
        _ => {
            return Err(scope_error(
                kind,
                format!("access must be one of {}", FOLDER_ACCESS.join(", ")),
            ));
        }
    };
    let count_error = || {
        scope_error(
            kind,
            format!("count must be 1 to {MAX_FOLDER_COUNT}, or \"many\""),
        )
    };
    let count = match &raw.count {
        None => Some(1),
        Some(toml::Value::String(many)) if many == "many" => None,
        Some(toml::Value::Integer(count)) if (1..=MAX_FOLDER_COUNT).contains(count) => {
            Some(u32::try_from(*count).map_err(|_| count_error())?)
        }
        Some(_) => return Err(count_error()),
    };
    Ok(Scope::Folder {
        writable,
        count,
        watch: raw.watch.unwrap_or(false),
    })
}

/// `feeds` names this plugin's own declared search feeds.
fn index_scope(raw: &RawCapability, feeds: &[SearchFeed]) -> Result<Scope, ManifestError> {
    let kind = CapabilityKind::Index;
    if raw.access.is_some() || raw.count.is_some() || raw.watch.is_some() {
        return Err(scope_error(kind, "takes only feeds"));
    }
    let requested = raw.feeds.clone().unwrap_or_default();
    if requested.is_empty() {
        return Err(scope_error(kind, "names no feed"));
    }
    let declared = |name: &String| feeds.iter().any(|feed| &feed.id == name);
    if let Some(unknown) = requested.iter().find(|name| !declared(name)) {
        return Err(scope_error(
            kind,
            format!("{unknown} is not one of this plugin's search feeds"),
        ));
    }
    Ok(Scope::Index { feeds: requested })
}

fn no_scope(kind: CapabilityKind, raw: &RawCapability) -> Result<Scope, ManifestError> {
    let any =
        raw.access.is_some() || raw.count.is_some() || raw.watch.is_some() || raw.feeds.is_some();
    if any {
        return Err(scope_error(kind, "takes no scope fields"));
    }
    Ok(Scope::None)
}
