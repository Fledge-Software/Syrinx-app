//! Shows a page's address readably without letting the text disguise which site it is (P1-32).

/// The address as the owner reads it: scheme, host and port, never a user name or password;
/// percent-escapes in the path, query and fragment decoded only into visible, non-deceptive
/// characters; and the host always in its ASCII (punycode) form, so neither a look-alike Unicode
/// name nor `familiar.example@other.test` can pose as a familiar site.
///
/// Anything that is not an `http`/`https` address is returned unchanged.
pub fn readable_address(address: &str) -> String {
    let Ok(parsed) = url::Url::parse(address) else {
        return address.to_owned();
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return address.to_owned();
    }
    let Some(host) = parsed.host_str() else {
        return address.to_owned();
    };
    // `url` serializes the host as ASCII (IPv6 in brackets); credentials are left out.
    let port = parsed
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    let rest = &parsed[url::Position::BeforePath..];
    format!("{}://{host}{port}{}", parsed.scheme(), decode_visible(rest))
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
/// `?`, `#`, `&` or `%` never changes how the address reads. Other characters are shown only when
/// they are letters or digits: punctuation, symbols, spaces, format and private-use characters
/// and unassigned code points (none of which are alphanumeric) stay escaped. Right-to-left
/// letters also stay escaped so they cannot reorder the address, as do a few known look-alikes.
fn shows_safely(character: char) -> bool {
    if character.is_ascii() {
        return character.is_ascii_alphanumeric() || "-._~".contains(character);
    }
    character.is_alphanumeric() && !is_right_to_left(character) && !is_look_alike(character)
}

// Hebrew, Arabic, Syriac, Thaana, N'Ko and the other right-to-left blocks.
fn is_right_to_left(character: char) -> bool {
    matches!(
        u32::from(character),
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF
    )
}

// Letters that are invisible fillers, or that pass for an address separator.
fn is_look_alike(character: char) -> bool {
    matches!(
        u32::from(character),
        0x115F | 0x1160 | 0x3164 | 0xFFA0 | 0x3007 | 0x2571 | 0x2800 | 0xFF61 | 0xFE56 | 0xFE5F
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
            // Direction, zero-width and space characters.
            '\u{202E}',
            '\u{200B}',
            '\u{FEFF}',
            '\u{3000}',
            '\u{00A0}',
            '\u{2800}',
            // Separator look-alikes.
            '\u{2215}',
            '\u{FF0F}',
            '\u{2571}',
            '\u{FF61}',
            '\u{FE56}',
            '\u{FE5F}',
            '\u{3164}',
            // Format characters (Cf), private use (Co) and an unassigned code point (Cn).
            '\u{0600}',
            '\u{0605}',
            '\u{06DD}',
            '\u{070F}',
            '\u{13430}',
            '\u{1343F}',
            '\u{1BCA0}',
            '\u{1BCA3}',
            '\u{E000}',
            '\u{F0000}',
            '\u{0378}',
            // Right-to-left letters, which could reorder the address around them.
            '\u{05D0}',
            '\u{0627}',
        ] {
            let address = format!("https://example.test/a{}b", escaped(character));
            assert_eq!(readable_address(&address), address, "{character:?}");
        }
    }

    #[test]
    fn user_names_and_passwords_are_never_shown() {
        assert_eq!(
            readable_address("https://accounts.example.com@evil.test/"),
            "https://evil.test/"
        );
        assert_eq!(
            readable_address("http://user:synthetic-secret@evil.test:8443/p?q#f"),
            "http://evil.test:8443/p?q#f"
        );
        assert_eq!(
            readable_address("https://user@[2001:db8::1]:8443/"),
            "https://[2001:db8::1]:8443/"
        );
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
