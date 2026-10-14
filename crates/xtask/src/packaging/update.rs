//! Runs `syrinx-update-tool`, which writes and signs P1-38's manifest; xtask may not link the verifier.

use super::manifest::{MANIFEST, layout};

/// P1-38's signed manifest, written beside `syrinx-package.json`.
pub const UPDATE_MANIFEST: &str = "manifest";

/// Oldest installed version allowed to apply a package; no release yet needs an earlier migration.
pub const MINIMUM_VERSION: &str = "0.0.0";

/// The release tool's package and binary, run with D38's `local` profile.
const TOOL: [&str; 9] = [
    "run",
    "--package",
    "syrinx-update",
    "--bin",
    "syrinx-update-tool",
    "--profile",
    "local",
    "--offline",
    "--locked",
];

/// What an update installs: every file `install-syrinx.ps1` copies into the program folder.
pub fn update_files() -> Vec<&'static str> {
    let mut files = layout();
    files.push(MANIFEST);
    files.sort_unstable();
    files
}

/// Cargo arguments that write the update manifest into `folder`.
pub fn manifest_command(folder: &str, version: &str) -> Vec<String> {
    let mut arguments = vec![
        "--package",
        folder,
        "--version",
        version,
        "--minimum-version",
        MINIMUM_VERSION,
    ];
    for file in update_files() {
        arguments.extend(["--file", file]);
    }
    tool_command("manifest", &arguments)
}

/// Cargo arguments that pass the owner's `sign-update` options to the tool's `sign`.
pub fn sign_command(options: &[String]) -> Vec<String> {
    let options: Vec<&str> = options.iter().map(String::as_str).collect();
    tool_command("sign", &options)
}

fn tool_command(subcommand: &str, arguments: &[&str]) -> Vec<String> {
    TOOL.iter()
        .chain(&["--", subcommand])
        .chain(arguments)
        .map(|argument| (*argument).to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The values that follow each `--file` in a command.
    fn listed_files(command: &[String]) -> Vec<&str> {
        command
            .windows(2)
            .filter(|pair| pair[0] == "--file")
            .map(|pair| pair[1].as_str())
            .collect()
    }

    #[test]
    fn the_update_lists_exactly_the_installed_program_files() {
        assert_eq!(
            update_files(),
            [
                "install-syrinx.ps1",
                "register-syrinx.ps1",
                "syrinx-common.ps1",
                "syrinx-package.json",
                "syrinx-registration.ps1",
                "syrinx.exe",
                "unregister-syrinx.ps1",
            ]
        );
        assert!(!update_files().contains(&UPDATE_MANIFEST));
    }

    #[test]
    fn package_asks_the_tool_for_the_manifest_of_its_own_files() {
        let command = manifest_command(r"C:\repo\target\package\Syrinx-0.1.0-x64", "0.1.0");
        let separator = command.iter().position(|argument| argument == "--");
        assert_eq!(separator, Some(TOOL.len()));
        assert_eq!(&command[..TOOL.len()], TOOL);
        assert_eq!(
            &command[TOOL.len() + 1..TOOL.len() + 8],
            [
                "manifest",
                "--package",
                r"C:\repo\target\package\Syrinx-0.1.0-x64",
                "--version",
                "0.1.0",
                "--minimum-version",
                MINIMUM_VERSION,
            ]
        );
        assert_eq!(listed_files(&command), update_files());
    }

    #[test]
    fn sign_update_forwards_the_owner_options_unchanged() {
        let options = ["--serial".to_owned(), "3".to_owned()];
        let command = sign_command(&options);
        assert_eq!(&command[..TOOL.len()], TOOL);
        assert_eq!(&command[TOOL.len()..], ["--", "sign", "--serial", "3"]);
    }
}
