//! Maps a key press over a focused page to Syrinx's own shortcut, by Windows virtual-key code.
//!
//! Every platform translates its key events into these codes first, so one table decides.

use super::BrowserShortcut;

/// Preserve page Escape unless this exact popup was visible when the key arrived.
pub(super) fn peek_escape(
    key: u32,
    control: bool,
    shift: bool,
    alt: bool,
    generation: u64,
) -> Option<BrowserShortcut> {
    (key == 0x1B && !control && !shift && !alt && generation != 0)
        .then_some(BrowserShortcut::DismissPeek(generation))
}

pub(super) fn shortcut_for_modifiers(
    key: u32,
    control: bool,
    shift: bool,
    alt: bool,
    right_alt: bool,
) -> Option<BrowserShortcut> {
    if key == 0x53 && control && alt && !shift && !right_alt {
        return Some(BrowserShortcut::ToggleSidebar);
    }
    if alt && !control && !shift {
        return match key {
            0x25 => Some(BrowserShortcut::Back),
            0x27 => Some(BrowserShortcut::Forward),
            _ => None,
        };
    }
    // F3 and Shift F3 step Syrinx's find bar, so WebView2's own never opens (#82).
    if key == 0x72 && !control && !alt {
        return Some(if shift {
            BrowserShortcut::FindPrevious
        } else {
            BrowserShortcut::FindNext
        });
    }
    if key == 0x7A && !control && !alt && !shift {
        return Some(BrowserShortcut::Fullscreen);
    }
    if key == 0x74 && !control && !alt && !shift {
        return Some(BrowserShortcut::Reload);
    }
    // F6 and Shift F6 leave the page for Syrinx's next or previous pane (P1-36).
    if key == 0x75 && !control && !alt {
        return Some(if shift {
            BrowserShortcut::FocusPreviousPane
        } else {
            BrowserShortcut::FocusNextPane
        });
    }
    if !control || alt {
        return None;
    }
    // Find, history and zoom are Syrinx's even over a focused page, as in Chrome (owner,
    // 2026-10-18). Print (Ctrl P) is not part of that decision and stays the page's under
    // the 2026-10-12 rule.
    if let Some(shortcut) = find_history_or_zoom(key, shift) {
        return Some(shortcut);
    }
    match (key, shift) {
        (0x4A, false) => Some(BrowserShortcut::Downloads),
        (0x54, false) => Some(BrowserShortcut::NewTab),
        (0x54, true) => Some(BrowserShortcut::ReopenTab),
        (0x57, false) => Some(BrowserShortcut::CloseTab),
        (0x4C, false) => Some(BrowserShortcut::Address),
        (0x4B, false) => Some(BrowserShortcut::Palette),
        (0x44, false) => Some(BrowserShortcut::SavePage),
        (0x52, false) => Some(BrowserShortcut::Reload),
        (0x09, false) => Some(BrowserShortcut::NextTab),
        (0x09, true) => Some(BrowserShortcut::PreviousTab),
        // VK_OEM_COMMA opens Settings, following Chrome and Edge (#91). The owner decided the
        // gear; taking Ctrl , from a focused page awaits the owner's confirmation.
        (0xBC, false) => Some(BrowserShortcut::Settings),
        _ => None,
    }
}

/// Ctrl F, Ctrl G (find next, #82), Ctrl H and the zoom keys. Plus and minus also count
/// shifted and on the keypad, as Chrome's table does; `VK_OEM_PLUS` and `VK_OEM_MINUS`
/// follow Chrome's key-code table, not the printed keys: Ctrl = zooms in on a US keyboard,
/// and on layouts without - on that key (such as French AZERTY) Ctrl - falls back to
/// WebView2's own zoom.
fn find_history_or_zoom(key: u32, shift: bool) -> Option<BrowserShortcut> {
    match (key, shift) {
        (0x46, false) => Some(BrowserShortcut::Find),
        (0x47, false) => Some(BrowserShortcut::FindNext),
        (0x48, false) => Some(BrowserShortcut::History),
        (0xBB | 0x6B, _) => Some(BrowserShortcut::ZoomIn),
        (0xBD | 0x6D, _) => Some(BrowserShortcut::ZoomOut),
        (0x30 | 0x60, false) => Some(BrowserShortcut::ZoomReset),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_is_forwarded_only_for_a_live_peek_without_modifiers() {
        assert_eq!(peek_escape(0x1B, false, false, false, 0), None);
        assert_eq!(
            peek_escape(0x1B, false, false, false, 7),
            Some(BrowserShortcut::DismissPeek(7))
        );
        for (control, shift, alt) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            assert_eq!(peek_escape(0x1B, control, shift, alt, 7), None);
        }
        assert_eq!(peek_escape(0x41, false, false, false, 7), None);
    }

    #[test]
    fn sidebar_pin_requires_exactly_control_alt_s() {
        for control in [false, true] {
            for alt in [false, true] {
                for shift in [false, true] {
                    let expected =
                        (control && alt && !shift).then_some(BrowserShortcut::ToggleSidebar);
                    assert_eq!(
                        shortcut_for_modifiers(0x53, control, shift, alt, false),
                        expected
                    );
                }
            }
        }
    }

    #[test]
    fn f6_moves_between_panes_and_shift_reverses() {
        assert_eq!(
            shortcut_for_modifiers(0x75, false, false, false, false),
            Some(BrowserShortcut::FocusNextPane)
        );
        assert_eq!(
            shortcut_for_modifiers(0x75, false, true, false, false),
            Some(BrowserShortcut::FocusPreviousPane)
        );
        for (control, alt) in [(true, false), (false, true), (true, true)] {
            assert_eq!(
                shortcut_for_modifiers(0x75, control, false, alt, false),
                None
            );
        }
    }

    #[test]
    fn control_comma_opens_settings_over_a_focused_page() {
        assert_eq!(
            shortcut_for_modifiers(0xBC, true, false, false, false),
            Some(BrowserShortcut::Settings)
        );
        // Shifted, with Alt or AltGr, or alone, the comma stays the page's.
        for (control, shift, alt, right_alt) in [
            (true, true, false, false),
            (true, false, true, false),
            (true, false, true, true),
            (false, false, false, false),
        ] {
            assert_eq!(
                shortcut_for_modifiers(0xBC, control, shift, alt, right_alt),
                None
            );
        }
    }

    #[test]
    fn altgr_s_stays_with_the_page() {
        assert_eq!(shortcut_for_modifiers(0x53, true, false, true, true), None);
        assert_eq!(
            shortcut_for_modifiers(0x53, true, false, true, false),
            Some(BrowserShortcut::ToggleSidebar)
        );
    }

    #[test]
    fn page_shortcuts_distinguish_browser_actions_from_shifted_variants() {
        for (key, shift, expected) in [
            (0x54, false, BrowserShortcut::NewTab),
            (0x54, true, BrowserShortcut::ReopenTab),
            (0x09, false, BrowserShortcut::NextTab),
            (0x09, true, BrowserShortcut::PreviousTab),
            (0x57, false, BrowserShortcut::CloseTab),
            (0x4C, false, BrowserShortcut::Address),
            (0x4B, false, BrowserShortcut::Palette),
            (0x44, false, BrowserShortcut::SavePage),
            (0x52, false, BrowserShortcut::Reload),
            (0x4A, false, BrowserShortcut::Downloads),
        ] {
            assert_eq!(
                shortcut_for_modifiers(key, true, shift, false, false),
                Some(expected)
            );
            assert_eq!(
                shortcut_for_modifiers(key, false, shift, false, false),
                None
            );
            assert_eq!(shortcut_for_modifiers(key, true, shift, true, false), None);
        }
        assert_eq!(
            shortcut_for_modifiers(0x25, false, false, true, false),
            Some(BrowserShortcut::Back)
        );
        assert_eq!(
            shortcut_for_modifiers(0x27, false, false, true, false),
            Some(BrowserShortcut::Forward)
        );
        assert_eq!(
            shortcut_for_modifiers(0x74, false, false, false, false),
            Some(BrowserShortcut::Reload)
        );
    }

    #[test]
    fn find_history_and_zoom_keys_reach_syrinx_before_a_focused_page() {
        for (key, shift, expected) in [
            (0x46, false, BrowserShortcut::Find),
            (0x48, false, BrowserShortcut::History),
            // Ctrl = and Ctrl + share a key; the keypad has its own.
            (0xBB, false, BrowserShortcut::ZoomIn),
            (0xBB, true, BrowserShortcut::ZoomIn),
            (0x6B, false, BrowserShortcut::ZoomIn),
            (0x6B, true, BrowserShortcut::ZoomIn),
            (0xBD, false, BrowserShortcut::ZoomOut),
            (0xBD, true, BrowserShortcut::ZoomOut),
            (0x6D, false, BrowserShortcut::ZoomOut),
            (0x6D, true, BrowserShortcut::ZoomOut),
            (0x30, false, BrowserShortcut::ZoomReset),
            (0x60, false, BrowserShortcut::ZoomReset),
        ] {
            assert_eq!(
                shortcut_for_modifiers(key, true, shift, false, false),
                Some(expected)
            );
            // Without Ctrl, or with Alt too (AltGr included), the key stays the page's.
            assert_eq!(
                shortcut_for_modifiers(key, false, shift, false, false),
                None
            );
            assert_eq!(shortcut_for_modifiers(key, true, shift, true, false), None);
            assert_eq!(shortcut_for_modifiers(key, true, shift, true, true), None);
        }
    }

    #[test]
    fn find_next_and_previous_keys_step_syrinx_find_not_webview2s_bar() {
        // F3, Shift F3 and Ctrl G (#82); Syrinx takes them so WebView2's own bar never opens.
        for (key, control, shift, expected) in [
            (0x72, false, false, BrowserShortcut::FindNext),
            (0x72, false, true, BrowserShortcut::FindPrevious),
            (0x47, true, false, BrowserShortcut::FindNext),
        ] {
            assert_eq!(
                shortcut_for_modifiers(key, control, shift, false, false),
                Some(expected)
            );
            // With Alt, or AltGr, the key stays the page's.
            assert_eq!(
                shortcut_for_modifiers(key, control, shift, true, false),
                None
            );
            assert_eq!(
                shortcut_for_modifiers(key, control, shift, true, true),
                None
            );
        }
        // Ctrl F3 and a bare G are not find keys; Ctrl Shift G waits on the owner.
        assert_eq!(
            shortcut_for_modifiers(0x72, true, false, false, false),
            None
        );
        assert_eq!(
            shortcut_for_modifiers(0x47, false, false, false, false),
            None
        );
        assert_eq!(shortcut_for_modifiers(0x47, true, true, false, false), None);
    }

    #[test]
    fn print_and_shifted_find_history_and_reset_stay_with_the_page() {
        assert_eq!(
            shortcut_for_modifiers(0x50, true, false, false, false),
            None
        );
        for key in [0x46, 0x48, 0x50, 0x30, 0x60] {
            assert_eq!(shortcut_for_modifiers(key, true, true, false, false), None);
        }
    }

    #[test]
    fn unrelated_and_extra_modified_keys_stay_with_the_page() {
        for key in [0x41, 0x43, 0x53, 0x56, 0x58, 0x5A] {
            assert_eq!(shortcut_for_modifiers(key, true, false, false, false), None);
        }
        for key in [0x57, 0x4C, 0x4B, 0x44, 0x52, 0x46, 0x4A, 0x50, 0x48] {
            assert_eq!(shortcut_for_modifiers(key, true, true, false, false), None);
        }
        for (control, shift, alt) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            assert_eq!(
                shortcut_for_modifiers(0x74, control, shift, alt, false),
                None
            );
        }
        assert_eq!(shortcut_for_modifiers(0x25, true, false, true, false), None);
        assert_eq!(shortcut_for_modifiers(0x27, false, true, true, false), None);
    }
}
