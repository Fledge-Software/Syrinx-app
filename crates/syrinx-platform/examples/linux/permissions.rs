//! Keeps consent denied, verifies re-prompting and retires pending requests on navigation/close.

use super::{
    fixture::{Fixture, reserve_profile},
    lifecycle::require,
    server::Server,
};
use std::time::Duration;
use syrinx_platform::browser::{BrowserCommand, BrowserError, BrowserEvent};
use syrinx_store::PermissionKind;

pub fn response(route: &str) -> Option<&'static str> {
    (route == "/permissions").then_some(r#"<!doctype html><title>permission-ready</title><link rel=icon href=data:,><p>Enter requests synthetic notification consent.</p><script>
addEventListener('keydown',async event=>{if(event.key==='Enter')document.title='permission-'+await Notification.requestPermission();});
</script>"#)
}

pub fn run() -> Result<(), String> {
    let mut server = Server::start()?;
    let mut fixture = Fixture::new(reserve_profile()?)?;
    fixture.parent.show()?;
    // A denial is one-shot: another native view in this same profile must prompt again.
    for id in [2000, 2001] {
        let request = request_consent(&mut fixture, &server, id)?;
        fixture.send(BrowserCommand::ResolvePermission {
            id,
            request,
            allowed: false,
        })?;
        let (mut finished, mut denied) = (false, false);
        fixture.wait_checked("denied permission delivered", |event, _| {
            finished |= matches!(event, BrowserEvent::PermissionFinished { id: actual, request: actual_request }
                if *actual == id && *actual_request == request);
            denied |= matches!(event, BrowserEvent::Title { id: actual, title }
                if *actual == id && title == "permission-denied");
            reject_grant(event)?;
            Ok(finished && denied)
        })?;
        fixture.send(BrowserCommand::Close { id })?;
        fixture.barrier()?;
    }
    let request = request_consent(&mut fixture, &server, 2003)?;
    fixture.send(BrowserCommand::Navigate {
        id: 2003,
        url: server.url("/permission-recovery"),
    })?;
    fixture.wait("navigation retires pending consent", |event, _| {
        matches!(event, BrowserEvent::PermissionFinished { id: 2003, request: actual }
            if *actual == request)
    })?;
    fixture.send(BrowserCommand::ResolvePermission {
        id: 2003,
        request,
        allowed: false,
    })?;
    fixture.wait("stale denied reply rejected", |event, _| {
        matches!(
            event,
            BrowserEvent::Error {
                tab: Some(2003),
                error: BrowserError::Permission
            }
        )
    })?;
    fixture.send(BrowserCommand::Close { id: 2003 })?;
    fixture.barrier()?;
    let request = request_consent(&mut fixture, &server, 2004)?;
    fixture.send(BrowserCommand::Close { id: 2004 })?;
    fixture.send(BrowserCommand::ResolvePermission {
        id: 2004,
        request,
        allowed: false,
    })?;
    fixture.wait("closed permission owner rejected", |event, _| {
        matches!(
            event,
            BrowserEvent::Error {
                tab: Some(2004),
                error: BrowserError::UnknownTab
            }
        )
    })?;
    require(
        fixture.parent.children()?.is_empty(),
        "Pending-consent close leaked a page",
    )?;
    fixture.stop(Duration::from_secs(5))?;
    server.finish()?;
    println!(
        "Linux permissions: deny/re-prompt, navigation cancellation, stale denied reply and pending-close cleanup passed. Allow/device consent remains owner-only."
    );
    Ok(())
}

fn request_consent(fixture: &mut Fixture, server: &Server, id: u64) -> Result<u64, String> {
    fixture.create(id, server.url("/permissions"))?;
    fixture.loaded(id)?;
    fixture.focus_page(id)?;
    fixture.parent.key(fixture.only_child()?, 0xff0d)?;
    let mut request = None;
    fixture.wait_checked("notification consent request", |event, _| {
        reject_grant(event)?;
        if let BrowserEvent::PermissionRequested {
            id: actual,
            request: number,
            uri,
            kind,
        } = event
        {
            require(
                *actual == id
                    && *kind == PermissionKind::Notifications
                    && *uri == server.url("/permissions"),
                "Unexpected permission owner/origin/kind",
            )?;
            request = Some(*number);
            return Ok(true);
        }
        Ok(false)
    })?;
    request.ok_or("Missing permission request".into())
}

fn reject_grant(event: &BrowserEvent) -> Result<(), String> {
    require(
        !matches!(event,BrowserEvent::Title{title,..} if title=="permission-granted"),
        "Synthetic permission was granted without consent",
    )
}
