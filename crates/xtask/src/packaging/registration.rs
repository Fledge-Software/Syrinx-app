//! The one list of per-user registry values that makes Atlas choosable as a browser (P1-37).
//!
//! `packaging/atlas-registration.ps1` and the design doc's key table are rendered from this file.

/// Product name shown in Default apps, the Start menu and Installed apps.
pub const PRODUCT: &str = "Atlas";

/// Registry name for every key Atlas owns; distinctive so another "Atlas" is never taken over.
pub const REGISTRY_NAME: &str = "AtlasBrowser";

/// Executable copied into the package and registered with Windows.
pub const EXECUTABLE: &str = "atlas-app.exe";

/// Program folder relative to `%LOCALAPPDATA%`, kept apart from every data folder.
pub const INSTALL_FOLDER: &str = r"Programs\Atlas";

/// Data folder of the installed app, relative to `%LOCALAPPDATA%` (atlas-app's `installed` feature).
pub const DATA_FOLDER: &str = r"Atlas\data";

/// Development and preview runs' data folder; uninstall only offers it behind a separate switch.
pub const PREVIEW_DATA_FOLDER: &str = "AtlasProductPreview";

/// Marker Windows passes before a link; P1-34 accepts a link only as the single argument after it.
pub const LINK_MARKER: &str = "--";

/// Whether `atlas-app.exe` accepts a link after `LINK_MARKER`; true since P1-34 merged.
pub const ACCEPTS_OPEN_URL: bool = true;

/// Generated PowerShell file shared by the install, register and unregister scripts.
pub const GENERATED_SCRIPT: &str = "atlas-registration.ps1";

/// Value written into every key root Atlas owns; keys without it are never changed or deleted.
pub const OWNER_MARKER_NAME: &str = "AtlasPackagingOwner";
pub const OWNER_MARKER_DATA: &str = "atlas-packaging-v1";

const START_MENU_INTERNET: &str = r"Software\Clients\StartMenuInternet\AtlasBrowser";
const CAPABILITIES: &str = r"Software\Clients\StartMenuInternet\AtlasBrowser\Capabilities";
const URL_PROG_ID: &str = r"Software\Classes\AtlasBrowserURL";
const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\AtlasBrowser";
const REGISTERED_APPLICATIONS: &str = r"Software\RegisteredApplications";

/// Registry data. Strings may hold `{exe}`, `{dir}` and `{version}`, filled in by the script.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Data {
    String(&'static str),
    Dword(u32),
}

/// One value under `HKEY_CURRENT_USER`. An empty name is the key's default value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegistryValue {
    pub key: &'static str,
    pub name: &'static str,
    pub data: Data,
}

const fn string(key: &'static str, name: &'static str, data: &'static str) -> RegistryValue {
    RegistryValue {
        key,
        name,
        data: Data::String(data),
    }
}

const fn dword(key: &'static str, name: &'static str, data: u32) -> RegistryValue {
    RegistryValue {
        key,
        name,
        data: Data::Dword(data),
    }
}

const fn marker(key: &'static str) -> RegistryValue {
    string(key, OWNER_MARKER_NAME, OWNER_MARKER_DATA)
}

/// Key trees `unregister-atlas.ps1` deletes whole, each only when it carries the owner marker.
pub const OWNED_KEYS: [&str; 3] = [START_MENU_INTERNET, URL_PROG_ID, UNINSTALL];

/// Single values in keys other software shares, removed only while they still hold Atlas's data.
pub const SHARED_VALUES: [RegistryValue; 1] =
    [string(REGISTERED_APPLICATIONS, REGISTRY_NAME, CAPABILITIES)];

/// Every value `register-atlas.ps1` writes under owned keys, all under `HKEY_CURRENT_USER`.
pub const VALUES: [RegistryValue; 26] = [
    // The browser entry Windows lists in Default apps.
    marker(START_MENU_INTERNET),
    string(START_MENU_INTERNET, "", "Atlas"),
    string(
        r"Software\Clients\StartMenuInternet\AtlasBrowser\DefaultIcon",
        "",
        "{exe},0",
    ),
    string(
        r"Software\Clients\StartMenuInternet\AtlasBrowser\shell\open\command",
        "",
        "\"{exe}\"",
    ),
    // What Atlas can handle, which Default apps offers to the owner.
    string(CAPABILITIES, "ApplicationName", "Atlas"),
    string(CAPABILITIES, "ApplicationDescription", "Atlas web browser"),
    string(CAPABILITIES, "ApplicationIcon", "{exe},0"),
    string(
        r"Software\Clients\StartMenuInternet\AtlasBrowser\Capabilities\StartMenu",
        "StartMenuInternet",
        "AtlasBrowser",
    ),
    string(
        r"Software\Clients\StartMenuInternet\AtlasBrowser\Capabilities\URLAssociations",
        "http",
        "AtlasBrowserURL",
    ),
    string(
        r"Software\Clients\StartMenuInternet\AtlasBrowser\Capabilities\URLAssociations",
        "https",
        "AtlasBrowserURL",
    ),
    // The ProgID that opens a link in Atlas once the owner chooses it.
    marker(URL_PROG_ID),
    string(URL_PROG_ID, "", "Atlas URL"),
    string(URL_PROG_ID, "FriendlyTypeName", "Atlas URL"),
    string(
        r"Software\Classes\AtlasBrowserURL\Application",
        "ApplicationName",
        "Atlas",
    ),
    string(
        r"Software\Classes\AtlasBrowserURL\Application",
        "ApplicationDescription",
        "Atlas web browser",
    ),
    string(
        r"Software\Classes\AtlasBrowserURL\Application",
        "ApplicationIcon",
        "{exe},0",
    ),
    string(
        r"Software\Classes\AtlasBrowserURL\DefaultIcon",
        "",
        "{exe},0",
    ),
    string(
        r"Software\Classes\AtlasBrowserURL\shell\open\command",
        "",
        "\"{exe}\" -- \"%1\"",
    ),
    // The Installed apps entry, whose Uninstall button runs the unregister script.
    marker(UNINSTALL),
    string(UNINSTALL, "DisplayName", "Atlas"),
    string(UNINSTALL, "DisplayVersion", "{version}"),
    string(UNINSTALL, "DisplayIcon", "{exe},0"),
    string(UNINSTALL, "InstallLocation", "{dir}"),
    string(
        UNINSTALL,
        "UninstallString",
        "powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File \"{dir}\\unregister-atlas.ps1\" -Uninstall -PauseAtEnd",
    ),
    dword(UNINSTALL, "NoModify", 1),
    dword(UNINSTALL, "NoRepair", 1),
];

/// Render the generated PowerShell data file, with CRLF line endings for Windows PowerShell.
pub fn render_script(package_files: &[&str]) -> String {
    let mut lines = vec![
        "# Generated from crates/xtask/src/packaging/registration.rs.".to_owned(),
        "# Do not edit by hand: change the Rust table, then run `cargo xtask write-registration`."
            .to_owned(),
        "# Every key is under HKEY_CURRENT_USER. Nothing here sets the default browser.".to_owned(),
        String::new(),
        format!("$AtlasProduct = {}", quote(PRODUCT)),
        format!("$AtlasRegistryName = {}", quote(REGISTRY_NAME)),
        format!("$AtlasExecutable = {}", quote(EXECUTABLE)),
        format!("$AtlasInstallFolder = {}", quote(INSTALL_FOLDER)),
        format!("$AtlasDataFolder = {}", quote(DATA_FOLDER)),
        format!("$AtlasPreviewDataFolder = {}", quote(PREVIEW_DATA_FOLDER)),
        format!("$AtlasPackageFiles = @({})", quoted_list(package_files)),
        format!("$AtlasLinkMarker = {}", quote(LINK_MARKER)),
        format!("$AtlasAcceptsOpenUrl = ${ACCEPTS_OPEN_URL}"),
        format!("$AtlasOwnerMarkerName = {}", quote(OWNER_MARKER_NAME)),
        format!("$AtlasOwnerMarkerData = {}", quote(OWNER_MARKER_DATA)),
        format!("$AtlasUninstallKey = {}", quote(UNINSTALL)),
        String::new(),
        "$AtlasRegistryValues = @(".to_owned(),
    ];
    lines.extend(VALUES.iter().map(render_value));
    lines.push(")".to_owned());
    lines.push(String::new());
    lines.push(format!("$AtlasOwnedKeys = @({})", quoted_list(&OWNED_KEYS)));
    lines.push("$AtlasSharedValues = @(".to_owned());
    lines.extend(SHARED_VALUES.iter().map(render_value));
    lines.push(")".to_owned());

    let mut script = lines.join("\r\n");
    script.push_str("\r\n");
    script
}

/// Render the design doc's Markdown key table, so the doc cannot drift from the scripts.
pub fn render_doc_table() -> String {
    let mut lines = vec![
        "| Key under `HKCU\\` | Value | Data |".to_owned(),
        "| --- | --- | --- |".to_owned(),
    ];
    lines.extend(VALUES.iter().chain(SHARED_VALUES.iter()).map(|value| {
        let name = if value.name.is_empty() {
            "(default)"
        } else {
            value.name
        };
        let data = match value.data {
            Data::String("") => "(empty)".to_owned(),
            Data::String(text) => format!("`{text}`"),
            Data::Dword(number) => format!("DWORD {number}"),
        };
        format!("| `{}` | `{name}` | {data} |", value.key)
    }));
    lines.join("\n")
}

fn render_value(value: &RegistryValue) -> String {
    // Write each value as one hashtable row the scripts can apply or remove.
    let (kind, data) = match value.data {
        Data::String(text) => ("String", quote(text)),
        Data::Dword(number) => ("DWord", number.to_string()),
    };
    format!(
        "    @{{ Key = {}; Name = {}; Kind = '{kind}'; Data = {data} }}",
        quote(value.key),
        quote(value.name)
    )
}

fn quoted_list(items: &[&str]) -> String {
    items
        .iter()
        .map(|item| quote(item))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Quote text as a PowerShell literal string, where only `'` needs escaping.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn under(key: &str, root: &str) -> bool {
        key == root || key.starts_with(&format!("{root}\\"))
    }

    fn owned(key: &str) -> bool {
        OWNED_KEYS.iter().any(|root| under(key, root))
    }

    #[test]
    fn every_owned_value_stays_under_an_owned_root() {
        for value in VALUES {
            assert!(owned(value.key), "{} is outside Atlas's keys", value.key);
            assert!(!value.key.contains("HKEY_LOCAL_MACHINE") && !value.key.starts_with("HKLM"));
        }
    }

    #[test]
    fn every_owned_root_carries_the_owner_marker() {
        for root in OWNED_KEYS {
            assert!(
                VALUES.contains(&marker(root)),
                "{root} lacks the owner marker"
            );
            assert!(root.ends_with(REGISTRY_NAME) || root.ends_with("AtlasBrowserURL"));
        }
    }

    #[test]
    fn shared_keys_are_never_owned_or_deleted_whole() {
        for shared in SHARED_VALUES {
            assert!(!owned(shared.key));
            assert!(!OWNED_KEYS.contains(&shared.key));
            assert_eq!(shared.name, REGISTRY_NAME);
        }
        assert!(!OWNED_KEYS.iter().any(|key| key.ends_with(r"\Classes")));
    }

    #[test]
    fn windows_can_offer_atlas_for_web_links_only() {
        let associations: Vec<_> = VALUES
            .iter()
            .filter(|value| value.key.ends_with(r"\URLAssociations"))
            .map(|value| (value.name, value.data))
            .collect();
        assert_eq!(
            associations,
            [
                ("http", Data::String("AtlasBrowserURL")),
                ("https", Data::String("AtlasBrowserURL"))
            ]
        );
        assert_eq!(SHARED_VALUES[0].data, Data::String(CAPABILITIES));
        // No "URL Protocol": the ProgID must not register an atlasbrowserurl: scheme.
        assert!(!VALUES.iter().any(|value| value.name == "URL Protocol"));
    }

    #[test]
    fn link_command_passes_the_link_as_one_argument_after_the_marker() {
        // P1-34 accepts a link only as the single quoted argument after `--`.
        let command = VALUES
            .iter()
            .find(|value| value.key == r"Software\Classes\AtlasBrowserURL\shell\open\command")
            .map(|value| value.data);
        let Some(Data::String(command)) = command else {
            panic!("the link command must be a string value");
        };
        assert_eq!(command, "\"{exe}\" -- \"%1\"");
        assert_eq!(command, format!("\"{{exe}}\" {LINK_MARKER} \"%1\""));
    }

    #[test]
    fn browser_launch_command_takes_no_arguments() {
        assert!(VALUES.contains(&string(
            r"Software\Clients\StartMenuInternet\AtlasBrowser\shell\open\command",
            "",
            "\"{exe}\"",
        )));
    }

    #[test]
    fn uninstall_runs_with_remote_signed_policy_and_pauses() {
        let command = VALUES
            .iter()
            .find(|value| value.name == "UninstallString")
            .map(|value| value.data);
        let Some(Data::String(command)) = command else {
            panic!("the uninstall command must be a string value");
        };
        assert!(command.contains("-ExecutionPolicy RemoteSigned"));
        assert!(!command.contains("Bypass"));
        assert!(command.ends_with("-Uninstall -PauseAtEnd"));
    }

    #[test]
    fn program_folder_is_separate_from_data() {
        for data in [DATA_FOLDER, PREVIEW_DATA_FOLDER] {
            assert!(!under(INSTALL_FOLDER, data) && !under(data, INSTALL_FOLDER));
        }
    }

    #[test]
    fn installed_app_selects_the_same_data_folder() {
        // atlas-app is a binary, so check its source rather than import its constant.
        let source = include_str!("../../../atlas-app/src/main.rs");
        let selection = format!(
            "if cfg!(feature = \"installed\") {{\n    r\"{DATA_FOLDER}\"\n}} else {{\n    \"{PREVIEW_DATA_FOLDER}\"\n}}"
        );
        assert!(
            source.replace("\r\n", "\n").contains(&selection),
            "atlas-app's DATA_FOLDER selection no longer matches the packaging table"
        );
    }

    #[test]
    fn rendering_escapes_quotes_and_uses_crlf() {
        assert_eq!(quote("it's"), "'it''s'");
        let script = render_script(&["a.exe"]);
        assert!(script.ends_with("\r\n"));
        assert!(!script.replace("\r\n", "").contains('\n'));
        assert_eq!(
            script.matches("@{ Key = ").count(),
            VALUES.len() + SHARED_VALUES.len()
        );
        assert!(script.contains("$AtlasPackageFiles = @('a.exe')"));
    }

    #[test]
    fn doc_table_lists_every_value() {
        let table = render_doc_table();
        assert_eq!(
            table.lines().count(),
            2 + VALUES.len() + SHARED_VALUES.len()
        );
        assert!(table.contains("| `Software\\RegisteredApplications` | `AtlasBrowser` |"));
    }
}
