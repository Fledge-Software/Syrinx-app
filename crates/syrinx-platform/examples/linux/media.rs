//! Runs the supported fullscreen and muted-audio protocol independently of suspension APIs.

mod audio;
mod fullscreen;

pub fn response(route: &str) -> Option<&'static str> {
    fullscreen::response(route).or_else(|| audio::response(route))
}

pub fn run() -> Result<(), String> {
    fullscreen::run()?;
    audio::run()
}
