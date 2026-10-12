//! Per-window frame demand, so the VSync thread repaints only windows with work to draw.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// A latch shared by a window, its GPUI frame waker and the VSync thread.
///
/// GPUI raises it when a window becomes dirty or schedules a next-frame
/// callback. The VSync thread only reads it; the UI thread consumes it when a
/// frame callback actually runs, so demand raised during that frame survives
/// for the next display tick.
#[derive(Clone)]
pub(crate) struct FrameDemand(Arc<AtomicBool>);

impl FrameDemand {
    /// A new window starts with demand so its first frame is always drawn.
    pub(crate) fn new() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }

    /// Asks for one more frame on the next display tick.
    pub(crate) fn request(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Whether a frame is still owed. Used by the VSync thread, which must not consume it.
    pub(crate) fn is_pending(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    /// Consumes the demand at the start of a frame that will run GPUI's callback.
    pub(crate) fn take(&self) -> bool {
        self.0.swap(false, Ordering::AcqRel)
    }
}

/// Whether the VSync thread should invalidate a window on this tick.
///
/// Hidden and minimized windows keep their demand; showing or restoring them
/// paints through their own native messages and then the latch.
pub(crate) fn should_invalidate(demand_pending: bool, visible: bool, minimized: bool) -> bool {
    demand_pending && visible && !minimized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_window_demands_its_first_frame() {
        assert!(FrameDemand::new().is_pending());
    }

    #[test]
    fn take_consumes_demand_once() {
        let demand = FrameDemand::new();
        assert!(demand.take());
        assert!(!demand.take());
        assert!(!demand.is_pending());
    }

    #[test]
    fn idle_window_stays_idle_until_requested() {
        let demand = FrameDemand::new();
        demand.take();
        assert!(!should_invalidate(demand.is_pending(), true, false));
        demand.request();
        assert!(should_invalidate(demand.is_pending(), true, false));
    }

    #[test]
    fn reading_demand_does_not_consume_it() {
        let demand = FrameDemand::new();
        assert!(demand.is_pending());
        assert!(demand.is_pending());
        assert!(demand.take());
    }

    #[test]
    fn demand_raised_during_a_frame_survives_that_frame() {
        let demand = FrameDemand::new();
        // Frame entry consumes the demand that started the frame...
        assert!(demand.take());
        // ...and GPUI re-invalidates while drawing.
        demand.request();
        assert!(demand.is_pending());
        assert!(should_invalidate(demand.is_pending(), true, false));
    }

    #[test]
    fn hidden_or_minimized_windows_keep_demand_without_invalidation() {
        let demand = FrameDemand::new();
        assert!(!should_invalidate(demand.is_pending(), false, false));
        assert!(!should_invalidate(demand.is_pending(), true, true));
        assert!(demand.is_pending());
    }

    #[test]
    fn clones_share_one_latch() {
        let window = FrameDemand::new();
        let waker = window.clone();
        let vsync = window.clone();
        window.take();
        waker.request();
        assert!(vsync.is_pending());
        assert!(window.take());
        assert!(!vsync.is_pending());
    }

    #[test]
    fn requests_from_another_thread_are_visible() {
        let demand = FrameDemand::new();
        demand.take();
        let remote = demand.clone();
        std::thread::spawn(move || remote.request())
            .join()
            .expect("request thread finishes");
        assert!(demand.take());
    }
}
