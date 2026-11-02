//! Shares bounded native fixtures across the three Linux example entry points.

mod cookies;
mod fixture;
mod history;
mod lifecycle;
mod server;

pub fn main(suite: &str) -> std::process::ExitCode {
    // Match at runtime so every suite's common helpers receive the same lint coverage.
    fixture::run(|| match suite {
        "page_close" => lifecycle::run(),
        "cookie_persistence" => cookies::run(),
        "navigation_history_spike" => history::run(),
        _ => Err("Unknown Linux fixture".into()),
    })
}
