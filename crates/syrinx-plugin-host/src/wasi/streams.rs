//! `wasi:io`: standard output and error into bounded buffers, an empty closed
//! standard input, and pollables that are either ready or a timer.

use std::time::Instant;

use wasmtime::component::Resource;

use super::bindings::wasi::io::error::{self, Error};
use super::bindings::wasi::io::poll::{self, Pollable};
use super::bindings::wasi::io::streams::{self, InputStream, OutputStream, StreamError};
use super::{MAX_TIMERS, OutputBuffer, WRITE_PERMIT_BYTES, WasiState, stop};

/// Output stream representations.
pub(super) const STDOUT_REP: u32 = 1;
pub(super) const STDERR_REP: u32 = 2;
/// The one input stream, standard input, which is always empty and closed.
pub(super) const STDIN_REP: u32 = 0;
/// A pollable that is always ready; the streams never wait.
const READY_REP: u32 = 0;
/// Timers are numbered from here, so they never collide with `READY_REP`.
pub(super) const FIRST_TIMER_REP: u32 = 16;

impl WasiState {
    fn output(&mut self, stream: &Resource<OutputStream>) -> wasmtime::Result<&mut OutputBuffer> {
        match stream.rep() {
            STDOUT_REP => Ok(&mut self.stdout),
            STDERR_REP => Ok(&mut self.stderr),
            other => Err(stop(format!("unknown output stream {other}"))),
        }
    }

    /// Spend `len` bytes of the stream's permit, as WASI 0.2 requires of every write.
    fn spend_permit(&mut self, stream: &Resource<OutputStream>, len: u64) -> wasmtime::Result<()> {
        let output = self.output(stream)?;
        if len > output.permit {
            return Err(stop(format!(
                "the plugin wrote {len} bytes with a permit of {}",
                output.permit
            )));
        }
        output.permit -= len;
        Ok(())
    }

    /// A one-shot write and flush may carry up to one permit without asking first.
    fn write_and_flush(
        &mut self,
        stream: &Resource<OutputStream>,
        bytes: &[u8],
    ) -> wasmtime::Result<Result<(), StreamError>> {
        if bytes.len() as u64 > WRITE_PERMIT_BYTES {
            return Err(stop("the plugin wrote more than 4096 bytes in one call"));
        }
        self.output(stream)?.append(bytes);
        Ok(Ok(()))
    }

    /// Add a timer that becomes ready at `deadline`.
    pub(super) fn add_timer(&mut self, deadline: Instant) -> wasmtime::Result<Resource<Pollable>> {
        if self.timers.len() >= MAX_TIMERS {
            return Err(stop("the plugin holds too many timers"));
        }
        let rep = self.next_timer;
        self.next_timer = self
            .next_timer
            .checked_add(1)
            .ok_or_else(|| stop("the plugin created too many timers"))?;
        self.timers.insert(rep, deadline);
        Ok(Resource::new_own(rep))
    }

    fn is_ready(&self, rep: u32) -> wasmtime::Result<bool> {
        if rep == READY_REP {
            return Ok(true);
        }
        let deadline = self
            .timers
            .get(&rep)
            .ok_or_else(|| stop(format!("unknown pollable {rep}")))?;
        Ok(Instant::now() >= *deadline)
    }
}

/// Waiting would block the plugin's thread; plugins answer and return instead.
fn refuse_sleep() -> wasmtime::Error {
    stop("the plugin tried to sleep, which plugins may not do")
}

fn ready() -> Resource<Pollable> {
    Resource::new_own(READY_REP)
}

impl error::Host for WasiState {}

impl error::HostError for WasiState {
    // The shim never creates an error resource: failed streams report `closed`.
    fn to_debug_string(&mut self, _error: Resource<Error>) -> wasmtime::Result<String> {
        Err(stop("no error resource exists"))
    }

    fn drop(&mut self, _error: Resource<Error>) -> wasmtime::Result<()> {
        Ok(())
    }
}

impl poll::Host for WasiState {
    /// Report the ready pollables; if none is ready the plugin would wait, which it may not.
    fn poll(&mut self, pollables: Vec<Resource<Pollable>>) -> wasmtime::Result<Vec<u32>> {
        let mut ready = Vec::new();
        for (index, pollable) in pollables.iter().enumerate() {
            if self.is_ready(pollable.rep())? {
                ready.push(u32::try_from(index)?);
            }
        }
        if ready.is_empty() {
            return Err(refuse_sleep());
        }
        Ok(ready)
    }
}

impl poll::HostPollable for WasiState {
    fn ready(&mut self, pollable: Resource<Pollable>) -> wasmtime::Result<bool> {
        self.is_ready(pollable.rep())
    }

    fn block(&mut self, pollable: Resource<Pollable>) -> wasmtime::Result<()> {
        if self.is_ready(pollable.rep())? {
            return Ok(());
        }
        Err(refuse_sleep())
    }

    fn drop(&mut self, pollable: Resource<Pollable>) -> wasmtime::Result<()> {
        self.timers.remove(&pollable.rep());
        Ok(())
    }
}

impl streams::Host for WasiState {}

impl streams::HostInputStream for WasiState {
    fn read(
        &mut self,
        _stream: Resource<InputStream>,
        _len: u64,
    ) -> wasmtime::Result<Result<Vec<u8>, StreamError>> {
        Ok(Err(StreamError::Closed))
    }

    fn blocking_read(
        &mut self,
        _stream: Resource<InputStream>,
        _len: u64,
    ) -> wasmtime::Result<Result<Vec<u8>, StreamError>> {
        Ok(Err(StreamError::Closed))
    }

    fn skip(
        &mut self,
        _stream: Resource<InputStream>,
        _len: u64,
    ) -> wasmtime::Result<Result<u64, StreamError>> {
        Ok(Err(StreamError::Closed))
    }

    fn blocking_skip(
        &mut self,
        _stream: Resource<InputStream>,
        _len: u64,
    ) -> wasmtime::Result<Result<u64, StreamError>> {
        Ok(Err(StreamError::Closed))
    }

    fn subscribe(
        &mut self,
        _stream: Resource<InputStream>,
    ) -> wasmtime::Result<Resource<Pollable>> {
        Ok(ready())
    }

    fn drop(&mut self, _stream: Resource<InputStream>) -> wasmtime::Result<()> {
        Ok(())
    }
}

impl streams::HostOutputStream for WasiState {
    /// Every write may carry up to one permit; the buffer's limit is applied when appending.
    fn check_write(
        &mut self,
        stream: Resource<OutputStream>,
    ) -> wasmtime::Result<Result<u64, StreamError>> {
        self.output(&stream)?.permit = WRITE_PERMIT_BYTES;
        Ok(Ok(WRITE_PERMIT_BYTES))
    }

    fn write(
        &mut self,
        stream: Resource<OutputStream>,
        contents: Vec<u8>,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        self.spend_permit(&stream, contents.len() as u64)?;
        self.output(&stream)?.append(&contents);
        Ok(Ok(()))
    }

    fn blocking_write_and_flush(
        &mut self,
        stream: Resource<OutputStream>,
        contents: Vec<u8>,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        self.write_and_flush(&stream, &contents)
    }

    fn flush(
        &mut self,
        _stream: Resource<OutputStream>,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        Ok(Ok(()))
    }

    fn blocking_flush(
        &mut self,
        _stream: Resource<OutputStream>,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        Ok(Ok(()))
    }

    fn subscribe(
        &mut self,
        _stream: Resource<OutputStream>,
    ) -> wasmtime::Result<Resource<Pollable>> {
        Ok(ready())
    }

    fn write_zeroes(
        &mut self,
        stream: Resource<OutputStream>,
        len: u64,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        self.spend_permit(&stream, len)?;
        // The permit bounds `len`, so the host never allocates on the plugin's say-so.
        let zeroes = vec![0; usize::try_from(len)?];
        self.output(&stream)?.append(&zeroes);
        Ok(Ok(()))
    }

    fn blocking_write_zeroes_and_flush(
        &mut self,
        stream: Resource<OutputStream>,
        len: u64,
    ) -> wasmtime::Result<Result<(), StreamError>> {
        if len > WRITE_PERMIT_BYTES {
            return Err(stop("the plugin wrote more than 4096 bytes in one call"));
        }
        let zeroes = vec![0; usize::try_from(len)?];
        self.write_and_flush(&stream, &zeroes)
    }

    // The only input stream is closed, so there is nothing to splice.
    fn splice(
        &mut self,
        _stream: Resource<OutputStream>,
        _source: Resource<InputStream>,
        _len: u64,
    ) -> wasmtime::Result<Result<u64, StreamError>> {
        Ok(Err(StreamError::Closed))
    }

    fn blocking_splice(
        &mut self,
        _stream: Resource<OutputStream>,
        _source: Resource<InputStream>,
        _len: u64,
    ) -> wasmtime::Result<Result<u64, StreamError>> {
        Ok(Err(StreamError::Closed))
    }

    fn drop(&mut self, _stream: Resource<OutputStream>) -> wasmtime::Result<()> {
        Ok(())
    }
}
