//! Compares WebKitGTK history reports with the shared NavigationList after real loads and jumps.

use super::{
    fixture::{Fixture, reserve_profile},
    lifecycle::require,
    server::Server,
};
use std::time::Duration;
use syrinx_core::HistoryDirection;
use syrinx_platform::browser::{BrowserCommand, BrowserEvent};

pub fn run() -> Result<(), String> {
    let mut server = Server::start()?;
    let mut fixture = Fixture::new(reserve_profile()?)?;
    fixture.create(1, server.url("/p/40"))?;
    fixture.at(&server.url("/p/40"))?;
    for page in 41..=44 {
        navigate(&mut fixture, &server, &format!("/p/{page}"))?;
    }
    jump(&mut fixture, &server, HistoryDirection::Back, 3, "/p/41")?;
    rows(&fixture, &server, HistoryDirection::Forward, &[42, 43, 44])?;
    jump(&mut fixture, &server, HistoryDirection::Forward, 2, "/p/43")?;
    rows(&fixture, &server, HistoryDirection::Back, &[42, 41, 40])?;
    fixture.send(BrowserCommand::Navigate {
        id: 1,
        url: server.url("/p/45?replace"),
    })?;
    fixture.at(&server.url("/p/45r"))?;
    rows(&fixture, &server, HistoryDirection::Back, &[43, 42, 41, 40])?;
    fixture.send(BrowserCommand::Reload { id: 1 })?;
    fixture.wait("reload complete", |event, _| {
        matches!(
            event,
            BrowserEvent::NavigationCompleted {
                id: 1,
                success: true,
                ..
            }
        )
    })?;
    rows(&fixture, &server, HistoryDirection::Back, &[43, 42, 41, 40])?;
    fixture.send(BrowserCommand::Navigate {
        id: 1,
        url: server.url("/p/push"),
    })?;
    fixture.at(&server.url("/p/pushed"))?;
    navigate(&mut fixture, &server, "/p/pushed#fragment")?;
    fixture.send(BrowserCommand::Navigate {
        id: 1,
        url: server.url("/p/302"),
    })?;
    fixture.at(&server.url("/p/redirected"))?;
    fixture.send(BrowserCommand::Navigate {
        id: 1,
        url: server.url("/p/script-redirect"),
    })?;
    fixture.at(&server.url("/p/script-redirect"))?;
    fixture.at(&server.url("/p/redirected"))?;
    fixture.stop(Duration::from_secs(5))?;
    server.finish()?;
    println!(
        "Linux navigation_history_spike: real loads, 3/2-row jumps, replaceState, reload, pushState, fragment and redirects passed."
    );
    Ok(())
}

fn navigate(fixture: &mut Fixture, server: &Server, path: &str) -> Result<(), String> {
    fixture.send(BrowserCommand::Navigate {
        id: 1,
        url: server.url(path),
    })?;
    fixture.at(&server.url(path))
}

fn jump(
    fixture: &mut Fixture,
    server: &Server,
    direction: HistoryDirection,
    steps: usize,
    path: &str,
) -> Result<(), String> {
    require(
        fixture.list.start_jump(direction, steps),
        "Shared list refused an available jump",
    )?;
    fixture.send(BrowserCommand::GoToOffset {
        id: 1,
        direction,
        steps: steps as u32,
    })?;
    fixture.at(&server.url(path))
}

fn rows(
    fixture: &Fixture,
    server: &Server,
    direction: HistoryDirection,
    pages: &[u32],
) -> Result<(), String> {
    let actual: Vec<_> = fixture
        .list
        .rows(direction, 20)
        .iter()
        .map(|row| row.address.to_owned())
        .collect();
    let expected: Vec<_> = pages
        .iter()
        .map(|page| server.url(&format!("/p/{page}")))
        .collect();
    require(
        actual == expected,
        "History row order differs from authored navigation sequence",
    )
}
