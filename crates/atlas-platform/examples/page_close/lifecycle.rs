//! Exercises native page reuse and rejected commands inside the existing disposable harness.

use super::*;
use windows::{
    Win32::{
        Foundation::{ERROR_SUCCESS, GetLastError, LPARAM, RECT, SetLastError},
        System::Threading::GetCurrentThreadId,
        UI::WindowsAndMessaging::{
            EnumChildWindows, GA_PARENT, GWL_STYLE, GetAncestor, GetWindowLongPtrW, GetWindowRect,
            GetWindowThreadProcessId, WS_VISIBLE,
        },
    },
    core::BOOL,
};

const BARRIER_ID: u64 = u64::MAX;
const STRESS_CYCLES: u64 = 12;

pub(super) fn exercise(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    parent: HWND,
    url: &str,
    deadline: Instant,
) -> Result<(), String> {
    require_children(parent, &[])?;
    create_blank(host, events, 2, deadline)?;
    let first = only_child(parent)?;
    require_visible(first, false)?;
    exercise_visibility_and_bounds(host, events, first, deadline)?;
    exercise_rejected_creation(host, events, parent, first, deadline)?;

    // Retry the failed ID, then close and recreate that same ID without disturbing the other page.
    create_blank(host, events, 3, deadline)?;
    let second = other_child(parent, first)?;
    send(host, BrowserCommand::ShowActive(Some(3)))?;
    barrier(host, events, deadline)?;
    require_visible(first, false)?;
    require_visible(second, true)?;
    close(host, events, 3, deadline)?;
    require_window(second, false)?;
    require_children(parent, &[first])?;
    create_blank(host, events, 3, deadline)?;
    let reopened = other_child(parent, first)?;
    send(host, BrowserCommand::ShowActive(Some(3)))?;
    barrier(host, events, deadline)?;
    require_visible(reopened, true)?;
    close(host, events, 2, deadline)?;
    require_window(first, false)?;
    require_children(parent, &[reopened])?;
    exercise_repeated_lifecycle(host, events, parent, reopened, url, deadline)
}

fn exercise_repeated_lifecycle(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    parent: HWND,
    survivor: HWND,
    url: &str,
    deadline: Instant,
) -> Result<(), String> {
    // Fresh IDs keep delayed callbacks from an earlier cycle from satisfying a later wait.
    for id in 100..100 + STRESS_CYCLES {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let result = lifecycle_cycle(host, events, parent, survivor, id, url, deadline);
        result.map_err(|error| {
            format!(
                "Synthetic lifecycle cycle {} failed ({remaining:?} left at start): {error}",
                id - 99
            )
        })?;
    }

    // Leave three exact native children for Stopped/join to release before parent destruction.
    create_blank(host, events, 200, deadline)?;
    let first = other_child(parent, survivor)?;
    create_blank(host, events, 201, deadline)?;
    let current = children(parent)?;
    let second = current
        .iter()
        .copied()
        .find(|child| *child != survivor && *child != first)
        .ok_or("Synthetic shutdown child is missing.")?;
    require_children(parent, &[survivor, first, second])?;
    send(host, BrowserCommand::ShowActive(Some(200)))?;
    barrier(host, events, deadline)?;
    require_visible(survivor, false)?;
    require_visible(first, true)?;
    require_visible(second, false)
}

fn lifecycle_cycle(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    parent: HWND,
    survivor: HWND,
    id: u64,
    url: &str,
    deadline: Instant,
) -> Result<(), String> {
    require_children(parent, &[survivor])?;
    create_close_fixture(host, events, id, url, deadline)?;
    let child = other_child(parent, survivor)?;
    require_visible(child, false)?;

    // Deferred close must leave each newly created controller usable through further commands.
    for active in [Some(id), None, Some(3), Some(id)] {
        send(host, BrowserCommand::ShowActive(active))?;
        barrier(host, events, deadline)
            .map_err(|error| format!("Active-page transition for synthetic tab {id}: {error}"))?;
        require_visible(child, active == Some(id))?;
        require_visible(survivor, active == Some(3))?;
        require_children(parent, &[survivor, child])?;
    }
    let offset = (id - 100) as i32;
    let bounds = PageBounds {
        x: 7 + offset,
        y: 11 + offset,
        width: 320 + offset as u32,
        height: 240 + offset as u32,
    };
    let actual = resize(host, events, child, bounds, deadline)
        .map_err(|error| format!("Resize for synthetic tab {id}: {error}"))?;
    if window_rect(survivor)? != actual {
        return Err("Repeated resize did not preserve matching survivor bounds.".into());
    }
    send(
        host,
        BrowserCommand::Navigate {
            id,
            url: "about:blank".into(),
        },
    )?;
    wait_navigation(events, id, deadline)?;
    require_children(parent, &[survivor, child])?;
    close(host, events, id, deadline)
        .map_err(|error| format!("Close acknowledgement for synthetic tab {id}: {error}"))?;
    // Check destruction before another creation can legitimately reuse this numeric HWND.
    require_window(child, false)?;
    require_children(parent, &[survivor])?;
    send(host, BrowserCommand::ShowActive(Some(3)))?;
    barrier(host, events, deadline).map_err(|error| {
        format!("Survivor activation after closing synthetic tab {id}: {error}")
    })?;
    require_visible(survivor, true)
}

fn exercise_visibility_and_bounds(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    child: HWND,
    deadline: Instant,
) -> Result<(), String> {
    for visible in [true, false, true] {
        send(host, BrowserCommand::ShowActive(visible.then_some(2)))?;
        barrier(host, events, deadline)?;
        require_visible(child, visible)?;
    }
    let first = resize(
        host,
        events,
        child,
        PageBounds {
            x: 7,
            y: 11,
            width: 320,
            height: 240,
        },
        deadline,
    )?;
    let second = resize(
        host,
        events,
        child,
        PageBounds {
            x: 19,
            y: 29,
            width: 480,
            height: 300,
        },
        deadline,
    )?;
    if second.left - first.left != 12 || second.top - first.top != 18 {
        return Err("Synthetic child position did not follow page bounds.".into());
    }
    send(
        host,
        BrowserCommand::Resize(PageBounds {
            width: u32::MAX,
            ..PageBounds::default()
        }),
    )?;
    wait_error(events, None, BrowserError::InvalidBounds, deadline)?;
    if window_rect(child)? != second {
        return Err("Rejected synthetic bounds changed the existing child.".into());
    }
    Ok(())
}

fn exercise_rejected_creation(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    parent: HWND,
    existing: HWND,
    deadline: Instant,
) -> Result<(), String> {
    send(
        host,
        BrowserCommand::Create {
            id: 2,
            url: "about:blank".into(),
        },
    )?;
    wait_creation_failure(events, 2, BrowserError::DuplicateTab, deadline)?;
    require_children(parent, &[existing])?;
    // The controller exists before Atlas rejects this URL; its owned Page must clean it up.
    // This does not inject a failure inside wry's unowned-construction interval.
    send(
        host,
        BrowserCommand::Create {
            id: 3,
            url: "about:settings".into(),
        },
    )?;
    wait_creation_failure(events, 3, BrowserError::InvalidUrl, deadline)?;
    barrier(host, events, deadline)?;
    require_children(parent, &[existing])?;
    require_visible(existing, true)?;
    Ok(())
}

fn create_blank(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    expected: u64,
    deadline: Instant,
) -> Result<(), String> {
    send(
        host,
        BrowserCommand::Create {
            id: expected,
            url: "about:blank".into(),
        },
    )?;
    let mut created = false;
    let mut navigated = false;
    while !(created && navigated) {
        match checked_event(events, deadline)? {
            BrowserEvent::Created { id } if id == expected => created = true,
            BrowserEvent::NavigationCompleted { id, success: true } if id == expected => {
                navigated = true
            }
            _ => {}
        }
    }
    Ok(())
}

fn close(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    id: u64,
    deadline: Instant,
) -> Result<(), String> {
    send(host, BrowserCommand::Close { id })?;
    send(host, BrowserCommand::Reload { id })?;
    wait_error(events, Some(id), BrowserError::UnknownTab, deadline)
}

pub(super) fn barrier(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    deadline: Instant,
) -> Result<(), String> {
    // This impossible live ID acknowledges preceding FIFO commands without adding a product API.
    send(host, BrowserCommand::Reload { id: BARRIER_ID })?;
    wait_error(events, Some(BARRIER_ID), BrowserError::UnknownTab, deadline)
}

fn wait_error(
    events: &mut UnboundedReceiver<BrowserEvent>,
    tab: Option<u64>,
    expected: BrowserError,
    deadline: Instant,
) -> Result<(), String> {
    loop {
        let event = next_event(events, deadline)?;
        if matches!(&event, BrowserEvent::Error { tab: actual, error } if *actual == tab && *error == expected)
        {
            return Ok(());
        }
        check_event(&event)?;
    }
}

fn wait_creation_failure(
    events: &mut UnboundedReceiver<BrowserEvent>,
    expected_id: u64,
    expected_error: BrowserError,
    deadline: Instant,
) -> Result<(), String> {
    loop {
        let event = next_event(events, deadline)?;
        if matches!(&event, BrowserEvent::CreationFailed { id, error } if *id == expected_id && *error == expected_error)
        {
            return Ok(());
        }
        check_event(&event)?;
    }
}

fn resize(
    host: &BrowserHost,
    events: &mut UnboundedReceiver<BrowserEvent>,
    child: HWND,
    bounds: PageBounds,
    deadline: Instant,
) -> Result<RECT, String> {
    send(host, BrowserCommand::Resize(bounds))?;
    barrier(host, events, deadline)?;
    let actual = window_rect(child)?;
    if actual.right - actual.left != bounds.width as i32
        || actual.bottom - actual.top != bounds.height as i32
    {
        return Err("Synthetic child size did not follow page bounds.".into());
    }
    Ok(actual)
}

fn window_rect(child: HWND) -> Result<RECT, String> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(child, &mut rect) }
        .map_err(|error| format!("Read synthetic child bounds failed ({}).", error.code().0))?;
    Ok(rect)
}

pub(super) fn require_visible(child: HWND, expected: bool) -> Result<(), String> {
    if is_visible(child)? != expected {
        return Err("Synthetic child visibility did not follow the active page.".into());
    }
    Ok(())
}

/// The child's own visibility bit; the harness parent stays hidden, so ancestors are ignored.
fn is_visible(child: HWND) -> Result<bool, String> {
    require_window(child, true)?;
    unsafe { SetLastError(ERROR_SUCCESS) };
    let style = unsafe { GetWindowLongPtrW(child, GWL_STYLE) };
    let error = unsafe { GetLastError() };
    if style == 0 && error != ERROR_SUCCESS {
        return Err(format!("Read synthetic child style failed ({}).", error.0));
    }
    Ok(style as u32 & WS_VISIBLE.0 != 0)
}

/// The bounds of the one visible page child; several or none is a failure.
pub(super) fn shown_child_rect(parent: HWND) -> Result<RECT, String> {
    let mut shown = Vec::new();
    for child in children(parent)? {
        if is_visible(child)? {
            shown.push(child);
        }
    }
    let [child] = shown.as_slice() else {
        return Err("Synthetic parent does not show exactly one page child.".into());
    };
    window_rect(*child)
}

struct Children {
    parent: HWND,
    direct: Vec<HWND>,
    inspection_failed: bool,
}

unsafe extern "system" fn collect_child(window: HWND, context: LPARAM) -> BOOL {
    // EnumChildWindows invokes this synchronously while the stack-owned collection remains alive.
    let children = unsafe { &mut *(context.0 as *mut Children) };
    let parent = unsafe { GetAncestor(window, GA_PARENT) };
    if parent.0.is_null() {
        children.inspection_failed = true;
        return BOOL(0);
    }
    if parent == children.parent {
        children.direct.push(window);
    }
    BOOL(1)
}

fn children(parent: HWND) -> Result<Vec<HWND>, String> {
    require_window(parent, true)?;
    let mut children = Children {
        parent,
        direct: Vec::new(),
        inspection_failed: false,
    };
    // The documented return value is unused; callback inspection failures are recorded directly.
    let _enumeration_result = unsafe {
        EnumChildWindows(
            Some(parent),
            Some(collect_child),
            LPARAM((&mut children as *mut Children) as isize),
        )
    };
    if children.inspection_failed {
        return Err("Synthetic child-parent inspection failed.".into());
    }
    require_thread_ownership(parent, &children.direct)?;
    Ok(children.direct)
}

fn require_thread_ownership(parent: HWND, children: &[HWND]) -> Result<(), String> {
    let mut process = 0;
    let parent_thread = unsafe { GetWindowThreadProcessId(parent, Some(&mut process)) };
    if parent_thread != unsafe { GetCurrentThreadId() } || process != std::process::id() {
        return Err("Synthetic parent lost its harness-thread ownership.".into());
    }
    let mut worker_thread = None;
    for child in children {
        let child_thread = unsafe { GetWindowThreadProcessId(*child, Some(&mut process)) };
        if child_thread == 0 || child_thread == parent_thread || process != std::process::id() {
            return Err("Synthetic page child is not owned by a separate browser thread.".into());
        }
        if worker_thread.is_some_and(|expected| expected != child_thread) {
            return Err("Synthetic page children disagree on browser-thread ownership.".into());
        }
        worker_thread = Some(child_thread);
    }
    Ok(())
}

pub(super) fn only_child(parent: HWND) -> Result<HWND, String> {
    let children = children(parent)?;
    match children.as_slice() {
        [child] => Ok(*child),
        _ => Err("Synthetic parent does not have exactly one page child.".into()),
    }
}

pub(super) fn other_child(parent: HWND, existing: HWND) -> Result<HWND, String> {
    let children = children(parent)?;
    if children.len() != 2 || !children.contains(&existing) {
        return Err("Synthetic retry did not retain exactly two page children.".into());
    }
    children
        .into_iter()
        .find(|child| *child != existing)
        .ok_or_else(|| "Synthetic retry child is missing.".into())
}

pub(super) fn require_children(parent: HWND, expected: &[HWND]) -> Result<(), String> {
    let children = children(parent)?;
    if children.len() != expected.len() || expected.iter().any(|child| !children.contains(child)) {
        return Err("Synthetic child set did not return to its expected lifecycle state.".into());
    }
    Ok(())
}
