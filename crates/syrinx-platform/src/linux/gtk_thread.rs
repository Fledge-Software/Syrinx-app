//! Runs GTK on one dedicated thread for the whole process, since GTK may be used only there.

use std::{
    sync::{OnceLock, mpsc},
    thread::{self, ThreadId},
};

use gtk::glib;

/// The GTK thread once it has started, or why it could not.
static GTK: OnceLock<Result<ThreadId, GtkUnavailable>> = OnceLock::new();

/// GTK could not start, usually because no X11 display is reachable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GtkUnavailable;

impl std::fmt::Display for GtkUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .write_str("GTK could not start; Syrinx needs an X11 display (XWayland on Wayland).")
    }
}

/// Runs `job` on the GTK thread and waits for its result, starting GTK on first use.
pub(crate) fn run<R: Send + 'static>(
    job: impl FnOnce() -> R + Send + 'static,
) -> Result<R, GtkUnavailable> {
    let gtk_thread = start()?;
    // A job sent from the GTK thread itself would wait on its own queue forever.
    if thread::current().id() == gtk_thread {
        return Ok(job());
    }
    let (reply, result) = mpsc::sync_channel(1);
    glib::MainContext::default().invoke(move || {
        // The waiting caller may have given up; its result then has nowhere to go.
        if reply.send(job()).is_err() {
            eprintln!("A GTK job finished after its caller stopped waiting.");
        }
    });
    result.recv().map_err(|_| GtkUnavailable)
}

/// Queues `job` on the GTK thread without waiting for it.
pub(crate) fn post(job: impl FnOnce() + Send + 'static) -> Result<(), GtkUnavailable> {
    start()?;
    glib::MainContext::default().invoke(job);
    Ok(())
}

fn start() -> Result<ThreadId, GtkUnavailable> {
    *GTK.get_or_init(|| {
        let (started, ready) = mpsc::sync_channel(1);
        let spawned = thread::Builder::new()
            .name("syrinx-gtk".into())
            .spawn(move || serve(started));
        if spawned.is_err() {
            return Err(GtkUnavailable);
        }
        ready.recv().unwrap_or(Err(GtkUnavailable))
    })
}

/// Initialises GTK on X11, reports the outcome, then runs GTK's main loop until the process ends.
fn serve(started: mpsc::SyncSender<Result<ThreadId, GtkUnavailable>>) {
    // Syrinx's own Xlib connection shares the library with GDK's, from another thread.
    if let Ok(xlib) = x11_dl::xlib::Xlib::open() {
        unsafe { (xlib.XInitThreads)() };
    }
    // Pages embed as X11 children of Syrinx's window, so Wayland is never used here.
    gtk::gdk::set_allowed_backends("x11");
    let outcome = gtk::init()
        .map(|()| thread::current().id())
        .map_err(|_| GtkUnavailable);
    let initialised = outcome.is_ok();
    if started.send(outcome).is_err() || !initialised {
        return;
    }
    gtk::main();
}
