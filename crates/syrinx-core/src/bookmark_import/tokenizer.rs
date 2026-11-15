//! Splits a bookmark export into tags and text without an HTML library or allocation per token.

/// One piece of the export. Names and attribute regions borrow from the input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Token<'a> {
    Start(StartTag<'a>),
    End(&'a str),
    Text(&'a str),
    Doctype(&'a str),
}

/// A start tag's name and its unparsed attribute text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct StartTag<'a> {
    pub name: &'a str,
    attributes: &'a str,
}

impl<'a> StartTag<'a> {
    pub fn is(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }

    /// Returns the raw (still entity-encoded) value of the first attribute named `wanted`.
    pub fn attribute(&self, wanted: &str) -> Option<&'a str> {
        Attributes {
            rest: self.attributes,
        }
        .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
        .map(|(_, value)| value)
    }
}

/// Yields tokens and the byte offset where each begins.
pub(super) struct Tokenizer<'a> {
    input: &'a str,
    position: usize,
}

impl<'a> Tokenizer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self { input, position: 0 }
    }

    fn rest(&self) -> &'a str {
        &self.input[self.position..]
    }

    /// Reads text up to the next `<` that begins markup.
    fn text(&mut self) -> &'a str {
        let rest = self.rest();
        let end = markup_start(rest).unwrap_or(rest.len());
        self.position += end;
        &rest[..end]
    }

    /// Reads markup starting at `<`; an unterminated construct consumes the rest of the input.
    fn markup(&mut self) -> Option<Token<'a>> {
        let rest = self.rest();
        if let Some(body) = rest.strip_prefix("<!--") {
            let length = body.find("-->").map_or(rest.len(), |end| 4 + end + 3);
            self.position += length;
            return None;
        }
        let close = rest.find('>').unwrap_or(rest.len());
        self.position += (close + 1).min(rest.len());
        let inner = &rest[1..close];
        if let Some(declaration) = inner.strip_prefix('!') {
            return Some(Token::Doctype(declaration));
        }
        if let Some(name) = inner.strip_prefix('/') {
            return Some(Token::End(tag_name(name).0));
        }
        let (name, attributes) = tag_name(inner);
        Some(Token::Start(StartTag { name, attributes }))
    }
}

impl<'a> Iterator for Tokenizer<'a> {
    type Item = (usize, Token<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        // Comments produce no token, so keep reading until a token or the end.
        while self.position < self.input.len() {
            let start = self.position;
            if markup_start(self.rest()) != Some(0) {
                return Some((start, Token::Text(self.text())));
            }
            if let Some(token) = self.markup() {
                return Some((start, token));
            }
        }
        None
    }
}

/// Finds the next `<` followed by a letter, `/` or `!`; other `<` characters are text.
fn markup_start(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    bytes.iter().enumerate().position(|(index, &byte)| {
        byte == b'<'
            && bytes
                .get(index + 1)
                .is_some_and(|next| next.is_ascii_alphabetic() || matches!(next, b'/' | b'!'))
    })
}

/// Splits a tag's inner text into its name and the remaining attribute text.
fn tag_name(inner: &str) -> (&str, &str) {
    let end = inner
        .find(|character: char| character.is_ascii_whitespace() || character == '/')
        .unwrap_or(inner.len());
    (&inner[..end], &inner[end..])
}

/// Iterates `NAME="value"`, `NAME='value'`, `NAME=value` and bare `NAME` attributes.
struct Attributes<'a> {
    rest: &'a str,
}

impl<'a> Iterator for Attributes<'a> {
    type Item = (&'a str, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        let text = self.rest.trim_start_matches(|character: char| {
            character.is_ascii_whitespace() || character == '/'
        });
        if text.is_empty() {
            return None;
        }
        let name_end = text
            .find(|character: char| {
                character.is_ascii_whitespace() || matches!(character, '=' | '/')
            })
            .unwrap_or(text.len());
        let name = &text[..name_end];
        let after_name =
            text[name_end..].trim_start_matches(|character: char| character.is_ascii_whitespace());
        let Some(after_equals) = after_name.strip_prefix('=') else {
            self.rest = after_name;
            return Some((name, ""));
        };
        let (value, rest) = attribute_value(
            after_equals.trim_start_matches(|character: char| character.is_ascii_whitespace()),
        );
        self.rest = rest;
        Some((name, value))
    }
}

/// Splits a quoted or unquoted value from the text after it; a missing quote ends at the tag.
fn attribute_value(text: &str) -> (&str, &str) {
    if let Some(quote) = text
        .chars()
        .next()
        .filter(|character| matches!(character, '"' | '\''))
    {
        let body = &text[1..];
        return match body.find(quote) {
            Some(end) => (&body[..end], &body[end + 1..]),
            None => (body, ""),
        };
    }
    let end = text
        .find(|character: char| character.is_ascii_whitespace())
        .unwrap_or(text.len());
    (&text[..end], &text[end..])
}

#[cfg(test)]
mod tests {
    use super::{Token, Tokenizer};

    fn tokens(input: &str) -> Vec<Token<'_>> {
        Tokenizer::new(input).map(|(_, token)| token).collect()
    }

    #[test]
    fn splits_tags_text_comments_and_declarations() {
        let found = tokens("<!DOCTYPE x><!-- note -->a<DT><A HREF=\"h\">T</A>");
        assert_eq!(found.len(), 6);
        assert_eq!(found[0], Token::Doctype("DOCTYPE x"));
        assert_eq!(found[1], Token::Text("a"));
        assert!(matches!(found[2], Token::Start(tag) if tag.is("dt")));
        assert!(
            matches!(found[3], Token::Start(tag) if tag.is("A") && tag.attribute("href") == Some("h"))
        );
        assert_eq!(found[4], Token::Text("T"));
        assert_eq!(found[5], Token::End("A"));
    }

    #[test]
    fn reads_every_attribute_quoting_style() {
        let found = tokens("<H3 ADD_DATE=12 a='single' PERSONAL_TOOLBAR_FOLDER=\"true\" bare>");
        let Token::Start(tag) = found[0] else {
            panic!("expected a start tag");
        };
        assert_eq!(tag.attribute("add_date"), Some("12"));
        assert_eq!(tag.attribute("A"), Some("single"));
        assert_eq!(tag.attribute("personal_toolbar_folder"), Some("true"));
        assert_eq!(tag.attribute("bare"), Some(""));
        assert_eq!(tag.attribute("missing"), None);
    }

    #[test]
    fn a_greater_than_sign_inside_a_quoted_value_ends_the_tag() {
        // Exports escape `>` in attributes, so a raw one is treated as the tag's end.
        let found = tokens("<A HREF=\"x>y\">T");
        assert_eq!(found.len(), 2);
        assert_eq!(found[1], Token::Text("y\">T"));
    }

    #[test]
    fn stray_less_than_signs_and_unterminated_markup_do_not_panic() {
        assert_eq!(tokens("a < b <3"), vec![Token::Text("a < b <3")]);
        assert_eq!(tokens("x<"), vec![Token::Text("x<")]);
        assert_eq!(tokens("<!-- open").len(), 0);
        let found = tokens("<A HREF=\"unterminated");
        assert!(
            matches!(found[0], Token::Start(tag) if tag.attribute("HREF") == Some("unterminated"))
        );
        assert_eq!(tokens("</"), vec![Token::End("")]);
    }

    #[test]
    fn offsets_point_at_each_token() {
        let offsets: Vec<usize> = Tokenizer::new("ab<P>cd")
            .map(|(offset, _)| offset)
            .collect();
        assert_eq!(offsets, vec![0, 2, 5]);
    }
}
