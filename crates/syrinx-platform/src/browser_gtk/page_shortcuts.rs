//! Reads a key press over a focused page as Syrinx's own shortcut, whatever the keyboard layout.

use gtk::gdk;

use super::super::{
    BrowserShortcut,
    keys::{peek_escape, shortcut_for_modifiers},
};

/// The facts of a GDK key event that decide its shortcut.
struct KeyPress {
    keyval: u32,
    control: bool,
    shift: bool,
    alt: bool,
    right_alt: bool,
}

/// The shared Windows table decides; GDK keys are translated to its virtual-key codes first.
pub(super) fn shortcut_for_key(
    key: &gdk::EventKey,
    peek_generation: u64,
) -> Option<BrowserShortcut> {
    let state = key.state();
    let press = KeyPress {
        keyval: *key.keyval(),
        control: state.contains(gdk::ModifierType::CONTROL_MASK),
        shift: state.contains(gdk::ModifierType::SHIFT_MASK),
        alt: state.contains(gdk::ModifierType::MOD1_MASK),
        // AltGr arrives as Mod5 (ISO Level 3), which the Windows table calls the right Alt.
        right_alt: state.contains(gdk::ModifierType::MOD5_MASK),
    };
    shortcut_for_press(&press, peek_generation, || latin_keyval(key))
}

/// The shortcut for a key by its own value, else by its value in the keymap's Latin layout.
fn shortcut_for_press(
    press: &KeyPress,
    peek_generation: u64,
    latin_keyval: impl FnOnce() -> Option<u32>,
) -> Option<BrowserShortcut> {
    let &KeyPress {
        keyval,
        control,
        shift,
        alt,
        right_alt,
    } = press;
    // A Latin layout is read only for Ctrl keys, never while typing text or with AltGr.
    let code = match virtual_key(keyval) {
        Some(code) => code,
        None if control && !right_alt => virtual_key(latin_keyval()?)?,
        None => return None,
    };
    peek_escape(code, control, shift, alt, peek_generation)
        .or_else(|| shortcut_for_modifiers(code, control, shift, alt, right_alt))
}

/// What the pressed key types in the keymap's first Latin layout, when the active one is not Latin.
fn latin_keyval(key: &gdk::EventKey) -> Option<u32> {
    let keymap = gdk::Keymap::for_display(&key.window()?.display())?;
    let a_positions: Vec<_> = keymap
        .entries_for_keyval(*gdk::keys::constants::a)
        .iter()
        .map(|entry| (entry.group(), entry.level()))
        .collect();
    let group = latin_group(key.group().into(), &a_positions)?;
    let (keyval, ..) =
        keymap.translate_keyboard_state(key.hardware_keycode().into(), key.state(), group)?;
    Some(keyval)
}

/// The first Latin layout (XKB group), given the (group, level) pairs that type `a`.
///
/// A layout is Latin when some key types `a` unshifted. `None` when the active one is, so its
/// letters and punctuation keep their own meaning, or when the keymap has no Latin layout.
fn latin_group(active: i32, a_positions: &[(i32, i32)]) -> Option<i32> {
    let latin_groups = a_positions
        .iter()
        .filter(|(_, level)| *level == 0)
        .map(|(group, _)| *group);
    if latin_groups.clone().any(|group| group == active) {
        return None;
    }
    latin_groups.min()
}

/// The Windows virtual-key code for a GDK key value, for the keys Syrinx's table names.
fn virtual_key(keyval: u32) -> Option<u32> {
    use gdk::keys::constants as key;
    let lower = gdk::keys::Key::from(keyval).to_lower();
    let value = *lower;
    match value {
        letter @ 0x61..=0x7a => Some(letter - 0x20),
        digit @ 0x30..=0x39 => Some(digit),
        _ if lower == key::Tab || lower == key::ISO_Left_Tab => Some(0x09),
        _ if lower == key::Escape => Some(0x1B),
        _ if lower == key::Left => Some(0x25),
        _ if lower == key::Right => Some(0x27),
        _ if lower == key::F3 => Some(0x72),
        _ if lower == key::F5 => Some(0x74),
        _ if lower == key::F6 => Some(0x75),
        _ if lower == key::F11 => Some(0x7A),
        _ if lower == key::comma => Some(0xBC),
        _ if lower == key::equal || lower == key::plus => Some(0xBB),
        _ if lower == key::minus || lower == key::underscore => Some(0xBD),
        _ if lower == key::KP_Add => Some(0x6B),
        _ if lower == key::KP_Subtract => Some(0x6D),
        _ if lower == key::KP_0 => Some(0x60),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use gdk::keys::constants as key;

    use super::*;

    /// One XKB group: the unshifted key value of each hardware keycode the tests press.
    type Layout = &'static [(u16, gdk::keys::Key)];

    // X11 keycodes, named for what the key types on a US QWERTY keyboard.
    const DIGIT_0: u16 = 19;
    const MINUS: u16 = 20;
    const Q: u16 = 24;
    const W: u16 = 25;
    const T: u16 = 28;
    const A: u16 = 38;
    const S: u16 = 39;
    const K: u16 = 45;
    const COMMA: u16 = 59;

    const US: Layout = &[
        (DIGIT_0, key::_0),
        (MINUS, key::minus),
        (Q, key::q),
        (W, key::w),
        (T, key::t),
        (A, key::a),
        (S, key::s),
        (K, key::k),
        (COMMA, key::comma),
    ];
    const DVORAK: Layout = &[
        (DIGIT_0, key::_0),
        (MINUS, key::bracketleft),
        (Q, key::apostrophe),
        (W, key::comma),
        (T, key::y),
        (A, key::a),
        (S, key::o),
        (K, key::t),
        (COMMA, key::w),
    ];
    const AZERTY: Layout = &[
        (DIGIT_0, key::agrave),
        (MINUS, key::parenright),
        (Q, key::a),
        (W, key::z),
        (T, key::t),
        (A, key::q),
        (S, key::s),
        (K, key::k),
        (COMMA, key::semicolon),
    ];
    const RUSSIAN: Layout = &[
        (DIGIT_0, key::_0),
        (MINUS, key::minus),
        (Q, key::Cyrillic_shorti),
        (W, key::Cyrillic_tse),
        (T, key::Cyrillic_ie),
        (A, key::Cyrillic_ef),
        (S, key::Cyrillic_yeru),
        (K, key::Cyrillic_el),
        (COMMA, key::Cyrillic_be),
    ];
    const GREEK: Layout = &[
        (Q, key::semicolon),
        (W, key::Greek_finalsmallsigma),
        (T, key::Greek_tau),
        (A, key::Greek_alpha),
        (S, key::Greek_sigma),
        (K, key::Greek_kappa),
        (COMMA, key::comma),
    ];
    const HEBREW: Layout = &[
        (Q, key::slash),
        (W, key::apostrophe),
        (T, key::hebrew_aleph),
        (A, key::hebrew_shin),
        (S, key::hebrew_dalet),
        (K, key::hebrew_lamed),
        (COMMA, key::hebrew_taw),
    ];

    const CONTROL: gdk::ModifierType = gdk::ModifierType::CONTROL_MASK;
    const SHIFT: gdk::ModifierType = gdk::ModifierType::SHIFT_MASK;
    const ALT: gdk::ModifierType = gdk::ModifierType::MOD1_MASK;
    const ALT_GR: gdk::ModifierType = gdk::ModifierType::MOD5_MASK;

    /// What a key types; Shift only upper-cases, which holds for the letters the tests shift.
    fn keyval(layout: Layout, keycode: u16, shift: bool) -> Option<u32> {
        let (_, value) = layout.iter().find(|(code, _)| *code == keycode)?;
        Some(if shift { *value.to_upper() } else { **value })
    }

    /// Where `a` is typed in a keymap, as `Keymap::entries_for_keyval` reports it.
    fn a_positions(keymap: &[Layout]) -> Vec<(i32, i32)> {
        keymap
            .iter()
            .zip(0..)
            .filter(|(layout, _)| layout.iter().any(|(_, value)| *value == key::a))
            .map(|(_, group)| (group, 0))
            .collect()
    }

    /// The shortcut for a hardware key pressed with a group of the keymap active.
    fn press(
        keymap: &[Layout],
        active: usize,
        keycode: u16,
        state: gdk::ModifierType,
    ) -> Option<BrowserShortcut> {
        let shift = state.contains(SHIFT);
        let press = KeyPress {
            keyval: keyval(keymap[active], keycode, shift).expect("the layout has the key"),
            control: state.contains(CONTROL),
            shift,
            alt: state.contains(ALT),
            right_alt: state.contains(ALT_GR),
        };
        let active = i32::try_from(active).expect("a group fits");
        shortcut_for_press(&press, 0, || {
            let group = latin_group(active, &a_positions(keymap))?;
            keyval(keymap[usize::try_from(group).ok()?], keycode, shift)
        })
    }

    #[test]
    fn non_latin_layouts_take_the_letter_from_the_latin_layout() {
        for layout in [RUSSIAN, GREEK, HEBREW] {
            // The Latin layout may come before or after the active one (`us,ru` and `ru,us`).
            for (keymap, active) in [([US, layout], 1), ([layout, US], 0)] {
                for (keycode, state, expected) in [
                    (T, CONTROL, BrowserShortcut::NewTab),
                    (T, CONTROL | SHIFT, BrowserShortcut::ReopenTab),
                    (W, CONTROL, BrowserShortcut::CloseTab),
                    (K, CONTROL, BrowserShortcut::Palette),
                    (S, CONTROL | ALT, BrowserShortcut::ToggleSidebar),
                ] {
                    assert_eq!(press(&keymap, active, keycode, state), Some(expected));
                }
            }
        }
    }

    #[test]
    fn non_latin_layouts_take_punctuation_and_digits_too() {
        assert_eq!(
            press(&[US, RUSSIAN], 1, COMMA, CONTROL),
            Some(BrowserShortcut::Settings)
        );
        assert_eq!(
            press(&[US, RUSSIAN], 1, DIGIT_0, CONTROL),
            Some(BrowserShortcut::ZoomReset)
        );
        // A key whose own value is a shortcut never needs the Latin layout.
        assert_eq!(
            press(&[GREEK], 0, COMMA, CONTROL),
            Some(BrowserShortcut::Settings)
        );
    }

    #[test]
    fn the_first_latin_layout_names_the_key() {
        // K on a QWERTY keyboard types T on Dvorak.
        assert_eq!(
            press(&[RUSSIAN, DVORAK, US], 0, K, CONTROL),
            Some(BrowserShortcut::NewTab)
        );
        assert_eq!(
            press(&[RUSSIAN, US, DVORAK], 0, K, CONTROL),
            Some(BrowserShortcut::Palette)
        );
    }

    #[test]
    fn an_active_latin_layout_is_never_read_as_another() {
        for (layout, keycode, expected) in [
            (DVORAK, K, Some(BrowserShortcut::NewTab)),
            (DVORAK, COMMA, Some(BrowserShortcut::CloseTab)),
            (DVORAK, W, Some(BrowserShortcut::Settings)),
            (DVORAK, T, None),
            (AZERTY, T, Some(BrowserShortcut::NewTab)),
            (AZERTY, W, None),
            // Punctuation and accents that are shortcuts in the US layout stay the page's.
            (DVORAK, MINUS, None),
            (AZERTY, DIGIT_0, None),
            (AZERTY, COMMA, None),
        ] {
            for (keymap, active) in [([layout, US], 0), ([US, layout], 1)] {
                assert_eq!(press(&keymap, active, keycode, CONTROL), expected);
            }
        }
    }

    #[test]
    fn altgr_and_plain_typing_stay_with_the_page() {
        let keymap = [US, RUSSIAN];
        for keycode in [T, W, K, S, COMMA] {
            for state in [
                gdk::ModifierType::empty(),
                SHIFT,
                ALT,
                ALT_GR,
                ALT_GR | SHIFT,
                ALT_GR | CONTROL,
                ALT_GR | CONTROL | SHIFT,
                ALT_GR | CONTROL | ALT,
            ] {
                assert_eq!(press(&keymap, 1, keycode, state), None);
            }
        }
        // Keys Syrinx never takes from a Latin layout stay the page's as well.
        for keycode in [A, Q, S] {
            assert_eq!(press(&keymap, 1, keycode, CONTROL), None);
        }
    }

    #[test]
    fn a_keymap_without_a_latin_layout_leaves_the_key_to_the_page() {
        assert_eq!(press(&[RUSSIAN], 0, T, CONTROL), None);
        assert_eq!(press(&[RUSSIAN, GREEK], 1, T, CONTROL), None);
    }

    #[test]
    fn a_layout_is_latin_only_when_it_types_a_unshifted() {
        // Some Cyrillic and Burmese layouts carry Latin letters on AltGr levels.
        assert_eq!(latin_group(0, &[(0, 2), (1, 0)]), Some(1));
        assert_eq!(latin_group(1, &[(0, 0), (1, 1)]), Some(0));
        assert_eq!(latin_group(0, &[(1, 2)]), None);
        assert_eq!(latin_group(0, &[]), None);
    }
}
