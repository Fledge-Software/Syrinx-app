//! Exercises authored navigation semantics and the shared list's intentional same-document limits.

use super::{Fixture, Server, reserve_profile};
use std::time::Duration;
use syrinx_core::{HistoryChange, HistoryDirection};
use syrinx_platform::browser::{BrowserCommand, BrowserEvent};

pub(super) fn run(server: &Server) -> Result<(), String> {
    for source in ["/h/anchor", "/h/assign", "/h/location-replace"] {
        script_move(server, source)?;
    }
    script_back(server)?;
    before_unload(server)?;
    cacheable(server)?;
    for (source, changed) in [
        ("/h/push", "/h/pushed"),
        ("/h/replace", "/h/replaced"),
        ("/h/fragment", "/h/fragment#section"),
    ] {
        state_traversal(server, source, changed)?;
    }
    Ok(())
}

/// Only served documents author script; the host never executes script in a page.
pub(super) fn response(route: &str) -> Option<String> {
    let script = match route {
        "/h/anchor" => {
            "const a=document.createElement('a');a.href='/h/landing';document.body.append(a);a.click();"
        }
        "/h/assign" => "location.assign('/h/landing');",
        "/h/location-replace" => "location.replace('/h/landing');",
        "/h/script-back" => "history.back();",
        "/h/push" => "history.pushState({},'', '/h/pushed');",
        "/h/replace" => "history.replaceState({},'', '/h/replaced');",
        "/h/fragment" => {
            "const a=document.createElement('a');a.href='#section';document.body.append(a);a.click();"
        }
        "/h/unload" => {
            "addEventListener('beforeunload',event=>{event.preventDefault();event.returnValue='';});"
        }
        _ if route.starts_with("/h/") || route.starts_with("/c/") => "",
        _ => return None,
    };
    let script = if route == "/h/unload" {
        script.to_owned()
    } else {
        format!(
            "addEventListener('load',()=>setTimeout(()=>{{if(sessionStorage.getItem('{route}'))return;sessionStorage.setItem('{route}','done');{script}}},150));"
        )
    };
    Some(format!(
        "<!doctype html><meta charset=utf-8><title>Synthetic history</title><link rel=icon href=data:,><p>Synthetic history document</p><div id=section>section</div><script>{script}</script>"
    ))
}

fn fresh(server: &Server, path: &str) -> Result<Fixture, String> {
    let mut fixture = Fixture::new(reserve_profile()?)?;
    // Authored timers need a shown owned parent; no external focus helper or gesture is used.
    fixture.parent.show()?;
    fixture.create(1, server.url(path))?;
    report(
        &mut fixture,
        server,
        path,
        HistoryChange::NewDocument {
            user_initiated: true,
        },
    )?;
    fixture.loaded(1)?;
    Ok(fixture)
}

fn script_move(server: &Server, source: &str) -> Result<(), String> {
    let mut fixture = fresh(server, "/h/base")?;
    navigate(&mut fixture, server, source)?;
    report(
        &mut fixture,
        server,
        "/h/landing",
        HistoryChange::NewDocument {
            user_initiated: false,
        },
    )?;
    // An ungestured document move hides its source, while retaining the earlier host visit.
    rows(
        &fixture,
        server,
        "authored document move",
        &["/h/base"],
        &[],
    )?;
    fixture.stop(Duration::from_secs(5))
}

fn script_back(server: &Server) -> Result<(), String> {
    let mut fixture = fresh(server, "/h/base")?;
    navigate(&mut fixture, server, "/h/script-back")?;
    report(
        &mut fixture,
        server,
        "/h/base",
        HistoryChange::BackOrForward,
    )?;
    rows(
        &fixture,
        server,
        "authored history.back",
        &[],
        &["/h/script-back"],
    )?;
    fixture.stop(Duration::from_secs(5))
}

fn before_unload(server: &Server) -> Result<(), String> {
    let mut fixture = fresh(server, "/h/unload")?;
    // No key/click gesture was delivered, so beforeunload cannot hold this navigation in a dialog.
    navigate(&mut fixture, server, "/h/after")?;
    fixture.loaded(1)?;
    rows(
        &fixture,
        server,
        "beforeunload without gesture",
        &["/h/unload"],
        &[],
    )?;
    traverse(
        &mut fixture,
        server,
        HistoryDirection::Back,
        1,
        "/h/unload",
        HistoryChange::BackOrForward,
        true,
    )?;
    rows(&fixture, server, "Back to beforeunload", &[], &["/h/after"])?;
    fixture.stop(Duration::from_secs(5))
}

fn cacheable(server: &Server) -> Result<(), String> {
    let mut fixture = fresh(server, "/c/1")?;
    for path in ["/c/2", "/c/3", "/c/4"] {
        navigate(&mut fixture, server, path)?;
        fixture.loaded(1)?;
    }
    traverse(
        &mut fixture,
        server,
        HistoryDirection::Back,
        3,
        "/c/1",
        HistoryChange::BackOrForward,
        true,
    )?;
    rows(
        &fixture,
        server,
        "cacheable Back three",
        &[],
        &["/c/2", "/c/3", "/c/4"],
    )?;
    fixture.barrier()?;
    traverse(
        &mut fixture,
        server,
        HistoryDirection::Forward,
        1,
        "/c/2",
        HistoryChange::BackOrForward,
        true,
    )?;
    rows(
        &fixture,
        server,
        "cacheable Forward one",
        &["/c/1"],
        &["/c/3", "/c/4"],
    )?;
    fixture.stop(Duration::from_secs(5))
}

fn state_traversal(server: &Server, source: &str, changed: &str) -> Result<(), String> {
    let mut fixture = fresh(server, "/h/base")?;
    navigate(&mut fixture, server, source)?;
    report(
        &mut fixture,
        server,
        changed,
        HistoryChange::SamePage { navigating: false },
    )?;
    rows(
        &fixture,
        server,
        "same-document change collapses current row",
        &["/h/base"],
        &[],
    )?;
    navigate(&mut fixture, server, "/h/after")?;
    fixture.loaded(1)?;
    rows(
        &fixture,
        server,
        "document after same-document change",
        &[changed, "/h/base"],
        &[],
    )?;
    traverse(
        &mut fixture,
        server,
        HistoryDirection::Back,
        1,
        changed,
        HistoryChange::BackOrForward,
        true,
    )?;
    rows(
        &fixture,
        server,
        "Back to renamed entry",
        &["/h/base"],
        &["/h/after"],
    )?;
    fixture.barrier()?;
    if source == "/h/replace" {
        traverse(
            &mut fixture,
            server,
            HistoryDirection::Back,
            1,
            "/h/base",
            HistoryChange::BackOrForward,
            true,
        )?;
        rows(
            &fixture,
            server,
            "replaceState added no engine entry",
            &[],
            &["/h/replaced", "/h/after"],
        )?;
        return fixture.stop(Duration::from_secs(5));
    }
    // WebKit retains the pre-push/fragment engine entry; the projected jump names /h/base.
    traverse(
        &mut fixture,
        server,
        HistoryDirection::Back,
        1,
        source,
        HistoryChange::SamePage { navigating: false },
        true,
    )?;
    rows(
        &fixture,
        server,
        "untracked same-document landing resets",
        &[],
        &[],
    )?;
    fixture.barrier()?;
    // Back/Forward remains available through engine reach even after the projected rows reset.
    traverse(
        &mut fixture,
        server,
        HistoryDirection::Forward,
        1,
        changed,
        HistoryChange::SamePage { navigating: false },
        false,
    )?;
    rows(
        &fixture,
        server,
        "same-document Forward after reset",
        &[],
        &[],
    )?;
    fixture.stop(Duration::from_secs(5))
}

fn navigate(fixture: &mut Fixture, server: &Server, path: &str) -> Result<(), String> {
    fixture.barrier()?;
    fixture.send(BrowserCommand::Navigate {
        id: 1,
        url: server.url(path),
    })?;
    report(
        fixture,
        server,
        path,
        HistoryChange::NewDocument {
            user_initiated: true,
        },
    )
}

fn traverse(
    fixture: &mut Fixture,
    server: &Server,
    direction: HistoryDirection,
    steps: u32,
    path: &str,
    change: HistoryChange,
    projected: bool,
) -> Result<(), String> {
    if projected {
        fixture.start_jump("authored-history traversal", direction, steps as usize)?;
    } else {
        let reach = fixture.list.engine_reach();
        let available = match direction {
            HistoryDirection::Back => reach.back,
            HistoryDirection::Forward => reach.forward,
        };
        super::super::lifecycle::require(
            available,
            "Engine traversal unavailable after projected reset",
        )?;
    }
    fixture.send(BrowserCommand::GoToOffset {
        id: 1,
        direction,
        steps,
    })?;
    report(fixture, server, path, change)
}

fn report(
    fixture: &mut Fixture,
    server: &Server,
    path: &str,
    change: HistoryChange,
) -> Result<(), String> {
    let address = server.url(path);
    let step = format!("history report {path}: {change:?}");
    fixture.wait_checked(&step, |event, _| {
        if matches!(
            event,
            BrowserEvent::NavigationCompleted {
                id: 1,
                success: false,
                cancelled: false
            }
        ) {
            return Err(format!("Navigation failed at {step}"));
        }
        let BrowserEvent::History { id: 1, report } = event else {
            return Ok(false);
        };
        if report.address != address {
            return Ok(false);
        }
        if report.change != change {
            return Err(format!(
                "{step}: actual kind={:?}, expected={change:?}",
                report.change
            ));
        }
        Ok(true)
    })?;
    fixture.at(&step, &address)
}

fn rows(
    fixture: &Fixture,
    server: &Server,
    step: &str,
    back: &[&str],
    forward: &[&str],
) -> Result<(), String> {
    for (direction, paths) in [
        (HistoryDirection::Back, back),
        (HistoryDirection::Forward, forward),
    ] {
        fixture.expect_rows(
            step,
            direction,
            &paths
                .iter()
                .map(|path| server.url(path))
                .collect::<Vec<_>>(),
        )?;
    }
    Ok(())
}
