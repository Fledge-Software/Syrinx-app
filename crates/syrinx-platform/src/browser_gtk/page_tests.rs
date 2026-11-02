//! Guards media consent, GTK allocation units and modal dialogs leaving the command pump.

use super::*;

#[test]
fn combined_media_cannot_use_a_camera_only_choice() {
    assert_eq!(media_permission_kind(true, true), None);
    assert_eq!(
        media_permission_kind(true, false),
        Some(PermissionKind::Camera)
    );
    assert_eq!(
        media_permission_kind(false, true),
        Some(PermissionKind::Microphone)
    );
    assert_eq!(media_permission_kind(false, false), None);
}

#[test]
fn physical_page_bounds_become_gtk_logical_allocations() {
    assert_eq!(logical_extent(1236, 2), 618);
    assert_eq!(logical_extent(1916, 2), 958);
    assert_eq!(logical_extent(619, 2), 310);
    assert_eq!(logical_extent(619, 1), 619);
    assert_eq!(logical_extent(0, 2), 1);
    assert_eq!(logical_extent(u32::MAX, 1), i32::MAX);
}

#[test]
fn a_modal_dialog_starts_after_the_host_borrow_and_skips_closed_pages() {
    let outcome = crate::gtk_thread::run(|| {
        let called = Rc::new(Cell::new(false));
        let host_available = Rc::new(Cell::new(false));
        let life = CallbackLife::new();
        let observed = called.clone();
        let available = host_available.clone();
        // A print command arrives with HOST borrowed; its nested loop must run later.
        super::super::HOST.with(|host| {
            let _borrow = host.borrow_mut();
            run_page_dialog(life.ticket(true), move || {
                observed.set(true);
                let free = super::super::HOST.with(|host| host.try_borrow_mut().is_ok());
                available.set(free);
                if free {
                    super::super::pump();
                }
            });
        });
        let before_dispatch = called.get();
        let closed = CallbackLife::new();
        let late = Rc::new(Cell::new(false));
        let observed = late.clone();
        run_page_dialog(closed.ticket(true), move || observed.set(true));
        closed.retire();
        // A lower-priority sentinel finishes after the queued dialog callbacks.
        let loop_ = glib::MainLoop::new(None, false);
        let finish = loop_.clone();
        glib::idle_add_local_full(glib::Priority::LOW, move || {
            finish.quit();
            glib::ControlFlow::Break
        });
        loop_.run();
        (
            before_dispatch,
            called.get(),
            host_available.get(),
            late.get(),
        )
    })
    .expect("GTK thread");
    assert_eq!(outcome, (false, true, true, false));
}
