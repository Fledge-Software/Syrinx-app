//! Checks that the code ranking typed text names no socket or HTTP API, so typing stays local (D9).

use std::fs;
use std::path::{Path, PathBuf};

/// Source on the typing path: the palette, address-bar parsing, the session worker that runs
/// each query and the headless crates behind them. A text scan cannot see a spawned process.
const QUERY_PATH: [&str; 8] = [
    "crates/atlas-core/src",
    "crates/atlas-search/src",
    "crates/atlas-store/src",
    "crates/atlas-app/src/palette",
    "crates/atlas-app/src/palette.rs",
    "crates/atlas-app/src/navigation.rs",
    "crates/atlas-app/src/session",
    "crates/atlas-app/src/session.rs",
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

/// Returns one violation per file and name found on the query path.
pub fn check(root: &Path) -> Result<Vec<String>, String> {
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
    use super::file_violations;

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
