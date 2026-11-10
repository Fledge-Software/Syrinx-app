//! Requires an adopted native popup to retain its opener, reply and actual document focus.

use super::{
    fixture::{Fixture, reserve_profile},
    lifecycle::require,
    server::Server,
};
use std::time::Duration;
use syrinx_platform::browser::{BrowserCommand, BrowserEvent};

const OPENER: &str = r#"<!doctype html><title>popup-ready</title><link rel=icon href=data:,><p>Enter opens a synthetic popup.</p><script>
window.syntheticMarker='syrinx synthetic opener';let child;
addEventListener('message',event=>{if(event.origin===location.origin&&event.source===child&&event.data==='opener read')document.title='popup-opener-ok';});
addEventListener('keydown',event=>{if(event.key==='Enter')child=window.open('/popup-child');});
</script>"#;
const CHILD: &str = r#"<!doctype html><title>popup-child-ready</title><link rel=icon href=data:,><input autofocus aria-label='Synthetic popup input'><script>
if(window.opener&&window.opener.syntheticMarker==='syrinx synthetic opener'){
window.opener.postMessage('opener read',location.origin);
const report=()=>{if(document.hasFocus())document.title='popup-focus-ok';};
addEventListener('focus',report);addEventListener('load',report);
}else document.title='popup-opener-broken';
</script>"#;

pub fn response(route: &str) -> Option<&'static str> {
    match route {
        "/popup-opener" => Some(OPENER),
        "/popup-child" => Some(CHILD),
        _ => None,
    }
}

pub fn run() -> Result<(), String> {
    let mut server = Server::start()?;
    let mut fixture = Fixture::new(reserve_profile()?)?;
    fixture.parent.show()?;
    fixture.create(1000, server.url("/popup-opener"))?;
    fixture.loaded(1000)?;
    fixture.focus_page(1000)?;
    let opener = fixture.only_child()?;
    fixture.parent.key(opener, 0xff0d)?;
    let mut request = None;
    fixture.wait_checked("gesture popup request", |event, _| {
        if let BrowserEvent::PopupRequested {
            id: 1000,
            request: actual,
            url,
        } = event
        {
            require(*url == server.url("/popup-child"), "Popup target changed")?;
            request = Some(*actual);
            return Ok(true);
        }
        require(
            !matches!(event, BrowserEvent::PopupBlocked { id: 1000 }),
            "Gesture popup was blocked",
        )?;
        Ok(false)
    })?;
    fixture.send(BrowserCommand::AdoptPopup {
        request: request.ok_or("Missing popup request")?,
        id: 1001,
    })?;
    let (mut created, mut replied, mut focused) = (false, false, false);
    // Focus commands are FIFO behind adoption; collect all three independent reports together.
    fixture.send(BrowserCommand::ShowActive(Some(1001)))?;
    fixture.send(BrowserCommand::Focus { id: 1001 })?;
    fixture.wait_checked("adopted document focus and opener reply", |event, _| {
        match event {
            BrowserEvent::Created { id: 1001 } => created = true,
            BrowserEvent::NavigationCompleted {
                id: 1000 | 1001,
                success: false,
                cancelled: false,
            } => {
                return Err("Synthetic popup navigation failed".into());
            }
            BrowserEvent::Title { id: 1000, title } if title == "popup-opener-ok" => replied = true,
            BrowserEvent::Title { id: 1001, title } if title == "popup-focus-ok" => focused = true,
            BrowserEvent::Title { id: 1001, title } if title == "popup-opener-broken" => {
                return Err("Adopted popup lost its opener".into());
            }
            _ => {}
        }
        Ok(created && replied && focused)
    })?;
    fixture.barrier()?;
    let children = fixture.parent.children()?;
    require(
        children.len() == 2 && children.contains(&opener),
        "Adoption changed native child ownership",
    )?;
    let child = *children
        .iter()
        .find(|child| **child != opener)
        .ok_or("Missing popup child")?;
    for active in [Some(1001), None, Some(1000), Some(1001)] {
        fixture.send(BrowserCommand::ShowActive(active))?;
        fixture.barrier()?;
        require(
            fixture.parent.visible(child)? == (active == Some(1001)),
            "Adopted popup visibility mismatch",
        )?;
        require(
            fixture.parent.visible(opener)? == (active == Some(1000)),
            "Opener visibility mismatch",
        )?;
    }
    fixture.stop(Duration::from_secs(5))?;
    require(
        fixture.parent.children()?.is_empty(),
        "Popup shutdown leaked native children",
    )?;
    server.finish()?;
    println!(
        "Linux popups: adoption, opener marker/reply, document focus and child visibility passed."
    );
    Ok(())
}
