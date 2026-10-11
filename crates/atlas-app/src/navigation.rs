//! Interprets explicit address submissions without sending typing to a search service.

pub fn destination(input: &str, force_search: bool) -> Result<String, &'static str> {
    let text = submission_text(input)?;
    if force_search {
        return Ok(web_search(text));
    }
    Ok(recognized_address(text)?.unwrap_or_else(|| web_search(text)))
}

// Keep the palette's literal-address row consistent with explicit navigation.
pub(crate) fn recognized_address(input: &str) -> Result<Option<String>, &'static str> {
    let text = submission_text(input)?;
    // Recognize schemes without changing the case of paths, queries or search text.
    let (scheme, remainder) = text.split_once(':').unwrap_or(("", text));
    if scheme.eq_ignore_ascii_case("about") && remainder == "blank" {
        return Ok(Some("about:blank".to_owned()));
    }
    if (scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http"))
        && remainder.starts_with("//")
    {
        return web_address(text).map(Some);
    }
    if text.contains("://")
        || scheme.eq_ignore_ascii_case("javascript")
        || scheme.eq_ignore_ascii_case("data")
    {
        return Err("This preview opens HTTP and HTTPS addresses.");
    }
    if !text.chars().any(char::is_whitespace)
        && (text.contains('.') || text.starts_with("localhost") || text.starts_with('['))
    {
        return web_address(&format!("https://{text}")).map(Some);
    }
    Ok(None)
}

// Both submission modes and row classification require nonempty trimmed text.
fn submission_text(input: &str) -> Result<&str, &'static str> {
    let text = input.trim();
    if text.is_empty() {
        return Err("Enter an address or a search.");
    }
    Ok(text)
}

fn web_address(text: &str) -> Result<String, &'static str> {
    let address = url::Url::parse(text).map_err(|_| "That address is not valid.")?;
    if address.host_str().is_none() {
        return Err("That address needs a website name.");
    }
    if !address.username().is_empty() || address.password().is_some() {
        return Err("Sign in on the website instead of putting credentials in an address.");
    }
    Ok(address.into())
}

fn web_search(text: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("q", text)
        .finish();
    format!("https://www.google.com/search?{query}")
}

#[cfg(test)]
mod tests {
    use super::{destination, recognized_address};

    #[test]
    fn explicit_navigation_and_search_remain_distinct() {
        assert_eq!(
            destination("example.test/path", false).unwrap(),
            "https://example.test/path"
        );
        assert_eq!(
            destination("two words & symbols", false).unwrap(),
            "https://www.google.com/search?q=two+words+%26+symbols"
        );
        assert_eq!(
            destination("example.test", true).unwrap(),
            "https://www.google.com/search?q=example.test"
        );
    }

    #[test]
    fn unsafe_schemes_and_empty_input_are_refused() {
        for text in [
            "",
            "   ",
            "javascript:1",
            "data:text/plain,synthetic",
            "file:///synthetic.txt",
        ] {
            assert!(destination(text, false).is_err());
        }
        assert_eq!(destination("about:blank", false).unwrap(), "about:blank");
    }

    #[test]
    fn scheme_case_does_not_change_navigation_or_address_validation() {
        for scheme in ["http", "HTTP", "hTtP", "https", "HTTPS", "hTtPs"] {
            assert_eq!(
                destination(
                    &format!("{scheme}://example.test/Case?Key=Value#Part"),
                    false
                )
                .unwrap(),
                format!(
                    "{}://example.test/Case?Key=Value#Part",
                    scheme.to_ascii_lowercase()
                )
            );
            assert_eq!(
                destination(&format!("{scheme}://synthetic@example.test/"), false),
                Err("Sign in on the website instead of putting credentials in an address.")
            );
            assert!(destination(&format!("{scheme}://[invalid]"), false).is_err());
        }
        assert_eq!(destination("aBoUt:blank", false).unwrap(), "about:blank");
    }

    #[test]
    fn refused_scheme_case_does_not_turn_an_address_into_a_search() {
        for text in [
            "JAVASCRIPT:1",
            "jAvAsCrIpT:1",
            "DATA:text/plain,synthetic",
            "DaTa:text/plain,synthetic",
            "FILE:///synthetic.txt",
        ] {
            assert_eq!(
                destination(text, false),
                Err("This preview opens HTTP and HTTPS addresses.")
            );
        }
    }

    #[test]
    fn explicit_web_search_preserves_scheme_and_query_case() {
        for text in [
            "HTTPS://example.test/Case?Key=Value#Part",
            "JAVASCRIPT:1",
            "aBoUt:blank",
        ] {
            let result = destination(text, true).unwrap();
            let address = url::Url::parse(&result).unwrap();
            assert_eq!(address.host_str(), Some("www.google.com"));
            assert_eq!(address.path(), "/search");
            assert_eq!(
                address.query_pairs().into_owned().collect::<Vec<_>>(),
                [("q".to_owned(), text.to_owned())]
            );
        }
    }

    #[test]
    fn literal_address_classification_matches_navigation() {
        for (input, expected) in [
            (
                " HTTPS://example.test/Case?Key=Value#Part ",
                "https://example.test/Case?Key=Value#Part",
            ),
            ("http://example.test", "http://example.test/"),
            ("example.test/path", "https://example.test/path"),
            ("localhost:8443/path", "https://localhost:8443/path"),
            ("[::1]:8443/path", "https://[::1]:8443/path"),
            ("aBoUt:blank", "about:blank"),
        ] {
            assert_eq!(
                recognized_address(input).unwrap().as_deref(),
                Some(expected)
            );
            assert_eq!(destination(input, false).unwrap(), expected);
        }
    }

    #[test]
    fn ordinary_search_and_refused_addresses_have_no_literal_destination() {
        for input in ["two words & symbols", "synthetic", "about:Blank"] {
            assert_eq!(recognized_address(input), Ok(None));
            assert_eq!(destination(input, false), destination(input, true));
        }
        for input in [
            "JAVASCRIPT:1",
            "DATA:text/plain,synthetic",
            "file:///synthetic.txt",
            "https://[invalid]",
            "https://synthetic@example.test/",
        ] {
            let error = recognized_address(input).unwrap_err();
            assert_eq!(destination(input, false), Err(error));
            assert!(destination(input, true).is_ok());
        }
        for input in ["", " \t\n"] {
            let error = recognized_address(input).unwrap_err();
            assert_eq!(destination(input, false), Err(error));
            assert_eq!(destination(input, true), Err(error));
        }
    }
}
