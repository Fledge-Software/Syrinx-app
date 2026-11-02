//! Syrinx's own WASI 0.2: output to bounded buffers, clocks, a seed, and nothing else.
//!
//! The owner chose this over `wasmtime-wasi` (G1, 2026-10-26) so that nothing
//! able to open a file or a socket is compiled into Syrinx.

mod cli;
mod clocks;
mod streams;

use std::collections::BTreeMap;
use std::time::Instant;

use wasmtime::component::{HasSelf, Linker};

mod bindings {
    wasmtime::component::bindgen!({
        path: [
            "../../vendor/wasi-0.2.12-wit/io.wit",
            "../../vendor/wasi-0.2.12-wit/clocks.wit",
            "../../vendor/wasi-0.2.12-wit/random.wit",
            "../../vendor/wasi-0.2.12-wit/cli.wit",
            "wit/wasi-shim.wit",
        ],
        world: "syrinx:wasi-shim/shim",
        imports: { default: trappable },
    });
}

/// The WASI interfaces a plugin may import, without their patch version.
/// Anything else (files, sockets, HTTP) is refused before instantiation.
pub const PROVIDED_INTERFACES: &[&str] = &[
    "wasi:io/error",
    "wasi:io/poll",
    "wasi:io/streams",
    "wasi:clocks/monotonic-clock",
    "wasi:clocks/wall-clock",
    "wasi:random/insecure-seed",
    "wasi:cli/environment",
    "wasi:cli/exit",
    "wasi:cli/stdin",
    "wasi:cli/stdout",
    "wasi:cli/stderr",
    "wasi:cli/terminal-input",
    "wasi:cli/terminal-output",
    "wasi:cli/terminal-stdin",
    "wasi:cli/terminal-stdout",
    "wasi:cli/terminal-stderr",
];

/// Each of standard output and standard error keeps at most this much; the rest is counted.
pub const OUTPUT_LIMIT_BYTES: usize = 64 * 1024;

/// The most a single write may carry, as WASI 0.2's `blocking-write-and-flush` allows.
const WRITE_PERMIT_BYTES: u64 = 4096;

/// A plugin may hold at most this many live timers.
const MAX_TIMERS: usize = 64;

/// What a plugin wrote to one output stream, bounded.
#[derive(Debug, Default)]
pub struct OutputBuffer {
    kept: Vec<u8>,
    dropped: u64,
    /// Bytes the next write may carry; WASI requires a `check-write` first.
    permit: u64,
}

impl OutputBuffer {
    /// The kept output, as text with invalid UTF-8 replaced.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.kept).into_owned()
    }

    /// Bytes written past the limit and thrown away.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Keep what fits under the limit and count the rest.
    fn append(&mut self, bytes: &[u8]) {
        let room = OUTPUT_LIMIT_BYTES.saturating_sub(self.kept.len());
        let kept = bytes.len().min(room);
        self.kept.extend_from_slice(&bytes[..kept]);
        self.dropped += (bytes.len() - kept) as u64;
    }
}

/// The per-store state behind the WASI interfaces.
#[derive(Debug)]
pub struct WasiState {
    started: Instant,
    pub stdout: OutputBuffer,
    pub stderr: OutputBuffer,
    /// Live timers by resource representation, with their deadline.
    timers: BTreeMap<u32, Instant>,
    next_timer: u32,
}

impl WasiState {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            stdout: OutputBuffer::default(),
            stderr: OutputBuffer::default(),
            timers: BTreeMap::new(),
            next_timer: streams::FIRST_TIMER_REP,
        }
    }
}

impl Default for WasiState {
    fn default() -> Self {
        Self::new()
    }
}

/// Link the sixteen interfaces, reading this state from the store's data.
pub fn add_to_linker<T: 'static>(
    linker: &mut Linker<T>,
    state: fn(&mut T) -> &mut WasiState,
) -> wasmtime::Result<()> {
    bindings::Shim::add_to_linker::<T, HasSelf<WasiState>>(linker, state)
}

/// Is this import one Syrinx provides? `name` is as a component lists it, `wasi:cli/stdout@0.2.9`.
pub fn provides(name: &str) -> bool {
    let Some((interface, version)) = name.split_once('@') else {
        return false;
    };
    version.starts_with("0.2.") && PROVIDED_INTERFACES.contains(&interface)
}

/// The error that stops a call when a plugin does something it may not.
fn stop(reason: impl Into<String>) -> wasmtime::Error {
    wasmtime::Error::msg(reason.into())
}
