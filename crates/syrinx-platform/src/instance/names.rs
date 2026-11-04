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
        Self::for_stem(&stem)
    }

    #[cfg(windows)]
    fn for_stem(stem: &str) -> Result<Self, InstanceError> {
        Ok(Self {
            mutex: format!("Local\\{stem}"),
            pipe: format!(r"\\.\pipe\{stem}"),
        })
    }

    /// A lock file and a socket in this user's private runtime folder.
    #[cfg(unix)]
    fn for_stem(stem: &str) -> Result<Self, InstanceError> {
        let folder = runtime_folder()?;
        Ok(Self {
            mutex: folder
                .join(format!("{stem}.lock"))
                .to_string_lossy()
                .into_owned(),
            pipe: folder
                .join(format!("{stem}.sock"))
                .to_string_lossy()
                .into_owned(),
        })
    }
}

/// `XDG_RUNTIME_DIR` when it is this user's private folder; otherwise `syrinx-<uid>` in the
/// temporary folder, created mode 0700. A shared folder such as `/tmp` would let another user
/// create the lock or socket first.
#[cfg(unix)]
fn runtime_folder() -> Result<std::path::PathBuf, InstanceError> {
    // SAFETY: getuid cannot fail and touches no memory.
    let uid = unsafe { libc::getuid() };
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from);
    private_folder(runtime, &std::env::temp_dir(), uid)
}

#[cfg(unix)]
fn private_folder(
    runtime: Option<std::path::PathBuf>,
    temporary: &Path,
    uid: libc::uid_t,
) -> Result<std::path::PathBuf, InstanceError> {
    use std::os::unix::fs::DirBuilderExt;

    if let Some(folder) = runtime.filter(|folder| folder.is_absolute() && is_private(folder, uid)) {
        return Ok(folder);
    }
    let folder = temporary.join(format!("syrinx-{uid}"));
    match std::fs::DirBuilder::new().mode(0o700).create(&folder) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(InstanceError::RuntimeFolder(error.kind())),
    }
    // An existing folder of that name may belong to someone else, or be open to others.
    if !is_private(&folder, uid) {
        return Err(InstanceError::Security);
    }
    Ok(folder)
}

/// A real folder, not a link, owned by `uid`, that no other user can enter or write.
#[cfg(unix)]
fn is_private(folder: &Path, uid: libc::uid_t) -> bool {
    use std::os::unix::fs::MetadataExt;

    std::fs::symlink_metadata(folder)
        .is_ok_and(|entry| entry.is_dir() && entry.uid() == uid && entry.mode() & 0o077 == 0)
}

/// Whether an existing lock file or socket belongs to `uid`; one that does not was placed by
/// another user and is never used or removed.
#[cfg(unix)]
pub(super) fn owned_by(path: &Path, uid: libc::uid_t) -> bool {
    use std::os::unix::fs::MetadataExt;

    std::fs::symlink_metadata(path).is_ok_and(|entry| entry.uid() == uid)
}

/// A stable key for a profile directory, the same for every spelling Windows treats as equal
/// here: either slash, any letter case, a trailing separator or the `\\?\` prefix.
#[cfg(windows)]
pub(super) fn profile_key(directory: &Path) -> Result<u64, InstanceError> {
    let absolute = std::path::absolute(directory).map_err(|_| InstanceError::Directory)?;
    let spelled = absolute.to_string_lossy().replace('/', "\\").to_lowercase();
    let unprefixed = match spelled.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with(r"unc\") => rest,
        _ => &spelled,
    };
    Ok(fnv1a(unprefixed.trim_end_matches('\\').as_bytes()))
}

/// Unix names are case-sensitive byte strings; only equivalent path components are folded.
#[cfg(unix)]
pub(super) fn profile_key(directory: &Path) -> Result<u64, InstanceError> {
    use std::os::unix::ffi::OsStrExt;

    let absolute = std::path::absolute(directory).map_err(|_| InstanceError::Directory)?;
    let normalized: std::path::PathBuf = absolute.components().collect();
    Ok(fnv1a(normalized.as_os_str().as_bytes()))
}

/// 64-bit FNV-1a: small, stable across Rust releases and needs no dependency.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(all(test, unix))]
mod unix_tests {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

    use super::*;

    #[test]
    fn distinct_unix_names_never_share_windows_normalization() {
        let profile = profile_key(Path::new("/synthetic/Profile")).expect("profile key");
        assert_ne!(profile_key(Path::new("/synthetic/profile")), Ok(profile));
        assert_ne!(profile_key(Path::new(r"/synthetic\Profile")), Ok(profile));
        let first = Path::new(OsStr::from_bytes(b"/synthetic/\xff"));
        let second = Path::new(OsStr::from_bytes(b"/synthetic/\xfe"));
        assert_ne!(profile_key(first), profile_key(second));
    }

    /// A fresh synthetic folder under the temporary folder, made with `mode`.
    fn scratch(name: &str, mode: u32) -> std::path::PathBuf {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

        let folder =
            std::env::temp_dir().join(format!("syrinx-names-{name}-{}", std::process::id()));
        // A run that failed earlier may have left its folder behind.
        if folder.exists() {
            std::fs::remove_dir_all(&folder).expect("remove an earlier run's scratch folder");
        }
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&folder)
            .expect("scratch folder");
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(mode))
            .expect("scratch mode");
        folder
    }

    #[test]
    fn a_shared_runtime_folder_is_replaced_by_a_private_one() {
        use std::os::unix::fs::MetadataExt;

        let uid = unsafe { libc::getuid() };
        let shared = scratch("shared", 0o777);
        let temporary = scratch("temporary", 0o700);
        let folder = private_folder(Some(shared.clone()), &temporary, uid).expect("private");
        assert_eq!(folder, temporary.join(format!("syrinx-{uid}")));
        let entry = std::fs::symlink_metadata(&folder).expect("created");
        assert_eq!(entry.mode() & 0o777, 0o700);
        assert_eq!(entry.uid(), uid);
        // A private runtime folder is used as it is.
        assert_eq!(
            private_folder(Some(temporary.clone()), &shared, uid),
            Ok(temporary.clone())
        );
        for folder in [shared, temporary] {
            std::fs::remove_dir_all(folder).expect("remove scratch");
        }
    }

    #[test]
    fn an_open_fallback_folder_or_another_users_file_is_refused() {
        use std::os::unix::fs::PermissionsExt;

        let uid = unsafe { libc::getuid() };
        let temporary = scratch("open-fallback", 0o700);
        let fallback = temporary.join(format!("syrinx-{uid}"));
        std::fs::create_dir(&fallback).expect("fallback");
        std::fs::set_permissions(&fallback, std::fs::Permissions::from_mode(0o777))
            .expect("open mode");
        assert_eq!(
            private_folder(None, &temporary, uid),
            Err(InstanceError::Security)
        );
        // An ordinary I/O failure keeps its cause rather than reading as a security refusal.
        assert_eq!(
            private_folder(None, &temporary.join("missing"), uid),
            Err(InstanceError::RuntimeFolder(std::io::ErrorKind::NotFound))
        );
        let file = temporary.join("synthetic.lock");
        std::fs::write(&file, b"").expect("synthetic lock");
        assert!(owned_by(&file, uid));
        assert!(!owned_by(&file, uid.wrapping_add(1)));
        std::fs::remove_dir_all(temporary).expect("remove scratch");
    }

    #[test]
    fn equivalent_unix_spellings_share_the_same_key() {
        let profile = profile_key(Path::new("/synthetic/Profile")).expect("profile key");
        for spelling in [
            "/synthetic/Profile/",
            "/synthetic/./Profile",
            "/synthetic//Profile",
        ] {
            assert_eq!(profile_key(Path::new(spelling)), Ok(profile));
        }
    }
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
