//! Checks that the code ranking typed text names no socket or HTTP API, so typing stays local (D9).

use std::fs;
use std::path::{Path, PathBuf};

/// Source on the typing path: the palette, address-bar parsing, the session worker that runs
/// each query and the headless crates behind them. A text scan cannot see a spawned process.
const QUERY_PATH: [&str; 8] = [
    "crates/syrinx-core/src",
    "crates/syrinx-search/src",
    "crates/syrinx-store/src",
    "crates/syrinx-app/src/palette",
    "crates/syrinx-app/src/palette.rs",
    "crates/syrinx-app/src/navigation.rs",
    "crates/syrinx-app/src/session",
    "crates/syrinx-app/src/session.rs",
];

/// Standard-library sockets and Windows HTTP entry points, which no dependency check can see.
const NETWORK_NAMES: [&str; 8] = [
    "TcpStream",
    "TcpListener",
    "UdpSocket",
    "ToSocketAddrs",
    "WinHttp",
    "InternetOpen",
    "WSAStartup",
    "http_client",
];

/// The one crate whose source may call WinHTTP: D9's one exception (P1-39c).
const WINHTTP_CRATE: &str = "syrinx-update-net";

/// Development tooling, which never ships, and names these APIs in its own checks.
const TOOLING_CRATE: &str = "xtask";

/// The prefix of every WinHTTP binding and module, in any import form (case-sensitive, so the
/// product name `WinHTTP` in prose does not count).
const WINHTTP_IDENTIFIER: &str = "WinHttp";

/// The library itself, which Windows names in any letter case.
const WINHTTP_LIBRARY: &str = "winhttp.dll";

/// Socket and network-management bindings, which GPUI's stack switches on in the `windows`
/// builds Syrinx shares (DW-68): `Networking::WinSock` in any import form and
/// `NetworkManagement`. Every `WSA` function or error name is matched as a token instead.
const SOCKET_IDENTIFIERS: [&str; 2] = ["WinSock", "NetworkManagement"];

/// The prefix of every Winsock function, type and error name, such as `WSAStartup`.
const WSA_PREFIX: &str = "WSA";

/// The socket and IP-helper libraries, as stems so `#[link(name = "ws2_32")]` and a
/// `LoadLibraryW` without `.dll` count too; Windows names them in any letter case.
const SOCKET_LIBRARIES: [&str; 4] = ["ws2_32", "wsock32", "mswsock", "iphlpapi"];

/// Returns one violation per file and name found on the query path, per other crate that
/// names WinHTTP, and per crate that names a socket binding. Cargo unifies features, so every
/// crate sharing a build could call what any one crate switched on.
pub fn check(root: &Path) -> Result<Vec<String>, String> {
    let mut violations = check_query_path(root)?;
    violations.extend(check_crate_confinement(root)?);
    Ok(violations)
}

fn check_query_path(root: &Path) -> Result<Vec<String>, String> {
    let mut violations = Vec::new();
    for entry in QUERY_PATH {
        for file in rust_files(&root.join(entry))? {
            let text = fs::read_to_string(&file)
                .map_err(|error| format!("Could not read {}: {error}", file.display()))?;
            let relative = file
                .strip_prefix(root)
                .unwrap_or(&file)
                .display()
                .to_string();
            violations.extend(file_violations(&relative, &text));
        }
    }
    Ok(violations)
}

/// Every Rust file in the workspace's crates except the tooling's must not name a socket
/// binding, and none but the download's may name WinHTTP.
fn check_crate_confinement(root: &Path) -> Result<Vec<String>, String> {
    let crates = root.join("crates");
    let entries = fs::read_dir(&crates)
        .map_err(|error| format!("Could not read {}: {error}", crates.display()))?;
    let mut violations = Vec::new();
    for entry in entries {
        let folder = entry
            .map_err(|error| format!("Could not read {}: {error}", crates.display()))?
            .path();
        let name = folder.file_name().and_then(|name| name.to_str());
        if !folder.is_dir() || name == Some(TOOLING_CRATE) {
            continue;
        }
        let may_call_winhttp = name == Some(WINHTTP_CRATE);
        for file in rust_files(&folder)? {
            let text = fs::read_to_string(&file)
                .map_err(|error| format!("Could not read {}: {error}", file.display()))?;
            let relative = file
                .strip_prefix(root)
                .unwrap_or(&file)
                .display()
                .to_string();
            if !may_call_winhttp {
                violations.extend(winhttp_violations(&relative, &text));
            }
            violations.extend(socket_violations(&relative, &text));
        }
    }
    Ok(violations)
}

fn winhttp_violations(path: &str, text: &str) -> Vec<String> {
    let identifier = text
        .contains(WINHTTP_IDENTIFIER)
        .then_some(WINHTTP_IDENTIFIER);
    let library = text
        .to_ascii_lowercase()
        .contains(WINHTTP_LIBRARY)
        .then_some(WINHTTP_LIBRARY);
    [identifier, library]
        .into_iter()
        .flatten()
        .map(|name| format!("{path}: {name}: only {WINHTTP_CRATE} may call WinHTTP (P1-39c)"))
        .collect()
}

fn socket_violations(path: &str, text: &str) -> Vec<String> {
    let lowercase = text.to_ascii_lowercase();
    let identifiers = SOCKET_IDENTIFIERS
        .into_iter()
        .filter(|name| text.contains(name));
    let wsa = has_wsa_token(text).then_some(WSA_PREFIX);
    let libraries = SOCKET_LIBRARIES
        .into_iter()
        .filter(|library| lowercase.contains(library));
    identifiers
        .chain(wsa)
        .chain(libraries)
        .map(|name| format!("{path}: {name}: no Syrinx crate may use Windows sockets (D9, DW-68)"))
        .collect()
}

/// `WSA` starting a word and followed by an uppercase letter, as in `WSAStartup` or
/// `WSAEWOULDBLOCK`, but not inside another name such as `AWSAccess`.
fn has_wsa_token(text: &str) -> bool {
    text.match_indices(WSA_PREFIX).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let after = text[start + WSA_PREFIX.len()..].chars().next();
        let starts_word =
            !before.is_some_and(|character| character.is_alphanumeric() || character == '_');
        starts_word && after.is_some_and(|character| character.is_ascii_uppercase())
    })
}

fn file_violations(path: &str, text: &str) -> Vec<String> {
    NETWORK_NAMES
        .iter()
        .filter(|name| text.contains(*name))
        .map(|name| format!("{path}: {name} is a network API on the typing path (D9)"))
        .collect()
}

// A listed file, or every Rust file under a listed directory; a missing entry is an error.
fn rust_files(path: &Path) -> Result<Vec<PathBuf>, String> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let entries = fs::read_dir(path)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let mut files = Vec::new();
    for entry in entries {
        let entry_path = entry
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?
            .path();
        if entry_path.is_dir() {
            files.extend(rust_files(&entry_path)?);
        } else if entry_path
            .extension()
            .is_some_and(|extension| extension == "rs")
        {
            files.push(entry_path);
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{check_crate_confinement, file_violations, socket_violations, winhttp_violations};

    #[test]
    fn no_crate_may_name_a_socket_binding() {
        // The shared `windows` build carries WinSock for GPUI; Syrinx's own source may not use it.
        for source in [
            "use windows::Win32::Networking::WinSock::{WSAStartup, WSADATA};",
            "use windows::Win32::Networking::{WinSock::socket};",
            "use windows::Win32::NetworkManagement::IpHelper::GetAdaptersAddresses;",
            "let code = WSAGetLastError();",
            "if error == WSAEWOULDBLOCK {",
            r#"windows_link::link!("WS2_32.dll" "system" fn closesocket());"#,
            r#"windows_link::link!("iphlpapi.dll" "system" fn GetIfTable());"#,
            r#"#[link(name = "ws2_32")] extern "system" {}"#,
            r#"#[link(name = "Wsock32")] extern "system" {}"#,
            r#"let module = LoadLibraryW(w!("mswsock"));"#,
        ] {
            let violations = socket_violations("crates/syrinx-platform/src/synthetic.rs", source);
            assert!(!violations.is_empty(), "{source}");
            assert!(violations[0].contains("no Syrinx crate may use Windows sockets"));
        }
        // Prose, address types and `WSA` inside another name are not socket calls.
        for source in [
            "// Winsock is not used here",
            "use std::net::Ipv4Addr;",
            "let policy = AWSAccessPolicy::new();",
            "const MY_WSA_LIKE: u8 = 0;",
            "// WSA is an acronym",
        ] {
            assert!(
                socket_violations("synthetic.rs", source).is_empty(),
                "{source}"
            );
        }
    }

    /// A fresh synthetic workspace under the system temporary folder, removed by the caller.
    fn synthetic_workspace(files: &[(&str, &str)]) -> Result<PathBuf, String> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("clock before 1970: {error}"))?
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "syrinx-network-sources-{}-{nanos}",
            std::process::id()
        ));
        for (path, text) in files {
            let file = root.join(path);
            let folder = file.parent().ok_or("synthetic file has no folder")?;
            fs::create_dir_all(folder)
                .map_err(|error| format!("Could not create {}: {error}", folder.display()))?;
            fs::write(&file, text)
                .map_err(|error| format!("Could not write {}: {error}", file.display()))?;
        }
        Ok(root)
    }

    fn remove_workspace(root: &Path) -> Result<(), String> {
        fs::remove_dir_all(root)
            .map_err(|error| format!("Could not remove {}: {error}", root.display()))
    }

    #[test]
    fn the_tooling_is_exempt_and_the_download_is_scanned_for_sockets() -> Result<(), String> {
        // xtask names these APIs in its own checks; the download may call WinHTTP, not sockets.
        let root = synthetic_workspace(&[
            (
                "crates/xtask/src/main.rs",
                "const NAMES: [&str; 2] = [\"WinSock\", \"WSAStartup\"];",
            ),
            (
                "crates/syrinx-update-net/src/lib.rs",
                "use windows::Win32::Networking::WinHttp::WinHttpOpen;\nlet code = WSAGetLastError();",
            ),
        ])?;
        let violations = check_crate_confinement(&root);
        remove_workspace(&root)?;
        let violations = violations?;
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(violations[0].contains("syrinx-update-net"));
        assert!(violations[0].contains("WSA: no Syrinx crate may use Windows sockets"));
        Ok(())
    }

    #[test]
    fn only_the_update_download_may_name_winhttp() {
        let calls = "use windows::Win32::Networking::WinHttp::WinHttpOpen;";
        let violations = winhttp_violations("crates/syrinx-platform/src/synthetic.rs", calls);
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(violations[0].contains("only syrinx-update-net may call WinHTTP"));
        // A brace-group import names no `Networking::WinHttp` path, but still the prefix.
        let grouped = "use windows::Win32::Networking::{WinHttp::WinHttpDetectAutoProxyConfigUrl};";
        assert_eq!(winhttp_violations("synthetic.rs", grouped).len(), 1);
        for library in ["winhttp.dll", "WINHTTP.DLL", "WinHTTP.dll"] {
            let linked = format!(r#"windows_core::link!("{library}" "system" fn Open());"#);
            assert_eq!(
                winhttp_violations("synthetic.rs", &linked).len(),
                1,
                "{library}"
            );
        }
        // Naming the product in prose is not a call.
        assert!(winhttp_violations("synthetic.rs", "// WinHTTP failed").is_empty());
    }

    #[test]
    fn socket_and_http_names_are_reported_and_address_types_are_not() {
        let violations = file_violations(
            "synthetic.rs",
            "use std::net::{Ipv4Addr, TcpStream};\nlet client = http_client::new();",
        );
        assert_eq!(violations.len(), 2, "{violations:?}");
        assert!(violations[0].contains("TcpStream"));
        assert!(file_violations("synthetic.rs", "use std::net::Ipv6Addr;").is_empty());
    }
}
