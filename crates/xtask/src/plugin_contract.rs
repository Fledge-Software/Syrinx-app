//! Parses the plugin contract `wit/v0_1`, so a WIT mistake fails the gate
//! instead of every plugin's build (#28).

use std::path::Path;

use wit_parser::Resolve;

/// The package and world every plugin targets.
const CONTRACT_FOLDER: &str = "wit/v0_1";
const CONTRACT_PACKAGE: &str = "syrinx:plugin@0.1.0";
const CONTRACT_WORLD: &str = "plugin";

pub fn check(root: &Path) -> Result<(), String> {
    let folder = root.join(CONTRACT_FOLDER);
    describe(&folder)?;
    println!("Plugin contract {CONTRACT_PACKAGE} parsed.");
    Ok(())
}

/// Parse the folder, then confirm its package and world are the contract's.
fn describe(folder: &Path) -> Result<(), String> {
    let mut resolve = Resolve::default();
    let (package, _) = resolve
        .push_dir(folder)
        .map_err(|error| format!("{}: {error:#}", folder.display()))?;
    let name = resolve.packages[package].name.to_string();
    if name != CONTRACT_PACKAGE {
        return Err(format!(
            "{} declares {name}, expected {CONTRACT_PACKAGE}",
            folder.display()
        ));
    }
    resolve
        .select_world(&[package], Some(CONTRACT_WORLD))
        .map_err(|error| format!("{}: {error:#}", folder.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    fn scratch(name: &str, wit: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("syrinx-xtask-wit-{name}-{}", std::process::id()));
        fs::create_dir_all(&folder).expect("scratch folder");
        fs::write(folder.join("plugin.wit"), wit).expect("scratch WIT");
        folder
    }

    #[test]
    fn the_repository_contract_parses() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert_eq!(describe(&root.join(CONTRACT_FOLDER)), Ok(()));
    }

    #[test]
    fn a_keyword_name_or_a_wrong_version_fails() {
        let keyword = scratch(
            "keyword",
            "package syrinx:plugin@0.1.0;\ninterface documents { list: func(); }\nworld plugin {}\n",
        );
        assert!(describe(&keyword).is_err());
        let version = scratch("version", "package syrinx:plugin@0.2.0;\nworld plugin {}\n");
        let error = describe(&version).expect_err("refused");
        assert!(error.contains("expected syrinx:plugin@0.1.0"), "{error}");
        for folder in [keyword, version] {
            fs::remove_dir_all(folder).expect("scratch folder removed");
        }
    }
}
