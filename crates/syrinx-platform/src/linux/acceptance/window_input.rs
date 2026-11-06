//! Limits synthetic keys and print cancellation to the fixture process's GTK widgets.

use gtk::{gdk, prelude::*};

pub(super) fn key(child: isize, keyval: u32) -> Result<(), String> {
    if !matches!(keyval, 0x20..=0x7e | 0xff0d | 0xff1b | 0xffc8) {
        return Err("Fixture key must be ASCII, Enter, Escape or F11".into());
    }
    require_native_focus(child)?;
    let window = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|widget| widget.downcast_ref::<gtk::Window>().cloned())
        .find(|window| {
            window.window().is_some_and(|native| unsafe {
                gdkx11::ffi::gdk_x11_window_get_xid(native.as_ptr().cast()) as isize == child
            })
        })
        .ok_or("No owned GTK page for fixture key")?;
    let focused = window
        .focused_widget()
        .ok_or("Fixture page has no focused widget")?;
    let native = focused
        .window()
        .ok_or("Focused fixture widget has no native window")?;
    // Read the keymap only; changing it would change the owner's keyboard globally.
    let display = gdk::Display::default().ok_or("No fixture display")?;
    let keymap = gdk::Keymap::for_display(&display).ok_or("No fixture keymap")?;
    let entries = keymap.entries_for_keyval(keyval);
    let mapping = entries
        .iter()
        .find(|entry| entry.level() == 0)
        .ok_or("Fixture key has no unshifted keyboard mapping")?;
    let hardware_keycode =
        u16::try_from(mapping.keycode()).map_err(|_| "Fixture keycode is out of range")?;
    let group =
        u8::try_from(mapping.group()).map_err(|_| "Fixture keyboard group is out of range")?;
    let keyboard = display
        .default_seat()
        .and_then(|seat| seat.keyboard())
        .ok_or("No native keyboard for fixture input")?;
    for kind in [gdk::EventType::KeyPress, gdk::EventType::KeyRelease] {
        let mut event = gdk::Event::new(kind);
        // The fresh event retains the owned widget window until GTK releases the event.
        unsafe {
            let key = event.as_ptr().cast::<gdk::ffi::GdkEventKey>();
            (*key).window = native.as_ptr();
            gtk::glib::gobject_ffi::g_object_ref(native.as_ptr().cast());
            (*key).send_event = 1;
            (*key).time = 0;
            (*key).keyval = keyval;
            (*key).hardware_keycode = hardware_keycode;
            (*key).group = group;
        }
        event.set_device(Some(&keyboard));
        gtk::main_do_event(&mut event);
    }
    Ok(())
}

pub(super) fn print_dialog_open() -> Result<bool, String> {
    Ok(print_dialog()?.is_some())
}

pub(super) fn cancel_print_dialog() -> Result<bool, String> {
    let Some(dialog) = print_dialog()? else {
        return Ok(false);
    };
    dialog.response(gtk::ResponseType::Cancel);
    Ok(true)
}

fn print_dialog() -> Result<Option<gtk::Dialog>, String> {
    let dialogs: Vec<_> = gtk::Window::list_toplevels()
        .into_iter()
        .filter(|widget| widget.type_().name() == "GtkPrintUnixDialog")
        .filter_map(|widget| widget.downcast_ref::<gtk::Dialog>().cloned())
        .filter(|dialog| dialog.is_visible())
        .collect();
    match dialogs.as_slice() {
        [] => Ok(None),
        [dialog] => Ok(Some(dialog.clone())),
        _ => Err("More than one process-owned print dialog appeared".into()),
    }
}

/// A GTK focus widget alone can be stale after chrome or another page takes X11 focus.
fn require_native_focus(child: isize) -> Result<(), String> {
    super::with_display(|xlib, display| {
        let (mut focused, mut revert) = (0, 0);
        unsafe {
            (xlib.XGetInputFocus)(display, &mut focused, &mut revert);
        }
        for _ in 0..32 {
            if focused == child as x11_dl::xlib::Window {
                return Ok(());
            }
            if focused <= 1 {
                break;
            }
            let (mut root, mut parent, mut count) = (0, 0, 0);
            let mut children = std::ptr::null_mut();
            let found = unsafe {
                (xlib.XQueryTree)(
                    display,
                    focused,
                    &mut root,
                    &mut parent,
                    &mut children,
                    &mut count,
                )
            };
            if !children.is_null() {
                unsafe {
                    (xlib.XFree)(children.cast());
                }
            }
            if found == 0 {
                return Err("Cannot read native fixture key ownership".into());
            }
            if parent == focused {
                break;
            }
            focused = parent;
        }
        Err("Native keyboard focus is outside the requested fixture page".into())
    })
}
