//! `syrinx-plugin.toml` exactly as written, before any rule is checked. Unknown keys are
//! refused here, so a misspelt field is an error rather than a silently ignored line.

use serde::Deserialize;

/// Everything but `schema`, which `check_schema` has already read and removed.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawManifest {
    pub plugin: RawPlugin,
    pub interface: RawInterface,
    pub entry: RawEntry,
    #[serde(default)]
    pub contributes: RawContributes,
    #[serde(default)]
    pub capabilities: Vec<RawCapability>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    pub repository: String,
    pub licence: String,
    #[serde(rename = "min-syrinx")]
    pub min_syrinx: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawInterface {
    pub world: String,
    /// `@unstable` WIT features the plugin was built with, such as `home-widgets`.
    #[serde(default)]
    pub unstable: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawEntry {
    pub module: String,
    pub ui: Option<String>,
    #[serde(default)]
    pub activation: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RawContributes {
    #[serde(default)]
    pub commands: Vec<RawCommand>,
    #[serde(default, rename = "tab-types")]
    pub tab_types: Vec<RawTabType>,
    #[serde(default, rename = "search-feeds")]
    pub search_feeds: Vec<RawSearchFeed>,
    #[serde(default)]
    pub widgets: Vec<RawWidget>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawCommand {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub form: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTabType {
    pub id: String,
    pub title: String,
    #[serde(default, rename = "ui-methods")]
    pub ui_methods: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSearchFeed {
    pub id: String,
    pub title: String,
    pub tiers: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawWidget {
    pub id: String,
    pub title: String,
}

/// One `[[capabilities]]` entry; the scope fields a kind allows are checked later.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawCapability {
    pub kind: String,
    pub reason: Option<String>,
    #[serde(default)]
    pub required: bool,
    pub trust: Option<String>,
    pub access: Option<String>,
    pub count: Option<toml::Value>,
    pub watch: Option<bool>,
    pub feeds: Option<Vec<String>>,
}
