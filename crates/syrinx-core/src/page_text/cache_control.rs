//! Reads `Cache-Control` strictly enough that an unreadable header never permits storing (D35).

/// What a response's `Cache-Control` headers say about keeping its text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Storing {
    Allowed,
    NoStore,
    /// A header Syrinx cannot read; treated as `no-store` by the caller.
    Malformed,
}

/// Combines every `Cache-Control` header value of one response (RFC 9111, section 5.2).
pub(super) fn storing(values: &[String]) -> Storing {
    let mut verdict = Storing::Allowed;
    for value in values {
        match directives(value) {
            Err(()) => return Storing::Malformed,
            Ok(names)
                if names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case("no-store")) =>
            {
                verdict = Storing::NoStore;
            }
            Ok(_) => {}
        }
    }
    verdict
}

/// The directive names in one header value, or an error if any part is malformed.
fn directives(value: &str) -> Result<Vec<&str>, ()> {
    let mut names = Vec::new();
    let mut rest = value;
    loop {
        rest = rest.trim_start_matches([' ', '\t']);
        // Empty list elements (`a,,b`) are allowed by the list syntax.
        if let Some(after) = rest.strip_prefix(',') {
            rest = after;
            continue;
        }
        if rest.is_empty() {
            return Ok(names);
        }
        let (name, after_name) = split_token(rest)?;
        names.push(name);
        rest = skip_argument(after_name)?;
        rest = rest.trim_start_matches([' ', '\t']);
        match rest.strip_prefix(',') {
            Some(after) => rest = after,
            None if rest.is_empty() => return Ok(names),
            None => return Err(()),
        }
    }
}

/// Splits a leading RFC 9110 token; an empty token is malformed.
fn split_token(text: &str) -> Result<(&str, &str), ()> {
    let end = text
        .find(|character: char| !is_token_character(character))
        .unwrap_or(text.len());
    if end == 0 {
        return Err(());
    }
    Ok(text.split_at(end))
}

/// Skips `=token` or `="quoted"` after a directive name, if present.
fn skip_argument(text: &str) -> Result<&str, ()> {
    let Some(argument) = text.strip_prefix('=') else {
        return Ok(text);
    };
    if let Some(quoted) = argument.strip_prefix('"') {
        return skip_quoted(quoted);
    }
    split_token(argument).map(|(_, rest)| rest)
}

/// Skips the remainder of a quoted string, honouring backslash escapes.
fn skip_quoted(text: &str) -> Result<&str, ()> {
    let mut characters = text.char_indices();
    while let Some((index, character)) = characters.next() {
        match character {
            '"' => return Ok(&text[index + 1..]),
            '\\' => {
                characters.next().ok_or(())?;
            }
            character if character.is_ascii_control() && character != '\t' => return Err(()),
            _ => {}
        }
    }
    Err(())
}

fn is_token_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(character)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict(values: &[&str]) -> Storing {
        storing(
            &values
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn every_spelling_of_no_store_is_found() {
        for values in [
            &["no-store"][..],
            &["No-Store"],
            &["NO-STORE"],
            &["private, no-store"],
            &["  no-store  "],
            &["max-age=0", "no-store"],
            &["no-cache=\"set-cookie, x\", no-store"],
            &["private,,no-store"],
            &["no-store=1"],
        ] {
            assert_eq!(verdict(values), Storing::NoStore, "{values:?}");
        }
    }

    #[test]
    fn caching_limits_other_than_no_store_allow_storing() {
        for values in [
            &[][..],
            &["max-age=0"],
            &["no-cache"],
            &["private, max-age=60"],
            &["no-cache=\"no-store\""],
            &[""],
            &["public", "max-age=10"],
        ] {
            assert_eq!(verdict(values), Storing::Allowed, "{values:?}");
        }
    }

    #[test]
    fn unreadable_headers_are_malformed() {
        for values in [
            &["no-store=\""][..],
            &["max-age=\"1"],
            &["=no-store"],
            &["max-age=0 no-store"],
            &["no-store; private"],
            &["max-age="],
            &["caf\u{e9}"],
            &["max-age=0", "\u{1}"],
        ] {
            assert_eq!(verdict(values), Storing::Malformed, "{values:?}");
        }
    }
}
