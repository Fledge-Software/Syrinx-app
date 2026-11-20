//! Verifies WebKit's frame delegation before Linux attributes consent to the top-level site.

use std::{
    cell::Cell,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use gtk::{glib, prelude::*};
use webkit2gtk::{PermissionRequestExt, WebView, WebViewExt};

use super::{Embed, PageBounds, configure};

#[test]
fn only_a_delegated_cross_origin_location_request_reaches_the_host() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("synthetic server");
    let port = listener.local_addr().expect("server address").port();
    listener.set_nonblocking(true).expect("nonblocking server");
    let stopped = Arc::new(AtomicBool::new(false));
    let stop = stopped.clone();
    let server = thread::spawn(move || serve(listener, port, stop));
    let outcome = crate::gtk_thread::run(move || {
        ["/top", "/undelegated", "/delegated"].map(|route| probe(port, route))
    });
    stopped.store(true, Ordering::Release);
    server
        .join()
        .expect("server thread")
        .expect("synthetic responses");
    let [top, undelegated, delegated] = outcome.expect("GTK thread");
    assert_eq!(top, (1, Some("denied".into())));
    assert_eq!(undelegated, (0, Some("denied".into())));
    assert_eq!(delegated, (1, Some("denied".into())));
}

/// Uses an ephemeral view and refuses every native request; no location is ever read.
fn probe(port: u16, route: &str) -> (u32, Option<String>) {
    let context = webkit2gtk::WebContext::with_website_data_manager(
        &webkit2gtk::WebsiteDataManager::new_ephemeral(),
    );
    let view = WebView::with_context(&context);
    configure(&view);
    // Exercise the same foreign-window embedding as pages, rather than a GTK toplevel.
    let xlib = x11_dl::xlib::Xlib::open().expect("Xlib");
    let display = gtk::gdk::Display::default()
        .expect("GDK display")
        .downcast::<gdkx11::X11Display>()
        .expect("X11 display");
    let raw = unsafe { gdkx11::ffi::gdk_x11_display_get_xdisplay(display.as_ptr()) }.cast();
    let parent = unsafe {
        let root = (xlib.XDefaultRootWindow)(raw);
        (xlib.XCreateSimpleWindow)(raw, root, 0, 0, 500, 400, 0, 0, 0)
    };
    let embed = Embed::new(
        parent as isize,
        PageBounds {
            x: 0,
            y: 0,
            width: 500,
            height: 400,
        },
    )
    .expect("embedded fixture");
    let (window, _keys) = embed.gtk_window();
    window.add(&view);
    window.show_all();
    embed.map(true);
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    view.connect_permission_request(move |_, request| {
        observed.set(observed.get() + 1);
        request.deny();
        true
    });
    let loop_ = glib::MainLoop::new(None, false);
    let complete = loop_.clone();
    view.connect_title_notify(move |view| {
        if view
            .title()
            .is_some_and(|title| matches!(title.as_str(), "denied" | "granted"))
        {
            complete.quit();
        }
    });
    let timeout = loop_.clone();
    let expired = Rc::new(Cell::new(false));
    let elapsed = expired.clone();
    let timer = glib::timeout_add_local_once(Duration::from_secs(10), move || {
        elapsed.set(true);
        timeout.quit();
    });
    view.load_uri(&format!("http://127.0.0.1:{port}{route}"));
    loop_.run();
    if !expired.get() {
        timer.remove();
    }
    let title = view.title().map(String::from);
    view.stop_loading();
    // SAFETY: the fixture owns these windows, and GTK runs only on its dedicated thread.
    unsafe { window.destroy() };
    drop(embed);
    unsafe {
        (xlib.XDestroyWindow)(raw, parent);
        (xlib.XFlush)(raw);
    }
    (calls.get(), title)
}

/// Serves only authored loopback documents, with no policy header or host-side injection.
fn serve(listener: TcpListener, port: u16, stop: Arc<AtomicBool>) -> std::io::Result<()> {
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => respond(stream, port)?,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn respond(mut stream: TcpStream, port: u16) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line)?;
    let route = line.split_whitespace().nth(1).unwrap_or("");
    let body = if matches!(route, "/top" | "/child") {
        "<!doctype html><title>waiting</title><script>
         function done(result) { document.title=result; parent.postMessage(result, '*'); }
         navigator.geolocation.getCurrentPosition(() => done('granted'), () => done('denied'));
         </script>"
            .to_owned()
    } else {
        let allow = if route == "/delegated" {
            " allow=\"geolocation\""
        } else {
            ""
        };
        format!(
            "<!doctype html><title>waiting</title><script>
            addEventListener('message', event => document.title=event.data);
            </script><iframe src=\"http://localhost:{port}/child\"{allow}></iframe>"
        )
    };
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}
