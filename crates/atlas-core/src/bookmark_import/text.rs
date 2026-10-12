//! Turns exported HTML text into plain display text: entities decoded, whitespace collapsed.

/// The longest entity reference recognized, including `&` and `;` (`&#x10FFFF;` is 10).
const MAX_ENTITY_LEN: usize = 12;

/// Decodes the entity references bookmark exports use; unknown references stay literal.
pub(super) fn decode_entities(raw: &str) -> String {
    let mut decoded = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(ampersand) = rest.find('&') {
        decoded.push_str(&rest[..ampersand]);
        rest = &rest[ampersand..];
        match decode_one(rest) {
            Some((character, consumed)) => {
                decoded.push(character);
                rest = &rest[consumed..];
            }
            None => {
                decoded.push('&');
                rest = &rest[1..];
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

/// Decodes one reference at the start of `text`, returning the character and bytes used.
fn decode_one(text: &str) -> Option<(char, usize)> {
    // Every recognized reference ends with a semicolon within a short window.
    let window_end = text.len().min(MAX_ENTITY_LEN);
    let semicolon = text.as_bytes()[..window_end]
        .iter()
        .position(|&byte| byte == b';')?;
    let body = &text[1..semicolon];
    let character = match body.strip_prefix('#') {
        Some(number) => decode_numeric(number)?,
        None => decode_named(body)?,
    };
    Some((character, semicolon + 1))
}

fn decode_numeric(number: &str) -> Option<char> {
    // Invalid code points (zero, surrogates, out of range) become U+FFFD, as browsers do.
    let value = match number.strip_prefix(['x', 'X']) {
        Some(hex) if is_digits(hex, 16) => u32::from_str_radix(hex, 16).ok()?,
        Some(_) => return None,
        None if is_digits(number, 10) => number.parse::<u32>().ok()?,
        None => return None,
    };
    if value == 0 {
        return Some(char::REPLACEMENT_CHARACTER);
    }
    Some(char::from_u32(value).unwrap_or(char::REPLACEMENT_CHARACTER))
}

fn is_digits(text: &str, radix: u32) -> bool {
    !text.is_empty() && text.chars().all(|character| character.is_digit(radix))
}

fn decode_named(name: &str) -> Option<char> {
    // Chrome, Edge and Firefox escape only these; other named references stay literal.
    match name {
        "amp" | "AMP" => Some('&'),
        "lt" | "LT" => Some('<'),
        "gt" | "GT" => Some('>'),
        "quot" | "QUOT" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{A0}'),
        _ => None,
    }
}

/// Collapses runs of whitespace (including CR, LF and no-break space) and trims the ends.
pub(super) fn collapse_whitespace(text: &str) -> String {
    let mut collapsed = String::with_capacity(text.len());
    for word in text
        .split(char::is_whitespace)
        .filter(|word| !word.is_empty())
    {
        if !collapsed.is_empty() {
            collapsed.push(' ');
        }
        collapsed.push_str(word);
    }
    collapsed
}

/// Shortens `text` to at most `max_chars` characters; returns whether anything was cut.
pub(super) fn truncate_chars(text: &mut String, max_chars: usize) -> bool {
    let Some((cut, _)) = text.char_indices().nth(max_chars) else {
        return false;
    };
    text.truncate(cut);
    true
}

/// Decodes and normalizes a title or folder name for display.
pub(super) fn display_text(raw: &str) -> String {
    // Remove characters that could hide or reorder text in the preview (including decoded ones).
    let visible: String = decode_entities(raw)
        .chars()
        .filter(|&character| !is_invisible_control(character))
        .collect();
    collapse_whitespace(&visible)
}

/// Control characters other than whitespace, and bidirectional overrides and isolates.
fn is_invisible_control(character: char) -> bool {
    (character.is_control() && !character.is_whitespace()) || is_bidi_control(character)
}

/// Embedding, override (U+202A–U+202E) and isolate (U+2066–U+2069) characters, which
/// can make text display in a different order from the one it is stored in.
pub(super) fn is_bidi_control(character: char) -> bool {
    matches!(character, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

#[cfg(test)]
mod tests {
    use super::{collapse_whitespace, decode_entities, display_text, truncate_chars};

    #[test]
    fn titles_lose_control_and_bidirectional_override_characters() {
        assert_eq!(display_text("A\0B\u{7}C\u{9F}D"), "ABCD");
        assert_eq!(display_text("evil\u{202E}txt.exe"), "eviltxt.exe");
        assert_eq!(
            display_text("\u{202A}a\u{202B}b\u{202C}c\u{202D}d\u{2066}e\u{2067}f\u{2068}g\u{2069}"),
            "abcdefg"
        );
        // Encoded forms are removed too, and whitespace controls still separate words.
        assert_eq!(display_text("x&#0;y&#x202E;z\r\nw"), "x\u{FFFD}yz w");
        // Ordinary right-to-left text and its marks are kept.
        assert_eq!(display_text("שלום\u{200F} title"), "שלום\u{200F} title");
    }

    #[test]
    fn decodes_the_entities_browsers_write() {
        assert_eq!(
            decode_entities("A &amp; B &lt;C&gt; &quot;D&quot; &#39;E&#39; &#x41;"),
            "A & B <C> \"D\" 'E' A"
        );
    }

    #[test]
    fn leaves_unknown_and_malformed_references_literal() {
        for literal in [
            "&copy;",
            "&;",
            "&#;",
            "&#xZZ;",
            "& amp;",
            "&amp",
            "&#12345678901234;",
        ] {
            assert_eq!(decode_entities(literal), literal);
        }
    }

    #[test]
    fn invalid_code_points_become_the_replacement_character() {
        assert_eq!(
            decode_entities("&#0;&#xD800;&#x110000;"),
            "\u{FFFD}\u{FFFD}\u{FFFD}"
        );
    }

    #[test]
    fn entities_next_to_multibyte_text_keep_character_boundaries() {
        assert_eq!(decode_entities("é&amp;ü&ö;"), "é&ü&ö;");
        assert_eq!(decode_entities("&é"), "&é");
    }

    #[test]
    fn whitespace_collapses_across_crlf_and_tabs() {
        assert_eq!(
            collapse_whitespace("  One\r\n\tTwo   Three \n"),
            "One Two Three"
        );
        assert_eq!(display_text("A&nbsp;&nbsp;B"), "A B");
    }

    #[test]
    fn truncation_respects_character_boundaries() {
        let mut text = "ééé".to_owned();
        assert!(truncate_chars(&mut text, 2));
        assert_eq!(text, "éé");
        assert!(!truncate_chars(&mut text, 2));
    }
}
