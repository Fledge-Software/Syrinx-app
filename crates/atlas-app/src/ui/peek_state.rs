//! Disambiguates resting on the hidden sidebar edge from pressing it to resize.

use std::time::Duration;

pub(super) const HOVER_DELAY: Duration = Duration::from_millis(250);
pub(super) const SAMPLE_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Default, Debug, PartialEq, Eq)]
pub(super) enum PeekState {
    #[default]
    Idle,
    Waiting(Duration),
    Open,
    Suppressed,
}

pub(super) struct HoverSample {
    pub eligible: bool,
    pub edge: bool,
    pub popup: bool,
    pub pressed: bool,
}

impl PeekState {
    /// A dismissal or press requires leaving the edge before another hover can open it.
    pub fn dismiss(&mut self) {
        *self = Self::Suppressed;
    }

    /// Uses elapsed monotonic time, never the number of timer ticks.
    pub fn sample(&mut self, now: Duration, sample: HoverSample) {
        if !sample.eligible || !(sample.edge || sample.popup) {
            *self = Self::Idle;
            return;
        }
        if sample.pressed && sample.edge {
            self.dismiss();
            return;
        }
        match self {
            Self::Idle if sample.edge && !sample.pressed => *self = Self::Waiting(now),
            Self::Waiting(since) if now.saturating_sub(*since) >= HOVER_DELAY => {
                *self = Self::Open;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(state: &mut PeekState, time: u64, edge: bool, popup: bool, pressed: bool) {
        state.sample(
            Duration::from_millis(time),
            HoverSample {
                eligible: true,
                edge,
                popup,
                pressed,
            },
        );
    }

    #[test]
    fn hovering_waits_and_crossing_into_popup_keeps_it_open() {
        let mut state = PeekState::default();
        sample(&mut state, 0, true, false, false);
        sample(&mut state, 249, true, false, false);
        assert_eq!(state, PeekState::Waiting(Duration::ZERO));
        sample(&mut state, 250, true, false, false);
        assert_eq!(state, PeekState::Open);
        sample(&mut state, 275, false, true, false);
        sample(&mut state, 300, false, true, true);
        assert_eq!(state, PeekState::Open);
        sample(&mut state, 325, false, false, false);
        assert_eq!(state, PeekState::Idle);
    }

    #[test]
    fn press_cancels_wait_and_release_does_not_open_under_the_pointer() {
        let mut state = PeekState::default();
        sample(&mut state, 0, true, false, false);
        sample(&mut state, 200, true, false, true);
        sample(&mut state, 500, true, false, false);
        assert_eq!(state, PeekState::Suppressed);
        sample(&mut state, 525, false, false, false);
        sample(&mut state, 550, true, false, false);
        sample(&mut state, 800, true, false, false);
        assert_eq!(state, PeekState::Open);
    }

    #[test]
    fn leaving_or_deactivation_cancels_a_pending_hover() {
        let mut state = PeekState::default();
        sample(&mut state, 0, true, false, false);
        sample(&mut state, 240, false, false, false);
        sample(&mut state, 250, true, false, false);
        assert_eq!(state, PeekState::Waiting(Duration::from_millis(250)));
        state.sample(
            Duration::from_secs(1),
            HoverSample {
                eligible: false,
                edge: true,
                popup: true,
                pressed: false,
            },
        );
        assert_eq!(state, PeekState::Idle);
    }

    #[test]
    fn escape_and_row_activation_require_a_fresh_hover() {
        let mut state = PeekState::Open;
        state.dismiss();
        sample(&mut state, 1000, true, false, false);
        assert_eq!(state, PeekState::Suppressed);
    }

    #[test]
    fn an_open_peek_yields_to_press_and_drag_on_the_exposed_strip() {
        let mut state = PeekState::Open;
        let mut sizing = atlas_ui::SidebarSizing::new(atlas_ui::SidebarWidthState::Hidden);
        sample(&mut state, 300, true, false, true);
        sizing.begin_drag(4.);
        sizing.drag_to(204.);
        assert_eq!(state, PeekState::Suppressed);
        assert_eq!(sizing.width(), 200.);
        state.sample(
            Duration::from_millis(700),
            HoverSample {
                eligible: !sizing.is_dragging(),
                edge: true,
                popup: false,
                pressed: true,
            },
        );
        assert_ne!(state, PeekState::Open);
        assert_eq!(sizing.release(), Some(atlas_ui::SidebarWidthState::Wide));
    }
}
