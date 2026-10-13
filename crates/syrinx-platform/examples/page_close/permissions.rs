//! Proves consent reaches a synthetic document and one-shot choices do not persist in WebView2.

use super::*;

const PAGE: &str = r#"<!doctype html><meta charset=utf-8><title>permission waiting</title>
<link rel=icon href=data:,><p>Synthetic notification consent fixture</p><script>
addEventListener('load', async () => {
  const result = await Notification.requestPermission();
  document.title = 'permission-' + result;
  if (result === 'granted') {
    const notice = new Notification('Syrinx synthetic notification', { body: 'Disposable fixture only' });
    notice.onshow = () => { document.title = 'notification-shown'; notice.close(); };
    notice.onerror = () => { document.title = 'notification-error'; };
  }
});
</script>"#;

pub(super) fn response(request: &[u8]) -> Option<&'static str> {
    request.starts_with(b"GET /permissions ").then_some(PAGE)
}

pub(super) fn exercise(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    parent: HWND,
    url: &str,
    deadline: Instant,
) -> Result<(), String> {
    while !matches!(checked_event(events, deadline)?, BrowserEvent::Ready) {}
    let base = url
        .strip_suffix("/close")
        .ok_or("Synthetic URL shape changed.")?;
    unsafe {
        let _previous_visibility = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
            parent,
            windows::Win32::UI::WindowsAndMessaging::SW_SHOW,
        );
    }
    send(
        host,
        BrowserCommand::Resize(PageBounds {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        }),
    )?;
    let storage_path = reserve_profile()?.join("PermissionState");
    let open_store = || {
        let lease = syrinx_platform::WindowsDirectoryLease::prepare(&storage_path)
            .map_err(|error| error.to_string())?;
        syrinx_store::Store::open(Arc::new(lease)).map_err(|error| error.to_string())
    };
    let mut store = open_store()?;
    // A new view in the same profile must request consent again after both deny and allow.
    for (id, allowed) in [(2000, false), (2001, true), (2002, true)] {
        send(
            host,
            BrowserCommand::Create {
                id,
                url: format!("{base}/permissions"),
            },
        )?;
        let mut requested = false;
        let expected = if allowed {
            "notification-shown"
        } else {
            "permission-denied"
        };
        loop {
            match checked_event(events, deadline)? {
                BrowserEvent::Created { id: current } if current == id => {
                    send(host, BrowserCommand::ShowActive(Some(id)))?;
                }
                BrowserEvent::PermissionRequested {
                    id: current,
                    request,
                    uri,
                    kind,
                } if current == id => {
                    if requested
                        || uri.trim_end_matches('/') != base
                        || kind != syrinx_store::PermissionKind::Notifications
                    {
                        return Err(format!(
                            "Synthetic permission mismatch: repeated={requested}, uri={uri:?}, kind={kind:?}."
                        ));
                    }
                    requested = true;
                    let remembered = store
                        .permission(base, kind)
                        .map_err(|error| error.to_string())?;
                    if remembered != (id == 2002).then_some(true) {
                        return Err(
                            "Synthetic remembered consent did not match the restarted store."
                                .into(),
                        );
                    }
                    if id == 2001 {
                        store
                            .remember_permission(base.into(), kind, allowed)
                            .map_err(|error| error.to_string())?;
                    }
                    send(
                        host,
                        BrowserCommand::ResolvePermission {
                            id,
                            request,
                            allowed,
                        },
                    )?;
                }
                BrowserEvent::Title { id: current, title }
                    if current == id && title == expected =>
                {
                    if !requested {
                        return Err(
                            "Synthetic permission succeeded without an explicit decision.".into(),
                        );
                    }
                    break;
                }
                BrowserEvent::Title { id: current, title }
                    if current == id && title == "notification-error" =>
                {
                    return Err("Synthetic notification default UI reported an error.".into());
                }
                _ => {}
            }
        }
        send(host, BrowserCommand::Close { id })?;
        store.shutdown().map_err(|error| error.to_string())?;
        store = open_store()?;
    }
    store.shutdown().map_err(|error| error.to_string())?;
    cancel_pending(host, events, base, deadline)
}

fn cancel_pending(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    base: &str,
    deadline: Instant,
) -> Result<(), String> {
    send(
        host,
        BrowserCommand::Create {
            id: 2003,
            url: format!("{base}/permissions"),
        },
    )?;
    let request = loop {
        if let BrowserEvent::PermissionRequested {
            id: 2003, request, ..
        } = checked_event(events, deadline)?
        {
            break request;
        }
    };
    send(
        host,
        BrowserCommand::Navigate {
            id: 2003,
            url: "about:blank".into(),
        },
    )?;
    loop {
        if matches!(checked_event(events, deadline)?, BrowserEvent::PermissionFinished { id: 2003, request: completed } if completed == request)
        {
            break;
        }
    }
    // A delayed allow after navigation must not revive the obsolete request.
    send(
        host,
        BrowserCommand::ResolvePermission {
            id: 2003,
            request,
            allowed: true,
        },
    )?;
    send(host, BrowserCommand::Close { id: 2003 })?;
    Ok(())
}
