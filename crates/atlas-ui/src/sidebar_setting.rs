//! Stores the sidebar's width state as one documented settings key (P1-29, DW-2).
//!
//! `atlas-store`'s `settings` table holds text; the drag release saves
//! [`sidebar_setting`] and start-up reads it back with [`parse_sidebar_setting`].

use crate::sidebar::SidebarWidthState;

/// The settings key for the persistent sidebar presentation.
pub const SIDEBAR_WIDTH_STATE_KEY: &str = "sidebar.width_state";

/// The stored value for each state. Peek is transient and never stored.
pub const fn sidebar_setting_value(state: SidebarWidthState) -> &'static str {
    match state {
        SidebarWidthState::Wide => "wide",
        SidebarWidthState::Compact => "favicon",
        SidebarWidthState::Hidden => "hidden",
    }
}

/// The key and value to save for `state`.
pub fn sidebar_setting(state: SidebarWidthState) -> (String, String) {
    (
        SIDEBAR_WIDTH_STATE_KEY.to_owned(),
        sidebar_setting_value(state).to_owned(),
    )
}

/// Reads a stored value back; anything else is unreadable and the caller keeps its default.
pub fn parse_sidebar_setting(value: &str) -> Option<SidebarWidthState> {
    SidebarWidthState::ALL
        .into_iter()
        .find(|state| sidebar_setting_value(*state) == value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_width_state_roundtrips_under_one_key() {
        for state in SidebarWidthState::ALL {
            let (key, value) = sidebar_setting(state);
            assert_eq!(key, "sidebar.width_state");
            assert_eq!(parse_sidebar_setting(&value), Some(state));
        }
        assert_eq!(sidebar_setting(SidebarWidthState::Compact).1, "favicon");
    }

    #[test]
    fn unknown_values_are_unreadable() {
        for value in ["", "Wide", "compact", "peek", " hidden"] {
            assert_eq!(parse_sidebar_setting(value), None, "{value}");
        }
    }
}
