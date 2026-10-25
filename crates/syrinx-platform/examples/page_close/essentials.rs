//! Exercises native controls on a focused synthetic document without downloading or printing files.

use super::*;
use syrinx_platform::browser::PageAction;

const PAGE: &str = "<!doctype html><meta charset=utf-8><title>Essentials fixture</title><link rel=icon href=data:,><p>syntheticneedle syntheticneedle SYNTHETICNEEDLE</p><input autofocus aria-label='Synthetic input'>";

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
                ..
            } => navigated = true,
            _ => {}
        }
    }
    println!("Synthetic essentials page created and navigated.");
    send(host, BrowserCommand::ShowActive(Some(3000)))?;
    send(host, BrowserCommand::Focus { id: 3000 })?;
    lifecycle::barrier(host, events, deadline)?;
    // A page that really holds the keys must not read as chrome focus, or #57's filter
    // would keep a stale ring.
    if syrinx_platform::browser::chrome_has_keyboard_focus(parent.0 as isize) {
        return Err("A focused synthetic page still reported chrome keyboard focus.".into());
    }
    syrinx_platform::browser::focus_chrome(parent.0 as isize).map_err(|error| error.to_string())?;
    if !syrinx_platform::browser::chrome_has_keyboard_focus(parent.0 as isize) {
        return Err("Synthetic find focus did not return to the chrome window.".into());
    }
    find_while_typing_in_chrome(host, events, parent, deadline)?;
    send(host, BrowserCommand::Focus { id: 3000 })?;
    for (request, text, case_sensitive, expected) in [
        (1, "syntheticneedle", false, 3),
        (2, "syntheticneedle", true, 2),
        (3, "", false, 0),
    ] {
        action(
            host,
            PageAction::Find {
                request,
                text: text.into(),
                case_sensitive,
            },
        )?;
        let (count, active) = find_result(events, request, deadline)?;
        if count != expected {
            return Err(format!(
                "Synthetic find count {count} differs from {expected}."
            ));
        }
        // ActiveMatchIndex counts from 1, which the bar's "N of M" label relies on.
        let first = if expected > 0 { 1 } else { -1 };
        if active != first {
            return Err(format!(
                "Synthetic find's active match {active} differs from {first}."
            ));
        }
        println!("Synthetic find completed with {expected} matches, active match {active}.");
        if expected > 1 {
            step_to_second_match(host, events, request, active, deadline)?;
        }
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
    exercise_mute(host, events, deadline)?;
    // ShowPrintUI opens preview only. No physical print or output file is requested.
    action(host, PageAction::Print)?;
    lifecycle::barrier(host, events, deadline)?;
    send(host, BrowserCommand::Close { id: 3000 })?;
    Ok(())
}

/// Mutes the shown page and a suspended hidden one, reading each back from `IsMuted` (#95).
fn exercise_mute(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    deadline: Instant,
) -> Result<(), String> {
    for muted in [true, false] {
        send(host, BrowserCommand::SetMuted { id: 3000, muted })?;
        wait_muted(events, 3000, muted, deadline)?;
    }
    println!("Synthetic mute and unmute read back on the shown page.");
    // A never-shown page is suspended once its load completes and the grace passes.
    send(
        host,
        BrowserCommand::Create {
            id: 3001,
            url: "about:blank".into(),
        },
    )?;
    while !matches!(
        checked_event(events, deadline)?,
        BrowserEvent::Suspended {
            id: 3001,
            suspended: true
        }
    ) {}
    // Muting must not wake the page; WebView2 may defer the read-back until it resumes.
    send(
        host,
        BrowserCommand::SetMuted {
            id: 3001,
            muted: true,
        },
    )?;
    let while_suspended = wait_muted_while_suspended(events, deadline)?;
    send(host, BrowserCommand::ShowActive(Some(3001)))?;
    if !while_suspended {
        wait_muted(events, 3001, true, deadline)?;
    }
    println!(
        "Synthetic mute of a suspended page read back {}.",
        if while_suspended {
            "while it stayed suspended"
        } else {
            "only after it was shown"
        }
    );
    for muted in [false, true] {
        send(host, BrowserCommand::SetMuted { id: 3001, muted })?;
        wait_muted(events, 3001, muted, deadline)?;
    }
    println!("Synthetic unmute and mute read back on the resumed page.");
    send(host, BrowserCommand::Close { id: 3001 })?;
    // Print, next, needs the essentials page shown and focused again.
    send(host, BrowserCommand::ShowActive(Some(3000)))?;
    send(host, BrowserCommand::Focus { id: 3000 })?;
    lifecycle::barrier(host, events, deadline)
}

/// Waits up to three seconds for the suspended page's mute read-back; `false` when none
/// came, meaning WebView2 deferred it. Fails if muting resumed the page.
fn wait_muted_while_suspended(
    events: &mut UnboundedReceiver<BrowserEvent>,
    deadline: Instant,
) -> Result<bool, String> {
    let window = (Instant::now() + Duration::from_secs(3)).min(deadline);
    loop {
        let event = match checked_event(events, window) {
            Ok(event) => event,
            Err(_) if Instant::now() >= window && window < deadline => return Ok(false),
            Err(error) => return Err(error),
        };
        match event {
            BrowserEvent::Suspended {
                id: 3001,
                suspended: false,
            } => return Err("Muting a suspended synthetic page resumed it.".into()),
            BrowserEvent::AudioChanged {
                id: 3001,
                muted: true,
                ..
            } => return Ok(true),
            _ => {}
        }
    }
}

/// Waits for the page's own `IsMuted` to report the requested state.
fn wait_muted(
    events: &mut UnboundedReceiver<BrowserEvent>,
    id: u64,
    expected: bool,
    deadline: Instant,
) -> Result<(), String> {
    loop {
        if let BrowserEvent::AudioChanged {
            id: changed, muted, ..
        } = checked_event(events, deadline)?
            && changed == id
            && muted == expected
        {
            return Ok(());
        }
    }
}

fn action(host: &BrowserHost, action: PageAction) -> Result<(), String> {
    send(host, BrowserCommand::PageAction { id: 3000, action })
}

/// Finds each growing prefix as typing would, while the chrome holds the keys (#57).
/// WebView2 may raise `GotFocus` meanwhile; Windows focus must still stay on the chrome.
fn find_while_typing_in_chrome(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    parent: HWND,
    deadline: Instant,
) -> Result<(), String> {
    let mut focus_events = 0;
    for (request, text) in (10..).zip(["s", "sy", "syn", "synthetic", "syntheticneedle"]) {
        action(
            host,
            PageAction::Find {
                request,
                text: text.into(),
                case_sensitive: false,
            },
        )?;
        loop {
            match checked_event(events, deadline)? {
                BrowserEvent::PageFocused { id: 3000 } => {
                    focus_events += 1;
                    require_chrome_focus(parent, "a page focus event during find")?;
                }
                BrowserEvent::FindResult {
                    id: 3000,
                    request: completed,
                    ..
                } if completed == request => break,
                _ => {}
            }
        }
        require_chrome_focus(parent, "a find result")?;
    }
    lifecycle::barrier(host, events, deadline)?;
    require_chrome_focus(parent, "the last find result settled")?;
    action(host, PageAction::StopFind)?;
    println!(
        "Synthetic find kept focus on the chrome over five queries ({focus_events} page focus events)."
    );
    Ok(())
}

/// Waits for the named request's result and returns its count and active match.
fn find_result(
    events: &mut UnboundedReceiver<BrowserEvent>,
    request: u64,
    deadline: Instant,
) -> Result<(i32, i32), String> {
    loop {
        if let BrowserEvent::FindResult {
            id: 3000,
            request: completed,
            count,
            active,
        } = checked_event(events, deadline)?
            && completed == request
        {
            return Ok((count, active));
        }
    }
}

/// Find next moves the active match one place on, which shows where WebView2 counts from.
fn step_to_second_match(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    request: u64,
    first: i32,
    deadline: Instant,
) -> Result<(), String> {
    action(host, PageAction::FindNext)?;
    loop {
        let (_, active) = find_result(events, request, deadline)?;
        if active == first {
            continue;
        }
        if active != first + 1 {
            return Err(format!(
                "Find next moved the active match from {first} to {active}."
            ));
        }
        println!("Synthetic find next moved the active match from {first} to {active}.");
        return Ok(());
    }
}

/// Fails the suite when Windows keyboard focus is no longer on the chrome window.
fn require_chrome_focus(parent: HWND, moment: &str) -> Result<(), String> {
    if syrinx_platform::browser::chrome_has_keyboard_focus(parent.0 as isize) {
        return Ok(());
    }
    Err(format!(
        "Native focus left the chrome window after {moment}."
    ))
}
