//! Exercises real hidden-tab suspension and local video fullscreen without script injection.

use super::*;
use std::collections::HashSet;
use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    UI::WindowsAndMessaging::{
        GUITHREADINFO, GetGUIThreadInfo, GetWindowThreadProcessId, IsChild, PostMessageW, SW_SHOW,
        ShowWindow, WM_KEYDOWN, WM_KEYUP,
    },
};

const VIDEO: &str = r#"<!doctype html><meta charset=utf-8><title>video-ready</title>
<link rel=icon href=data:,><style>body{margin:0;background:#123;color:white}video{width:100%;height:100%;object-fit:contain}</style>
<p>Enter: fullscreen synthetic video. X: exit.</p><video id=v muted autoplay></video><script>
const canvas=document.createElement('canvas');canvas.width=640;canvas.height=360;
const ctx=canvas.getContext('2d');ctx.fillStyle='#258';ctx.fillRect(0,0,640,360);
ctx.fillStyle='white';ctx.font='32px sans-serif';ctx.fillText('Syrinx synthetic video',100,180);
v.srcObject=canvas.captureStream(0);
addEventListener('keydown',e=>{if(e.key==='Enter')v.requestFullscreen().catch(()=>document.title='fullscreen-error');if(e.key==='x')document.exitFullscreen();});
addEventListener('fullscreenchange',()=>document.title=document.fullscreenElement?'video-full':'video-exited');
</script>"#;

const SLOW: &str = "<!doctype html><meta charset=utf-8><title>slow-hidden</title>\
    <link rel=icon href=data:,><p>Synthetic slow hidden page</p>";
const SLOW_ID: u64 = 1490;

pub(super) fn response(request: &[u8]) -> Option<&'static str> {
    if is_slow(request) {
        return Some(SLOW);
    }
    request
        .starts_with(b"GET /synthetic-video ")
        .then_some(VIDEO)
}

/// The slow page's body is always held past the hidden-page grace.
pub(super) fn is_slow(request: &[u8]) -> bool {
    request.starts_with(b"GET /slow-hidden ")
}

/// A hidden page whose load outlasts the grace must finish loading before it is suspended.
fn exercise_slow_hidden_load(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    url: &str,
    deadline: Instant,
) -> Result<(), String> {
    let base = url
        .strip_suffix("/close")
        .ok_or("Synthetic fixture URL shape changed.")?;
    send(
        host,
        BrowserCommand::Create {
            id: SLOW_ID,
            url: format!("{base}/slow-hidden"),
        },
    )?;
    let started = Instant::now();
    let mut completed = None;
    loop {
        match checked_event(events, deadline)? {
            BrowserEvent::NavigationCompleted {
                id: SLOW_ID,
                success: true,
            } => completed = Some(started.elapsed()),
            BrowserEvent::Suspended {
                id: SLOW_ID,
                suspended: true,
            } => break,
            _ => {}
        }
    }
    let Some(loaded) = completed else {
        return Err("Slow hidden page was suspended before it finished loading.".into());
    };
    if loaded < STALL_PAST_SUSPENSION_GRACE {
        return Err("Slow hidden page finished before its held body was sent.".into());
    }
    println!(
        "Slow hidden page loaded after {:.1} s and was suspended only afterwards.",
        loaded.as_secs_f64()
    );
    // Release it so the eight-tab phase below starts from an empty parent.
    send(host, BrowserCommand::Close { id: SLOW_ID })?;
    lifecycle::barrier(host, events, deadline)
}

pub(super) fn exercise(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    parent: HWND,
    url: &str,
    deadline: Instant,
) -> Result<(), String> {
    while !matches!(checked_event(events, deadline)?, BrowserEvent::Ready) {}
    exercise_slow_hidden_load(host, events, url, deadline)?;
    for id in 1500..1508 {
        send(
            host,
            BrowserCommand::Create {
                id,
                url: "about:blank".into(),
            },
        )?;
    }
    let mut suspended = HashSet::new();
    while suspended.len() != 8 {
        if let BrowserEvent::Suspended {
            id,
            suspended: true,
        } = checked_event(events, deadline)?
        {
            suspended.insert(id);
        }
    }
    println!("Eight native IsSuspended readbacks passed.");
    // Move the card while every page is suspended; the page shown next resumes, then takes it.
    let card = PageBounds {
        x: 20,
        y: 40,
        width: 400,
        height: 300,
    };
    send(host, BrowserCommand::Resize(card))?;
    send(host, BrowserCommand::ShowActive(Some(1500)))?;
    while !matches!(
        checked_event(events, deadline)?,
        BrowserEvent::Suspended {
            id: 1500,
            suspended: false
        }
    ) {}
    lifecycle::barrier(host, events, deadline)?;
    let shown = lifecycle::shown_child_rect(parent)?;
    if shown.right - shown.left != card.width as i32
        || shown.bottom - shown.top != card.height as i32
    {
        return Err("Resumed synthetic page did not take the card moved while suspended".into());
    }
    println!("Synthetic activation resumed tab 1500 at the card moved while it was suspended.");
    // Close and recreate a logical identity while old callbacks may still be queued.
    for id in 1500..1508 {
        send(host, BrowserCommand::Close { id })?;
    }
    lifecycle::barrier(host, events, deadline)?;
    lifecycle::require_children(parent, &[])?;
    println!("Synthetic eight-tab teardown passed.");
    let base = url.strip_suffix("/close").ok_or("Synthetic base changed")?;
    send(
        host,
        BrowserCommand::Resize(PageBounds {
            x: 12,
            y: 24,
            width: 480,
            height: 320,
        }),
    )?;
    send(
        host,
        BrowserCommand::Create {
            id: 1500,
            url: format!("{base}/synthetic-video"),
        },
    )?;
    unsafe {
        let _previous_visibility = ShowWindow(parent, SW_SHOW);
    }
    send(host, BrowserCommand::ShowActive(Some(1500)))?;
    while !matches!(
        checked_event(events, deadline)?,
        BrowserEvent::NavigationCompleted {
            id: 1500,
            success: true
        }
    ) {}
    println!("Synthetic video navigation completed.");
    unsafe {
        let _previous_visibility = ShowWindow(parent, SW_SHOW);
    }
    send(host, BrowserCommand::ShowActive(Some(1500)))?;
    send(host, BrowserCommand::Focus { id: 1500 })?;
    lifecycle::barrier(host, events, deadline)?;
    let child = lifecycle::only_child(parent)?;
    let original = page_rect(child)?;
    for exit in [0, 1, 2, 3] {
        // Separate user gestures; the browser can throttle rapid fullscreen re-entry.
        thread::sleep(Duration::from_millis(1500));
        println!("Synthetic fullscreen enter cycle {exit}.");
        key(child, 0x0D)?;
        wait_fullscreen(events, true, deadline)?;
        send(
            host,
            BrowserCommand::Resize(PageBounds {
                x: 0,
                y: 0,
                width: 640,
                height: 480,
            }),
        )?;
        lifecycle::barrier(host, events, deadline)?;
        let full = page_rect(child)?;
        if full.2 - full.0 != 640 || full.3 - full.1 != 480 {
            return Err("Synthetic fullscreen page did not fill its requested viewport".into());
        }
        println!("Synthetic fullscreen exit cycle {exit}.");
        match exit {
            0 => key(child, 0x1B)?,
            1 => {
                key(child, 0x7A)?;
                while !matches!(
                    checked_event(events, deadline)?,
                    BrowserEvent::Shortcut(syrinx_platform::browser::BrowserShortcut::Fullscreen)
                ) {}
                send(host, BrowserCommand::ExitFullscreen { id: 1500 })?;
            }
            2 => key(child, 0x58)?,
            _ => {
                // Model F11/Escape received by Syrinx chrome after focus left the controller.
                unsafe { windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(parent)) }
                    .map_err(|_| "Synthetic parent focus failed")?;
                send(host, BrowserCommand::ExitFullscreen { id: 1500 })?;
            }
        }
        wait_fullscreen(events, false, deadline)?;
        send(
            host,
            BrowserCommand::Resize(PageBounds {
                x: 12,
                y: 24,
                width: 480,
                height: 320,
            }),
        )?;
        lifecycle::barrier(host, events, deadline)?;
        if page_rect(child)? != original {
            return Err("Synthetic fullscreen page did not restore its exact rectangle".into());
        }
    }
    println!(
        "Synthetic video fullscreen exits passed, including F11/Escape with native focus on chrome."
    );
    Ok(())
}

fn wait_fullscreen(
    events: &mut UnboundedReceiver<BrowserEvent>,
    active: bool,
    deadline: Instant,
) -> Result<(), String> {
    loop {
        match checked_event(events, deadline)? {
            BrowserEvent::Fullscreen {
                id: 1500,
                active: value,
            } if value == active => break,
            BrowserEvent::Title { id: 1500, title } if title == "fullscreen-error" => {
                return Err("Synthetic fullscreen request was rejected".into());
            }
            _ => {}
        }
    }
    Ok(())
}

fn key(child: HWND, key: usize) -> Result<(), String> {
    let thread = unsafe { GetWindowThreadProcessId(child, None) };
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(thread, &mut info) }.map_err(|_| "Synthetic focus query failed")?;
    if !unsafe { IsChild(child, info.hwndFocus) }.as_bool() && child != info.hwndFocus {
        return Err("Synthetic key target is outside the fixture page".into());
    }
    unsafe { PostMessageW(Some(info.hwndFocus), WM_KEYDOWN, WPARAM(key), LPARAM(1)) }
        .map_err(|_| "Synthetic keydown failed")?;
    unsafe {
        PostMessageW(
            Some(info.hwndFocus),
            WM_KEYUP,
            WPARAM(key),
            LPARAM(0xC0000001),
        )
    }
    .map_err(|_| "Synthetic keyup failed".into())
}

fn page_rect(child: HWND) -> Result<(i32, i32, i32, i32), String> {
    let mut rect = windows::Win32::Foundation::RECT::default();
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(child, &mut rect) }
        .map_err(|_| "Synthetic bounds readback failed")?;
    Ok((rect.left, rect.top, rect.right, rect.bottom))
}
