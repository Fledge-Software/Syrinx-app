//! Verifies native views outlive page-requested closure until the durable host command.

use super::{
    fixture::{Fixture, reserve_profile},
    server::Server,
};
use std::time::Duration;
use syrinx_platform::{
    acceptance,
    browser::{BrowserCommand, BrowserError, BrowserEvent, PageBounds},
};

pub fn run() -> Result<(), String> {
    acceptance::check_native_probes()?;
    let mut server = Server::start()?;
    let mut fixture = Fixture::new(reserve_profile()?)?;
    initial_states(&mut fixture, &server)?;
    fixture.barrier()?;
    let survivor = fixture.parent.children()?;
    require(
        survivor.len() == 1,
        "Survivor must have exactly one native child",
    )?;
    for id in 100..112 {
        fixture.create(id, server.url("/close"))?;
        fixture.wait("page close request", |event, _| {
            matches!(event, BrowserEvent::CloseRequested { id: actual } if *actual == id)
        })?;
        let children = fixture.parent.children()?;
        require(
            children.len() == 2,
            "Page-requested close destroyed its view before host approval",
        )?;
        let closing = *children
            .iter()
            .find(|child| !survivor.contains(child))
            .ok_or("Missing closing page child")?;
        for active in [Some(id), None, Some(1), Some(id)] {
            fixture.send(BrowserCommand::ShowActive(active))?;
            fixture.barrier()?;
            require(
                fixture.parent.visible(closing)? == (active == Some(id)),
                "Deferred page visibility did not follow active selection",
            )?;
            require_children(&fixture, &children)?;
            require(
                fixture.parent.visible(survivor[0])? == (active == Some(1)),
                "Deferred close changed survivor visibility",
            )?;
        }
        require(
            fixture.parent.visible(closing)?,
            "Deferred page was not usable after show/hide",
        )?;
        let bounds = PageBounds {
            x: 7 + (id - 100) as i32,
            y: 11 + (id - 100) as i32,
            width: 320 + (id - 100) as u32,
            height: 240 + (id - 100) as u32,
        };
        let survivor_before = fixture.parent.geometry(survivor[0])?;
        resize(&mut fixture, closing, bounds)?;
        require_geometry(
            fixture.parent.geometry(survivor[0])?,
            survivor_before,
            "Resize moved hidden survivor",
        )?;
        fixture.send(BrowserCommand::Navigate {
            id,
            url: server.url("/deferred-usable"),
        })?;
        fixture.loaded(id)?;
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
        fixture.send(BrowserCommand::ShowActive(Some(1)))?;
        fixture.barrier()?;
        require_geometry(
            fixture.parent.geometry(survivor[0])?,
            (bounds.x, bounds.y, bounds.width, bounds.height),
            "Survivor activation did not take current bounds",
        )?;
        fixture.send(BrowserCommand::Navigate {
            id,
            url: server.url("/stale"),
        })?;
        fixture.wait("closed ID rejection", |event, _| {
            matches!(event, BrowserEvent::Error { tab: Some(actual), error: BrowserError::UnknownTab }
                if *actual == id)
        })?;
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
    fixture.create(200, server.url("/shutdown"))?;
    fixture.barrier()?;
    let active_shutdown = *fixture
        .parent
        .children()?
        .iter()
        .find(|child| !survivor.contains(child))
        .ok_or("Missing shutdown page")?;
    fixture.create(201, server.url("/shutdown"))?;
    fixture.send(BrowserCommand::ShowActive(Some(200)))?;
    fixture.barrier()?;
    let shutdown = fixture.parent.children()?;
    require(
        shutdown.len() == 3,
        "Shutdown must release three exact native children",
    )?;
    for child in &shutdown {
        require(
            fixture.parent.visible(*child)? == (*child == active_shutdown),
            "Shutdown child visibility mismatch",
        )?;
    }
    fixture.stop(Duration::from_secs(5))?;
    for child in shutdown {
        require(
            !acceptance::window_exists(child),
            "Native child outlived shutdown",
        )?;
    }
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

/// Keep creation hidden, reject invalid commands, and retry/reuse the same logical identity.
fn initial_states(fixture: &mut Fixture, server: &Server) -> Result<(), String> {
    fixture.send(BrowserCommand::Create {
        id: 1,
        url: server.url("/survivor"),
    })?;
    fixture.wait("hidden creation", |event, _| {
        matches!(event, BrowserEvent::Created { id: 1 })
    })?;
    fixture.loaded(1)?;
    let child = fixture.only_child()?;
    require(
        !fixture.parent.visible(child)?,
        "New native page was already mapped",
    )?;
    for active in [Some(1), None, Some(1)] {
        fixture.send(BrowserCommand::ShowActive(active))?;
        fixture.barrier()?;
        require(
            fixture.parent.visible(child)? == active.is_some(),
            "ShowActive did not change native visibility",
        )?;
    }
    for bounds in [
        PageBounds {
            x: 7,
            y: 11,
            width: 320,
            height: 240,
        },
        PageBounds {
            x: 19,
            y: 29,
            width: 480,
            height: 300,
        },
    ] {
        resize(fixture, child, bounds)?;
    }
    let before = fixture.parent.geometry(child)?;
    fixture.send(BrowserCommand::Resize(PageBounds {
        width: u32::MAX,
        ..PageBounds::default()
    }))?;
    fixture.wait("invalid bounds rejected", |event, _| {
        matches!(
            event,
            BrowserEvent::Error {
                tab: None,
                error: BrowserError::InvalidBounds
            }
        )
    })?;
    require_geometry(
        fixture.parent.geometry(child)?,
        before,
        "Rejected bounds moved native child",
    )?;
    for (id, url, error) in [
        (1, server.url("/duplicate"), BrowserError::DuplicateTab),
        (3, "about:settings".into(), BrowserError::InvalidUrl),
    ] {
        fixture.send(BrowserCommand::Create { id, url })?;
        fixture.wait("creation rejected", |event, _| {
            matches!(event, BrowserEvent::CreationFailed { id: actual, error: actual_error }
                if *actual == id && *actual_error == error)
        })?;
        fixture.barrier()?;
        require(
            fixture.parent.children()? == vec![child],
            "Rejected create leaked or replaced a child",
        )?;
    }
    for _ in 0..2 {
        fixture.create(3, server.url("/retry"))?;
        fixture.loaded(3)?;
        let children = fixture.parent.children()?;
        require(
            children.len() == 2 && children.contains(&child),
            "Retry lost survivor",
        )?;
        let retry = *children
            .iter()
            .find(|current| **current != child)
            .ok_or("Missing retry child")?;
        require(
            fixture.parent.visible(retry)? && !fixture.parent.visible(child)?,
            "Retry active visibility mismatch",
        )?;
        fixture.send(BrowserCommand::Close { id: 3 })?;
        fixture.barrier()?;
        require(
            !acceptance::window_exists(retry),
            "Reused ID leaked old native child",
        )?;
    }
    fixture.send(BrowserCommand::ShowActive(Some(1)))?;
    fixture.barrier()
}

fn resize(fixture: &mut Fixture, child: isize, bounds: PageBounds) -> Result<(), String> {
    fixture.send(BrowserCommand::Resize(bounds))?;
    fixture.barrier()?;
    require_geometry(
        fixture.parent.geometry(child)?,
        (bounds.x, bounds.y, bounds.width, bounds.height),
        "Native page rectangle differs from requested physical bounds",
    )
}

/// XMapRaised changes XQueryTree's stacking order, so compare identities as a set.
fn require_children(fixture: &Fixture, expected: &[isize]) -> Result<(), String> {
    let current = fixture.parent.children()?;
    require(
        current.len() == expected.len() && expected.iter().all(|child| current.contains(child)),
        "Deferred close changed native child set",
    )
}

/// Keep both physical rectangles in failures; a label alone cannot diagnose scaling or offsets.
pub(super) fn require_geometry(
    actual: (i32, i32, u32, u32),
    expected: (i32, i32, u32, u32),
    reason: &str,
) -> Result<(), String> {
    require(
        actual == expected,
        &format!("{reason}: expected {expected:?}, actual {actual:?}"),
    )
}
