//! Shows a page's address readably without letting the text disguise which site it is (P1-32).

/// The address as the owner reads it: percent-escapes in the path, query and fragment are
/// decoded only into visible, non-deceptive characters, and the host always stays in its ASCII
/// (punycode) form, so a look-alike Unicode name can never pose as a familiar site.
///
/// Anything that is not an `http`/`https` address is returned unchanged.
pub fn readable_address(address: &str) -> String {
    let Ok(parsed) = url::Url::parse(address) else {
        return address.to_owned();
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return address.to_owned();
    }
    // `url` already serializes the host as ASCII; only the part after the port is decoded.
    let origin = &parsed[..url::Position::AfterPort];
    let rest = &parsed[url::Position::BeforePath..];
    format!("{origin}{}", decode_visible(rest))
}

/// Decodes each run of `%XX` escapes that forms valid UTF-8, keeping the escape for any
/// character that `shows_safely` refuses and for runs that are not valid UTF-8.
fn decode_visible(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut remaining = text;
    while let Some(start) = remaining.find('%') {
        output.push_str(&remaining[..start]);
        let (run, bytes) = escape_run(&remaining[start..]);
        if run.is_empty() {
            output.push('%');
            remaining = &remaining[start + 1..];
            continue;
        }
        push_run(&mut output, run, &bytes);
        remaining = &remaining[start + run.len()..];
    }
    output.push_str(remaining);
    output
}

// The longest prefix of `%XX` escapes, and the bytes it encodes.
fn escape_run(text: &str) -> (&str, Vec<u8>) {
    let mut bytes = Vec::new();
    let mut length = 0;
    while let Some(byte) = text
        .get(length..length + 3)
        .and_then(|escape| escape.strip_prefix('%'))
        .and_then(|hex| u8::from_str_radix(hex, 16).ok())
    {
        bytes.push(byte);
        length += 3;
    }
    (&text[..length], bytes)
}

// Write one run, decoded character by character where that is safe to show.
fn push_run(output: &mut String, run: &str, bytes: &[u8]) {
    let Ok(decoded) = std::str::from_utf8(bytes) else {
        output.push_str(run);
        return;
    };
    let mut offset = 0;
    for character in decoded.chars() {
        let escaped = &run[offset..offset + 3 * character.len_utf8()];
        offset += escaped.len();
        if shows_safely(character) {
            output.push(character);
        } else {
            output.push_str(escaped);
        }
    }
}

/// Decoded ASCII is limited to letters, digits and `-._~`, so a reserved character such as `/`,
/// `?`, `#`, `&` or `%` never changes how the address reads. Other characters are shown unless
/// they are invisible, change text direction or imitate an address separator.
fn shows_safely(character: char) -> bool {
    if character.is_ascii() {
        return character.is_ascii_alphanumeric() || "-._~".contains(character);
    }
    !(character.is_control() || character.is_whitespace() || is_deceptive(character))
}

fn is_deceptive(character: char) -> bool {
    matches!(
        u32::from(character),
        // Soft hyphen, combining grapheme joiner, Arabic letter mark and Hangul fillers.
        0x00AD | 0x034F | 0x061C | 0x115F | 0x1160 | 0x3164 | 0xFFA0
        // Khmer vowel inherents, Mongolian selectors and zero-width or direction marks.
        | 0x17B4..=0x17B5 | 0x180B..=0x180F | 0x200B..=0x200F | 0x202A..=0x202E
        // Word joiners, invisible operators, direction isolates and variation selectors.
        | 0x2060..=0x206F | 0xFE00..=0xFE0F | 0xFEFF | 0xFFF0..=0xFFFF
        | 0x1D173..=0x1D17A | 0xE0000..=0xE0FFF
        // Characters that look like `/`, `?`, `#` or `.` in an address.
        | 0x0337 | 0x0338 | 0x2024 | 0x2044 | 0x2215 | 0x29F8 | 0x3002 | 0xFE52 | 0xFF03
        | 0xFF0E | 0xFF0F | 0xFF1F
    )
}

/// The escape form of `character`, for tests that build deceptive inputs.
#[cfg(test)]
fn escaped(character: char) -> String {
    use std::fmt::Write as _;
    let mut buffer = [0; 4];
    character
        .encode_utf8(&mut buffer)
        .bytes()
        .fold(String::new(), |mut text, byte| {
            write!(text, "%{byte:02X}").expect("writing to a String cannot fail");
            text
        })
}

#[cfg(test)]
mod tests {
    use super::{escaped, readable_address};

    #[test]
    fn readable_text_is_decoded_in_the_path_query_and_fragment() {
        assert_eq!(
            readable_address(
                "https://example.test/caf%C3%A9/%E6%97%A5%E6%9C%AC?q=na%C3%AFve#r%C3%A9sum%C3%A9"
            ),
            "https://example.test/café/日本?q=naïve#résumé"
        );
        assert_eq!(
            readable_address("https://example.test/%7Euser/a%2Db"),
            "https://example.test/~user/a-b"
        );
    }

    #[test]
    fn reserved_and_invisible_characters_stay_escaped() {
        let address = "https://example.test/a%2Fb%3Fc%23d%25e%26f%3Dg%20h%0Ai";
        assert_eq!(readable_address(address), address);
        for character in [
            '\u{202E}', '\u{200B}', '\u{FEFF}', '\u{2215}', '\u{FF0F}', '\u{3000}',
        ] {
            let address = format!("https://example.test/a{}b", escaped(character));
            assert_eq!(readable_address(&address), address, "{character:?}");
        }
    }

    #[test]
    fn invalid_escapes_and_partial_utf8_are_left_alone() {
        for address in [
            "https://example.test/%C3",
            "https://example.test/%C3%28",
            "https://example.test/%zz%",
            "https://example.test/100%",
        ] {
            assert_eq!(readable_address(address), address);
        }
    }

    #[test]
    fn hosts_stay_ascii_so_look_alikes_show_as_punycode() {
        // A Cyrillic "а" posing as Latin; the address bar shows the punycode the page really uses.
        let address = url::Url::parse("https://\u{0430}pple.example/").expect("synthetic address");
        let shown = readable_address(address.as_str());
        assert!(shown.starts_with("https://xn--"), "{shown}");
        assert!(shown.is_ascii());
        assert_eq!(
            readable_address("https://xn--bcher-kva.example:8443/"),
            "https://xn--bcher-kva.example:8443/"
        );
    }

    #[test]
    fn other_schemes_and_damaged_text_are_unchanged() {
        for address in [
            "about:blank",
            "data:text/plain,%C3%A9",
            "not an address",
            "",
        ] {
            assert_eq!(readable_address(address), address);
        }
    }
}
