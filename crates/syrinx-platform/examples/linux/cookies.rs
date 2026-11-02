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
    server.finish()?;
    println!(
        "Linux cookie_persistence: unused host, orderly persistent-cookie reopen and bounded shutdown passed."
    );
    Ok(())
}
