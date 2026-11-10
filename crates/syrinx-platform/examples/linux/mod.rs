//! Shares bounded native fixtures across the three Linux example entry points.

mod child_process;
mod controls;
mod cookies;
mod downloads;
mod environment;
mod fixture;
mod history;
mod lifecycle;
mod media;
mod permissions;
mod popups;
mod security;
mod server;
mod trace;

pub fn main(suite: &str) -> std::process::ExitCode {
    let environment = match environment::Environment::prepare() {
        Ok(environment) => environment,
        Err(error) => {
            eprintln!("Cannot prepare private native fixture environment: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };
    // SAFETY: this entry point has not started any thread or initialized any native library.
    unsafe { environment.install() };
    // Internal cookie workers still prepare fresh process-local directories before any threads.
    match cookies::child_arguments(std::env::args_os().skip(1).collect()) {
        Ok(Some(child)) => return fixture::run(|| child.run()),
        Ok(None) => {}
        Err(error) => {
            eprintln!("Invalid cookie child invocation: {error}");
            return std::process::ExitCode::FAILURE;
        }
    }
    // Match at runtime so every suite's common helpers receive the same lint coverage.
    fixture::run(|| match suite {
        "page_close" => {
            lifecycle::run()?;
            popups::run()?;
            controls::run()?;
            permissions::run()?;
            media::run()?;
            security::run()?;
            downloads::run(&environment.downloads)
        }
        "cookie_persistence" => cookies::run(),
        "navigation_history_spike" => history::run(),
        _ => Err("Unknown Linux fixture".into()),
    })
}
