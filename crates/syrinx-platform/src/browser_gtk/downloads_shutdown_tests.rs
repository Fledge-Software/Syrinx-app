//! Proves native download shutdown releases contexts and cancels without choosing a file.

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
