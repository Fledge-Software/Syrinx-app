//! Names the mutex and pipe after the user and profile, so test profiles never reach the owner's.

use std::path::Path;

use super::InstanceError;

/// The kernel object names one user's Atlas uses for one profile directory.
pub(super) struct InstanceNames {
    pub(super) mutex: String,
    pub(super) pipe: String,
}

impl InstanceNames {
    pub(super) fn new(directory: &Path, user_sid: &str) -> Result<Self, InstanceError> {
        let stem = format!("AtlasInstance-{user_sid}-{:016x}", profile_key(directory)?);
        Ok(Self {
            // `Local\` keeps the mutex inside this Windows session.
            mutex: format!("Local\\{stem}"),
            pipe: format!(r"\\.\pipe\{stem}"),
        })
    }
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn equal_spellings_share_a_key_and_other_profiles_do_not() {
        let base = profile_key(Path::new(r"C:\Synthetic\AtlasProfile")).expect("key");
        for spelling in [
            r"C:\Synthetic\AtlasProfile\",
            r"c:\synthetic\atlasprofile",
            "C:/Synthetic/AtlasProfile",
            r"\\?\C:\Synthetic\AtlasProfile",
        ] {
            assert_eq!(profile_key(Path::new(spelling)), Ok(base), "{spelling}");
        }
        for other in [
            r"C:\Synthetic\AtlasProfile2",
            r"C:\Synthetic\Other",
            r"D:\Synthetic\AtlasProfile",
        ] {
            assert_ne!(profile_key(Path::new(other)), Ok(base), "{other}");
        }
    }

    #[test]
    fn names_carry_the_user_and_the_profile() {
        let names = InstanceNames::new(Path::new(r"C:\Synthetic\Profile"), "S-1-5-21-0-0-0-1000")
            .expect("names");
        assert!(
            names
                .mutex
                .starts_with(r"Local\AtlasInstance-S-1-5-21-0-0-0-1000-")
        );
        assert!(
            names
                .pipe
                .starts_with(r"\\.\pipe\AtlasInstance-S-1-5-21-0-0-0-1000-")
        );
        let other_user =
            InstanceNames::new(Path::new(r"C:\Synthetic\Profile"), "S-1-5-21-0-0-0-1001")
                .expect("names");
        assert_ne!(names.pipe, other_user.pipe);
    }

    #[test]
    fn the_key_is_stable_across_builds() {
        // A changed key would let two Atlas versions run on one profile during an update.
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
