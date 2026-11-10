//! Waits for window-manager activation and actual X11 focus before fixture input.

use std::{
    ffi::c_ulong,
    thread,
    time::{Duration, Instant},
};
use x11_dl::xlib;

use super::{input, with_display};

const LIMIT: Duration = Duration::from_secs(15);

pub(super) fn activate(window: isize) -> Result<(), String> {
    // MapRequest is asynchronous: do not send activation before the WM knows this client.
    wait_for("the window manager to register the fixture", || {
        on_display(move |xlib, display| {
            let root = unsafe { (xlib.XDefaultRootWindow)(display) };
            Ok(windows(xlib, display, root, c"_NET_CLIENT_LIST")?.contains(&(window as _)))
        })
    })?;
    if activation_observed(window)? {
        return Ok(());
    }
    on_display(move |xlib, display| {
        let root = unsafe { (xlib.XDefaultRootWindow)(display) };
        let atom =
            unsafe { (xlib.XInternAtom)(display, c"_NET_ACTIVE_WINDOW".as_ptr(), xlib::False) };
        let mut event = activation_message(window as _, atom, display);
        let sent = unsafe {
            (xlib.XSendEvent)(
                display,
                root,
                xlib::False,
                xlib::SubstructureRedirectMask | xlib::SubstructureNotifyMask,
                &mut event,
            )
        };
        if sent == 0 {
            return Err("Cannot request fixture activation from the window manager".into());
        }
        unsafe { (xlib.XFlush)(display) };
        Ok(())
    })?;
    wait_for(
        "WM activation and keyboard focus inside the fixture",
        || activation_observed(window),
    )
}

pub(super) fn wait_page(child: isize) -> Result<(), String> {
    wait_for("keyboard focus inside the requested fixture page", || {
        crate::gtk_thread::run(move || input::native_focus_within(child))
            .map_err(|error| error.to_string())?
    })
}

pub(super) fn wait_chrome(window: isize) -> Result<(), String> {
    wait_for("keyboard focus on the fixture chrome", || {
        on_display(move |xlib, display| {
            let (mut focused, mut revert) = (0, 0);
            unsafe { (xlib.XGetInputFocus)(display, &mut focused, &mut revert) };
            Ok(focused == window as c_ulong)
        })
    })
}

fn activation_observed(window: isize) -> Result<bool, String> {
    let wm_active = on_display(move |xlib, display| {
        let root = unsafe { (xlib.XDefaultRootWindow)(display) };
        Ok(windows(xlib, display, root, c"_NET_ACTIVE_WINDOW")?.as_slice() == [window as c_ulong])
    })?;
    let native_inside = crate::gtk_thread::run(move || input::native_focus_within(window))
        .map_err(|error| error.to_string())??;
    Ok(activation_ready(wm_active, native_inside))
}

fn activation_ready(wm_active: bool, native_inside: bool) -> bool {
    wm_active && native_inside
}

fn activation_message(window: c_ulong, atom: c_ulong, display: *mut xlib::Display) -> xlib::XEvent {
    let mut data = xlib::ClientMessageData::new();
    // EWMH source 2 represents this explicit test action, not a product focus-steal request.
    data.set_long(0, 2);
    xlib::XEvent {
        client_message: xlib::XClientMessageEvent {
            type_: xlib::ClientMessage,
            serial: 0,
            send_event: xlib::True,
            display,
            window,
            message_type: atom,
            format: 32,
            data,
        },
    }
}

/// Poll on the caller; each native operation still runs on the GTK thread.
fn on_display<T: Send + 'static>(
    job: impl FnOnce(&xlib::Xlib, *mut xlib::Display) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    crate::gtk_thread::run(move || with_display(job)).map_err(|error| error.to_string())?
}

/// Read only opaque window IDs, never desktop titles or application contents.
fn windows(
    xlib: &xlib::Xlib,
    display: *mut xlib::Display,
    root: c_ulong,
    name: &std::ffi::CStr,
) -> Result<Vec<c_ulong>, String> {
    let atom = unsafe { (xlib.XInternAtom)(display, name.as_ptr(), xlib::False) };
    let (mut actual, mut format, mut count, mut remaining) = (0, 0, 0, 0);
    let mut data = std::ptr::null_mut();
    let status = unsafe {
        (xlib.XGetWindowProperty)(
            display,
            root,
            atom,
            0,
            1024,
            xlib::False,
            xlib::XA_WINDOW,
            &mut actual,
            &mut format,
            &mut count,
            &mut remaining,
            &mut data,
        )
    };
    let result = if status != 0 || remaining != 0 {
        Err("Cannot read bounded window-manager client property".into())
    } else if actual == 0 && count == 0 {
        Ok(Vec::new())
    } else if actual != xlib::XA_WINDOW || format != 32 || (count > 0 && data.is_null()) {
        Err("Invalid window-manager client property".into())
    } else if count == 0 {
        Ok(Vec::new())
    } else {
        Ok(unsafe { std::slice::from_raw_parts(data.cast::<c_ulong>(), count as usize) }.to_vec())
    };
    if !data.is_null() {
        unsafe { (xlib.XFree)(data.cast()) };
    }
    result
}

fn wait_for(reason: &str, mut ready: impl FnMut() -> Result<bool, String>) -> Result<(), String> {
    let deadline = Instant::now() + LIMIT;
    loop {
        if ready()? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("Timed out waiting for {reason}"));
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_native_focus_cannot_acknowledge_window_manager_activation() {
        let mut snapshots = [(false, true), (true, false), (true, true)].into_iter();
        let mut observed = Vec::new();
        wait_for("activation", || {
            let state = snapshots.next().expect("acknowledgement must arrive");
            observed.push(state);
            Ok(activation_ready(state.0, state.1))
        })
        .unwrap();
        assert_eq!(observed, [(false, true), (true, false), (true, true)]);
    }

    #[test]
    fn activation_request_names_the_owned_toplevel_and_direct_test_action() {
        let message = unsafe { activation_message(42, 77, std::ptr::null_mut()).client_message };
        assert_eq!(message.type_, xlib::ClientMessage);
        assert_eq!(message.window, 42);
        assert_eq!(message.message_type, 77);
        assert_eq!(message.format, 32);
        assert_eq!(message.data.as_longs(), &[2, 0, 0, 0, 0]);
    }

    #[test]
    fn native_query_failure_is_not_replaced_with_a_focus_timeout() {
        assert_eq!(
            wait_for("activation", || Err("synthetic X11 error".into())),
            Err("synthetic X11 error".into())
        );
    }
}
