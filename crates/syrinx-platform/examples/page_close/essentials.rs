//! Exercises native controls on a focused synthetic document without downloading or printing files.

use super::*;
use syrinx_platform::browser::PageAction;
use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    UI::{
        Input::KeyboardAndMouse::{GetFocus, SetFocus},
        WindowsAndMessaging::{GetWindowTextW, SendMessageW, WM_CHAR, WS_CHILD, WS_VISIBLE},
    },
};

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
                ..
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
    find_updates_keep_chrome_focus(host, events, parent, deadline)?;
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

/// Types into native chrome after each result, without refocusing between query updates.
fn find_updates_keep_chrome_focus(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    parent: HWND,
    deadline: Instant,
) -> Result<(), String> {
    let input = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("EDIT"),
            w!(""),
            WS_CHILD | WS_VISIBLE,
            8,
            8,
            240,
            28,
            Some(parent),
            None,
            None,
            None,
        )
    }
    .map_err(|error| format!("Create synthetic find input failed ({}).", error.code().0))?;
    let result = type_after_find_results(host, events, input, deadline);
    let cleanup = unsafe { DestroyWindow(input) }
        .map_err(|error| format!("Destroy synthetic find input failed ({}).", error.code().0));
    combine(result, cleanup)
}

fn type_after_find_results(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    input: HWND,
    deadline: Instant,
) -> Result<(), String> {
    // Observe the deliberate page focus before beginning the chrome-owned search.
    send(host, BrowserCommand::Focus { id: 3000 })?;
    while !matches!(
        checked_event(events, deadline)?,
        BrowserEvent::PageFocused { id: 3000 }
    ) {}
    lifecycle::barrier(host, events, deadline)?;
    unsafe { SetFocus(Some(input)) }
        .map_err(|error| format!("Focus synthetic find input failed ({}).", error.code().0))?;
    if unsafe { GetFocus() } != input {
        return Err("Synthetic find input did not receive setup focus.".into());
    }

    let mut expected = String::new();
    for (index, character) in "syntheticneedlex".chars().enumerate() {
        if unsafe { GetFocus() } != input {
            return Err(format!(
                "DW-71: native find took chrome focus before character {index}."
            ));
        }
        // Send only to the actual focused fixture control, never to a guessed page handle.
        unsafe {
            SendMessageW(
                input,
                WM_CHAR,
                Some(WPARAM(character as usize)),
                Some(LPARAM(1)),
            )
        };
        expected.push(character);
        let mut text = [0_u16; 32];
        let length = unsafe { GetWindowTextW(input, &mut text) } as usize;
        let actual = String::from_utf16(&text[..length])
            .map_err(|_| "Synthetic find input returned invalid text.")?;
        if actual != expected {
            return Err(format!("DW-71: find input lost character {index}."));
        }
        let request = 100 + index as u64;
        action(
            host,
            PageAction::Find {
                request,
                text: actual,
            },
        )?;
        wait_for_chrome_find(
            events,
            input,
            request,
            if character == 'x' { 0 } else { 2 },
            deadline,
        )?;
    }
    action(host, PageAction::StopFind)?;
    println!("DW-71: typing after every native find result kept chrome focus.");
    Ok(())
}

fn wait_for_chrome_find(
    events: &mut UnboundedReceiver<BrowserEvent>,
    input: HWND,
    request: u64,
    expected_count: i32,
    deadline: Instant,
) -> Result<(), String> {
    loop {
        match checked_event(events, deadline)? {
            BrowserEvent::PageFocused { id: 3000 } => {
                return Err(format!(
                    "DW-71: find emitted PageFocused for request {request}; native chrome focus retained: {}.",
                    unsafe { GetFocus() } == input
                ));
            }
            BrowserEvent::FindCompleted {
                id: 3000,
                request: completed,
                count,
            } if completed == request => {
                if count != expected_count {
                    return Err(format!(
                        "Synthetic incremental find count {count} differs from {expected_count}."
                    ));
                }
                if unsafe { GetFocus() } != input {
                    return Err(format!(
                        "DW-71: native find took chrome focus at request {request}."
                    ));
                }
                return Ok(());
            }
            _ => {}
        }
    }
}
