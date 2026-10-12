//! The one list of per-user registry values that makes Atlas choosable as a browser (P1-37).
//!
//! `packaging/atlas-registration.ps1` is rendered from this file; the gate refuses drift.

/// Product name shown in Default apps, the Start menu and Installed apps.
pub const PRODUCT: &str = "Atlas";

/// Executable copied into the package and registered with Windows.
pub const EXECUTABLE: &str = "atlas-app.exe";

/// Program folder relative to `%LOCALAPPDATA%`, kept apart from every data folder.
pub const INSTALL_FOLDER: &str = r"Programs\Atlas";

/// Data folders relative to `%LOCALAPPDATA%`: the preview profile, then the platform default.
pub const DATA_FOLDERS: [&str; 2] = ["AtlasProductPreview", r"Atlas\data"];

/// Switch Windows passes before a link; P1-34 must treat the rest of the command line as one URL.
pub const OPEN_URL_SWITCH: &str = "--open-url";

/// Whether `atlas-app.exe` accepts `OPEN_URL_SWITCH` yet. P1-34 sets this when it lands.
pub const ACCEPTS_OPEN_URL: bool = false;

/// Generated PowerShell file shared by the install, register and unregister scripts.
pub const GENERATED_SCRIPT: &str = "atlas-registration.ps1";

const START_MENU_INTERNET: &str = r"Software\Clients\StartMenuInternet\Atlas";
const CAPABILITIES: &str = r"Software\Clients\StartMenuInternet\Atlas\Capabilities";
const URL_PROG_ID: &str = r"Software\Classes\AtlasURL";
const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Atlas";
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

/// Every value `register-atlas.ps1` writes, all under `HKEY_CURRENT_USER`.
pub const VALUES: [RegistryValue; 25] = [
    // The browser entry Windows lists in Default apps.
    string(START_MENU_INTERNET, "", "Atlas"),
    string(
        r"Software\Clients\StartMenuInternet\Atlas\DefaultIcon",
        "",
        "{exe},0",
    ),
    string(
        r"Software\Clients\StartMenuInternet\Atlas\shell\open\command",
        "",
        "\"{exe}\"",
    ),
    // What Atlas can handle, which Default apps offers to the owner.
    string(CAPABILITIES, "ApplicationName", "Atlas"),
    string(CAPABILITIES, "ApplicationDescription", "Atlas web browser"),
    string(CAPABILITIES, "ApplicationIcon", "{exe},0"),
    string(
        r"Software\Clients\StartMenuInternet\Atlas\Capabilities\StartMenu",
        "StartMenuInternet",
        "Atlas",
    ),
    string(
        r"Software\Clients\StartMenuInternet\Atlas\Capabilities\URLAssociations",
        "http",
        "AtlasURL",
    ),
    string(
        r"Software\Clients\StartMenuInternet\Atlas\Capabilities\URLAssociations",
        "https",
        "AtlasURL",
    ),
    // Points Windows at the capabilities above; a value, never a key Atlas owns.
    string(
        REGISTERED_APPLICATIONS,
        "Atlas",
        r"Software\Clients\StartMenuInternet\Atlas\Capabilities",
    ),
    // The ProgID that opens a link in Atlas once the owner chooses it.
    string(URL_PROG_ID, "", "Atlas URL"),
    string(URL_PROG_ID, "FriendlyTypeName", "Atlas URL"),
    string(URL_PROG_ID, "URL Protocol", ""),
    string(
        r"Software\Classes\AtlasURL\Application",
        "ApplicationName",
        "Atlas",
    ),
    string(
        r"Software\Classes\AtlasURL\Application",
        "ApplicationDescription",
        "Atlas web browser",
    ),
    string(
        r"Software\Classes\AtlasURL\Application",
        "ApplicationIcon",
        "{exe},0",
    ),
    string(r"Software\Classes\AtlasURL\DefaultIcon", "", "{exe},0"),
    string(
        r"Software\Classes\AtlasURL\shell\open\command",
        "",
        "\"{exe}\" --open-url %1",
    ),
    // The Installed apps entry, whose Uninstall button runs the unregister script.
    string(UNINSTALL, "DisplayName", "Atlas"),
    string(UNINSTALL, "DisplayVersion", "{version}"),
    string(UNINSTALL, "DisplayIcon", "{exe},0"),
    string(UNINSTALL, "InstallLocation", "{dir}"),
    string(
        UNINSTALL,
        "UninstallString",
        "powershell.exe -NoProfile -ExecutionPolicy Bypass -File \"{dir}\\unregister-atlas.ps1\" -Uninstall",
    ),
    dword(UNINSTALL, "NoModify", 1),
    dword(UNINSTALL, "NoRepair", 1),
];

/// Key trees `unregister-atlas.ps1` deletes whole. Shared keys are never listed here.
pub const OWNED_KEYS: [&str; 3] = [START_MENU_INTERNET, URL_PROG_ID, UNINSTALL];

/// Single values `unregister-atlas.ps1` deletes from keys other software shares.
pub const SHARED_VALUES: [(&str, &str); 1] = [(REGISTERED_APPLICATIONS, "Atlas")];

/// Render the generated PowerShell data file, with CRLF line endings for Windows PowerShell.
pub fn render_script() -> String {
    let mut lines = vec![
        "# Generated from crates/xtask/src/packaging/registration.rs.".to_owned(),
        "# Do not edit by hand: change the Rust table, then run `cargo xtask write-registration`."
            .to_owned(),
        "# Every key is under HKEY_CURRENT_USER. Nothing here sets the default browser.".to_owned(),
        String::new(),
        format!("$AtlasProduct = {}", quote(PRODUCT)),
        format!("$AtlasExecutable = {}", quote(EXECUTABLE)),
        format!("$AtlasInstallFolder = {}", quote(INSTALL_FOLDER)),
        format!("$AtlasDataFolders = @({})", quoted_list(&DATA_FOLDERS)),
        format!("$AtlasOpenUrlSwitch = {}", quote(OPEN_URL_SWITCH)),
        format!("$AtlasAcceptsOpenUrl = ${ACCEPTS_OPEN_URL}"),
        String::new(),
        "$AtlasRegistryValues = @(".to_owned(),
    ];
    lines.extend(VALUES.iter().map(render_value));
    lines.push(")".to_owned());
    lines.push(String::new());
    lines.push(format!("$AtlasOwnedKeys = @({})", quoted_list(&OWNED_KEYS)));
    lines.push("$AtlasSharedValues = @(".to_owned());
    lines.extend(
        SHARED_VALUES
            .iter()
            .map(|(key, name)| format!("    @{{ Key = {}; Name = {} }}", quote(key), quote(name))),
    );
    lines.push(")".to_owned());

    let mut script = lines.join("\r\n");
    script.push_str("\r\n");
    script
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

    const ALLOWED_ROOTS: [&str; 4] = [
        START_MENU_INTERNET,
        URL_PROG_ID,
        UNINSTALL,
        REGISTERED_APPLICATIONS,
    ];

    fn under(key: &str, root: &str) -> bool {
        key == root || key.starts_with(&format!("{root}\\"))
    }

    #[test]
    fn every_value_stays_in_atlas_per_user_keys() {
        for value in VALUES {
            assert!(
                ALLOWED_ROOTS.iter().any(|root| under(value.key, root)),
                "{} is outside Atlas's keys",
                value.key
            );
            assert!(!value.key.contains("HKEY_LOCAL_MACHINE") && !value.key.starts_with("HKLM"));
        }
    }

    #[test]
    fn unregistering_removes_everything_registering_writes() {
        for value in VALUES {
            let owned = OWNED_KEYS.iter().any(|root| under(value.key, root));
            let shared = SHARED_VALUES.contains(&(value.key, value.name));
            assert!(
                owned || shared,
                "{}\\{} would be left behind",
                value.key,
                value.name
            );
        }
    }

    #[test]
    fn shared_keys_are_never_deleted_whole() {
        assert!(!OWNED_KEYS.contains(&REGISTERED_APPLICATIONS));
        assert!(!OWNED_KEYS.iter().any(|key| key.ends_with(r"\Classes")));
    }

    #[test]
    fn windows_can_offer_atlas_for_web_links() {
        let associations: Vec<_> = VALUES
            .iter()
            .filter(|value| value.key.ends_with(r"\URLAssociations"))
            .map(|value| (value.name, value.data))
            .collect();
        assert_eq!(
            associations,
            [
                ("http", Data::String("AtlasURL")),
                ("https", Data::String("AtlasURL"))
            ]
        );
        assert!(VALUES.contains(&string(REGISTERED_APPLICATIONS, "Atlas", CAPABILITIES)));
    }

    #[test]
    fn link_command_uses_the_single_argument_switch() {
        // The link stays unquoted and last, so P1-34 can take the raw remainder as one URL.
        let command = VALUES
            .iter()
            .find(|value| value.key == r"Software\Classes\AtlasURL\shell\open\command")
            .map(|value| value.data);
        let Some(Data::String(command)) = command else {
            panic!("the link command must be a string value");
        };
        assert_eq!(command, format!("\"{{exe}}\" {OPEN_URL_SWITCH} %1"));
    }

    #[test]
    fn program_folder_is_separate_from_data() {
        for data in DATA_FOLDERS {
            assert!(!under(INSTALL_FOLDER, data) && !under(data, INSTALL_FOLDER));
        }
    }

    #[test]
    fn rendering_escapes_quotes_and_uses_crlf() {
        assert_eq!(quote("it's"), "'it''s'");
        let script = render_script();
        assert!(script.ends_with("\r\n"));
        assert!(!script.replace("\r\n", "").contains('\n'));
        assert_eq!(
            script.matches("@{ Key = ").count(),
            VALUES.len() + SHARED_VALUES.len()
        );
    }
}
