//! `wasi:clocks` and `wasi:random/insecure-seed`: the host's clocks, and a hash seed.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use wasmtime::component::Resource;

use super::WasiState;
use super::bindings::wasi::clocks::monotonic_clock::{self, Pollable};
use super::bindings::wasi::clocks::wall_clock::{self, Datetime};
use super::bindings::wasi::random::insecure_seed;

impl monotonic_clock::Host for WasiState {
    /// Nanoseconds since this store was created.
    fn now(&mut self) -> wasmtime::Result<u64> {
        Ok(u64::try_from(self.started.elapsed().as_nanos())?)
    }

    fn resolution(&mut self) -> wasmtime::Result<u64> {
        Ok(1)
    }

    fn subscribe_instant(&mut self, when: u64) -> wasmtime::Result<Resource<Pollable>> {
        let deadline = self.started + Duration::from_nanos(when);
        self.add_timer(deadline)
    }

    fn subscribe_duration(&mut self, after: u64) -> wasmtime::Result<Resource<Pollable>> {
        let deadline = Instant::now() + Duration::from_nanos(after);
        self.add_timer(deadline)
    }
}

impl wall_clock::Host for WasiState {
    fn now(&mut self) -> wasmtime::Result<Datetime> {
        let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH)?;
        Ok(Datetime {
            seconds: since_epoch.as_secs(),
            nanoseconds: since_epoch.subsec_nanos(),
        })
    }

    fn resolution(&mut self) -> wasmtime::Result<Datetime> {
        Ok(Datetime {
            seconds: 0,
            nanoseconds: 1,
        })
    }
}

impl insecure_seed::Host for WasiState {
    /// A seed for the plugin's hash maps, from the standard library's own random keys.
    fn insecure_seed(&mut self) -> wasmtime::Result<(u64, u64)> {
        let keys = RandomState::new();
        Ok((keys.hash_one(1_u8), keys.hash_one(2_u8)))
    }
}
