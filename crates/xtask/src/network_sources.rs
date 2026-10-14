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

/// Returns one violation per file and name found on the query path, and per other crate
/// that names WinHTTP. Cargo unifies features, so every crate could call it once one asks.
pub fn check(root: &Path) -> Result<Vec<String>, String> {
    let mut violations = check_query_path(root)?;
    violations.extend(check_winhttp_confinement(root)?);
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

/// Every Rust file in the workspace's crates, except the download's and the tooling's, must not
/// name WinHTTP.
fn check_winhttp_confinement(root: &Path) -> Result<Vec<String>, String> {
    let crates = root.join("crates");
    let entries = fs::read_dir(&crates)
        .map_err(|error| format!("Could not read {}: {error}", crates.display()))?;
    let mut violations = Vec::new();
    for entry in entries {
        let folder = entry
            .map_err(|error| format!("Could not read {}: {error}", crates.display()))?
            .path();
        let name = folder.file_name().and_then(|name| name.to_str());
        if !folder.is_dir() || matches!(name, Some(WINHTTP_CRATE | TOOLING_CRATE)) {
            continue;
        }
        for file in rust_files(&folder)? {
            let text = fs::read_to_string(&file)
                .map_err(|error| format!("Could not read {}: {error}", file.display()))?;
            let relative = file
                .strip_prefix(root)
                .unwrap_or(&file)
                .display()
                .to_string();
            violations.extend(winhttp_violations(&relative, &text));
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
    use super::{file_violations, winhttp_violations};

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
