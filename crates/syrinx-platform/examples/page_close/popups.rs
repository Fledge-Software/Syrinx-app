//! Proves adopted loopback popups retain opener access, reply messages and page focus.

use super::{
    BrowserCommand, BrowserEvent, BrowserHost, HWND, Instant, UnboundedReceiver, checked_event,
    send,
};
use windows::Win32::UI::WindowsAndMessaging::{SW_SHOW, SetForegroundWindow, ShowWindow};

const OPENER: &str = r#"<!doctype html><meta charset=utf-8><title>popup waiting</title>
<link rel=icon href=data:,><p>Synthetic popup opener</p><script>
window.syntheticMarker = 'syrinx synthetic opener';
let child;
addEventListener('message', event => {
  if (event.origin === location.origin && event.source === child && event.data === 'opener read')
    document.title = 'popup-opener-ok';
});
addEventListener('load', () => { child = window.open('/popup-child'); });
</script>"#;
const CHILD: &str = r#"<!doctype html><meta charset=utf-8><title>popup child</title>
<link rel=icon href=data:,><input autofocus aria-label="Synthetic popup input"><script>
if (window.opener && window.opener.syntheticMarker === 'syrinx synthetic opener') {
  window.opener.postMessage('opener read', location.origin);
  const reportFocus = () => { if (document.hasFocus()) document.title = 'popup-focus-ok'; };
  addEventListener('focus', reportFocus);
  addEventListener('load', reportFocus);
} else { document.title = 'popup-opener-broken'; }
</script>"#;

pub(super) fn response(request: &[u8]) -> Option<&'static str> {
    if request.starts_with(b"GET /popup-opener ") {
        return Some(OPENER);
    }
    if request.starts_with(b"GET /popup-child ") {
        return Some(CHILD);
    }
    None
}

pub(super) fn exercise(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    parent: HWND,
    url: &str,
    deadline: Instant,
) -> Result<(), String> {
    while !matches!(checked_event(events, deadline)?, BrowserEvent::Ready) {}
    send(
        host,
        BrowserCommand::Resize(super::PageBounds {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        }),
    )?;
    // This explicit native acceptance fixture needs an interactive window to prove page focus.
    unsafe {
        let _previous_visibility = ShowWindow(parent, SW_SHOW);
    }
    let base = url
        .strip_suffix("/close")
        .ok_or("Synthetic fixture URL shape changed.")?;
    send(
        host,
        BrowserCommand::Create {
            id: 1000,
            url: format!("{base}/popup-opener"),
        },
    )?;
    let mut opener = None;
    let mut child = None;
    let mut adopted = false;
    let mut replied = false;
    let mut focused = false;
    let mut focus_requested = false;
    while !(adopted && replied && focused) {
        match checked_event(events, deadline)? {
            BrowserEvent::Created { id: 1000 } => {
                opener = Some(super::lifecycle::only_child(parent)?);
            }
            BrowserEvent::PopupRequested {
                id: 1000,
                request,
                url,
            } => {
                if url != format!("{base}/popup-child") {
                    return Err("Unexpected synthetic popup target.".into());
                }
                send(host, BrowserCommand::AdoptPopup { request, id: 1001 })?;
            }
            BrowserEvent::Created { id: 1001 } => {
                child = Some(super::lifecycle::other_child(
                    parent,
                    opener.ok_or("Synthetic opener was not created before its popup.")?,
                )?);
                adopted = true;
                send(host, BrowserCommand::ShowActive(Some(1001)))?;
            }
            BrowserEvent::Title { id: 1000, title } if title == "popup-opener-ok" => replied = true,
            BrowserEvent::Title { id: 1001, title } if title == "popup-focus-ok" => focused = true,
            BrowserEvent::Title { id: 1001, title } if title == "popup-opener-broken" => {
                return Err("Adopted synthetic popup lost its opener relationship.".into());
            }
            _ => {}
        }
        if adopted && replied && !focus_requested {
            println!("Synthetic popup adoption, opener access and message reply passed.");
            if !unsafe { SetForegroundWindow(parent) }.as_bool() {
                println!(
                    "Windows declined foreground activation; checking native page focus independently."
                );
            }
            send(host, BrowserCommand::Focus { id: 1001 })?;
            focus_requested = true;
        }
    }
    // A title can arrive before Focus finishes; acknowledge MoveFocus and verify active visibility.
    super::lifecycle::barrier(host, events, deadline)?;
    super::lifecycle::require_visible(opener.ok_or("Synthetic opener is missing.")?, false)?;
    super::lifecycle::require_visible(child.ok_or("Synthetic adopted child is missing.")?, true)?;
    send(host, BrowserCommand::Close { id: 1001 })?;
    send(host, BrowserCommand::Close { id: 1000 })?;
    Ok(())
}
