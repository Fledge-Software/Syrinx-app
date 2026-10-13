//! Exercises native controls on a focused synthetic document without downloading or printing files.

use super::*;
use syrinx_platform::browser::PageAction;

const PAGE: &str = "<!doctype html><meta charset=utf-8><title>Essentials fixture</title><link rel=icon href=data:,><p>syntheticneedle syntheticneedle</p><input autofocus aria-label='Synthetic input'>";

pub(super) fn response(request: &[u8]) -> Option<&'static str> {
    request.starts_with(b"GET /essentials ").then_some(PAGE)
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
    send(
        host,
        BrowserCommand::Create {
            id: 3000,
            url: format!("{base}/essentials"),
        },
    )?;
    let mut created = false;
    let mut navigated = false;
    while !(created && navigated) {
        match checked_event(events, deadline)? {
            BrowserEvent::Created { id: 3000 } => created = true,
            BrowserEvent::NavigationCompleted {
                id: 3000,
                success: true,
            } => navigated = true,
            _ => {}
        }
    }
    println!("Synthetic essentials page created and navigated.");
    send(host, BrowserCommand::ShowActive(Some(3000)))?;
    send(host, BrowserCommand::Focus { id: 3000 })?;
    lifecycle::barrier(host, events, deadline)?;
    syrinx_platform::browser::focus_chrome(parent.0 as isize).map_err(|error| error.to_string())?;
    if unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() } != parent {
        return Err("Synthetic find focus did not return to the chrome window.".into());
    }
    send(host, BrowserCommand::Focus { id: 3000 })?;
    for (request, text, expected) in [(1, "syntheticneedle", 2), (2, "", 0)] {
        action(
            host,
            PageAction::Find {
                request,
                text: text.into(),
            },
        )?;
        loop {
            if let BrowserEvent::FindCompleted {
                id: 3000,
                request: completed,
                count,
            } = checked_event(events, deadline)?
            {
                if completed != request {
                    continue;
                }
                if count != expected {
                    return Err(format!(
                        "Synthetic find count {count} differs from {expected}."
                    ));
                }
                break;
            }
        }
        println!("Synthetic find completed with {expected} matches.");
        action(host, PageAction::StopFind)?;
    }
    for (command, expected) in [
        (PageAction::Downloads, true),
        (PageAction::CloseDownloads, false),
    ] {
        action(host, command)?;
        loop {
            if matches!(checked_event(events, deadline)?, BrowserEvent::DownloadDialog { id: 3000, open } if open == expected)
            {
                break;
            }
        }
    }
    for (command, expected) in [
        (PageAction::ZoomIn, 1.1),
        (PageAction::ZoomOut, 1.),
        (PageAction::ZoomIn, 1.1),
        (PageAction::ZoomReset, 1.),
    ] {
        action(host, command)?;
        loop {
            if matches!(checked_event(events, deadline)?, BrowserEvent::ZoomChanged { id: 3000, factor } if (factor - expected).abs() < 0.001)
            {
                break;
            }
        }
    }
    println!("Synthetic download-dialog and zoom phases passed.");
    // ShowPrintUI opens preview only. No physical print or output file is requested.
    action(host, PageAction::Print)?;
    lifecycle::barrier(host, events, deadline)?;
    send(host, BrowserCommand::Close { id: 3000 })?;
    Ok(())
}

fn action(host: &BrowserHost, action: PageAction) -> Result<(), String> {
    send(host, BrowserCommand::PageAction { id: 3000, action })
}
