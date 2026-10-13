//! Turns address-bar submissions into owner-facing results through `atlas-core`'s one rule.

/// Where submitted text goes; Alt Enter (`force_search`) always searches Google (D9).
pub fn destination(input: &str, force_search: bool) -> Result<String, &'static str> {
    atlas_core::entry_destination(input, force_search).map_err(atlas_core::EntryError::message)
}

// Keep the palette's literal-address row consistent with explicit navigation.
pub(crate) fn recognized_address(input: &str) -> Result<Option<String>, &'static str> {
    atlas_core::recognized_address(input).map_err(atlas_core::EntryError::message)
}
