//! Names the mutex and pipe after the user, session and profile, so test profiles never meet.

use std::path::Path;

use super::InstanceError;

/// The kernel object names one user's Syrinx uses for one profile directory.
pub(super) struct InstanceNames {
    pub(super) mutex: String,
    pub(super) pipe: String,
}

impl InstanceNames {
    /// The pipe namespace is machine-wide while `Local\` is per session, so the stem also
    /// names the session: both objects then have the same scope.
    pub(super) fn new(
        directory: &Path,
        user_sid: &str,
        session: u32,
    ) -> Result<Self, InstanceError> {
        let key = profile_key(directory)?;
        let stem = format!("SyrinxInstance-{user_sid}-s{session}-{key:016x}");
        Ok(Self::for_stem(&stem))
    }

    #[cfg(windows)]
    fn for_stem(stem: &str) -> Self {
        Self {
            mutex: format!("Local\\{stem}"),
            pipe: format!(r"\\.\pipe\{stem}"),
        }
    }

    /// A lock file and a socket in this user's private runtime folder.
    #[cfg(unix)]
    fn for_stem(stem: &str) -> Self {
        let folder = runtime_folder();
        Self {
            mutex: folder
                .join(format!("{stem}.lock"))
                .to_string_lossy()
                .into_owned(),
            pipe: folder
                .join(format!("{stem}.sock"))
                .to_string_lossy()
                .into_owned(),
        }
    }
}

/// `XDG_RUNTIME_DIR` is per user and mode 0700; without one, the user's own temporary folder.
#[cfg(unix)]
fn runtime_folder() -> std::path::PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .filter(|folder| folder.is_absolute() && folder.is_dir())
        .unwrap_or_else(std::env::temp_dir)
}

/// A stable key for a profile directory, the same for every spelling Windows treats as equal
/// here: either slash, any letter case, a trailing separator or the `\\?\` prefix.
pub(super) fn profile_key(directory: &Path) -> Result<u64, InstanceError> {
    let absolute = std::path::absolute(directory).map_err(|_| InstanceError::Directory)?;
    let spelled = absolute.to_string_lossy().replace('/', "\\").to_lowercase();
    let unprefixed = match spelled.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with(r"unc\") => rest,
        _ => &spelled,
    };
    Ok(fnv1a(unprefixed.trim_end_matches('\\').as_bytes()))
}

/// 64-bit FNV-1a: small, stable across Rust releases and needs no dependency.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(all(test, windows))]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn equal_spellings_share_a_key_and_other_profiles_do_not() {
        let base = profile_key(Path::new(r"C:\Synthetic\SyrinxProfile")).expect("key");
        for spelling in [
            r"C:\Synthetic\SyrinxProfile\",
            r"c:\synthetic\syrinxprofile",
            "C:/Synthetic/SyrinxProfile",
            r"\\?\C:\Synthetic\SyrinxProfile",
        ] {
            assert_eq!(profile_key(Path::new(spelling)), Ok(base), "{spelling}");
        }
        for other in [
            r"C:\Synthetic\SyrinxProfile2",
            r"C:\Synthetic\Other",
            r"D:\Synthetic\SyrinxProfile",
        ] {
            assert_ne!(profile_key(Path::new(other)), Ok(base), "{other}");
        }
    }

    #[test]
    fn names_carry_the_user_the_session_and_the_profile() {
        let profile = Path::new(r"C:\Synthetic\Profile");
        let names = InstanceNames::new(profile, "S-1-5-21-0-0-0-1000", 1).expect("names");
        assert!(
            names
                .mutex
                .starts_with(r"Local\SyrinxInstance-S-1-5-21-0-0-0-1000-s1-")
        );
        assert!(
            names
                .pipe
                .starts_with(r"\\.\pipe\SyrinxInstance-S-1-5-21-0-0-0-1000-s1-")
        );
        let other_user = InstanceNames::new(profile, "S-1-5-21-0-0-0-1001", 1).expect("names");
        assert_ne!(names.pipe, other_user.pipe);
        let other_session = InstanceNames::new(profile, "S-1-5-21-0-0-0-1000", 2).expect("names");
        assert_ne!(names.pipe, other_session.pipe);
    }

    #[test]
    fn the_key_is_stable_across_builds() {
        // A changed key would let two Syrinx versions run on one profile during an update.
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
