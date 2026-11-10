//! Exercises real page downloads, popup ownership, shared close deferral and cancellation.

use super::{
    environment::verify_downloads,
    fixture::{Fixture, reserve_profile},
    lifecycle::require,
    server::Server,
};
use std::{fs, path::Path, time::Duration};
use syrinx_core::{DownloadControl, DownloadInterruption, DownloadState};
use syrinx_platform::{acceptance, browser::BrowserCommand};

pub mod files;
mod observation;
use files::{CANCEL, CLOSING, HELD, POPUP, SAME};
use observation::Watch;

const OPENER: u64 = 5000;
const CHILD: u64 = 5001;

pub fn response(route: &str) -> Option<&'static str> {
    match route {
        "/download-same" => Some(
            r#"<!doctype html><title>download-ready</title><link rel=icon href=data:,><script>addEventListener('keydown',e=>{if(e.key==='Enter')location.href='/download/synthetic-same.zip'});</script>"#,
        ),
        "/download-popup" => Some(
            r#"<!doctype html><title>download-ready</title><link rel=icon href=data:,><script>addEventListener('keydown',e=>{if(e.key==='Enter')window.open('/download/synthetic-popup.zip')});</script>"#,
        ),
        "/download-closing" => Some(
            r#"<!doctype html><title>download-ready</title><link rel=icon href=data:,><script>addEventListener('keydown',e=>{if(e.key==='Enter')window.open('/download-closing-child')});</script>"#,
        ),
        "/download-held" => Some(
            r#"<!doctype html><title>download-ready</title><link rel=icon href=data:,><script>addEventListener('keydown',e=>{if(e.key==='Enter')window.open('/download-held-child')});</script>"#,
        ),
        "/download-cancel" => Some(
            r#"<!doctype html><title>download-ready</title><link rel=icon href=data:,><script>addEventListener('keydown',e=>{if(e.key==='Enter')location.href='/download/synthetic-cancel.zip'});</script>"#,
        ),
        "/download-closing-child" => Some(
            r#"<!doctype html><title>closing-download</title><link rel=icon href=data:,><script>addEventListener('load',()=>{location.href='/download/synthetic-closing.zip';setTimeout(()=>window.close(),300)});</script>"#,
        ),
        "/download-held-child" => Some(
            r#"<!doctype html><title>held-download</title><link rel=icon href=data:,><script>addEventListener('load',()=>{location.href='/download/synthetic-held.zip';setTimeout(()=>window.close(),300)});</script>"#,
        ),
        _ => None,
    }
}

pub fn run(folder: &Path) -> Result<(), String> {
    verify_downloads(folder, acceptance::fixture_download_directory()?)?;
    require(
        fs::read_dir(folder)
            .map_err(|error| error.to_string())?
            .next()
            .is_none(),
        "Scratch Downloads must start empty",
    )?;
    for (route, name, popup) in [
        ("/download-same", SAME, None),
        (
            "/download-popup",
            POPUP,
            Some("/download/synthetic-popup.zip"),
        ),
        (
            "/download-closing",
            CLOSING,
            Some("/download-closing-child"),
        ),
        ("/download-held", HELD, Some("/download-held-child")),
        ("/download-cancel", CANCEL, None),
    ] {
        exercise(folder, route, name, popup)?;
    }
    require(
        clean_destination(folder)?,
        "Cancelled download left a destination or partial file in scratch Downloads",
    )?;
    println!(
        "Linux downloads: same tab, adopted/self-closing popups, held close deferral, exact bytes and Cancel cleanup passed. WebKit has no pause/resume or engine download-dialog API; no SmartScreen counterpart is claimed."
    );
    Ok(())
}

/// Require only the four complete files, including while the cancelled host remains alive.
fn clean_destination(folder: &Path) -> Result<bool, String> {
    let mut actual = fs::read_dir(folder)
        .map_err(|error| error.to_string())?
        .map(|entry| {
            entry
                .map(|entry| entry.file_name())
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    actual.sort();
    let mut expected: Vec<std::ffi::OsString> = [SAME, POPUP, CLOSING, HELD]
        .into_iter()
        .map(Into::into)
        .collect();
    expected.sort();
    Ok(actual == expected)
}

fn exercise(folder: &Path, route: &str, name: &str, popup: Option<&str>) -> Result<(), String> {
    println!("Linux download case: {name}");
    let mut server = Server::start()?;
    let mut fixture = Fixture::new(reserve_profile()?)?;
    fixture.parent.show()?;
    fixture.create(OPENER, server.url(route))?;
    fixture.loaded(OPENER)?;
    fixture.focus_page(OPENER)?;
    let opener = fixture.only_child()?;
    let mut watch = Watch::new(name, popup.map(|route| server.url(route)));
    // Fail closed immediately before the gesture that can create a destination.
    verify_downloads(folder, acceptance::fixture_download_directory()?)?;
    fixture.parent.key(opener, 0xff0d)?;
    if name == HELD {
        until(&mut fixture, &mut watch, |watch| {
            watch.deferred && watch.receiving()
        })?;
        fixture.barrier_checked(|event| watch.inspect_without_command(event))?;
        require(
            !watch.closed,
            "Shared close deferral released an active download",
        )?;
        let children = fixture.parent.children()?;
        require(
            children.len() == 2 && children.contains(&opener),
            "Held download lost its native popup or opener before completion",
        )?;
        server.release_download(HELD)?;
    }
    if name == CANCEL {
        until(&mut fixture, &mut watch, Watch::receiving)?;
        cancel(&mut fixture, &mut watch, &server, folder)?;
    } else {
        until(&mut fixture, &mut watch, |watch| {
            watch.completed() && (!matches!(name, CLOSING | HELD) || watch.closed)
        })?;
        let saved = fs::read(folder.join(name)).map_err(|error| error.to_string())?;
        require(
            saved == files::body(name),
            "Downloaded file bytes differ from the synthetic attachment",
        )?;
    }
    fixture.barrier_checked(|event| watch.inspect_without_command(event))?;
    if watch.closed {
        require(
            fixture.parent.children()? == vec![opener],
            "Completed self-closing popup kept its native child",
        )?;
    }
    fixture.stop(Duration::from_secs(5))?;
    server.finish()?;
    println!("Linux download case passed: {name}");
    Ok(())
}

fn cancel(
    fixture: &mut Fixture,
    watch: &mut Watch,
    server: &Server,
    folder: &Path,
) -> Result<(), String> {
    let download = watch.seen()?;
    require(
        download.offered_controls(false) == [DownloadControl::Cancel],
        "WebKit download offered unsupported controls",
    )?;
    fixture.send(BrowserCommand::ControlDownload {
        download: download.id,
        control: DownloadControl::Cancel,
    })?;
    until(fixture, watch, |watch| {
        watch.seen().is_ok_and(|download| {
            download.state == DownloadState::Interrupted(DownloadInterruption::Cancelled)
        })
    })?;
    require(
        watch.seen()?.offered_controls(false).is_empty(),
        "Cancelled download still offers controls",
    )?;
    server.release_download(CANCEL)?;
    // Keep observing the terminal state after the server is allowed to finish its remaining write.
    fixture.watch_for(
        "cancelled transfer stays interrupted",
        Duration::from_millis(300),
        |event| watch.inspect_without_command(event),
    )?;
    let deadline = std::time::Instant::now() + super::fixture::LIMIT;
    while !clean_destination(folder)? {
        if std::time::Instant::now() >= deadline {
            return Err(
                "Cancel left a destination or partial file while the host was still alive".into(),
            );
        }
        fixture.watch_for(
            "cancel partial cleanup",
            Duration::from_millis(20),
            |event| watch.inspect_without_command(event),
        )?;
    }
    Ok(())
}

fn until(
    fixture: &mut Fixture,
    watch: &mut Watch,
    done: impl Fn(&Watch) -> bool,
) -> Result<(), String> {
    let deadline = std::time::Instant::now() + super::fixture::LIMIT;
    while !done(watch) {
        if std::time::Instant::now() >= deadline {
            return Err("Download scenario exceeded its event deadline".into());
        }
        let mut command = None;
        fixture.wait_checked("native download lifecycle", |event, _| {
            command = watch.observe(event)?;
            Ok(command.is_some() || done(watch))
        })?;
        if let Some(command) = command {
            fixture.send(command)?;
        }
    }
    Ok(())
}
