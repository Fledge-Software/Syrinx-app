//! Checks a synthetic persistent cookie through ordinary and bounded host shutdown.

use super::{
    fixture::{Fixture, reserve_profile},
    lifecycle::require,
    server::Server,
};
use std::time::{Duration, Instant};

pub fn run() -> Result<(), String> {
    let mut server = Server::start()?;
    let mut idle = Fixture::new(reserve_profile()?)?;
    let started = Instant::now();
    idle.stop(Duration::from_secs(2))?;
    require(
        started.elapsed() < Duration::from_secs(2),
        "Unused host shutdown exceeded its bound",
    )?;
    for bound in [Duration::from_secs(5), Duration::from_millis(4500)] {
        let profile = reserve_profile()?;
        let mut writer = Fixture::new(profile.clone())?;
        writer.create(1, server.url("/cookie/set"))?;
        writer.title("cookie-written")?;
        let started = Instant::now();
        writer.stop(bound)?;
        require(
            started.elapsed() < bound,
            "Bounded host shutdown exceeded its deadline",
        )?;
        drop(writer);
        let mut reader = Fixture::new(profile)?;
        reader.create(1, server.url("/cookie/read"))?;
        reader.title("cookie-present")?;
        reader.stop(Duration::from_secs(5))?;
    }
    fresh_processes(&server)?;
    server.finish()?;
    println!(
        "Linux cookie_persistence: unused host, orderly persistent-cookie reopen, bounded shutdown and fresh-process HTTP cookie persistence passed."
    );
    Ok(())
}

/// A worker opens only the synthetic profile explicitly supplied by its parent.
pub struct CookieChild {
    writer: bool,
    profile: std::path::PathBuf,
    origin: String,
}

pub fn child_arguments(arguments: Vec<std::ffi::OsString>) -> Result<Option<CookieChild>, String> {
    if arguments
        .first()
        .is_none_or(|argument| argument != "--syrinx-cookie-child")
    {
        return Ok(None);
    }
    if arguments.len() != 4 {
        return Err("Cookie worker needs a mode, profile and loopback origin".into());
    }
    let writer = match arguments[1].to_str() {
        Some("writer") => true,
        Some("reader") => false,
        _ => return Err("Unknown cookie worker mode".into()),
    };
    let profile = std::path::PathBuf::from(&arguments[2]);
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::symlink_metadata(&profile).map_err(|error| error.to_string())?;
    if profile.parent() != Some(std::env::temp_dir().as_path())
        || !profile
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("syrinx-linux-fixture-"))
        || !metadata.is_dir()
        || metadata.permissions().mode() & 0o777 != 0o700
    {
        return Err("Cookie worker profile is not a private synthetic directory".into());
    }
    let origin = arguments[3]
        .to_str()
        .ok_or("Non-UTF-8 cookie origin")?
        .to_owned();
    let port = origin
        .strip_prefix("http://127.0.0.1:")
        .ok_or("Cookie worker origin is not loopback HTTP")?
        .parse::<u16>()
        .map_err(|_| "Invalid cookie worker port")?;
    if port == 0 {
        return Err("Cookie worker port is zero".into());
    }
    Ok(Some(CookieChild {
        writer,
        profile,
        origin,
    }))
}

impl CookieChild {
    pub fn run(self) -> Result<(), String> {
        let mut fixture = Fixture::new(self.profile)?;
        let route = if self.writer { "set" } else { "read" };
        fixture.create(1, format!("{}/cookie/process/{route}", self.origin))?;
        fixture.loaded(1)?;
        fixture.stop(Duration::from_secs(5))?;
        println!("Fresh cookie {route} process completed its load and host shutdown.");
        Ok(())
    }
}

fn fresh_processes(server: &Server) -> Result<(), String> {
    use super::child_process::OwnedChild;
    use std::process::Command;
    let profile = reserve_profile()?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    // The writer must have exited before the reader can open the same on-disk profile.
    for mode in ["writer", "reader"] {
        let mut child = OwnedChild::spawn(
            Command::new(&executable)
                .arg("--syrinx-cookie-child")
                .arg(mode)
                .arg(&profile)
                .arg(server.url("")),
        )?;
        let status = child.wait(Duration::from_secs(45))?;
        require(
            status.success(),
            &format!("Fresh cookie {mode} process failed: {status}"),
        )?;
        println!("Fresh cookie {mode} process exited successfully.");
    }
    require(
        server.cookie_received(),
        "Fresh reader did not send the persistent synthetic Cookie header",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_protocol_is_explicit_private_and_loopback_only() {
        let profile = reserve_profile().unwrap();
        let args = |mode: &str, origin: &str| {
            vec![
                "--syrinx-cookie-child".into(),
                mode.into(),
                profile.clone().into_os_string(),
                origin.into(),
            ]
        };
        assert!(child_arguments(vec![]).unwrap().is_none());
        assert!(
            child_arguments(args("writer", "http://127.0.0.1:43210"))
                .unwrap()
                .unwrap()
                .writer
        );
        assert!(
            !child_arguments(args("reader", "http://127.0.0.1:43210"))
                .unwrap()
                .unwrap()
                .writer
        );
        for (mode, origin) in [
            ("suite", "http://127.0.0.1:43210"),
            ("reader", "http://example.test:80"),
            ("reader", "http://127.0.0.1:0"),
            ("reader", "http://127.0.0.1:43210/path"),
        ] {
            assert!(child_arguments(args(mode, origin)).is_err());
        }
        assert!(child_arguments(vec!["--syrinx-cookie-child".into()]).is_err());
        std::fs::remove_dir(profile).unwrap();
    }
}
