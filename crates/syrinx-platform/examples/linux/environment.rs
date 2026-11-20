//! Isolates native fixture caches and downloads before any library starts a thread.

use std::{fs, os::unix::fs::DirBuilderExt, path::PathBuf};

use super::fixture::reserve_profile;

pub struct Environment {
    root: PathBuf,
    pub downloads: PathBuf,
}

impl Environment {
    pub fn prepare() -> Result<Self, String> {
        let root = reserve_profile()?;
        for name in ["data", "cache", "config", "Downloads"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(name))
                .map_err(|error| error.to_string())?;
        }
        let downloads = root.join("Downloads");
        let path = downloads
            .to_str()
            .ok_or("Non-UTF-8 scratch Downloads path")?;
        if path.contains(['"', '\\', '\n', '\r', '$', '`']) {
            return Err("Scratch Downloads path cannot be encoded in user-dirs.dirs".into());
        }
        fs::write(
            root.join("config/user-dirs.dirs"),
            format!("XDG_DOWNLOAD_DIR=\"{path}\"\n"),
        )
        .map_err(|error| error.to_string())?;
        Ok(Self { root, downloads })
    }

    /// Call only from the example entry point, before watchdog, GTK or server initialization.
    pub unsafe fn install(&self) {
        for (variable, directory) in [
            ("XDG_DATA_HOME", "data"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_CONFIG_HOME", "config"),
        ] {
            // SAFETY: main calls this before starting threads or initializing native libraries.
            unsafe { std::env::set_var(variable, self.root.join(directory)) };
        }
    }
}

pub fn verify_downloads(expected: &std::path::Path, actual: Option<PathBuf>) -> Result<(), String> {
    if actual.as_deref() != Some(expected) {
        return Err(
            "GLib Downloads directory is not the prepared private scratch destination".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn missing_or_different_downloads_refuses_home_fallback() {
        let expected = std::path::Path::new("/synthetic/private/Downloads");
        assert!(verify_downloads(expected, None).is_err());
        assert!(verify_downloads(expected, Some("/synthetic/home/Downloads".into())).is_err());
        assert!(verify_downloads(expected, Some(expected.into())).is_ok());
    }

    #[test]
    fn prepared_environment_has_private_directories_and_explicit_destination() {
        let environment = Environment::prepare().unwrap();
        for directory in ["", "data", "cache", "config", "Downloads"] {
            assert_eq!(
                fs::metadata(environment.root.join(directory))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        assert_eq!(
            fs::read_to_string(environment.root.join("config/user-dirs.dirs")).unwrap(),
            format!("XDG_DOWNLOAD_DIR=\"{}\"\n", environment.downloads.display())
        );
        fs::remove_dir_all(environment.root).unwrap();
    }
}
