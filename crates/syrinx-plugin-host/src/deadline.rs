//! How long each plugin export may run, and the deadline a host service the guest waits on
//! must answer by. The figures come from `docs/dev/plugin-runtime-bounds.md`.

use std::time::{Duration, Instant};

use crate::limits::EPOCH_TICK;

/// One export of `syrinx:plugin`, each with its own deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Export {
    /// Instantiation (the component's start functions) and `lifecycle.activate`.
    Activate,
    Deactivate,
    Run,
    FormFor,
    Validate,
    Submit,
    Opened,
    Message,
    Closed,
    Changes,
    Resolve,
    Deliver,
}

impl Export {
    /// Every export, in the order the bounds document lists them.
    pub const ALL: [Self; 12] = [
        Self::Activate,
        Self::Deactivate,
        Self::Run,
        Self::FormFor,
        Self::Validate,
        Self::Submit,
        Self::Opened,
        Self::Message,
        Self::Closed,
        Self::Changes,
        Self::Resolve,
        Self::Deliver,
    ];

    /// Epoch ticks the export may run before it traps with `Trap::Interrupt`.
    pub const fn deadline_ticks(self) -> u64 {
        match self {
            Self::Activate | Self::Run | Self::Submit | Self::Changes => 100,
            Self::Opened | Self::Message => 50,
            Self::Deactivate | Self::FormFor | Self::Closed | Self::Resolve | Self::Deliver => 25,
            // Runs as the owner types, so a slow validator must not hold the next keystroke.
            Self::Validate => 10,
        }
    }

    /// The deadline as wall time.
    pub fn deadline(self) -> Duration {
        let ticks = u32::try_from(self.deadline_ticks()).unwrap_or(u32::MAX);
        EPOCH_TICK.saturating_mul(ticks)
    }

    /// The export's name in the WIT world.
    pub fn name(self) -> &'static str {
        match self {
            Self::Activate => "lifecycle.activate",
            Self::Deactivate => "lifecycle.deactivate",
            Self::Run => "commands.run",
            Self::FormFor => "forms.form-for",
            Self::Validate => "forms.validate",
            Self::Submit => "forms.submit",
            Self::Opened => "views.opened",
            Self::Message => "views.message",
            Self::Closed => "views.closed",
            Self::Changes => "search-feed.changes",
            Self::Resolve => "search-feed.resolve",
            Self::Deliver => "events.deliver",
        }
    }
}

/// When the export now running must finish. An epoch deadline cannot interrupt a host
/// function the guest is waiting on, so every host service answers by this instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallDeadline(Instant);

impl CallDeadline {
    /// The deadline of `export` starting now.
    pub fn starting_now(export: Export) -> Self {
        Self(Instant::now() + export.deadline())
    }

    /// Time left before the deadline; zero once it has passed.
    pub fn remaining(self) -> Duration {
        self.0.saturating_duration_since(Instant::now())
    }

    /// How long a host service may wait for a dependency: half the time left, so the plugin
    /// still has time to handle the service's `unavailable` answer before its own deadline.
    pub fn service_budget(self) -> Duration {
        self.remaining() / 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_deadline_is_a_whole_number_of_ticks_and_names_its_export() {
        for export in Export::ALL {
            assert!(export.deadline_ticks() > 0, "{}", export.name());
            assert_eq!(
                export.deadline().as_millis(),
                u128::from(export.deadline_ticks()) * EPOCH_TICK.as_millis()
            );
        }
    }

    #[test]
    fn a_passed_deadline_has_nothing_remaining_and_a_service_gets_half_of_what_is_left() {
        let deadline = CallDeadline(Instant::now());
        assert_eq!(deadline.remaining(), Duration::ZERO);
        assert_eq!(deadline.service_budget(), Duration::ZERO);
        let running = CallDeadline::starting_now(Export::Run);
        assert!(running.service_budget() <= Export::Run.deadline() / 2);
        assert!(running.service_budget() > Export::Run.deadline() / 4);
    }
}
