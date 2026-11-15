//! Proves native download shutdown releases contexts and cancels without choosing a file, and
//! that a finished download is reported complete and a cancelled one never is.

#[test]
fn shutdown_releases_tracked_views_and_their_context() {
    let released = crate::gtk_thread::run(|| {
        use gtk::prelude::*;
        let context = webkit2gtk::WebContext::with_website_data_manager(
            &webkit2gtk::WebsiteDataManager::new_ephemeral(),
        );
        let weak_context = context.downgrade();
        let (events, _outgoing) = futures::channel::mpsc::unbounded();
        let downloads = super::Downloads::watch(
            &context,
            super::EventSink(events, None),
            std::rc::Rc::default(),
        );
        let weak_registry = std::rc::Rc::downgrade(&downloads.0);
        let view = webkit2gtk::WebView::with_context(&context);
        downloads.track(1, &view);
        // SAFETY: this fixture owns the view, on GTK's dedicated thread.
        unsafe { view.destroy() };
        drop(view);
        downloads.shutdown();
        downloads.shutdown();
        drop(downloads);
        drop(context);
        (
            weak_context.upgrade().is_none(),
            weak_registry.upgrade().is_none(),
        )
    })
    .expect("GTK thread");
    assert_eq!(released, (true, true));
}

#[test]
fn shutdown_cancels_native_downloads_before_they_can_choose_a_file() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback listener");
    let port = listener.local_addr().expect("listener address").port();
    let outcome = crate::gtk_thread::run(move || {
        use gtk::glib;
        use webkit2gtk::{DownloadExt, WebContextExt};
        let context = webkit2gtk::WebContext::with_website_data_manager(
            &webkit2gtk::WebsiteDataManager::new_ephemeral(),
        );
        let (events, _outgoing) = futures::channel::mpsc::unbounded();
        let downloads = super::Downloads::watch(
            &context,
            super::EventSink(events, None),
            std::rc::Rc::default(),
        );
        // The server sends no response: no destination or owner's directory is consulted.
        let starting = glib::MainLoop::new(None, false);
        let started = std::rc::Rc::new(std::cell::Cell::new(false));
        let observed = started.clone();
        let ready = starting.clone();
        context.connect_download_started(move |_, _| {
            observed.set(true);
            ready.quit();
        });
        let download = context
            .download_uri(&format!("http://127.0.0.1:{port}/synthetic"))
            .expect("native download");
        let expired = std::rc::Rc::new(std::cell::Cell::new(false));
        let elapsed = expired.clone();
        let deadline = starting.clone();
        let timer = glib::timeout_add_local_once(std::time::Duration::from_secs(5), move || {
            elapsed.set(true);
            deadline.quit();
        });
        if !started.get() {
            starting.run();
        }
        if !expired.get() {
            timer.remove();
        }
        let tracked = downloads.0.borrow().running.len();
        let cancelled = std::rc::Rc::new(std::cell::Cell::new(false));
        let observed = cancelled.clone();
        let loop_ = glib::MainLoop::new(None, false);
        let finished = loop_.clone();
        download.connect_failed(move |_, error| {
            observed.set(
                error.kind::<webkit2gtk::DownloadError>()
                    == Some(webkit2gtk::DownloadError::CancelledByUser),
            );
            finished.quit();
        });
        downloads.shutdown();
        let empty = downloads.0.borrow().running.is_empty();
        let deadline = loop_.clone();
        let expired = std::rc::Rc::new(std::cell::Cell::new(false));
        let elapsed = expired.clone();
        let timer = glib::timeout_add_local_once(std::time::Duration::from_secs(5), move || {
            elapsed.set(true);
            deadline.quit();
        });
        if !cancelled.get() {
            loop_.run();
        }
        if !expired.get() {
            timer.remove();
        }
        (tracked, empty, cancelled.get())
    })
    .expect("GTK thread");
    drop(listener);
    assert_eq!(outcome, (1, true, true));
}

/// Answers every connection with a short body sent in chunks, so no length is announced, as a
/// server generating the file on the fly does.
fn serve_unsized_body(listener: std::net::TcpListener, body: &'static [u8]) {
    use std::io::{Read, Write};
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
            return;
        };
        let mut request = [0u8; 4096];
        if stream.read(&mut request).is_err() {
            continue;
        }
        let head = "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
                    Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
        let chunk = format!("{:x}\r\n", body.len());
        let sent = stream
            .write_all(head.as_bytes())
            .and_then(|()| stream.write_all(chunk.as_bytes()))
            .and_then(|()| stream.write_all(body))
            .and_then(|()| stream.write_all(b"\r\n0\r\n\r\n"));
        if let Err(error) = sent {
            eprintln!("The synthetic download server could not answer: {error}");
        }
    }
}

/// Answers every connection with the head and first chunk of an endless body, then holds the
/// connection until the client drops it, so the download is still running when it is cancelled.
fn serve_stalled_body(listener: std::net::TcpListener) {
    use std::io::{Read, Write};
    const FIRST_CHUNK: usize = 8192;
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
            return;
        };
        let mut request = [0u8; 4096];
        if stream.read(&mut request).is_err() {
            continue;
        }
        // A first chunk larger than WebKit's content-sniffing buffer, so data is delivered.
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
             Transfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n",
            FIRST_CHUNK,
            "x".repeat(FIRST_CHUNK)
        );
        if let Err(error) = stream.write_all(head.as_bytes()) {
            eprintln!("The synthetic download server could not answer: {error}");
            continue;
        }
        // Returns once the cancelled client closes its end.
        if let Err(error) = stream.read(&mut request) {
            eprintln!("The synthetic download connection failed: {error}");
        }
    }
}

/// A fresh synthetic folder, removed however the test ends so a failing run leaves nothing.
struct SyntheticFolder(std::path::PathBuf);

impl SyntheticFolder {
    fn new(name: &str, port: u16) -> Self {
        let folder =
            std::env::temp_dir().join(format!("syrinx-{name}-{}-{port}", std::process::id()));
        std::fs::create_dir(&folder).expect("fresh synthetic folder");
        Self(folder)
    }
}

impl Drop for SyntheticFolder {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("Could not remove the synthetic download folder: {error}");
        }
    }
}

/// Downloads from the loopback `port` into `target` through the registry's own handlers and
/// returns every reported change; `cancel` cancels the download once its first data arrives.
fn download_states(
    port: u16,
    target: std::path::PathBuf,
    cancel: bool,
) -> Vec<(syrinx_core::DownloadState, u64)> {
    crate::gtk_thread::run(move || {
        use gtk::glib;
        use webkit2gtk::{DownloadExt, WebContextExt};
        let context = webkit2gtk::WebContext::with_website_data_manager(
            &webkit2gtk::WebsiteDataManager::new_ephemeral(),
        );
        let (events, mut outgoing) = futures::channel::mpsc::unbounded();
        let registry =
            super::Downloads(std::rc::Rc::new(std::cell::RefCell::new(super::Registry {
                events: super::EventSink(events, None),
                pages: std::collections::HashMap::new(),
                popups: std::rc::Rc::default(),
                running: std::collections::HashMap::new(),
                held: std::collections::HashMap::new(),
                next: 0,
                stopped: false,
            })));
        let download = context
            .download_uri(&format!("http://127.0.0.1:{port}/synthetic"))
            .expect("native download");
        // The registry's own handlers report it; only the destination is this test's.
        let id = registry.register(&download);
        let uri = glib::filename_to_uri(&target, None).expect("synthetic destination");
        let watcher = registry.clone();
        download.connect_decide_destination(move |download, _| {
            download.set_destination(&uri);
            watcher.report_progress(id, download);
            true
        });
        if cancel {
            download.connect_received_data(|download, _| download.cancel());
        }
        let finished = glib::MainLoop::new(None, false);
        let quit = finished.clone();
        download.connect_finished(move |_| quit.quit());
        let deadline = finished.clone();
        let expired = std::rc::Rc::new(std::cell::Cell::new(false));
        let elapsed = expired.clone();
        let timer = glib::timeout_add_local_once(std::time::Duration::from_secs(10), move || {
            elapsed.set(true);
            deadline.quit();
        });
        finished.run();
        // A timer that fired has already removed itself.
        if !expired.get() {
            timer.remove();
        }
        let mut states = Vec::new();
        while let Ok(event) = outgoing.try_recv() {
            if let super::BrowserEvent::DownloadChanged {
                state, received, ..
            } = event
            {
                states.push((state, received));
            }
        }
        states
    })
    .expect("GTK thread")
}

#[test]
fn a_download_without_an_announced_length_is_reported_complete() {
    const BODY: &[u8] = b"synthetic download body";
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback listener");
    let port = listener.local_addr().expect("listener address").port();
    std::thread::spawn(move || serve_unsized_body(listener, BODY));
    let folder = SyntheticFolder::new("download-complete", port);
    let target = folder.0.join("synthetic.bin");
    let states = download_states(port, target.clone(), false);
    let written = std::fs::read(&target).expect("downloaded file");
    assert_eq!(written, BODY, "the whole body arrived");
    assert_eq!(
        states.last(),
        Some(&(syrinx_core::DownloadState::Completed, BODY.len() as u64)),
        "{states:?}"
    );
}

#[test]
fn a_cancelled_download_is_never_reported_complete() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback listener");
    let port = listener.local_addr().expect("listener address").port();
    std::thread::spawn(move || serve_stalled_body(listener));
    let folder = SyntheticFolder::new("download-cancelled", port);
    let states = download_states(port, folder.0.join("synthetic.bin"), true);
    assert!(
        !states
            .iter()
            .any(|(state, _)| *state == syrinx_core::DownloadState::Completed),
        "{states:?}"
    );
    assert!(
        matches!(
            states.last(),
            Some((
                syrinx_core::DownloadState::Interrupted(
                    syrinx_core::DownloadInterruption::Cancelled
                ),
                _
            ))
        ),
        "{states:?}"
    );
}
