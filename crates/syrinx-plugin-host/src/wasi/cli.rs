//! `wasi:cli`: no environment, no arguments, no terminal, and an `exit` that stops the call.

use wasmtime::component::Resource;

use super::bindings::wasi::cli::{
    environment, exit, stderr, stdin, stdout, terminal_input, terminal_output, terminal_stderr,
    terminal_stdin, terminal_stdout,
};
use super::bindings::wasi::io::streams::{InputStream, OutputStream};
use super::streams::{STDERR_REP, STDIN_REP, STDOUT_REP};
use super::{WasiState, stop};

impl environment::Host for WasiState {
    fn get_environment(&mut self) -> wasmtime::Result<Vec<(String, String)>> {
        Ok(Vec::new())
    }

    fn get_arguments(&mut self) -> wasmtime::Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn initial_cwd(&mut self) -> wasmtime::Result<Option<String>> {
        Ok(None)
    }
}

impl exit::Host for WasiState {
    /// A plugin never ends Syrinx; exiting stops the call with a named reason.
    fn exit(&mut self, status: Result<(), ()>) -> wasmtime::Result<()> {
        Err(stop(format!("the plugin called exit with {status:?}")))
    }

    fn exit_with_code(&mut self, status_code: u8) -> wasmtime::Result<()> {
        Err(stop(format!(
            "the plugin called exit with code {status_code}"
        )))
    }
}

impl stdin::Host for WasiState {
    fn get_stdin(&mut self) -> wasmtime::Result<Resource<InputStream>> {
        Ok(Resource::new_own(STDIN_REP))
    }
}

impl stdout::Host for WasiState {
    fn get_stdout(&mut self) -> wasmtime::Result<Resource<OutputStream>> {
        Ok(Resource::new_own(STDOUT_REP))
    }
}

impl stderr::Host for WasiState {
    fn get_stderr(&mut self) -> wasmtime::Result<Resource<OutputStream>> {
        Ok(Resource::new_own(STDERR_REP))
    }
}

impl terminal_input::Host for WasiState {}

impl terminal_input::HostTerminalInput for WasiState {
    fn drop(&mut self, _terminal: Resource<terminal_input::TerminalInput>) -> wasmtime::Result<()> {
        Ok(())
    }
}

impl terminal_output::Host for WasiState {}

impl terminal_output::HostTerminalOutput for WasiState {
    fn drop(
        &mut self,
        _terminal: Resource<terminal_output::TerminalOutput>,
    ) -> wasmtime::Result<()> {
        Ok(())
    }
}

impl terminal_stdin::Host for WasiState {
    fn get_terminal_stdin(
        &mut self,
    ) -> wasmtime::Result<Option<Resource<terminal_input::TerminalInput>>> {
        Ok(None)
    }
}

impl terminal_stdout::Host for WasiState {
    fn get_terminal_stdout(
        &mut self,
    ) -> wasmtime::Result<Option<Resource<terminal_output::TerminalOutput>>> {
        Ok(None)
    }
}

impl terminal_stderr::Host for WasiState {
    fn get_terminal_stderr(
        &mut self,
    ) -> wasmtime::Result<Option<Resource<terminal_output::TerminalOutput>>> {
        Ok(None)
    }
}
