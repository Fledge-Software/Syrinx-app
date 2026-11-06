//! Verifies native views outlive page-requested closure until the durable host command.

use super::{
    fixture::{Fixture, reserve_profile},
    server::Server,
};
use std::time::Duration;
use syrinx_platform::{
    acceptance,
    browser::{BrowserCommand, BrowserError, BrowserEvent},
};

pub fn run() -> Result<(), String> {
    acceptance::check_native_probes()?;
    let mut server = Server::start()?;
    let mut fixture = Fixture::new(reserve_profile()?)?;
    fixture.create(1, server.url("/survivor"))?;
    fixture.barrier()?;
    let survivor = fixture.parent.children()?;
    require(
        survivor.len() == 1,
        "Survivor must have exactly one native child",
    )?;
    for id in 100..112 {
        fixture.create(id, server.url("/close"))?;
        fixture.wait("page close request", |event, _| matches!(event, BrowserEvent::CloseRequested { id: actual } if *actual == id))?;
        let children = fixture.parent.children()?;
        require(
            children.len() == 2,
            "Page-requested close destroyed its view before host approval",
        )?;
        let closing = *children
            .iter()
            .find(|child| !survivor.contains(child))
            .ok_or("Missing closing page child")?;
        fixture.send(BrowserCommand::Close { id })?;
        fixture.barrier()?;
        require(
            !acceptance::window_exists(closing),
            "Closed page still has an X11 window",
        )?;
        require(
            fixture.parent.children()? == survivor,
            "Closing a page affected the survivor",
        )?;
        fixture.send(BrowserCommand::Navigate {
            id,
            url: server.url("/stale"),
        })?;
        fixture.wait("closed ID rejection", |event, _| matches!(event, BrowserEvent::Error { tab: Some(actual), error: BrowserError::UnknownTab } if *actual == id))?;
    }
    fixture.send(BrowserCommand::Create {
        id: 9,
        url: "javascript:1".into(),
    })?;
    fixture.wait("unsafe creation rejection", |event, _| {
        matches!(
            event,
            BrowserEvent::CreationFailed {
                id: 9,
                error: BrowserError::InvalidUrl
            }
        )
    })?;
    require(
        fixture.parent.children()? == survivor,
        "Rejected creation leaked a child",
    )?;
    fixture.stop(Duration::from_secs(5))?;
    require(
        fixture.parent.children()?.is_empty(),
        "Stopped host left native page children",
    )?;
    require(
        !acceptance::window_exists(survivor[0]),
        "Survivor outlived host shutdown",
    )?;
    server.finish()?;
    println!(
        "Linux page_close: 12 deferred closes, exact native-child release, stale commands, unsafe creation and shutdown passed."
    );
    Ok(())
}

pub fn require(condition: bool, reason: &str) -> Result<(), String> {
    if !condition {
        return Err(reason.into());
    }
    Ok(())
}
